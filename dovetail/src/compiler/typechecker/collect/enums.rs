use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName, Variance};
use crate::parser::ast::{EnumDecl, EnumVariantPayload};

use crate::typechecker::registry::{EnumTypeSignature, VariantPayload};
use crate::typechecker::types::{TraitBounds, Type};

use super::Collector;

impl Collector<'_> {
    /// Pre-register an enum type name so that forward references resolve.
    /// Called in Pass 0 before full collection.
    pub(super) fn pre_register_enum(&mut self, decl: &EnumDecl) {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(decl.name.value.clone()),
        };
        // Always register as `Type::Enum(fqn, mn)`, matching how records and
        // classes are pre-registered. Generic-ness lives in the side-table
        // `enum_types: EnumTypeSignature` only. `resolve_type_name` consults
        // that table when instantiating with explicit type args.
        let ty = Type::Enum(fqn.clone(), MangledName::for_type(&fqn));
        self.package_registry.register_type(fqn.clone(), ty);

        // For generic enums, pre-register a skeleton EnumTypeSignature so that
        // forward references (e.g. trait methods returning Option<T>) can resolve
        // type argument counts during collect.
        if !decl.type_params.is_empty() {
            let type_params: Vec<TypeParamName> = decl
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.name.value.clone()))
                .collect();
            let type_param_variances: Vec<Variance> = decl
                .type_params
                .iter()
                .map(|tp| tp.variance)
                .collect();
            self.package_registry.register_enum_type(
                fqn,
                EnumTypeSignature {
                    visibility: decl.visibility,
                    construction_private: decl.construction_private,
                    fqn: Fqn {
                        package: self.package_path.clone(),
                        symbol: SymbolName(decl.name.value.clone()),
                    },
                    type_params,
                    type_param_variances,
                    variants: vec![],
                    trait_bounds: crate::typechecker::types::TraitBounds::empty(),
                    source_file: "".into(),
                    span: decl.name.span.clone(),
                },
            );
        }
    }

    /// Collect an enum declaration into the package registry.
    pub(super) fn collect_enum(&mut self, decl: &EnumDecl) {
        if !decl.type_params.is_empty() {
            self.collect_generic_enum(decl);
            return;
        }

        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(decl.name.value.clone()),
        };

        if let Some(doc) = &decl.doc_comment {
            self.package_registry.register_doc_comment(fqn.clone(), doc.clone());
        }

        // Resolve variant payload types and check for duplicate variant names
        let mut variants = Vec::new();
        let mut seen_variants = std::collections::BTreeSet::new();
        for variant in &decl.variants {
            if !seen_variants.insert(variant.name.value.clone()) {
                self.diagnostics.error(
                    variant.name.span.clone(),
                    format!(
                        "duplicate variant '{}' in enum '{}'",
                        variant.name.value, decl.name.value
                    ),
                );
            }
            let payload = match &variant.payload {
                EnumVariantPayload::None => VariantPayload::None,
                EnumVariantPayload::Tuple(types) => {
                    let resolved: Vec<Type> =
                        types.iter().map(|te| self.resolve_type_expr(te)).collect();
                    VariantPayload::Tuple(resolved)
                }
                EnumVariantPayload::Record(fields) => {
                    let mut seen_fields = std::collections::BTreeSet::new();
                    let mut resolved_fields = Vec::new();
                    for field in fields {
                        if !seen_fields.insert(field.name.value.clone()) {
                            self.diagnostics.error(
                                field.name.span.clone(),
                                format!(
                                    "duplicate field '{}' in variant '{}' of enum '{}'",
                                    field.name.value, variant.name.value, decl.name.value
                                ),
                            );
                        }
                        let ty = self.resolve_type_expr(&field.type_annotation);
                        resolved_fields.push((field.name.value.clone(), ty));
                    }
                    VariantPayload::Record(resolved_fields)
                }
            };
            if let Some(doc) = &variant.doc_comment {
                self.package_registry.register_sub_doc_comment(fqn.clone(), variant.name.value.clone(), doc.clone());
            }
            variants.push((variant.name.value.clone(), payload));
        }

        // Re-register the type (already pre-registered, but ensure consistency)
        let mangled_name = MangledName::for_type(&fqn);
        let ty = Type::Enum(fqn.clone(), mangled_name);
        self.package_registry.register_type(fqn.clone(), ty);

        // Register the enum type info
        self.package_registry.register_enum_type(
            fqn.clone(),
            EnumTypeSignature {
                visibility: decl.visibility,
                construction_private: decl.construction_private,
                fqn,
                type_params: vec![],
                type_param_variances: vec![],
                variants,
                trait_bounds: TraitBounds::empty(),
                source_file: decl.name.span.file.clone(),
                span: decl.span.clone(),
            },
        );
    }

    /// Collect a generic enum declaration into the package registry.
    fn collect_generic_enum(&mut self, decl: &EnumDecl) {
        let type_params: Vec<TypeParamName> = decl
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.name.value.clone()))
            .collect();
        let type_param_variances: Vec<Variance> =
            decl.type_params.iter().map(|tp| tp.variance).collect();

        // Resolve trait bounds from where clause
        let trait_bounds = self.resolve_trait_bounds(&decl.where_clause, &type_params);

        // Build type param map: name → Type::TypeParameter(name)
        let type_params_map = Type::type_param_map(&type_params, &trait_bounds);

        // Resolve variant payload types (may contain TypeParameter)
        let mut variants = Vec::new();
        let mut seen_variants = std::collections::BTreeSet::new();
        for variant in &decl.variants {
            if !seen_variants.insert(variant.name.value.clone()) {
                self.diagnostics.error(
                    variant.name.span.clone(),
                    format!(
                        "duplicate variant '{}' in enum '{}'",
                        variant.name.value, decl.name.value
                    ),
                );
            }
            let payload = match &variant.payload {
                EnumVariantPayload::None => VariantPayload::None,
                EnumVariantPayload::Tuple(types) => {
                    let resolved: Vec<Type> = types
                        .iter()
                        .map(|te| self.resolve_type_expr_with_type_params(te, &type_params_map))
                        .collect();
                    VariantPayload::Tuple(resolved)
                }
                EnumVariantPayload::Record(fields) => {
                    let mut seen_fields = std::collections::BTreeSet::new();
                    let mut resolved_fields = Vec::new();
                    for field in fields {
                        if !seen_fields.insert(field.name.value.clone()) {
                            self.diagnostics.error(
                                field.name.span.clone(),
                                format!(
                                    "duplicate field '{}' in variant '{}' of enum '{}'",
                                    field.name.value, variant.name.value, decl.name.value
                                ),
                            );
                        }
                        let ty = self.resolve_type_expr_with_type_params(
                            &field.type_annotation,
                            &type_params_map,
                        );
                        resolved_fields.push((field.name.value.clone(), ty));
                    }
                    VariantPayload::Record(resolved_fields)
                }
            };
            variants.push((variant.name.value.clone(), payload));
        }

        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(decl.name.value.clone()),
        };

        if let Some(doc) = &decl.doc_comment {
            self.package_registry.register_doc_comment(fqn.clone(), doc.clone());
        }
        for variant in &decl.variants {
            if let Some(doc) = &variant.doc_comment {
                self.package_registry.register_sub_doc_comment(fqn.clone(), variant.name.value.clone(), doc.clone());
            }
        }

        self.package_registry.register_enum_type(
            fqn.clone(),
            EnumTypeSignature {
                visibility: decl.visibility,
                construction_private: decl.construction_private,
                fqn,
                type_params,
                type_param_variances,
                variants,
                trait_bounds,
                source_file: decl.name.span.file.clone(),
                span: decl.span.clone(),
            },
        );
    }
}
