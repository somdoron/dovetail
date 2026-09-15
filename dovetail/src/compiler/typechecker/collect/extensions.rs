use std::collections::BTreeMap;

use crate::common::types::{Fqn, SymbolName, TypeParamName};
use crate::parser::ast::{Expr, ExtensionDecl};

use crate::typechecker::registry::{ExtMethodSignature, ExtensionBlockSignature};
use crate::typechecker::types::{TraitBound, TraitBounds, Type};

use super::Collector;

impl Collector<'_> {
    /// Collect an extension declaration: register each method in the extension methods registry.
    pub(super) fn collect_extension(&mut self, ext: &ExtensionDecl) {
        if !ext.type_params.is_empty() {
            // Generic extension: `extension ArrayHelper<T> for Array<T> = ...`
            self.collect_generic_extension(ext);
            return;
        }

        let for_type = self.resolve_type_expr(&ext.for_type);
        if for_type.is_error() {
            return;
        }
        if matches!(&for_type, Type::InterfaceObject { traits, .. } if traits.len() > 1) {
            self.diagnostics.error(
                ext.for_type.span(),
                "cannot declare an extension for an intersection type; extend a single interface instead".to_string(),
            );
            return;
        }

        let type_fqn = for_type.to_fqn();
        self.collect_named_extension(ext, &ext.name.value.clone(), &type_fqn, &for_type);
    }

    fn collect_generic_extension(&mut self, ext: &ExtensionDecl) {
        // Build type params
        let type_params: Vec<TypeParamName> = ext
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();

        // Resolve extension-level trait bounds from where clause
        let ext_trait_bounds = self.resolve_trait_bounds(&ext.where_clause, &type_params);

        // Build type param map with extension bounds
        let type_params_map = Type::type_param_map(&type_params, &ext_trait_bounds);

        // Resolve for_type with type params (e.g. Array<T> → Type::Array(Box::new(TypeParameter(T))))
        let for_type = self.resolve_type_expr_with_type_params(&ext.for_type, &type_params_map);
        if matches!(&for_type, Type::InterfaceObject { traits, .. } if traits.len() > 1) {
            self.diagnostics.error(
                ext.for_type.span(),
                "cannot declare an extension for an intersection type; extend a single interface instead".to_string(),
            );
            return;
        }
        if for_type.is_error() {
            return;
        }

        // Validate extension bounds cover the record's bounds
        self.validate_extension_covers_type_bounds(ext, &type_params, &ext_trait_bounds, &for_type);

        self.collect_named_generic_extension(
            ext,
            &ext.name.value.clone(),
            &type_params,
            &type_params_map,
            &for_type,
            &ext_trait_bounds,
        );
    }

    fn collect_named_generic_extension(
        &mut self,
        ext: &ExtensionDecl,
        ext_name_str: &str,
        type_params: &[TypeParamName],
        type_params_map: &BTreeMap<String, Type>,
        for_type: &Type,
        ext_trait_bounds: &TraitBounds,
    ) {
        let ext_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(ext_name_str.to_string()),
        };

        if let Some(doc) = &ext.doc_comment {
            self.package_registry
                .register_doc_comment(ext_fqn.clone(), doc.clone());
        }

        let mut methods = Vec::new();
        let mut properties = Vec::new();

        for method in &ext.methods {
            // Build method-level type params
            let method_type_params: Vec<TypeParamName> = method
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();

            // Resolve method-level trait bounds from where clause
            let all_method_type_params: Vec<TypeParamName> = type_params
                .iter()
                .chain(method_type_params.iter())
                .cloned()
                .collect();
            let mut method_trait_bounds = self.resolve_method_trait_bounds(
                &method.where_clause,
                &all_method_type_params,
                &method_type_params,
                ext_trait_bounds,
            );
            method_trait_bounds.merge(ext_trait_bounds);

            // Build combined type_params_map (extension-level + method-level)
            let combined_type_params_map =
                Type::type_param_map(&all_method_type_params, &method_trait_bounds);

            let return_type = match &method.return_type {
                Some(type_expr) => {
                    self.resolve_type_expr_with_type_params(type_expr, &combined_type_params_map)
                }
                None => Type::Unit,
            };

            let params: Vec<(String, Type)> = method
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

            let has_self = !params.is_empty() && params[0].0 == "self";
            if has_self && !crate::typechecker::subtyping::identical(&params[0].1, for_type) {
                self.diagnostics.error(
                    method.params[0].span.clone(),
                    format!(
                        "type of 'self' parameter must be '{}', found '{}'",
                        for_type, params[0].1
                    ),
                );
                continue;
            }

            methods.push(ExtMethodSignature {
                name: SymbolName(method.name.value.clone()),
                visibility: method.visibility,
                method_type_params,
                trait_bounds: method_trait_bounds,
                params,
                return_type,
                is_property: false,
                is_intrinsic: matches!(method.body, Expr::Intrinsic(_)),
                is_async: method.is_async,
                span: method.span.clone(),
            });
        }

        // Collect generic extension properties
        for property in &ext.properties {
            let property_type =
                self.resolve_type_expr_with_type_params(&property.return_type, type_params_map);
            let params: Vec<(String, Type)> = property
                .params
                .iter()
                .map(|p| {
                    let ty = self
                        .resolve_type_expr_with_type_params(&p.type_annotation, type_params_map);
                    (p.name.value.clone(), ty)
                })
                .collect();
            if property.body.is_none() {
                continue;
            }
            let body = property.body.as_ref().unwrap();

            properties.push(ExtMethodSignature {
                name: SymbolName(property.name.value.clone()),
                visibility: property.visibility,
                method_type_params: vec![],
                trait_bounds: ext_trait_bounds.clone(),
                params,
                return_type: property_type,
                is_property: true,
                is_intrinsic: matches!(body, Expr::Intrinsic(_)),
                is_async: false,
                span: property.span.clone(),
            });
        }

        if self
            .package_registry
            .lookup_trait(&ext_fqn, &self.package_path)
            .or_else(|| {
                self.dependency_registry
                    .lookup_trait(&ext_fqn, &self.package_path)
            })
            .is_some()
        {
            // A trait claims the `Name.member(...)` explicit-call form, so an
            // extension sharing the name would be unreachable — reject early.
            self.diagnostics.error(
                ext.name.span.clone(),
                format!(
                    "named extension '{}' conflicts with a trait of the same name; rename one — the trait claims '{}.member(...)' calls",
                    ext.name.value, ext.name.value,
                ),
            );
        }
        if self
            .package_registry
            .has_extension_block_for_type(&ext_fqn, for_type)
        {
            self.diagnostics.error(
                ext.name.span.clone(),
                format!(
                    "duplicate named extension '{}' for type '{}' in this package",
                    ext_name_str, for_type
                ),
            );
            return;
        }

        self.package_registry
            .register_extension_block(ExtensionBlockSignature {
                ext_fqn,
                for_type: for_type.clone(),
                type_params: type_params.to_vec(),
                trait_bounds: ext_trait_bounds.clone(),
                methods,
                properties,
                span: ext.name.span.clone(),
                source_file: ext.name.span.file.clone(),
                package: self.package_path.clone(),
            });
    }

    /// Validate that the extension's where clause covers all trait bounds
    /// required by the target generic record type.
    fn validate_extension_covers_type_bounds(
        &mut self,
        ext: &ExtensionDecl,
        _ext_type_params: &[TypeParamName],
        ext_trait_bounds: &TraitBounds,
        for_type: &Type,
    ) {
        // Only applies to generic record for_types
        let (fqn, type_args) = match for_type {
            Type::GenericRecord { fqn, type_args, .. } => (fqn, type_args),
            _ => return,
        };

        // Look up the record's type signature
        let record_sig = self
            .package_registry
            .lookup_generic_record_by_fqn(fqn, &self.package_path)
            .or_else(|| {
                self.dependency_registry
                    .lookup_generic_record_by_fqn(fqn, &self.package_path)
            });
        let record_sig = match record_sig {
            Some(sig) => sig.clone(),
            None => return,
        };

        if record_sig.trait_bounds.is_empty() {
            return;
        }

        // For each record type param with bounds, find the corresponding extension type param
        // via positional mapping: record_sig.type_params[i] → type_args[i] which should be TypeParameter(ext_tp)
        for (i, record_tp) in record_sig.type_params.iter().enumerate() {
            let required_bounds = match record_sig.trait_bounds.get(record_tp) {
                Some(bounds) if !bounds.is_empty() => bounds,
                _ => continue,
            };

            // type_args[i] should be TypeParameter(ext_tp_name, _)
            let ext_tp_name = match type_args.get(i) {
                Some((_, Type::TypeVariable(name, _) | Type::GenericParam(name, _, _))) => name,
                _ => continue, // concrete type arg — will be checked at instantiation
            };

            // Check that the extension's bounds for this type param cover all required traits
            let ext_bounds = ext_trait_bounds.get(ext_tp_name);
            for required_bound in required_bounds {
                let covered = ext_bounds
                    .map(|bounds| {
                        bounds.iter().any(|bound| match (required_bound, bound) {
                            (TraitBound::IsClass, TraitBound::IsClass) => true,
                            (TraitBound::IsClass, TraitBound::Named(actual)) => {
                                actual.is_class_bound()
                            }
                            (TraitBound::Named(required), TraitBound::Named(actual)) => {
                                actual.trait_fqn == required.trait_fqn
                            }
                            _ => false,
                        })
                    })
                    .unwrap_or(false);
                if !covered {
                    let trait_name = required_bound
                        .named()
                        .map_or("class", |b| b.trait_fqn.symbol.0.as_str());
                    self.diagnostics.error(
                        ext.name.span.clone(),
                        format!(
                            "generic extension for '{}' must include trait bound '{}: {}' from the type definition",
                            ext.name.value, ext_tp_name.0, trait_name
                        ),
                    );
                }
            }
        }
    }

    fn collect_named_extension(
        &mut self,
        ext: &ExtensionDecl,
        ext_name_str: &str,
        _type_fqn: &Fqn,
        for_type: &Type,
    ) {
        let ext_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(ext_name_str.to_string()),
        };

        if let Some(doc) = &ext.doc_comment {
            self.package_registry
                .register_doc_comment(ext_fqn.clone(), doc.clone());
        }

        let mut methods = Vec::new();
        let mut properties = Vec::new();

        for method in &ext.methods {
            let return_type = match &method.return_type {
                Some(type_expr) => self.resolve_type_expr(type_expr),
                None => Type::Unit,
            };

            let params: Vec<(String, Type)> = method
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr(&p.type_annotation);
                    (p.name.value.clone(), ty)
                })
                .collect();

            let has_self = !params.is_empty() && params[0].0 == "self";
            if has_self && !crate::typechecker::subtyping::identical(&params[0].1, for_type) {
                self.diagnostics.error(
                    method.params[0].span.clone(),
                    format!(
                        "type of 'self' parameter must be '{}', found '{}'",
                        for_type, params[0].1
                    ),
                );
                continue;
            }

            methods.push(ExtMethodSignature {
                name: SymbolName(method.name.value.clone()),
                visibility: method.visibility,
                method_type_params: vec![],
                trait_bounds: TraitBounds::empty(),
                params,
                return_type,
                is_property: false,
                is_intrinsic: matches!(method.body, Expr::Intrinsic(_)),
                is_async: method.is_async,
                span: method.span.clone(),
            });
        }

        // Collect properties for named extension
        for property in &ext.properties {
            let property_type = self.resolve_type_expr(&property.return_type);
            let params: Vec<(String, Type)> = property
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr(&p.type_annotation);
                    (p.name.value.clone(), ty)
                })
                .collect();
            let body = match property.body.as_ref() {
                Some(b) => b,
                None => continue,
            };

            properties.push(ExtMethodSignature {
                name: SymbolName(property.name.value.clone()),
                visibility: property.visibility,
                method_type_params: vec![],
                trait_bounds: TraitBounds::empty(),
                params,
                return_type: property_type,
                is_property: true,
                is_intrinsic: matches!(body, Expr::Intrinsic(_)),
                is_async: false,
                span: property.span.clone(),
            });
        }

        if self
            .package_registry
            .lookup_trait(&ext_fqn, &self.package_path)
            .or_else(|| {
                self.dependency_registry
                    .lookup_trait(&ext_fqn, &self.package_path)
            })
            .is_some()
        {
            // A trait claims the `Name.member(...)` explicit-call form, so an
            // extension sharing the name would be unreachable — reject early.
            self.diagnostics.error(
                ext.name.span.clone(),
                format!(
                    "named extension '{}' conflicts with a trait of the same name; rename one — the trait claims '{}.member(...)' calls",
                    ext.name.value, ext.name.value,
                ),
            );
        }
        if self
            .package_registry
            .has_extension_block_for_type(&ext_fqn, for_type)
        {
            self.diagnostics.error(
                ext.name.span.clone(),
                format!(
                    "duplicate named extension '{}' for type '{}' in this package",
                    ext_name_str, for_type
                ),
            );
            return;
        }

        self.package_registry
            .register_extension_block(ExtensionBlockSignature {
                ext_fqn,
                for_type: for_type.clone(),
                type_params: vec![],
                trait_bounds: TraitBounds::empty(),
                methods,
                properties,
                span: ext.name.span.clone(),
                source_file: ext.name.span.file.clone(),
                package: self.package_path.clone(),
            });
    }
}
