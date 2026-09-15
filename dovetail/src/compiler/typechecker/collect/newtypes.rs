use crate::common::types::{Fqn, SymbolName, TypeParamName, Variance};
use crate::parser::ast::NewtypeDecl;

use crate::typechecker::registry::NewtypeSignature;
use crate::typechecker::types::{TraitBounds, Type};

use super::Collector;

impl Collector<'_> {
    /// Pre-register a newtype name so that forward references resolve.
    /// Called in Pass 0 before full collection.
    pub(super) fn pre_register_newtype(&mut self, decl: &NewtypeDecl) {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(decl.name.value.clone()),
        };
        let ty = Type::Newtype(fqn.clone(), Box::new(Type::Error));
        self.package_registry.register_type(fqn.clone(), ty);

        // For generic newtypes, pre-register a skeleton NewtypeSignature so that
        // forward references from other files in the same package resolve correctly.
        if !decl.type_params.is_empty() {
            let type_params: Vec<TypeParamName> = decl
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.name.value.clone()))
                .collect();
            let type_param_variances: Vec<Variance> =
                decl.type_params.iter().map(|tp| tp.variance).collect();
            self.package_registry.register_newtype_type(
                fqn.clone(),
                NewtypeSignature {
                    visibility: decl.visibility,
                    fqn,
                    inner_private: decl.inner_private,
                    inner_type: Type::Error,
                    type_params,
                    type_param_variances,
                    trait_bounds: TraitBounds::empty(),
                    source_file: "".into(),
                    span: decl.name.span.clone(),
                },
            );
        }
    }

    /// Try to collect a non-generic newtype declaration.
    /// Returns `true` if the inner type resolved without errors, `false` otherwise.
    /// On failure, diagnostics are rolled back and the registry is not updated.
    pub(super) fn try_collect_newtype(&mut self, decl: &NewtypeDecl) -> bool {
        if decl.intrinsic {
            self.diagnostics
                .error(decl.span.clone(), "unsupported intrinsic type declaration");
            return true;
        }
        let diag_count = self.diagnostics.len();
        let inner_type = self.resolve_type_expr(&decl.inner_type);

        if inner_type.contains_error() {
            self.diagnostics.truncate(diag_count);
            return false;
        }

        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(decl.name.value.clone()),
        };

        if let Some(doc) = &decl.doc_comment {
            self.package_registry
                .register_doc_comment(fqn.clone(), doc.clone());
        }

        let ty = Type::Newtype(fqn.clone(), Box::new(inner_type.clone()));
        self.package_registry.register_type(fqn.clone(), ty);

        self.package_registry.register_newtype_type(
            fqn.clone(),
            NewtypeSignature {
                visibility: decl.visibility,
                fqn,
                inner_private: decl.inner_private,
                inner_type,
                type_params: vec![],
                type_param_variances: vec![],
                trait_bounds: TraitBounds::empty(),
                source_file: decl.name.span.file.clone(),
                span: decl.name.span.clone(),
            },
        );

        true
    }

    /// Collect a newtype declaration into the package registry.
    pub(super) fn collect_newtype(&mut self, decl: &NewtypeDecl) {
        if !decl.type_params.is_empty() {
            self.collect_generic_newtype(decl);
            return;
        }
        if decl.intrinsic {
            self.diagnostics
                .error(decl.span.clone(), "unsupported intrinsic type declaration");
            return;
        }

        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(decl.name.value.clone()),
        };

        if let Some(doc) = &decl.doc_comment {
            self.package_registry
                .register_doc_comment(fqn.clone(), doc.clone());
        }

        let inner_type = self.resolve_type_expr(&decl.inner_type);

        // Update the type in the registry with the resolved inner type
        let ty = Type::Newtype(fqn.clone(), Box::new(inner_type.clone()));
        self.package_registry.register_type(fqn.clone(), ty);

        // Register the newtype signature
        self.package_registry.register_newtype_type(
            fqn.clone(),
            NewtypeSignature {
                visibility: decl.visibility,
                fqn,
                inner_private: decl.inner_private,
                inner_type,
                type_params: vec![],
                type_param_variances: vec![],
                trait_bounds: TraitBounds::empty(),
                source_file: decl.name.span.file.clone(),
                span: decl.name.span.clone(),
            },
        );
    }

    /// Collect a generic newtype declaration into the package registry.
    fn collect_generic_newtype(&mut self, decl: &NewtypeDecl) {
        let type_params: Vec<TypeParamName> = decl
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.name.value.clone()))
            .collect();
        let type_param_variances: Vec<Variance> =
            decl.type_params.iter().map(|tp| tp.variance).collect();

        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(decl.name.value.clone()),
        };

        if let Some(doc) = &decl.doc_comment {
            self.package_registry
                .register_doc_comment(fqn.clone(), doc.clone());
        }

        // Resolve trait bounds from where clause
        let trait_bounds = self.resolve_trait_bounds(&decl.where_clause, &type_params);

        // Build type param map: name → Type::TypeParameter(name)
        let type_params_map = Type::type_param_map(&type_params, &trait_bounds);

        // Resolve inner type (may contain TypeParameter)
        let inner_type = if decl.intrinsic {
            if fqn.to_string() != "standard.prelude.ReadonlySlice"
                || decl.type_params.len() != 1
                || decl.type_params[0].variance != Variance::Covariant
            {
                self.diagnostics
                    .error(decl.span.clone(), "unsupported intrinsic type declaration");
                Type::Error
            } else {
                let fields = vec![Type::Any, Type::Int32, Type::Int32];
                let name = crate::common::types::MangledName::for_tuple(&fields);
                Type::Tuple(fields, name)
            }
        } else {
            self.resolve_type_expr_with_type_params(&decl.inner_type, &type_params_map)
        };

        // Don't update registry.types for generic newtypes — the placeholder stays.
        // Concrete types are produced during inference when type args are provided.

        // Register the newtype signature
        self.package_registry.register_newtype_type(
            fqn.clone(),
            NewtypeSignature {
                visibility: decl.visibility,
                fqn,
                inner_private: decl.inner_private,
                inner_type,
                type_params,
                type_param_variances,
                trait_bounds,
                source_file: decl.name.span.file.clone(),
                span: decl.name.span.clone(),
            },
        );
    }
}
