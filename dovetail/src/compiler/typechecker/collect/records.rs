use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName, Variance};
use crate::parser::ast::RecordDecl;

use crate::typechecker::registry::RecordTypeSignature;
use crate::typechecker::types::{TraitBounds, Type};

use super::Collector;

impl Collector<'_> {
    /// Pre-register a record type name so that forward references resolve.
    /// Called in Pass 0 before full collection.
    pub(super) fn pre_register_record(&mut self, rec: &RecordDecl) {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(rec.name.value.clone()),
        };
        let mangled_name = MangledName::for_type(&fqn);
        let ty = Type::Record(fqn.clone(), mangled_name);
        self.package_registry.register_type(fqn.clone(), ty);

        // For generic records, pre-register a skeleton RecordTypeSignature so that
        // forward references (e.g. an enum variant using Slice<T> defined in another
        // file of the same package) can resolve type argument counts during collect.
        if !rec.type_params.is_empty() {
            let type_params: Vec<TypeParamName> = rec
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.name.value.clone()))
                .collect();
            let type_param_variances: Vec<Variance> =
                rec.type_params.iter().map(|tp| tp.variance).collect();
            self.package_registry.register_record_type(
                fqn.clone(),
                RecordTypeSignature {
                    visibility: rec.visibility,
                    construction_private: rec.construction_private,
                    is_string_literal: rec.string_literal.is_some(),
                    fqn,
                    type_params,
                    type_param_variances,
                    fields: vec![],
                    trait_bounds: TraitBounds::empty(),
                    source_file: "".into(),
                    span: rec.name.span.clone(),
                },
            );
        }
    }

    /// Collect a record declaration into the package registry.
    pub(super) fn collect_record(&mut self, rec: &RecordDecl) {
        if !rec.type_params.is_empty() {
            self.collect_generic_record(rec);
            return;
        }

        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(rec.name.value.clone()),
        };

        if let Some(doc) = &rec.doc_comment {
            self.package_registry
                .register_doc_comment(fqn.clone(), doc.clone());
        }

        let mangled_name = MangledName::for_type(&fqn);

        // Resolve field types and check for duplicate field names
        let mut fields = Vec::new();
        let mut seen_fields = std::collections::BTreeSet::new();
        for field in &rec.fields {
            let ty = self.resolve_type_expr(&field.type_annotation);
            if !seen_fields.insert(field.name.value.clone()) {
                self.diagnostics.error(
                    field.name.span.clone(),
                    format!(
                        "duplicate field '{}' in record '{}'",
                        field.name.value, rec.name.value
                    ),
                );
            }
            if let Some(doc) = &field.doc_comment {
                self.package_registry.register_sub_doc_comment(
                    fqn.clone(),
                    field.name.value.clone(),
                    doc.clone(),
                );
            }
            fields.push((field.name.value.clone(), ty));
        }

        // Register the type
        let ty = Type::Record(fqn.clone(), mangled_name);
        self.package_registry.register_type(fqn.clone(), ty);

        // Register the record type info
        self.package_registry.register_record_type(
            fqn.clone(),
            RecordTypeSignature {
                visibility: rec.visibility,
                construction_private: rec.construction_private,
                is_string_literal: rec.string_literal.is_some(),
                fqn,
                type_params: vec![],
                type_param_variances: vec![],
                fields,
                trait_bounds: TraitBounds::empty(),
                source_file: rec.name.span.file.clone(),
                span: rec.span.clone(),
            },
        );
    }

    /// Collect a generic record declaration into the package registry.
    fn collect_generic_record(&mut self, rec: &RecordDecl) {
        let type_params: Vec<TypeParamName> = rec
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.name.value.clone()))
            .collect();
        let type_param_variances: Vec<Variance> =
            rec.type_params.iter().map(|tp| tp.variance).collect();

        // Resolve trait bounds from where clause
        let trait_bounds = self.resolve_trait_bounds(&rec.where_clause, &type_params);

        // Build type param map: name → Type::TypeParameter(name)
        let type_params_map = Type::type_param_map(&type_params, &trait_bounds);

        // Resolve field types (may contain TypeParameter)
        let mut fields = Vec::new();
        let mut seen_fields = std::collections::BTreeSet::new();
        for field in &rec.fields {
            let ty =
                self.resolve_type_expr_with_type_params(&field.type_annotation, &type_params_map);
            if !seen_fields.insert(field.name.value.clone()) {
                self.diagnostics.error(
                    field.name.span.clone(),
                    format!(
                        "duplicate field '{}' in record '{}'",
                        field.name.value, rec.name.value
                    ),
                );
            }
            fields.push((field.name.value.clone(), ty));
        }

        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(rec.name.value.clone()),
        };

        if let Some(doc) = &rec.doc_comment {
            self.package_registry
                .register_doc_comment(fqn.clone(), doc.clone());
        }
        for field in &rec.fields {
            if let Some(doc) = &field.doc_comment {
                self.package_registry.register_sub_doc_comment(
                    fqn.clone(),
                    field.name.value.clone(),
                    doc.clone(),
                );
            }
        }

        self.package_registry.register_record_type(
            fqn.clone(),
            RecordTypeSignature {
                visibility: rec.visibility,
                construction_private: rec.construction_private,
                is_string_literal: rec.string_literal.is_some(),
                fqn,
                type_params,
                type_param_variances,
                fields,
                trait_bounds,
                source_file: rec.name.span.file.clone(),
                span: rec.span.clone(),
            },
        );
    }
}
