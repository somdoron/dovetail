mod class_defaults;
mod classes;
mod enums;
mod extensions;
mod functions;
mod globals;
mod implementation_matching;
mod implements;
pub(crate) use implements::{
    expand_gats, expand_trait_bound_gats, rename_method_bounds, substitute_trait_bounds,
    substitute_trait_type_params,
};
mod modules;
mod newtypes;
mod records;
mod trait_flatten;
mod traits;
mod type_aliases;
mod types;

use std::collections::VecDeque;

use crate::common::diagnostics::Diagnostics;
use crate::common::types::{Fqn, PackagePath, SymbolName, TypeParamName};
use crate::parser::ast::{
    Declaration, ImportDecl, NewtypeDecl, SourceFile, TraitConstraint, TypeAliasDecl,
};
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{TraitBounds, Type};

/// Holds the state for the collect phase.
///
/// The dependency registry is read-only (from previous packages).
/// Declarations from this package are accumulated into a new package registry.
pub(super) struct Collector<'a> {
    package_path: PackagePath,
    dependency_registry: &'a Registry,
    package_registry: Registry,
    diagnostics: &'a mut Diagnostics,
    /// Import declarations from the current file (for cross-package name resolution).
    current_file_imports: &'a [ImportDecl],
}

impl<'a> Collector<'a> {
    fn new(
        package_path: PackagePath,
        dependency_registry: &'a Registry,
        diagnostics: &'a mut Diagnostics,
    ) -> Self {
        const EMPTY_IMPORTS: &[ImportDecl] = &[];
        Self {
            package_path,
            dependency_registry,
            package_registry: Registry::new(),
            diagnostics,
            current_file_imports: EMPTY_IMPORTS,
        }
    }

    /// Resolve a bare name to its FQN using imports, then same-package fallback.
    ///
    /// Mirrors `resolve_fqn` from the inference phase:
    /// 1. Check file imports — if an import's last segment (or alias) matches, construct FQN
    /// 2. Same-package — construct FQN from current package path + name
    ///
    /// The `exists` callback checks whether the FQN actually exists in the registries.
    fn resolve_name_to_fqn(&self, name: &str, exists: impl Fn(&Fqn) -> bool) -> Option<Fqn> {
        // 1. Check imports
        for import in self.current_file_imports {
            let local_name = import
                .alias
                .as_ref()
                .map(|a| a.value.as_str())
                .unwrap_or_else(|| import.path.last().map(|s| s.value.as_str()).unwrap_or(""));

            if local_name == name && import.path.len() >= 2 {
                let pkg_segments: Vec<String> = import.path[..import.path.len() - 1]
                    .iter()
                    .map(|s| s.value.clone())
                    .collect();
                let Some(last) = import.path.last() else {
                    continue;
                };
                let symbol_name = &last.value;
                let fqn = Fqn {
                    package: PackagePath(pkg_segments),
                    symbol: SymbolName(symbol_name.clone()),
                };

                if exists(&fqn) {
                    return Some(fqn);
                }
            }
        }

        // 2. Same-package fallback
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(name.to_string()),
        };
        if exists(&fqn) {
            return Some(fqn);
        }

        // 3. Prelude fallback — check standard.prelude
        let prelude_fqn = Fqn {
            package: PackagePath(vec!["standard".into(), "prelude".into()]),
            symbol: SymbolName(name.to_string()),
        };
        if exists(&prelude_fqn) {
            return Some(prelude_fqn);
        }

        None
    }

    /// Resolve where clause trait constraints to a TraitBounds.
    /// Validates that each constrained type param exists and each trait name resolves.
    pub(super) fn resolve_trait_bounds(
        &mut self,
        where_clause: &[TraitConstraint],
        type_params: &[TypeParamName],
    ) -> TraitBounds {
        let scope = Type::type_param_map(type_params, &TraitBounds::empty());
        self.resolve_trait_bounds_in_scope(where_clause, type_params, &scope)
    }

    pub(super) fn resolve_method_trait_bounds(
        &mut self,
        where_clause: &[TraitConstraint],
        all_parameters: &[TypeParamName],
        method_parameters: &[TypeParamName],
        enclosing_bounds: &TraitBounds,
    ) -> TraitBounds {
        let mut scope = Type::type_param_map(all_parameters, enclosing_bounds);
        scope.extend(Type::type_param_map(
            method_parameters,
            &TraitBounds::empty(),
        ));
        self.resolve_trait_bounds_in_scope(where_clause, all_parameters, &scope)
    }

    pub(super) fn resolve_trait_bounds_in_scope(
        &mut self,
        where_clause: &[TraitConstraint],
        type_params: &[TypeParamName],
        preliminary_map: &std::collections::BTreeMap<String, Type>,
    ) -> TraitBounds {
        let scope = crate::typechecker::bound_projections::resolve_scope(
            where_clause,
            preliminary_map,
            |constraint, scope| {
                let before = self.diagnostics.len();
                let resolved = self.resolve_trait_bounds_once(
                    std::slice::from_ref(constraint),
                    type_params,
                    scope,
                );
                let failed = self.diagnostics.len() != before;
                self.diagnostics.truncate(before);
                if failed { None } else { Some(resolved) }
            },
        );
        self.resolve_trait_bounds_once(where_clause, type_params, &scope)
    }

    fn resolve_trait_bounds_once(
        &mut self,
        where_clause: &[TraitConstraint],
        type_params: &[TypeParamName],
        preliminary_map: &std::collections::BTreeMap<String, Type>,
    ) -> TraitBounds {
        use crate::typechecker::types::TraitBound;

        let mut trait_bounds = TraitBounds::empty();

        for constraint in where_clause {
            let tp_name = TypeParamName(constraint.type_param.value.clone());

            // Validate that the type param exists
            if !type_params.contains(&tp_name) {
                self.diagnostics.error(
                    constraint.type_param.span.clone(),
                    format!(
                        "type parameter '{}' in where clause is not declared on this item",
                        constraint.type_param.value
                    ),
                );
                continue;
            }

            let mut bounds = Vec::new();
            for trait_bound in &constraint.trait_bounds {
                let crate::parser::ast::TypeBound::Named(trait_bound) = trait_bound else {
                    bounds.push(TraitBound::IsClass);
                    continue;
                };
                if let Some((fqn, trait_sig)) = self.resolve_trait(&trait_bound.name.value) {
                    let resolved_type_args: Vec<Type> = trait_bound
                        .type_args
                        .iter()
                        .map(|te| self.resolve_type_expr_with_type_params(te, preliminary_map))
                        .collect();

                    // Operators and associated-type bindings require a complete trait application.
                    let requires_arguments = !trait_bound.associated_types.is_empty()
                        || (fqn.package.to_string() == "standard.prelude"
                            && matches!(
                                fqn.symbol.0.as_str(),
                                "Add" | "Sub" | "Mul" | "Div" | "Concat"
                            ));
                    if (!resolved_type_args.is_empty() || requires_arguments)
                        && resolved_type_args.len() != trait_sig.type_params.len()
                    {
                        self.diagnostics.error(
                            trait_bound.span.clone(),
                            format!(
                                "trait '{}' expects {} type argument(s), found {}",
                                trait_bound.name.value,
                                trait_sig.type_params.len(),
                                resolved_type_args.len()
                            ),
                        );
                        continue;
                    }

                    let mut associated_types = std::collections::BTreeMap::new();
                    for (name, te) in &trait_bound.associated_types {
                        match trait_sig
                            .associated_types
                            .iter()
                            .find(|a| a.name == name.value)
                        {
                            Some(assoc) if assoc.type_params.is_empty() => {}
                            Some(_) => {
                                self.diagnostics.error(
                                    name.span.clone(),
                                    "cannot bind a generic associated type".to_string(),
                                );
                                continue;
                            }
                            None => {
                                self.diagnostics.error(
                                    name.span.clone(),
                                    format!("unknown associated type '{}'", name.value),
                                );
                                continue;
                            }
                        }
                        let ty = self.resolve_type_expr_with_type_params(te, preliminary_map);
                        if associated_types.insert(name.value.clone(), ty).is_some() {
                            self.diagnostics.error(
                                name.span.clone(),
                                format!("duplicate associated type binding '{}'", name.value),
                            );
                        }
                    }
                    bounds.push(TraitBound::Named(
                        crate::typechecker::types::NamedTraitBound {
                            associated_types,
                            trait_fqn: fqn,
                            type_args: resolved_type_args,
                            kind: crate::typechecker::types::BoundKind::HasTrait,
                        },
                    ));
                } else if let Some(fqn) = self
                    .resolve_class(&trait_bound.name.value)
                    .or_else(|| Type::from_primitive(&trait_bound.name.value).map(|ty| ty.to_fqn()))
                {
                    // Nominal subtype bound: a class or a prelude primitive.
                    if !trait_bound.type_args.is_empty() || !trait_bound.associated_types.is_empty()
                    {
                        self.diagnostics.error(
                            trait_bound.span.clone(),
                            format!(
                                "subtype bounds cannot have type arguments or associated types (on '{}')",
                                trait_bound.name.value,
                            ),
                        );
                        continue;
                    }
                    bounds.push(TraitBound::Named(
                        crate::typechecker::types::NamedTraitBound {
                            associated_types: Default::default(),
                            trait_fqn: fqn,
                            type_args: vec![],
                            kind: crate::typechecker::types::BoundKind::SubtypeOf,
                        },
                    ));
                } else {
                    self.diagnostics.error(
                        trait_bound.name.span.clone(),
                        format!(
                            "unknown trait, class, or primitive: '{}'",
                            trait_bound.name.value
                        ),
                    );
                }
            }

            if !bounds.is_empty() {
                if let Some(name) = trait_bounds.conflicting_associated_binding(&tp_name, &bounds) {
                    self.diagnostics.error(
                        constraint.span.clone(),
                        format!("conflicting associated type binding '{}'", name),
                    );
                }
                trait_bounds.insert(tp_name, bounds);
            }
        }

        trait_bounds
    }

    /// Resolve non-generic newtypes and type aliases using a worklist with fixpoint.
    /// Items that can't be resolved yet (because they reference other unresolved items)
    /// are retried until all are resolved or no more progress can be made.
    fn resolve_transparent_types(&mut self, files: &[&'a SourceFile]) {
        enum Item<'a> {
            Newtype(&'a NewtypeDecl, &'a [ImportDecl]),
            TypeAlias(&'a TypeAliasDecl, &'a [ImportDecl]),
        }

        let mut worklist: VecDeque<Item<'a>> = VecDeque::new();
        for file in files {
            for decl in &file.declarations {
                match decl {
                    Declaration::Newtype(nt) if nt.type_params.is_empty() => {
                        worklist.push_back(Item::Newtype(nt, &file.imports));
                    }
                    Declaration::TypeAlias(ta) if ta.type_params.is_empty() => {
                        worklist.push_back(Item::TypeAlias(ta, &file.imports));
                    }
                    _ => {}
                }
            }
        }

        let mut failures = 0;
        while let Some(item) = worklist.pop_front() {
            if failures > worklist.len() {
                // Full round with no progress — remaining items have circular
                // dependencies or genuinely invalid types. They'll be collected
                // in Pass 1 with proper error diagnostics.
                worklist.push_front(item);
                break;
            }

            let succeeded = match &item {
                Item::Newtype(nt, imports) => {
                    self.current_file_imports = imports;
                    self.try_collect_newtype(nt)
                }
                Item::TypeAlias(ta, imports) => {
                    self.current_file_imports = imports;
                    self.try_collect_type_alias(ta)
                }
            };

            if succeeded {
                failures = 0;
            } else {
                failures += 1;
                worklist.push_back(item);
            }
        }
    }

    fn collect_module(mut self, files: &[&'a SourceFile]) -> Registry {
        self.package_registry
            .register_package(self.package_path.clone());

        // Pass 0: Pre-register all type and trait names (enables forward references)
        // Set current_file_imports so that generic class pre-registration can resolve
        // constructor parameter types that reference imported external types.
        for file in files {
            self.current_file_imports = &file.imports;
            for decl in &file.declarations {
                match decl {
                    Declaration::Record(rec) => self.pre_register_record(rec),
                    Declaration::Enum(e) => self.pre_register_enum(e),
                    Declaration::Trait(t) => self.pre_register_trait(t),
                    Declaration::Newtype(nt) => self.pre_register_newtype(nt),
                    Declaration::TypeAlias(ta) => self.pre_register_type_alias(ta),
                    Declaration::Class(class) => self.pre_register_class(class),
                    _ => {}
                }
            }
        }

        // Pass 0.5: Resolve non-generic newtypes and type aliases before other types.
        // These are "transparent" types whose resolved form is embedded in the Type value
        // (unlike records/enums which are just references). If collected in the same pass
        // as records, declaration order can cause records to capture stale placeholders.
        // Uses a worklist with fixpoint: retry unresolved items until no more progress.
        self.resolve_transparent_types(files);

        // Pass 1a: collect traits (own members + raw supers). Traits go first
        // so Pass 1b can flatten `extends` hierarchies before anything (class
        // `implements` completeness in particular) reads trait member sets.
        self.collect_traits_in_dependency_order(files);

        // Pass 1b: flatten trait hierarchies (cycle detection, the
        // interface-extends-only-interfaces rule, §1.2 same-name merges).
        self.flatten_traits();

        // Pass 1c: collect the remaining type definitions (records, enums,
        // newtypes, type aliases, classes). Newtypes and type aliases resolved
        // in Pass 0.5 get harmlessly re-registered here; any that couldn't be
        // resolved (circular deps or genuinely invalid) are collected here
        // with proper error diagnostics.
        for file in files {
            self.current_file_imports = &file.imports;
            for decl in &file.declarations {
                match decl {
                    Declaration::Record(rec) => self.collect_record(rec),
                    Declaration::Enum(e) => self.collect_enum(e),
                    Declaration::Trait(_) => {}
                    Declaration::Newtype(nt) => self.collect_newtype(nt),
                    Declaration::TypeAlias(ta) => self.collect_type_alias(ta),
                    Declaration::Class(class) => self.collect_class(class),
                    _ => {}
                }
            }
        }

        // Pass 2: collect functions, modules, extensions, implementations, globals
        let mut unresolved_globals = Vec::new();
        for file in files {
            self.current_file_imports = &file.imports;
            for decl in &file.declarations {
                match decl {
                    Declaration::Function(func) => self.collect_function(func),
                    Declaration::GlobalVar(global) => {
                        if let Some(unresolved) = self.collect_global(global) {
                            unresolved_globals.push(unresolved);
                        }
                    }
                    Declaration::Extension(ext) => self.collect_extension(ext),
                    Declaration::Implement(impl_decl) => self.collect_implement(impl_decl),
                    Declaration::Module(module) => self.collect_module_decl(module),
                    _ => {}
                }
            }
        }

        // Pass 3: fixpoint inference on untyped globals
        self.resolve_untyped_globals(unresolved_globals);

        // Public class fields may use operator implementations from this package.
        // Their signatures are now available, so finish deferred field inference.
        for file in files {
            self.current_file_imports = &file.imports;
            for declaration in &file.declarations {
                if let Declaration::Class(class) = declaration {
                    self.resolve_class_field_types(class);
                }
            }
        }

        self.package_registry
    }
}

/// Collect phase: walk the AST and register all declarations into a new package registry.
///
/// The dependency registry (from previous packages) is read-only and used for type resolution.
/// Returns a registry containing only this package's declarations.
pub fn collect(
    package_path: &PackagePath,
    files: &[&SourceFile],
    dependency_registry: &Registry,
    diagnostics: &mut Diagnostics,
) -> Registry {
    let collector = Collector::new(package_path.clone(), dependency_registry, diagnostics);
    collector.collect_module(files)
}
