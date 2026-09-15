use std::collections::BTreeMap;

use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName, Visibility};
use crate::parser::ast::{ClassDecl, NamedType, TypeExpr};
use crate::typechecker::registry::{ClassTypeSignature, GenericClassMethodDef, TraitSignature};
use crate::typechecker::types::{TraitBounds, Type};

use super::Collector;
use super::implements::substitute_trait_type_params;

struct DefaultMember<'a> {
    name: &'a String,
    params: &'a [(String, Type)],
    result: &'a Type,
    method_parameters: &'a [TypeParamName],
    bounds: TraitBounds,
    source: &'a Fqn,
    is_property: bool,
}

struct SuppliedDefault {
    name: SymbolName,
    definition: GenericClassMethodDef,
    source: Fqn,
    source_parameters: Vec<Type>,
}

fn default_members(signature: &TraitSignature) -> impl Iterator<Item = DefaultMember<'_>> {
    signature
        .methods
        .iter()
        .filter_map(|method| {
            Some(DefaultMember {
                name: &method.name,
                params: &method.params,
                result: &method.return_type,
                method_parameters: &method.type_params,
                bounds: method.trait_bounds.clone(),
                source: method.default_source.as_ref()?,
                is_property: false,
            })
        })
        .chain(signature.properties.iter().filter_map(|property| {
            Some(DefaultMember {
                name: &property.name,
                params: &property.params,
                result: &property.return_type,
                method_parameters: &[],
                bounds: TraitBounds::empty(),
                source: property.default_source.as_ref()?,
                is_property: true,
            })
        }))
}

fn default_definition(
    class: &ClassTypeSignature,
    member: &DefaultMember<'_>,
    substitution: &BTreeMap<TypeParamName, Type>,
) -> GenericClassMethodDef {
    let own_parameters: Vec<_> = member
        .method_parameters
        .iter()
        .map(|name| TypeParamName(format!("$method${}", name.0)))
        .collect();
    let mut member_substitution = substitution.clone();
    member_substitution.extend(
        member.method_parameters.iter().cloned().zip(
            own_parameters
                .iter()
                .map(|name| Type::TypeVariable(name.clone(), vec![])),
        ),
    );
    let mut bounds = class.trait_bounds.clone();
    bounds.merge(&super::implements::rename_method_bounds(
        &member.bounds,
        member.method_parameters,
        &own_parameters,
        substitution,
    ));
    GenericClassMethodDef {
        visibility: Visibility::Public,
        class_type_params: class.type_params.clone(),
        method_type_params: own_parameters,
        params: member
            .params
            .iter()
            .map(|(name, ty)| {
                (
                    name.clone(),
                    substitute_trait_type_params(ty, &member_substitution),
                )
            })
            .collect(),
        return_type: substitute_trait_type_params(member.result, &member_substitution),
        trait_bounds: bounds,
        is_final_method: false,
        is_abstract_method: false,
        is_property: member.is_property,
        is_async: false,
    }
}

struct DefaultApplication {
    signature: TraitSignature,
    parameters: Vec<Type>,
    substitution: BTreeMap<TypeParamName, Type>,
}

impl DefaultApplication {
    fn candidate(
        &self,
        class: &ClassTypeSignature,
        member: &DefaultMember<'_>,
    ) -> Option<SuppliedDefault> {
        if !member
            .params
            .first()
            .is_some_and(|(name, _)| name == "self")
        {
            return None;
        }
        let overloaded = self
            .signature
            .methods
            .iter()
            .filter(|method| method.name == *member.name)
            .count()
            > 1;
        // Ordinary defaults on non-generic classes use the existing concrete
        // member collector. Generic and overloaded defaults need templates.
        if class.type_params.is_empty() && member.method_parameters.is_empty() && !overloaded {
            return None;
        }
        let source_parameters = if *member.source == self.signature.fqn {
            self.parameters.clone()
        } else {
            self.signature
                .super_closure
                .iter()
                .find(|(origin, _)| origin == member.source)
                .map(|(_, parameters)| {
                    parameters
                        .iter()
                        .map(|ty| substitute_trait_type_params(ty, &self.substitution))
                        .collect()
                })
                .unwrap_or_default()
        };
        Some(SuppliedDefault {
            name: SymbolName(member.name.clone()),
            definition: default_definition(class, member, &self.substitution),
            source: member.source.clone(),
            source_parameters,
        })
    }
}

fn class_receiver(class: &ClassTypeSignature, parameters: &BTreeMap<String, Type>) -> Type {
    let mangled_name = MangledName::for_type(&class.fqn);
    if class.type_params.is_empty() {
        return Type::Class(class.fqn.clone(), mangled_name);
    }
    Type::GenericClass {
        fqn: class.fqn.clone(),
        mangled_name,
        type_args: class
            .type_param_variances
            .iter()
            .copied()
            .zip(
                class
                    .type_params
                    .iter()
                    .map(|name| parameters[&name.0].clone()),
            )
            .collect(),
    }
}

fn has_member_kind_conflict(class: &ClassTypeSignature, candidate: &SuppliedDefault) -> bool {
    let expected = &candidate.definition;
    let generic_conflict = class
        .generic_instance_methods
        .get(&candidate.name)
        .into_iter()
        .flatten()
        .any(|member| {
            if member.is_property == expected.is_property {
                return false;
            }
            let mut expected = expected.clone();
            expected.is_property = member.is_property;
            generic_signature_matches(member, &expected)
        });
    generic_conflict
        || (expected.method_type_params.is_empty()
            && class
                .instance_methods
                .get(&candidate.name)
                .into_iter()
                .flatten()
                .any(|member| {
                    member.is_property != expected.is_property
                        && member.params.len() == expected.params.len()
                        && member.params.iter().zip(&expected.params).all(
                            |((_, actual), (_, expected))| {
                                crate::typechecker::subtyping::identical(actual, expected)
                            },
                        )
                }))
}

fn has_explicit_member(class: &ClassTypeSignature, candidate: &SuppliedDefault) -> bool {
    class
        .generic_instance_methods
        .get(&candidate.name)
        .into_iter()
        .flatten()
        .any(|member| generic_signature_matches(member, &candidate.definition))
        || class
            .instance_methods
            .get(&candidate.name)
            .into_iter()
            .flatten()
            .any(|member| {
                candidate.definition.method_type_params.is_empty()
                    && member.is_property == candidate.definition.is_property
                    && member.params == candidate.definition.params
            })
}

impl Collector<'_> {
    pub(super) fn collect_generic_class_defaults(&mut self, class: &ClassDecl, class_fqn: &Fqn) {
        let Some(mut signature) = self.package_registry.get_class_type(class_fqn).cloned() else {
            return;
        };
        let parameters = Type::type_param_map(&signature.type_params, &signature.trait_bounds);
        let receiver = class_receiver(&signature, &parameters);
        let mut supplied = Vec::new();
        for application in &class.implements {
            let TypeExpr::Named(named) = application else {
                continue;
            };
            let Some(application) = self.resolve_default_application(named, &parameters, &receiver)
            else {
                continue;
            };
            for member in default_members(&application.signature) {
                let Some(candidate) = application.candidate(&signature, &member) else {
                    continue;
                };
                self.register_class_default(
                    &class.name.value,
                    &named.span,
                    &mut signature,
                    candidate,
                    &mut supplied,
                );
            }
        }
        self.package_registry
            .register_class_type(class_fqn.clone(), signature);
    }

    fn resolve_default_application(
        &mut self,
        application: &NamedType,
        parameters: &BTreeMap<String, Type>,
        receiver: &Type,
    ) -> Option<DefaultApplication> {
        let trait_fqn = self.resolve_name_to_fqn(&application.name.value, |fqn| {
            self.package_registry.get_trait(fqn).is_some()
                || self.dependency_registry.get_trait(fqn).is_some()
        })?;
        let signature = self
            .package_registry
            .get_trait(&trait_fqn)
            .or_else(|| self.dependency_registry.get_trait(&trait_fqn))?
            .clone();
        let parameters: Vec<_> = application
            .type_args
            .iter()
            .map(|ty| self.resolve_type_expr_with_type_params(ty, parameters))
            .collect();
        let mut substitution: BTreeMap<_, _> = signature
            .type_params
            .iter()
            .cloned()
            .zip(parameters.iter().cloned())
            .collect();
        substitution.insert(TypeParamName("Self".to_string()), receiver.clone());
        Some(DefaultApplication {
            signature,
            parameters,
            substitution,
        })
    }

    fn register_class_default(
        &mut self,
        class_name: &str,
        span: &Span,
        class: &mut ClassTypeSignature,
        candidate: SuppliedDefault,
        supplied: &mut Vec<SuppliedDefault>,
    ) {
        if has_member_kind_conflict(class, &candidate) {
            self.diagnostics.error(span.clone(), format!(
                "class '{class_name}' has conflicting method and property declarations for '{}'", candidate.name,
            ));
            return;
        }
        if let Some(previous) = supplied.iter().find(|previous| {
            previous.name == candidate.name
                && generic_signature_matches(&previous.definition, &candidate.definition)
        }) {
            if previous.source != candidate.source
                || previous.source_parameters != candidate.source_parameters
            {
                self.diagnostics.error(span.clone(), format!(
                    "class '{class_name}' inherits conflicting defaults for '{}'; define the member explicitly", candidate.name,
                ));
            }
            return;
        }
        if has_explicit_member(class, &candidate)
            || self.has_inherited_trait_member(
                class,
                &candidate.name,
                &candidate.definition.params,
                &candidate.definition.return_type,
                &candidate.definition.method_type_params,
                candidate.definition.is_property,
            )
        {
            return;
        }
        class
            .generic_instance_methods
            .entry(candidate.name.clone())
            .or_default()
            .push(candidate.definition.clone());
        class
            .default_supplied_members
            .insert(candidate.name.clone(), candidate.source.clone());
        supplied.push(candidate);
    }

    pub(super) fn has_inherited_trait_member(
        &self,
        class: &crate::typechecker::registry::ClassTypeSignature,
        name: &SymbolName,
        parameters: &[(String, Type)],
        result: &Type,
        method_parameters: &[TypeParamName],
        is_property: bool,
    ) -> bool {
        let mut parent = class.parent_type_expr.clone().or_else(|| {
            class.parent_class.as_ref().map(|fqn| {
                Type::Class(
                    fqn.clone(),
                    crate::common::types::MangledName::for_type(fqn),
                )
            })
        });
        while let Some(parent_type) = parent {
            let Some(fqn) = parent_type.try_to_fqn() else {
                break;
            };
            let Some(signature) = self
                .package_registry
                .get_class_type(&fqn)
                .or_else(|| self.dependency_registry.get_class_type(&fqn))
            else {
                break;
            };
            let arguments: Vec<_> = match &parent_type {
                Type::GenericClass { type_args, .. } => {
                    type_args.iter().map(|(_, ty)| ty.clone()).collect()
                }
                _ => vec![],
            };
            let substitution: BTreeMap<_, _> = signature
                .type_params
                .iter()
                .cloned()
                .zip(arguments)
                .collect();
            let matches = |actual: &[(String, Type)],
                           actual_result: &Type,
                           substitution: &BTreeMap<TypeParamName, Type>| {
                actual.len() == parameters.len()
                    && actual.iter().zip(parameters).all(
                        |((actual_name, actual), (expected_name, expected))| {
                            if actual_name == "self" || expected_name == "self" {
                                actual_name == expected_name
                            } else {
                                crate::typechecker::subtyping::identical(
                                    &substitute_trait_type_params(actual, substitution),
                                    expected,
                                )
                            }
                        },
                    )
                    && crate::typechecker::subtyping::identical(
                        &substitute_trait_type_params(actual_result, substitution),
                        result,
                    )
            };
            if method_parameters.is_empty()
                && signature
                    .instance_methods
                    .get(name)
                    .into_iter()
                    .flatten()
                    .any(|member| {
                        member.is_property == is_property
                            && matches(&member.params, &member.return_type, &substitution)
                    })
            {
                return true;
            }
            for member in signature
                .generic_instance_methods
                .get(name)
                .into_iter()
                .flatten()
            {
                if member.is_property != is_property
                    || member.method_type_params.len() != method_parameters.len()
                {
                    continue;
                }
                let mut member_substitution = substitution.clone();
                member_substitution.extend(
                    member.method_type_params.iter().cloned().zip(
                        method_parameters
                            .iter()
                            .map(|name| Type::TypeVariable(name.clone(), vec![])),
                    ),
                );
                if matches(&member.params, &member.return_type, &member_substitution) {
                    return true;
                }
            }
            parent = signature
                .parent_type_expr
                .as_ref()
                .map(|ty| substitute_trait_type_params(ty, &substitution))
                .or_else(|| {
                    signature.parent_class.as_ref().map(|fqn| {
                        Type::Class(
                            fqn.clone(),
                            crate::common::types::MangledName::for_type(fqn),
                        )
                    })
                });
        }
        false
    }
}

fn generic_signature_matches(
    actual: &GenericClassMethodDef,
    expected: &GenericClassMethodDef,
) -> bool {
    if actual.is_property != expected.is_property
        || actual.method_type_params.len() != expected.method_type_params.len()
        || actual.params.len() != expected.params.len()
    {
        return false;
    }
    let substitution: BTreeMap<_, _> = actual
        .method_type_params
        .iter()
        .cloned()
        .zip(
            expected
                .method_type_params
                .iter()
                .map(|name| Type::TypeVariable(name.clone(), vec![])),
        )
        .collect();
    actual
        .params
        .iter()
        .zip(&expected.params)
        .all(|((_, actual), (_, expected))| {
            crate::typechecker::subtyping::identical(
                &substitute_trait_type_params(actual, &substitution),
                expected,
            )
        })
}
