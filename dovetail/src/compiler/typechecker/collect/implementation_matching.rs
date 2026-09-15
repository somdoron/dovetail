//! Resolve implementation annotations with each candidate's declared evidence.

use std::collections::BTreeMap;

use crate::common::types::TypeParamName;
use crate::parser::ast::FunctionDecl;
use crate::typechecker::types::{TraitBounds, Type};

use super::{
    Collector, expand_trait_bound_gats, rename_method_bounds, substitute_trait_type_params,
};

pub(super) struct MethodScope<'a> {
    pub parameters: &'a [TypeParamName],
    pub enclosing_bounds: &'a TraitBounds,
    pub trait_substitution: &'a BTreeMap<TypeParamName, Type>,
    pub associated_types: &'a BTreeMap<TypeParamName, (Vec<TypeParamName>, Type)>,
}

impl Collector<'_> {
    pub(super) fn implementation_matches_method(
        &mut self,
        method: &FunctionDecl,
        expected: &[(String, Type)],
        declared_parameters: &[TypeParamName],
        declared_bounds: &TraitBounds,
        context: &MethodScope<'_>,
    ) -> bool {
        if method.params.len() != expected.len()
            || method.type_params.len() != declared_parameters.len()
        {
            return false;
        }
        let parameters: Vec<_> = method
            .type_params
            .iter()
            .map(|parameter| TypeParamName(parameter.value.clone()))
            .collect();
        let mut bounds = context.enclosing_bounds.clone();
        bounds.merge(&expand_trait_bound_gats(
            &rename_method_bounds(
                declared_bounds,
                declared_parameters,
                &parameters,
                context.trait_substitution,
            ),
            context.associated_types,
        ));
        let scope = Type::type_param_map(context.parameters, &bounds);
        let before = self.diagnostics.len();
        let actual: Vec<_> = method
            .params
            .iter()
            .map(|parameter| {
                self.resolve_type_expr_with_type_params(&parameter.type_annotation, &scope)
            })
            .collect();
        let failed = self.diagnostics.len() != before;
        self.diagnostics.truncate(before);
        let mut substitution = context.trait_substitution.clone();
        substitution.extend(
            declared_parameters.iter().cloned().zip(
                parameters
                    .into_iter()
                    .map(|parameter| Type::TypeVariable(parameter, vec![])),
            ),
        );
        !failed
            && expected.iter().zip(&actual).all(|((_, expected), actual)| {
                let expected = substitute_trait_type_params(expected, &substitution);
                let expected = super::expand_gats(&expected, context.associated_types);
                crate::typechecker::subtyping::identical(&expected, actual)
            })
    }
}
