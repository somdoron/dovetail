use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName};
use crate::parser::ast::{Expr, FunctionDecl};

use crate::typechecker::registry::{FunctionSignature, GenericFunctionDef};
use crate::typechecker::types::Type;

use super::Collector;

impl Collector<'_> {
    /// Collect a single function declaration into the package registry.
    pub(super) fn collect_function(&mut self, func: &FunctionDecl) {
        if !func.type_params.is_empty() {
            self.collect_generic_function(func);
            return;
        }

        self.resolve_trait_bounds(&func.where_clause, &[]);
        let return_type = match &func.return_type {
            Some(type_expr) => self.resolve_type_expr(type_expr),
            None => Type::Unit, // default return type
        };

        // Resolve parameter types
        let params: Vec<(String, Type)> = func
            .params
            .iter()
            .map(|p| {
                let ty = self.resolve_type_expr(&p.type_annotation);
                (p.name.value.clone(), ty)
            })
            .collect();

        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(func.name.value.clone()),
        };

        if let Some(doc) = &func.doc_comment {
            self.package_registry.register_doc_comment(fqn.clone(), doc.clone());
        }

        let param_types: Vec<&Type> = params.iter().map(|(_, ty)| ty).collect();
        let mangled_name = MangledName::for_function(&fqn, &param_types);

        let registered = self.package_registry.register_function(
            fqn,
            FunctionSignature {
                visibility: func.visibility,
                mangled_name,
                params,
                return_type,
                source_file: func.name.span.file.clone(),
                is_intrinsic: matches!(func.body, Expr::Intrinsic(_)),
                is_property: false,
                is_final_method: false,
                is_abstract_method: false,
            },
        );
        if !registered {
            self.diagnostics.error(
                func.name.span.clone(),
                format!("duplicate function: '{}'", func.name.value),
            );
        }
    }

    /// Collect a generic function declaration into the package registry.
    fn collect_generic_function(&mut self, func: &FunctionDecl) {
        let type_params: Vec<TypeParamName> = func
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();

        // Resolve trait bounds first so TypeParameter types carry their bounds
        let trait_bounds = self.resolve_trait_bounds(&func.where_clause, &type_params);

        // Build type param map: name → Type::TypeParameter(name, bounds)
        let type_params_map = Type::type_param_map(&type_params, &trait_bounds);

        // Resolve parameter types (may contain TypeParameter)
        let params: Vec<(String, Type)> = func
            .params
            .iter()
            .map(|p| {
                let ty =
                    self.resolve_type_expr_with_type_params(&p.type_annotation, &type_params_map);
                (p.name.value.clone(), ty)
            })
            .collect();

        let return_type = match &func.return_type {
            Some(type_expr) => self.resolve_type_expr_with_type_params(type_expr, &type_params_map),
            None => Type::Unit,
        };

        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(func.name.value.clone()),
        };

        if let Some(doc) = &func.doc_comment {
            self.package_registry.register_doc_comment(fqn.clone(), doc.clone());
        }

        self.package_registry.register_generic_function(
            fqn,
            GenericFunctionDef {
                visibility: func.visibility,
                type_params,
                params,
                return_type,
                body: func.body.clone(),
                span: func.name.span.clone(),
                container_name: None,
                trait_bounds,
                is_async: func.is_async,
                is_intrinsic: matches!(func.body, Expr::Intrinsic(_)),
            },
        );
    }
}
