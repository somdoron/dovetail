use crate::common::types::{Fqn, SymbolName, TypeParamName};
use std::collections::{BTreeMap, BTreeSet};

use crate::parser::ast::{Declaration, ImportDecl, SourceFile, TraitDecl};
use crate::typechecker::registry::{AssociatedTypeSig, TraitMethodSig, TraitPropertySig, TraitSignature, TraitSuperRef};
use crate::typechecker::types::{TraitBounds, Type};

use super::Collector;

impl<'a> Collector<'a> {
    /// Resolve parent declarations first so inherited associated type names
    /// are in scope even when a child appears earlier in the source files.
    pub(super) fn collect_traits_in_dependency_order(&mut self, files: &[&'a SourceFile]) {
        let mut declarations = BTreeMap::new();
        let mut declaration_order = Vec::new();
        let mut duplicates = Vec::new();
        for file in files {
            for decl in &file.declarations {
                let Declaration::Trait(decl) = decl else { continue };
                let fqn = Fqn {
                    package: self.package_path.clone(),
                    symbol: SymbolName(decl.name.value.clone()),
                };
                match declarations.entry(fqn) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        declaration_order.push(entry.key().clone());
                        entry.insert((decl, file.imports.as_slice()));
                    }
                    std::collections::btree_map::Entry::Occupied(_) => {
                        duplicates.push((decl, file.imports.as_slice()));
                    }
                }
            }
        }
        let mut visited = BTreeSet::new();
        // Preserve source order for unrelated traits: their method bounds can
        // refer to associated types declared by an earlier trait.
        for fqn in declaration_order {
            self.collect_trait_after_supers(&fqn, &declarations, &mut visited);
        }
        // Visit duplicates too, preserving their member diagnostics and the
        // duplicate-declaration error emitted by register_trait.
        for (decl, imports) in duplicates {
            self.current_file_imports = imports;
            self.collect_trait(decl);
        }
    }

    fn collect_trait_after_supers(
        &mut self,
        fqn: &Fqn,
        declarations: &BTreeMap<Fqn, (&'a TraitDecl, &'a [ImportDecl])>,
        visited: &mut BTreeSet<Fqn>,
    ) {
        // The flattening pass diagnoses cycles; this set only bounds traversal.
        if !visited.insert(fqn.clone()) {
            return;
        }
        let Some(&(decl, imports)) = declarations.get(fqn) else { return };
        for super_ref in &decl.supers {
            self.current_file_imports = imports;
            if let Some((super_fqn, _)) = self.resolve_trait(&super_ref.name.value) {
                self.collect_trait_after_supers(&super_fqn, declarations, visited);
            }
        }
        self.current_file_imports = imports;
        self.collect_trait(decl);
    }
}

impl Collector<'_> {
    /// Pre-register a trait name so that forward references resolve.
    /// Called in Pass 0 before full collection.
    pub(super) fn pre_register_trait(&mut self, trait_decl: &TraitDecl) {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(trait_decl.name.value.clone()),
        };
        let type_params: Vec<TypeParamName> = trait_decl
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        self.package_registry.pre_register_trait(fqn, type_params, trait_decl.is_interface);
    }

    pub(super) fn collect_trait(&mut self, trait_decl: &TraitDecl) {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(trait_decl.name.value.clone()),
        };

        if let Some(doc) = &trait_decl.doc_comment {
            self.package_registry.register_doc_comment(fqn.clone(), doc.clone());
        }

        // Collect trait-level type param names
        let trait_type_params: Vec<TypeParamName> = trait_decl
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();

        // Build trait-level type params map: "Self" + trait type params (e.g. <T>)
        let mut trait_type_params_map = Type::type_param_map(&trait_type_params, &TraitBounds::empty());
        trait_type_params_map.insert("Self".to_string(), Type::SelfType);

        // Collect associated types and add them to the type params map
        let mut associated_type_sigs = Vec::new();
        let mut seen_names = std::collections::BTreeSet::new();

        for assoc_type in &trait_decl.associated_types {
            if !seen_names.insert(assoc_type.name.value.clone()) {
                self.diagnostics.error(
                    assoc_type.name.span.clone(),
                    format!(
                        "duplicate associated type '{}' in trait '{}'",
                        assoc_type.name.value, trait_decl.name.value
                    ),
                );
                continue;
            }
            let gat_type_params: Vec<TypeParamName> = assoc_type.type_params.iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();
            if let Some(doc) = &assoc_type.doc_comment {
                self.package_registry.register_sub_doc_comment(fqn.clone(), assoc_type.name.value.clone(), doc.clone());
            }
            associated_type_sigs.push(AssociatedTypeSig {
                name: assoc_type.name.value.clone(),
                span: assoc_type.name.span.clone(),
                type_params: gat_type_params,
                origin: None,
            });
            // Add associated type as a type parameter so method signatures can reference it.
            // For GATs, resolve_type_expr_with_type_params will create TypeConstructor
            // when type_args are present.
            trait_type_params_map.insert(
                assoc_type.name.value.clone(),
                Type::TypeVariable(TypeParamName(assoc_type.name.value.clone()), vec![]),
            );
        }

        // Resolve the `extends` clause. Only names and arity are validated
        // here; the interface-extends-only-interfaces rule, cycle detection,
        // and member flattening run in Pass 1b (`flatten_traits`), once every
        // trait's own members are collected.
        let mut supers: Vec<TraitSuperRef> = Vec::new();
        for super_ref in &trait_decl.supers {
            let Some((super_fqn, super_sig)) = self.resolve_trait(&super_ref.name.value) else {
                self.diagnostics.error(
                    super_ref.name.span.clone(),
                    format!("unknown trait: '{}'", super_ref.name.value),
                );
                continue;
            };
            if crate::typechecker::types::is_tuple_constraint(&super_fqn) {
                self.diagnostics.error(super_ref.span.clone(), "Tuple is a built-in structural constraint; use a where bound instead of trait inheritance");
                continue;
            }
            if super_fqn == fqn {
                self.diagnostics.error(
                    super_ref.name.span.clone(),
                    format!("trait '{}' cannot extend itself", trait_decl.name.value),
                );
                continue;
            }
            if super_ref.type_args.len() != super_sig.type_params.len() {
                self.diagnostics.error(
                    super_ref.span.clone(),
                    format!(
                        "trait '{}' expects {} type argument(s), but {} were provided",
                        super_ref.name.value,
                        super_sig.type_params.len(),
                        super_ref.type_args.len(),
                    ),
                );
                continue;
            }
            if supers.iter().any(|existing| existing.fqn == super_fqn) {
                self.diagnostics.error(
                    super_ref.name.span.clone(),
                    format!("duplicate super trait '{}' in extends clause", super_ref.name.value),
                );
                continue;
            }
            let type_args: Vec<Type> = super_ref
                .type_args
                .iter()
                .map(|te| self.resolve_type_expr_with_type_params(te, &trait_type_params_map))
                .collect();
            supers.push(TraitSuperRef {
                fqn: super_fqn,
                type_args,
                span: super_ref.span.clone(),
            });
        }

        // Supers are collected but not flattened yet. Walk their declarations
        // to bring both direct and transitive associated type names into scope.
        let mut pending: Vec<_> = supers.iter().map(|s| s.fqn.clone()).collect();
        let mut visited = BTreeSet::new();
        while let Some(super_fqn) = pending.pop() {
            if !visited.insert(super_fqn.clone()) {
                continue;
            }
            let Some(super_sig) = self.package_registry.get_trait(&super_fqn)
                .or_else(|| self.dependency_registry.get_trait(&super_fqn)) else { continue };
            pending.extend(super_sig.supers.iter().map(|s| s.fqn.clone()));
            for assoc in &super_sig.associated_types {
                trait_type_params_map.entry(assoc.name.clone()).or_insert_with(|| {
                    Type::TypeVariable(TypeParamName(assoc.name.clone()), vec![])
                });
            }
        }

        let mut methods = Vec::new();
        let mut seen_methods = std::collections::BTreeSet::new();

        // Check associated type names don't conflict with method/property names
        for assoc in &associated_type_sigs {
            seen_methods.insert(assoc.name.clone());
        }

        for method in &trait_decl.methods {
            if !seen_methods.insert(method.name.value.clone()) {
                self.diagnostics.error(
                    method.name.span.clone(),
                    format!(
                        "duplicate method '{}' in trait '{}'",
                        method.name.value, trait_decl.name.value
                    ),
                );
            }

            // Collect method-level type param names
            let method_type_params: Vec<TypeParamName> = method
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();

            // Resolve method-level trait bounds from where clause
            let mut bound_scope = trait_type_params_map.clone();
            bound_scope.extend(Type::type_param_map(&method_type_params, &TraitBounds::empty()));
            let method_trait_bounds = self.resolve_trait_bounds_in_scope(
                &method.where_clause, &method_type_params, &bound_scope,
            );

            // Per-method type params extend the trait-level map
            let mut method_type_params_map = trait_type_params_map.clone();
            let method_tp_map = Type::type_param_map(&method_type_params, &method_trait_bounds);
            method_type_params_map.extend(method_tp_map);

            let params: Vec<(String, Type)> = method
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

            let return_type = match &method.return_type {
                Some(te) => {
                    self.resolve_type_expr_with_type_params(te, &method_type_params_map)
                }
                None => Type::Unit,
            };

            if let Some(doc) = &method.doc_comment {
                self.package_registry.register_sub_doc_comment(fqn.clone(), method.name.value.clone(), doc.clone());
            }
            let default_source = match &method.body {
                None => None,
                Some(body) => {
                    if matches!(body, crate::parser::ast::Expr::Intrinsic(_)) {
                        self.diagnostics.error(
                            method.name.span.clone(),
                            format!(
                                "default body of '{}' in trait '{}' cannot be intrinsic",
                                method.name.value, trait_decl.name.value
                            ),
                        );
                        None
                    } else {
                        Some(fqn.clone())
                    }
                }
            };
            methods.push(TraitMethodSig {
                name: method.name.value.clone(),
                type_params: method_type_params,
                params,
                return_type,
                trait_bounds: method_trait_bounds,
                span: method.span.clone(),
                origin: None,
                default_source,
            });
        }

        // Collect trait properties
        let mut properties = Vec::new();
        for property in &trait_decl.properties {
            // Check for duplicate property names
            if seen_methods.contains(&property.name.value) {
                self.diagnostics.error(
                    property.name.span.clone(),
                    format!(
                        "duplicate name '{}' in trait '{}' (conflicts with method or property)",
                        property.name.value, trait_decl.name.value
                    ),
                );
                continue;
            }
            if !seen_methods.insert(property.name.value.clone()) {
                self.diagnostics.error(
                    property.name.span.clone(),
                    format!(
                        "duplicate property '{}' in trait '{}'",
                        property.name.value, trait_decl.name.value
                    ),
                );
                continue;
            }

            let params: Vec<(String, Type)> = property
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr_with_type_params(
                        &p.type_annotation,
                        &trait_type_params_map,
                    );
                    (p.name.value.clone(), ty)
                })
                .collect();

            let return_type = self.resolve_type_expr_with_type_params(
                &property.return_type,
                &trait_type_params_map,
            );

            if let Some(doc) = &property.doc_comment {
                self.package_registry.register_sub_doc_comment(fqn.clone(), property.name.value.clone(), doc.clone());
            }
            let default_source = match &property.body {
                None => None,
                Some(body) => {
                    if matches!(body, crate::parser::ast::Expr::Intrinsic(_)) {
                        self.diagnostics.error(
                            property.name.span.clone(),
                            format!(
                                "default body of '{}' in trait '{}' cannot be intrinsic",
                                property.name.value, trait_decl.name.value
                            ),
                        );
                        None
                    } else {
                        Some(fqn.clone())
                    }
                }
            };
            properties.push(TraitPropertySig {
                name: property.name.value.clone(),
                params,
                return_type,
                span: property.span.clone(),
                origin: None,
                default_source,
            });
        }

        let registered = self.package_registry.register_trait(
            fqn.clone(),
            TraitSignature {
                visibility: trait_decl.visibility,
                fqn,
                type_params: trait_type_params,
                supers,
                super_closure: vec![],
                methods,
                method_dispatch_names: vec![],
                properties,
                associated_types: associated_type_sigs,
                is_interface: trait_decl.is_interface,
                source_file: trait_decl.name.span.file.clone(),
                span: trait_decl.name.span.clone(),
            },
        );
        if !registered {
            self.diagnostics.error(
                trait_decl.name.span.clone(),
                format!(
                    "duplicate {}: '{}'",
                    if trait_decl.is_interface { "interface" } else { "trait" },
                    trait_decl.name.value
                ),
            );
        }
    }
}
