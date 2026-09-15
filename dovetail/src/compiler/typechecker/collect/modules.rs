use std::collections::BTreeMap;

use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName, Variance};
use crate::parser::ast::{Expr, ModuleDecl};

use crate::typechecker::registry::{
    FunctionSignature, GenericModuleGlobalDef, GenericModuleMemberDef, GenericModuleMembers,
    GlobalSignature, ModuleInfo,
};
use crate::typechecker::types::{TraitBound, TraitBounds, Type};

use super::Collector;

impl Collector<'_> {
    /// Resolve a module's associated type from both the registry and intrinsic types.
    /// Returns `Some(Type)` if the module name matches a registered type or a primitive type.
    fn resolve_module_associated_type(&self, module_fqn: &Fqn) -> Option<Type> {
        if let Some(ty) = self.package_registry.lookup_type_by_fqn(module_fqn) {
            return Some(ty.clone());
        }
        if let Some(ty) = self.dependency_registry.lookup_type_by_fqn(module_fqn) {
            return Some(ty.clone());
        }
        if let Some(ty) = Type::from_primitive(&module_fqn.symbol.0) {
            return Some(ty);
        }
        // Check if the name resolves to a trait → return InterfaceObject
        if self
            .package_registry
            .lookup_trait(module_fqn, &self.package_path)
            .is_some()
            || self
                .dependency_registry
                .lookup_trait(module_fqn, &self.package_path)
                .is_some()
        {
            return Some(Type::interface_object(module_fqn.clone(), vec![]));
        }
        None
    }

    /// Collect a module declaration: register its members with module-qualified FQN symbols.
    ///
    /// Each member gets a FQN like `{ package: "a", symbol: "Math.double" }`.
    /// Members are also stored directly in `ModuleInfo` for resolution.
    ///
    /// If the module name matches an existing type in the same package, the module
    /// is a "module for a type" and instance members (with `self`) are allowed.
    pub(super) fn collect_module_decl(&mut self, module: &ModuleDecl) {
        if !module.type_params.is_empty() {
            self.collect_generic_module_decl(module);
            return;
        }

        let module_name = &module.name.value;

        // Build module FQN: package_path + module_name
        let module_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(module_name.clone()),
        };

        if let Some(doc) = &module.doc_comment {
            self.package_registry
                .register_doc_comment(module_fqn.clone(), doc.clone());
        }

        // Check for duplicate module in the same package
        if self
            .package_registry
            .has_module_in_package(&self.package_path, module_name)
        {
            self.diagnostics.error(
                module.name.span.clone(),
                format!(
                    "duplicate module '{}' in package '{}'",
                    module_name, self.package_path
                ),
            );
            return;
        }

        // If the module name matches a generic record, generic enum, generic trait, or intrinsic generic type, error: requires type parameters
        if self
            .package_registry
            .lookup_generic_record_by_fqn(&module_fqn, &self.package_path)
            .is_some()
            || module_name == "Array"
        {
            self.diagnostics.error(
                module.name.span.clone(),
                format!(
                    "module '{}' is for a generic type and requires type parameters",
                    module_name
                ),
            );
            return;
        }

        // Check for generic traits (traits with type_params) that require type parameters
        if let Some(trait_sig) = self
            .package_registry
            .lookup_trait(&module_fqn, &self.package_path)
            .or_else(|| {
                self.dependency_registry
                    .lookup_trait(&module_fqn, &self.package_path)
            })
            && !trait_sig.type_params.is_empty()
        {
            self.diagnostics.error(
                module.name.span.clone(),
                format!(
                    "module '{}' is for a generic trait and requires type parameters",
                    module_name
                ),
            );
            return;
        }

        // Detect module-for-type: does a type with the same FQN exist (including intrinsics)?
        let is_for_type = self.resolve_module_associated_type(&module_fqn).is_some();

        let mut member_functions: BTreeMap<SymbolName, Vec<FunctionSignature>> = BTreeMap::new();
        let mut member_globals: BTreeMap<SymbolName, GlobalSignature> = BTreeMap::new();
        let mut generic_members = GenericModuleMembers::new();

        // Collect functions
        for func in &module.functions {
            let has_self = !func.params.is_empty() && func.params[0].name.value == "self";

            // Reject `self` parameters in standalone modules
            if has_self && !is_for_type {
                self.diagnostics.error(
                    func.name.span.clone(),
                    format!(
                        "'self' parameter not allowed in standalone module function '{}'",
                        func.name.value
                    ),
                );
                continue;
            }

            // Generic function in non-generic module: collect as GenericModuleMemberDef
            if !func.type_params.is_empty() {
                let method_type_params: Vec<TypeParamName> = func
                    .type_params
                    .iter()
                    .map(|tp| TypeParamName(tp.value.clone()))
                    .collect();
                let method_trait_bounds =
                    self.resolve_trait_bounds(&func.where_clause, &method_type_params);
                let method_type_params_map =
                    Type::type_param_map(&method_type_params, &method_trait_bounds);

                let return_type = match &func.return_type {
                    Some(type_expr) => {
                        self.resolve_type_expr_with_type_params(type_expr, &method_type_params_map)
                    }
                    None => Type::Unit,
                };

                let params: Vec<(String, Type)> = func
                    .params
                    .iter()
                    .map(|p| {
                        let ty = self.resolve_type_expr_with_type_params(
                            &p.type_annotation,
                            &method_type_params_map,
                        );
                        (p.name.value.clone(), ty)
                    })
                    .collect();

                // Validate self type matches associated type for module-for-type
                if has_self {
                    let self_type = &params[0].1;
                    if let Some(expected_type) = self.resolve_module_associated_type(&module_fqn) {
                        if *self_type != expected_type {
                            self.diagnostics.error(
                                func.name.span.clone(),
                                format!(
                                    "'self' parameter type mismatch in module function '{}': expected '{}', found '{}'",
                                    func.name.value, expected_type, self_type
                                ),
                            );
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                let for_type = if is_for_type {
                    match self.resolve_module_associated_type(&module_fqn) {
                        Some(ty) => ty,
                        None => continue,
                    }
                } else {
                    Type::Unit
                };

                let member_name = SymbolName(func.name.value.clone());
                generic_members.add(
                    member_name,
                    GenericModuleMemberDef {
                        visibility: func.visibility,
                        type_params: vec![],
                        for_type,
                        params,
                        return_type,
                        body: func.body.clone(),
                        is_intrinsic: matches!(func.body, Expr::Intrinsic(_)),
                        is_property: false,
                        source_file: func.name.span.file.clone(),
                        package: self.package_path.clone(),
                        method_type_params,
                        is_async: func.is_async,
                        trait_bounds: method_trait_bounds,
                    },
                );
                continue;
            }

            self.resolve_trait_bounds(&func.where_clause, &[]);

            let return_type = match &func.return_type {
                Some(type_expr) => self.resolve_type_expr(type_expr),
                None => Type::Unit,
            };

            let params: Vec<(String, Type)> = func
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr(&p.type_annotation);
                    (p.name.value.clone(), ty)
                })
                .collect();

            // Validate self type matches associated type for module-for-type
            if has_self {
                let self_type = &params[0].1;
                if let Some(expected_type) = self.resolve_module_associated_type(&module_fqn) {
                    if *self_type != expected_type {
                        self.diagnostics.error(
                            func.name.span.clone(),
                            format!(
                                "'self' parameter type mismatch in module function '{}': expected '{}', found '{}'",
                                func.name.value, expected_type, self_type
                            ),
                        );
                        continue;
                    }
                } else {
                    continue;
                }
            }

            // Module-qualified FQN: e.g. { package: "a", symbol: "Math.double" }
            let qualified_symbol = SymbolName(format!("{}.{}", module_name, func.name.value));
            let fqn = Fqn {
                package: self.package_path.clone(),
                symbol: qualified_symbol,
            };

            let param_types: Vec<&Type> = params.iter().map(|(_, ty)| ty).collect();
            let mangled_name = MangledName::for_function(&fqn, &param_types);

            let sig = FunctionSignature {
                visibility: func.visibility,
                mangled_name,
                params,
                return_type,
                source_file: func.name.span.file.clone(),
                is_intrinsic: matches!(func.body, Expr::Intrinsic(_)),
                is_property: false,
                is_final_method: false,
                is_abstract_method: false,
            };

            // Register in main Registry for codegen
            let registered = self.package_registry.register_function(fqn, sig.clone());
            if !registered {
                self.diagnostics.error(
                    func.name.span.clone(),
                    format!("duplicate function: '{}'", func.name.value),
                );
            }

            // Store in ModuleInfo for resolution
            member_functions
                .entry(SymbolName(func.name.value.clone()))
                .or_default()
                .push(sig);
        }

        // Collect module properties (registered as property functions)
        for property in &module.properties {
            let has_self = !property.params.is_empty() && property.params[0].name.value == "self";

            // Reject `self` parameters in standalone module properties
            if has_self && !is_for_type {
                self.diagnostics.error(
                    property.name.span.clone(),
                    format!(
                        "'self' parameter not allowed in standalone module property '{}'",
                        property.name.value
                    ),
                );
                continue;
            }

            // Generic property in non-generic module: collect as GenericModuleMemberDef
            if !property.type_params.is_empty() {
                let method_type_params: Vec<TypeParamName> = property
                    .type_params
                    .iter()
                    .map(|tp| TypeParamName(tp.value.clone()))
                    .collect();
                let method_type_params_map =
                    Type::type_param_map(&method_type_params, &TraitBounds::empty());

                let return_type = self.resolve_type_expr_with_type_params(
                    &property.return_type,
                    &method_type_params_map,
                );

                let params: Vec<(String, Type)> = property
                    .params
                    .iter()
                    .map(|p| {
                        let ty = self.resolve_type_expr_with_type_params(
                            &p.type_annotation,
                            &method_type_params_map,
                        );
                        (p.name.value.clone(), ty)
                    })
                    .collect();

                if has_self {
                    let self_type = &params[0].1;
                    if let Some(expected_type) = self.resolve_module_associated_type(&module_fqn) {
                        if *self_type != expected_type {
                            self.diagnostics.error(
                                property.name.span.clone(),
                                format!(
                                    "'self' parameter type mismatch in module property '{}': expected '{}', found '{}'",
                                    property.name.value, expected_type, self_type
                                ),
                            );
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                let for_type = if is_for_type {
                    match self.resolve_module_associated_type(&module_fqn) {
                        Some(ty) => ty,
                        None => continue,
                    }
                } else {
                    Type::Unit
                };

                let body = match &property.body {
                    Some(b) => b.clone(),
                    None => continue,
                };

                let member_name = SymbolName(property.name.value.clone());
                generic_members.add(
                    member_name,
                    GenericModuleMemberDef {
                        visibility: property.visibility,
                        type_params: vec![],
                        for_type,
                        params,
                        return_type,
                        is_intrinsic: matches!(body, Expr::Intrinsic(_)),
                        body,
                        is_property: true,
                        source_file: property.name.span.file.clone(),
                        package: self.package_path.clone(),
                        method_type_params,
                        is_async: false,
                        // A property has no `where` clause to carry: see
                        // `PropertyDecl`, which has no such field.
                        trait_bounds: TraitBounds::empty(),
                    },
                );
                continue;
            }

            let return_type = self.resolve_type_expr(&property.return_type);

            // Resolve params (may include self for instance properties)
            let params: Vec<(String, Type)> = property
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr(&p.type_annotation);
                    (p.name.value.clone(), ty)
                })
                .collect();

            // Validate self type matches associated type for module-for-type
            if has_self {
                let self_type = &params[0].1;
                if let Some(expected_type) = self.resolve_module_associated_type(&module_fqn) {
                    if *self_type != expected_type {
                        self.diagnostics.error(
                            property.name.span.clone(),
                            format!(
                                "'self' parameter type mismatch in module property '{}': expected '{}', found '{}'",
                                property.name.value, expected_type, self_type
                            ),
                        );
                        continue;
                    }
                } else {
                    continue;
                }
            }

            let qualified_symbol = SymbolName(format!("{}.{}", module_name, property.name.value));
            let fqn = Fqn {
                package: self.package_path.clone(),
                symbol: qualified_symbol,
            };

            let param_types: Vec<&Type> = params.iter().map(|(_, ty)| ty).collect();
            let mangled_name = if params.is_empty() {
                MangledName::for_function_no_params(&fqn)
            } else {
                MangledName::for_function(&fqn, &param_types)
            };

            let is_intrinsic = property
                .body
                .as_ref()
                .is_some_and(|b| matches!(b, Expr::Intrinsic(_)));
            let sig = FunctionSignature {
                visibility: property.visibility,
                mangled_name,
                params,
                return_type,
                source_file: property.name.span.file.clone(),
                is_intrinsic,
                is_property: true,
                is_final_method: false,
                is_abstract_method: false,
            };

            let registered = self.package_registry.register_function(fqn, sig.clone());
            if !registered {
                self.diagnostics.error(
                    property.name.span.clone(),
                    format!("duplicate module property: '{}'", property.name.value),
                );
            }

            member_functions
                .entry(SymbolName(property.name.value.clone()))
                .or_default()
                .push(sig);
        }

        // Collect globals
        for global in &module.globals {
            let qualified_symbol = SymbolName(format!("{}.{}", module_name, global.name.value));
            let fqn = Fqn {
                package: self.package_path.clone(),
                symbol: qualified_symbol,
            };
            let mangled_name = MangledName::for_global(&fqn);
            let source_file = global.name.span.file.clone();

            if let Some(type_expr) = &global.type_annotation {
                let ty = self.resolve_type_expr(type_expr);
                let sig = GlobalSignature {
                    visibility: global.visibility,
                    mangled_name,
                    ty,
                    mutable: global.mutable,
                    source_file,
                };
                let registered = self.package_registry.register_global(fqn, sig.clone());
                if !registered {
                    self.diagnostics.error(
                        global.name.span.clone(),
                        format!("duplicate global: '{}'", global.name.value),
                    );
                }
                member_globals.insert(SymbolName(global.name.value.clone()), sig);
            } else {
                // Reuse the existing try_infer_expr_type for untyped globals
                if let Some(ty) = self.try_infer_expr_type(&global.value, &source_file) {
                    let sig = GlobalSignature {
                        visibility: global.visibility,
                        mangled_name,
                        ty,
                        mutable: global.mutable,
                        source_file,
                    };
                    let registered = self.package_registry.register_global(fqn, sig.clone());
                    if !registered {
                        self.diagnostics.error(
                            global.name.span.clone(),
                            format!("duplicate global: '{}'", global.name.value),
                        );
                    }
                    member_globals.insert(SymbolName(global.name.value.clone()), sig);
                } else {
                    self.diagnostics.error(
                        global.name.span.clone(),
                        format!(
                            "module global '{}' requires a type annotation",
                            global.name.value
                        ),
                    );
                }
            }
        }

        // Register the module itself
        let source_file = module.name.span.file.clone();
        self.package_registry.register_module(
            module_fqn,
            ModuleInfo {
                fqn: Fqn {
                    package: self.package_path.clone(),
                    symbol: SymbolName(module_name.clone()),
                },
                functions: member_functions,
                globals: member_globals,
                generic_globals: BTreeMap::new(),
                generic_members,
                type_param_variances: vec![],
                trait_bounds: TraitBounds::empty(),
                source_file,
            },
        );
    }

    /// Collect a generic module declaration: `module Box<T> = ...`
    /// Functions and properties are stored as GenericModuleMemberDefs for deferred inference.
    /// Globals are collected concretely (no type params in their types).
    fn collect_generic_module_decl(&mut self, module: &ModuleDecl) {
        let module_name = &module.name.value;

        // Build type params
        let type_params: Vec<TypeParamName> = module
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        // Build module FQN (type_params_map is built after we know record_trait_bounds)
        let module_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(module_name.clone()),
        };

        if let Some(doc) = &module.doc_comment {
            self.package_registry
                .register_doc_comment(module_fqn.clone(), doc.clone());
        }

        // Check for duplicate module
        if self
            .package_registry
            .has_module_in_package(&self.package_path, module_name)
        {
            self.diagnostics.error(
                module.name.span.clone(),
                format!(
                    "duplicate module '{}' in package '{}'",
                    module_name, self.package_path
                ),
            );
            return;
        }

        // Generic modules must be for a generic type (including intrinsic Array)
        let (for_type, expected_type_param_count, record_trait_bounds, module_type_param_variances) =
            if module_name == "Array" {
                if type_params.len() != 1 {
                    self.diagnostics.error(
                        module.name.span.clone(),
                        format!(
                            "'Array' requires exactly 1 type parameter, found {}",
                            type_params.len()
                        ),
                    );
                    return;
                }
                let record_trait_bounds = TraitBounds::empty();
                let bounds: Vec<TraitBound> = record_trait_bounds
                    .get(&type_params[0])
                    .cloned()
                    .unwrap_or_default();
                let for_type =
                    Type::Array(Box::new(Type::TypeVariable(type_params[0].clone(), bounds)));
                (
                    for_type,
                    1usize,
                    record_trait_bounds,
                    vec![Variance::Covariant],
                )
            } else if let Some(record_sig) = self
                .package_registry
                .lookup_generic_record_by_fqn(&module_fqn, &self.package_path)
                .cloned()
            {
                let record_trait_bounds = record_sig.trait_bounds.clone();
                let variances = record_sig.type_param_variances.clone();
                let type_args: Vec<(Variance, Type)> = type_params
                    .iter()
                    .enumerate()
                    .map(|(i, tp)| {
                        let bounds: Vec<TraitBound> =
                            record_trait_bounds.get(tp).cloned().unwrap_or_default();
                        let variance = record_sig
                            .type_param_variances
                            .get(i)
                            .copied()
                            .unwrap_or(Variance::Invariant);
                        (variance, Type::TypeVariable(tp.clone(), bounds))
                    })
                    .collect();
                let for_type = Type::GenericRecord {
                    fqn: module_fqn.clone(),
                    mangled_name: MangledName::for_type(&module_fqn),
                    type_args,
                };
                (
                    for_type,
                    record_sig.type_params.len(),
                    record_trait_bounds,
                    variances,
                )
            } else if let Some(enum_sig) = self
                .package_registry
                .lookup_generic_enum_by_fqn(&module_fqn, &self.package_path)
                .cloned()
            {
                let enum_trait_bounds = enum_sig.trait_bounds.clone();
                let variances = enum_sig.type_param_variances.clone();
                let type_args: Vec<(Variance, Type)> = type_params
                    .iter()
                    .enumerate()
                    .map(|(i, tp)| {
                        let bounds: Vec<TraitBound> =
                            enum_trait_bounds.get(tp).cloned().unwrap_or_default();
                        let variance = enum_sig
                            .type_param_variances
                            .get(i)
                            .copied()
                            .unwrap_or(Variance::Invariant);
                        (variance, Type::TypeVariable(tp.clone(), bounds))
                    })
                    .collect();
                let for_type = Type::GenericEnum {
                    fqn: module_fqn.clone(),
                    mangled_name: MangledName::for_type(&module_fqn),
                    type_args,
                };
                (
                    for_type,
                    enum_sig.type_params.len(),
                    enum_trait_bounds,
                    variances,
                )
            } else if let Some(newtype_sig) = self
                .package_registry
                .lookup_generic_newtype_by_fqn(&module_fqn, &self.package_path)
                .cloned()
                .or_else(|| {
                    self.dependency_registry
                        .lookup_generic_newtype_by_fqn(&module_fqn, &self.package_path)
                        .cloned()
                })
            {
                let newtype_trait_bounds = newtype_sig.trait_bounds.clone();
                let variances = newtype_sig.type_param_variances.clone();
                let type_args: Vec<(Variance, Type)> = type_params
                    .iter()
                    .enumerate()
                    .map(|(i, tp)| {
                        let bounds: Vec<TraitBound> =
                            newtype_trait_bounds.get(tp).cloned().unwrap_or_default();
                        let variance = newtype_sig
                            .type_param_variances
                            .get(i)
                            .copied()
                            .unwrap_or(Variance::Invariant);
                        (variance, Type::TypeVariable(tp.clone(), bounds))
                    })
                    .collect();
                // Compute concrete_inner_type by substituting type params in the inner type template.
                let inner_subst: std::collections::BTreeMap<String, Type> = newtype_sig
                    .type_params
                    .iter()
                    .zip(type_args.iter())
                    .map(|(tp, (_, ty))| (tp.0.clone(), ty.clone()))
                    .collect();
                let concrete_inner =
                    super::types::substitute_type_params_in(&newtype_sig.inner_type, &inner_subst);
                let for_type = Type::GenericNewtype {
                    fqn: module_fqn.clone(),
                    type_args,
                    concrete_inner_type: Box::new(concrete_inner),
                };
                (
                    for_type,
                    newtype_sig.type_params.len(),
                    newtype_trait_bounds,
                    variances,
                )
            } else if let Some(class_sig) = self
                .package_registry
                .lookup_class_type(&module_fqn, &self.package_path)
                .cloned()
                .or_else(|| {
                    self.dependency_registry
                        .lookup_class_type(&module_fqn, &self.package_path)
                        .cloned()
                })
                .filter(|sig| !sig.type_params.is_empty())
            {
                let class_trait_bounds = class_sig.trait_bounds.clone();
                let variances = class_sig.type_param_variances.clone();
                let type_args: Vec<(Variance, Type)> = type_params
                    .iter()
                    .enumerate()
                    .map(|(i, tp)| {
                        let bounds: Vec<TraitBound> =
                            class_trait_bounds.get(tp).cloned().unwrap_or_default();
                        let variance = class_sig
                            .type_param_variances
                            .get(i)
                            .copied()
                            .unwrap_or(Variance::Invariant);
                        (variance, Type::TypeVariable(tp.clone(), bounds))
                    })
                    .collect();
                let for_type = Type::GenericClass {
                    fqn: module_fqn.clone(),
                    mangled_name: MangledName::for_type(&module_fqn),
                    type_args,
                };
                (
                    for_type,
                    class_sig.type_params.len(),
                    class_trait_bounds,
                    variances,
                )
            } else if let Some(trait_sig) = self
                .package_registry
                .lookup_trait(&module_fqn, &self.package_path)
                .cloned()
                .or_else(|| {
                    self.dependency_registry
                        .lookup_trait(&module_fqn, &self.package_path)
                        .cloned()
                })
            {
                // Generic trait module: build InterfaceObject for_type
                let trait_type_args: Vec<Type> = type_params
                    .iter()
                    .map(|tp| Type::TypeVariable(tp.clone(), vec![]))
                    .collect();
                let for_type = Type::interface_object(module_fqn.clone(), trait_type_args);
                let variances = vec![Variance::Invariant; trait_sig.type_params.len()];
                (
                    for_type,
                    trait_sig.type_params.len(),
                    TraitBounds::empty(),
                    variances,
                )
            } else {
                self.diagnostics.error(
                    module.name.span.clone(),
                    format!(
                        "generic module '{}' must be for a generic type",
                        module_name
                    ),
                );
                return;
            };

        // Validate type param count matches
        if type_params.len() != expected_type_param_count {
            self.diagnostics.error(
                module.name.span.clone(),
                format!(
                    "module '{}' has {} type parameters, but type '{}' has {}",
                    module_name,
                    type_params.len(),
                    module_name,
                    expected_type_param_count
                ),
            );
            return;
        }

        let type_params_map = Type::type_param_map(&type_params, &record_trait_bounds);

        let mut generic_members = GenericModuleMembers::new();
        let mut member_generic_globals: BTreeMap<SymbolName, GenericModuleGlobalDef> =
            BTreeMap::new();

        // Collect functions as GenericModuleMemberDefs
        for func in &module.functions {
            let has_self = !func.params.is_empty() && func.params[0].name.value == "self";

            // Build method-level type params (if any)
            let method_type_params: Vec<TypeParamName> = func
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();

            // Resolve method-level trait bounds from where clause
            let all_func_type_params: Vec<TypeParamName> = type_params
                .iter()
                .chain(method_type_params.iter())
                .cloned()
                .collect();
            let method_trait_bounds = self.resolve_method_trait_bounds(
                &func.where_clause,
                &all_func_type_params,
                &method_type_params,
                &record_trait_bounds,
            );

            let mut combined_bounds = record_trait_bounds.clone();
            combined_bounds.merge(&method_trait_bounds);
            let combined_type_params_map =
                Type::type_param_map(&all_func_type_params, &combined_bounds);

            let return_type = match &func.return_type {
                Some(type_expr) => {
                    self.resolve_type_expr_with_type_params(type_expr, &combined_type_params_map)
                }
                None => Type::Unit,
            };

            let params: Vec<(String, Type)> = func
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr_with_type_params(
                        &p.type_annotation,
                        &combined_type_params_map,
                    );
                    (p.name.value.clone(), ty)
                })
                .collect();

            // Validate self type matches for_type
            if has_self && !crate::typechecker::subtyping::identical(&params[0].1, &for_type) {
                self.diagnostics.error(
                    func.name.span.clone(),
                    format!(
                        "type of 'self' parameter must be '{}', found '{}'",
                        for_type, params[0].1
                    ),
                );
                continue;
            }

            let member_name = SymbolName(func.name.value.clone());
            generic_members.add(
                member_name,
                GenericModuleMemberDef {
                    visibility: func.visibility,
                    type_params: type_params.clone(),
                    for_type: for_type.clone(),
                    params,
                    return_type,
                    body: func.body.clone(),
                    is_intrinsic: matches!(func.body, Expr::Intrinsic(_)),
                    is_property: false,
                    source_file: func.name.span.file.clone(),
                    package: self.package_path.clone(),
                    method_type_params,
                    is_async: func.is_async,
                    trait_bounds: method_trait_bounds,
                },
            );
        }

        // Collect properties as GenericModuleMemberDefs
        for property in &module.properties {
            let has_self = !property.params.is_empty() && property.params[0].name.value == "self";

            // Build property-level type params (if any)
            let property_type_params: Vec<TypeParamName> = property
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();

            // Build combined type_params_map (module-level + property-level) for type resolution
            let combined_type_params_map: BTreeMap<String, Type> =
                if property_type_params.is_empty() {
                    type_params_map.clone()
                } else {
                    let mut combined = type_params_map.clone();
                    for tp in &property_type_params {
                        combined.insert(tp.0.clone(), Type::TypeVariable(tp.clone(), vec![]));
                    }
                    combined
                };

            let return_type = self.resolve_type_expr_with_type_params(
                &property.return_type,
                &combined_type_params_map,
            );

            let params: Vec<(String, Type)> = property
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr_with_type_params(
                        &p.type_annotation,
                        &combined_type_params_map,
                    );
                    (p.name.value.clone(), ty)
                })
                .collect();

            // Validate self type matches for_type
            if has_self && !crate::typechecker::subtyping::identical(&params[0].1, &for_type) {
                self.diagnostics.error(
                    property.name.span.clone(),
                    format!(
                        "type of 'self' parameter must be '{}', found '{}'",
                        for_type, params[0].1
                    ),
                );
                continue;
            }

            let body = match &property.body {
                Some(b) => b.clone(),
                None => continue,
            };
            let prop_name = SymbolName(property.name.value.clone());
            generic_members.add(
                prop_name,
                GenericModuleMemberDef {
                    visibility: property.visibility,
                    type_params: type_params.clone(),
                    for_type: for_type.clone(),
                    params,
                    return_type,
                    is_intrinsic: matches!(body, Expr::Intrinsic(_)),
                    body,
                    is_property: true,
                    source_file: property.name.span.file.clone(),
                    package: self.package_path.clone(),
                    method_type_params: property_type_params,
                    is_async: false,
                    trait_bounds: TraitBounds::empty(),
                },
            );
        }

        // Collect globals with type params in scope as GenericModuleGlobalDef
        for global in &module.globals {
            let source_file = global.name.span.file.clone();

            let ty = if let Some(type_expr) = &global.type_annotation {
                self.resolve_type_expr_with_type_params(type_expr, &type_params_map)
            } else if let Some(ty) = self.try_infer_expr_type(&global.value, &source_file) {
                ty
            } else {
                self.diagnostics.error(
                    global.name.span.clone(),
                    format!(
                        "cannot infer type of module global '{}'; add a type annotation",
                        global.name.value
                    ),
                );
                Type::Error
            };

            member_generic_globals.insert(
                SymbolName(global.name.value.clone()),
                GenericModuleGlobalDef {
                    visibility: global.visibility,
                    type_params: type_params.clone(),
                    ty,
                    mutable: global.mutable,
                    body: global.value.clone(),
                    source_file,
                    package: self.package_path.clone(),
                },
            );
        }

        // Register the module
        let source_file = module.name.span.file.clone();
        self.package_registry.register_module(
            module_fqn.clone(),
            ModuleInfo {
                fqn: module_fqn,
                functions: BTreeMap::new(),
                globals: BTreeMap::new(),
                generic_globals: member_generic_globals,
                generic_members,
                type_param_variances: module_type_param_variances,
                trait_bounds: record_trait_bounds,
                source_file,
            },
        );
    }
}
