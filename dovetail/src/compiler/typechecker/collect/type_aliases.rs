use crate::common::types::{Fqn, SymbolName, TypeParamName};
use crate::parser::ast::TypeAliasDecl;

use crate::typechecker::registry::TypeAliasSignature;
use crate::typechecker::types::{TraitBounds, Type};

use super::Collector;

impl Collector<'_> {
    /// Pre-register a type alias name so that forward references resolve.
    /// Called in Pass 0 before full collection.
    pub(super) fn pre_register_type_alias(&mut self, decl: &TypeAliasDecl) {
        // Only register placeholder for non-generic aliases.
        // Generic aliases are looked up via lookup_generic_type_alias_by_fqn().
        if !decl.type_params.is_empty() {
            return;
        }
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(decl.name.value.clone()),
        };
        // Register placeholder; will be replaced with resolved type in Pass 1
        self.package_registry.register_type(fqn, Type::Error);
    }

    /// Try to collect a non-generic type alias declaration.
    /// Returns `true` if the expanded type resolved without errors, `false` otherwise.
    /// On failure, diagnostics are rolled back and the registry is not updated.
    pub(super) fn try_collect_type_alias(&mut self, decl: &TypeAliasDecl) -> bool {
        let diag_count = self.diagnostics.len();
        let expanded_type = self.resolve_type_expr(&decl.type_expr);

        if expanded_type.contains_error() {
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

        self.package_registry
            .register_type(fqn.clone(), expanded_type.clone());

        self.package_registry.register_type_alias(
            fqn.clone(),
            TypeAliasSignature {
                visibility: decl.visibility,
                // `@stringLiteral public type sql = SqlBuilder` — marking the
                // alias rather than the target keeps the builder's own
                // descriptive name intact.
                is_string_literal: decl.string_literal.is_some(),
                fqn,
                type_params: vec![],
                trait_bounds: TraitBounds::empty(),
                expanded_type,
                source_file: decl.name.span.file.clone(),
            },
        );

        true
    }

    /// Collect a type alias declaration into the package registry.
    pub(super) fn collect_type_alias(&mut self, decl: &TypeAliasDecl) {
        if !decl.type_params.is_empty() {
            self.collect_generic_type_alias(decl);
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

        let expanded_type = self.resolve_type_expr(&decl.type_expr);

        // Store the expanded type directly under the alias FQN
        self.package_registry
            .register_type(fqn.clone(), expanded_type.clone());

        // Register the type alias signature for visibility tracking
        self.package_registry.register_type_alias(
            fqn.clone(),
            TypeAliasSignature {
                visibility: decl.visibility,
                // `@stringLiteral public type sql = SqlBuilder` — marking the
                // alias rather than the target keeps the builder's own
                // descriptive name intact.
                is_string_literal: decl.string_literal.is_some(),
                fqn,
                type_params: vec![],
                trait_bounds: TraitBounds::empty(),
                expanded_type,
                source_file: decl.name.span.file.clone(),
            },
        );
    }

    /// Collect a generic type alias declaration (e.g. `type Maybe<T> = Option<T>`).
    fn collect_generic_type_alias(&mut self, decl: &TypeAliasDecl) {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(decl.name.value.clone()),
        };

        if let Some(doc) = &decl.doc_comment {
            self.package_registry
                .register_doc_comment(fqn.clone(), doc.clone());
        }

        let type_params: Vec<TypeParamName> = decl
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();

        let trait_bounds = self.resolve_trait_bounds(&decl.where_clause, &type_params);

        let type_params_map = Type::type_param_map(&type_params, &trait_bounds);

        let expanded_type =
            self.resolve_type_expr_with_type_params(&decl.type_expr, &type_params_map);

        // Do NOT register in registry.types — generic aliases have no single concrete type.
        // They are expanded at each use site via lookup_generic_type_alias_by_fqn().

        self.package_registry.register_type_alias(
            fqn.clone(),
            TypeAliasSignature {
                visibility: decl.visibility,
                // `@stringLiteral public type sql = SqlBuilder` — marking the
                // alias rather than the target keeps the builder's own
                // descriptive name intact.
                is_string_literal: decl.string_literal.is_some(),
                fqn,
                type_params,
                trait_bounds,
                expanded_type,
                source_file: decl.name.span.file.clone(),
            },
        );
    }
}
