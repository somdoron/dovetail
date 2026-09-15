use std::collections::{BTreeMap, BTreeSet};

use crate::common::types::{MangledName, SymbolName, TypeParamName};
use crate::parser::ast::FunctionDecl;
use crate::typechecker::collect::{rename_method_bounds, substitute_trait_type_params};
use crate::typechecker::registry::{ClassTypeSignature, TraitMethodSig, TraitSignature};
use crate::typechecker::subtyping::class_parent;
use crate::typechecker::types::{TraitBounds, Type};

use super::Inference;

impl Inference<'_> {
    /// An inherited member used to satisfy a new trait application must accept
    /// every call permitted by that trait, just like an explicit implementation.
    pub(super) fn check_inherited_class_trait_contracts(&mut self, class: &ClassTypeSignature) {
        for (trait_fqn, arguments) in &class.trait_impls {
            let Some(signature) = self
                .registry
                .lookup_trait(trait_fqn, &self.package_path)
                .cloned()
            else {
                continue;
            };
            let mut substitution: BTreeMap<_, _> = signature
                .type_params
                .iter()
                .cloned()
                .zip(arguments.iter().cloned())
                .collect();
            substitution.insert(TypeParamName("Self".to_string()), class_type(class));
            for contract in &signature.methods {
                let application = TraitMethodApplication::new(contract, class, &substitution);
                let Some(requirement) =
                    self.inherited_trait_requirement(class, contract, &application)
                else {
                    continue;
                };
                let evidence = Type::type_param_map(&application.type_params, &application.bounds)
                    .into_iter()
                    .map(|(name, ty)| (TypeParamName(name), ty))
                    .collect();
                let arguments: Vec<_> = requirement
                    .arguments
                    .iter()
                    .map(|ty| substitute_trait_type_params(ty, &evidence))
                    .collect();
                for failure in self.unsatisfied_trait_bounds(
                    &requirement.bounds,
                    &requirement.type_params,
                    &arguments,
                ) {
                    self.diagnostics.error(
                        class.span.clone(),
                        format!(
                            "inherited method '{}' cannot strengthen its trait contract: {failure}",
                            contract.name,
                        ),
                    );
                }
            }
        }
    }

    fn inherited_trait_requirement(
        &self,
        class: &ClassTypeSignature,
        contract: &TraitMethodSig,
        application: &TraitMethodApplication,
    ) -> Option<InheritedRequirement> {
        let mut receiver = class_type(class);
        let mut visited = BTreeSet::new();
        let name = SymbolName(contract.name.clone());
        let instance = contract
            .params
            .first()
            .is_some_and(|(name, _)| name == "self");
        loop {
            let fqn = receiver.try_to_fqn()?;
            if !visited.insert(fqn.clone()) {
                return None;
            }
            let signature = self.registry.lookup_class_type(&fqn, &self.package_path)?;
            let arguments = match &receiver {
                Type::GenericClass { type_args, .. } => {
                    type_args.iter().map(|(_, ty)| ty.clone()).collect()
                }
                _ => Vec::new(),
            };
            let substitution: BTreeMap<_, _> = signature
                .type_params
                .iter()
                .cloned()
                .zip(arguments)
                .collect();
            let ordinary = if instance {
                &signature.instance_methods
            } else {
                &signature.static_methods
            };
            if application.method_params.is_empty()
                && ordinary.get(&name).is_some_and(|methods| {
                    methods.iter().any(|method| {
                        !method.is_property
                            && application.matches(
                                &method.params,
                                &method.return_type,
                                &substitution,
                            )
                    })
                })
            {
                return None;
            }
            let generic = if instance {
                &signature.generic_instance_methods
            } else {
                &signature.generic_static_methods
            };
            for method in generic.get(&name).into_iter().flatten() {
                if method.is_property
                    || method.method_type_params.len() != application.method_params.len()
                {
                    continue;
                }
                let mut combined = substitution.clone();
                combined.extend(
                    method.method_type_params.iter().cloned().zip(
                        application
                            .method_params
                            .iter()
                            .map(|name| Type::TypeVariable(name.clone(), vec![])),
                    ),
                );
                if !application.matches(&method.params, &method.return_type, &combined) {
                    continue;
                }
                // Local declarations have their own contract check. Synthesized
                // defaults already carry the trait's declared requirements.
                if fqn == class.fqn {
                    return None;
                }
                let type_params: Vec<_> = signature
                    .type_params
                    .iter()
                    .chain(&method.method_type_params)
                    .cloned()
                    .collect();
                let arguments = type_params
                    .iter()
                    .map(|name| combined[name].clone())
                    .collect();
                return Some(InheritedRequirement {
                    bounds: method.trait_bounds.clone(),
                    type_params,
                    arguments,
                });
            }
            receiver = class_parent(self.registry, &receiver)?;
        }
    }

    pub(super) fn check_class_method_contract(
        &mut self,
        method: &FunctionDecl,
        class: &ClassTypeSignature,
    ) {
        if class.trait_impls.is_empty() && (!method.is_override || method.where_clause.is_empty()) {
            return;
        }
        let method_params: Vec<_> = method
            .type_params
            .iter()
            .map(|parameter| TypeParamName(parameter.value.clone()))
            .collect();
        let all_params: Vec<_> = class
            .type_params
            .iter()
            .chain(&method_params)
            .cloned()
            .collect();
        let required =
            self.resolve_trait_bounds_from_where_clause(&method.where_clause, &all_params);
        let previous = self.current_type_params.clone();
        let mut signature_bounds = class.trait_bounds.clone();
        signature_bounds.merge(&required);
        let scope = self.type_param_map(&all_params, &signature_bounds);
        self.current_type_params.extend(
            scope
                .into_iter()
                .map(|(name, ty)| (TypeParamName(name), ty)),
        );
        let parameter_types: Vec<_> = method
            .params
            .iter()
            .skip(1)
            .map(|parameter| self.resolve_type_expr(&parameter.type_annotation))
            .collect();
        self.current_type_params = previous;
        for (trait_fqn, arguments) in &class.trait_impls {
            let implements_member = self
                .registry
                .lookup_trait(trait_fqn, &self.package_path)
                .is_some_and(|signature| {
                    class_method_matches_trait_contract(
                        method,
                        &method_params,
                        &parameter_types,
                        signature,
                        arguments,
                        class_type(class),
                    )
                });
            if implements_member {
                self.implementation_method_bounds(
                    method,
                    &class_type(class),
                    trait_fqn,
                    arguments,
                    &class.type_params,
                    &class.trait_bounds,
                    &BTreeMap::new(),
                );
            }
        }
        if !method.is_override || method.where_clause.is_empty() {
            return;
        }
        let Some(inherited) =
            self.inherited_method_bounds(method, class, &method_params, &parameter_types)
        else {
            return;
        };
        let mut available = class.trait_bounds.clone();
        available.merge(&inherited);
        let evidence = Type::type_param_map(&all_params, &available);
        let arguments: Vec<_> = all_params
            .iter()
            .map(|name| evidence[&name.0].clone())
            .collect();
        for failure in self.unsatisfied_trait_bounds(&required, &all_params, &arguments) {
            self.diagnostics.error(
                method.name.span.clone(),
                format!(
                    "override '{}' cannot strengthen its inherited method contract: {failure}",
                    method.name.value,
                ),
            );
        }
    }

    fn inherited_method_bounds(
        &self,
        method: &FunctionDecl,
        class: &ClassTypeSignature,
        method_params: &[TypeParamName],
        parameter_types: &[Type],
    ) -> Option<TraitBounds> {
        let mut receiver = class_type(class);
        let mut visited = BTreeSet::new();
        while let Some(parent) = class_parent(self.registry, &receiver) {
            let fqn = parent.try_to_fqn()?;
            if !visited.insert(fqn.clone()) {
                return None;
            }
            let signature = self.registry.lookup_class_type(&fqn, &self.package_path)?;
            let name = SymbolName(method.name.value.clone());
            if let Some(definitions) = signature.generic_instance_methods.get(&name) {
                let arguments = match &parent {
                    Type::GenericClass { type_args, .. } => {
                        type_args.iter().map(|(_, ty)| ty.clone()).collect()
                    }
                    _ => Vec::new(),
                };
                let substitution: BTreeMap<_, _> = signature
                    .type_params
                    .iter()
                    .cloned()
                    .zip(arguments)
                    .collect();
                let definition = definitions.iter().find(|definition| {
                    if definition.params.len() != method.params.len()
                        || definition.method_type_params.len() != method_params.len()
                    {
                        return false;
                    }
                    let mut combined = substitution.clone();
                    combined.extend(
                        definition.method_type_params.iter().cloned().zip(
                            method_params
                                .iter()
                                .cloned()
                                .map(|name| Type::TypeVariable(name, vec![])),
                        ),
                    );
                    definition.params.iter().skip(1).zip(parameter_types).all(
                        |((_, inherited), actual)| {
                            let inherited = substitute_trait_type_params(inherited, &combined);
                            crate::typechecker::subtyping::identical(&inherited, actual)
                        },
                    )
                });
                if let Some(definition) = definition {
                    return Some(inherited_bounds(
                        &definition.trait_bounds,
                        &substitution,
                        &definition.method_type_params,
                        method_params,
                    ));
                }
            }
            if method_params.is_empty()
                && signature
                    .instance_methods
                    .get(&name)
                    .is_some_and(|methods| {
                        methods.iter().any(|candidate| {
                            candidate.params.len() == method.params.len()
                                && candidate.params.iter().skip(1).zip(parameter_types).all(
                                    |((_, inherited), actual)| {
                                        crate::typechecker::subtyping::identical(inherited, actual)
                                    },
                                )
                        })
                    })
            {
                return Some(TraitBounds::empty());
            }
            receiver = parent;
        }
        None
    }
}

struct InheritedRequirement {
    bounds: TraitBounds,
    type_params: Vec<TypeParamName>,
    arguments: Vec<Type>,
}

struct TraitMethodApplication {
    type_params: Vec<TypeParamName>,
    method_params: Vec<TypeParamName>,
    parameters: Vec<(String, Type)>,
    result: Type,
    bounds: TraitBounds,
}

impl TraitMethodApplication {
    fn new(
        contract: &TraitMethodSig,
        class: &ClassTypeSignature,
        substitution: &BTreeMap<TypeParamName, Type>,
    ) -> Self {
        let method_params: Vec<_> = contract
            .type_params
            .iter()
            .enumerate()
            .map(|(index, _)| TypeParamName(format!("$contract${index}")))
            .collect();
        let mut combined = substitution.clone();
        combined.extend(
            contract.type_params.iter().cloned().zip(
                method_params
                    .iter()
                    .map(|name| Type::TypeVariable(name.clone(), vec![])),
            ),
        );
        let mut bounds = class.trait_bounds.clone();
        bounds.merge(&rename_method_bounds(
            &contract.trait_bounds,
            &contract.type_params,
            &method_params,
            substitution,
        ));
        Self {
            type_params: class
                .type_params
                .iter()
                .chain(&method_params)
                .cloned()
                .collect(),
            method_params,
            parameters: contract
                .params
                .iter()
                .map(|(name, ty)| (name.clone(), substitute_trait_type_params(ty, &combined)))
                .collect(),
            result: substitute_trait_type_params(&contract.return_type, &combined),
            bounds,
        }
    }

    fn matches(
        &self,
        parameters: &[(String, Type)],
        result: &Type,
        substitution: &BTreeMap<TypeParamName, Type>,
    ) -> bool {
        parameters.len() == self.parameters.len()
            && parameters.iter().zip(&self.parameters).all(
                |((name, actual), (expected_name, expected))| {
                    if name == "self" || expected_name == "self" {
                        return name == expected_name;
                    }
                    crate::typechecker::subtyping::identical(
                        &substitute_trait_type_params(actual, substitution),
                        expected,
                    )
                },
            )
            && crate::typechecker::subtyping::identical(
                &substitute_trait_type_params(result, substitution),
                &self.result,
            )
    }
}

fn class_method_matches_trait_contract(
    method: &FunctionDecl,
    method_params: &[TypeParamName],
    parameter_types: &[Type],
    signature: &TraitSignature,
    arguments: &[Type],
    self_type: Type,
) -> bool {
    let mut substitution: BTreeMap<_, _> = signature
        .type_params
        .iter()
        .cloned()
        .zip(arguments.iter().cloned())
        .collect();
    substitution.insert(TypeParamName("Self".to_string()), self_type);
    signature.methods.iter().any(|candidate| {
        if candidate.name != method.name.value
            || candidate.params.len() != method.params.len()
            || candidate.type_params.len() != method_params.len()
        {
            return false;
        }
        let mut combined = substitution.clone();
        combined.extend(
            candidate.type_params.iter().cloned().zip(
                method_params
                    .iter()
                    .cloned()
                    .map(|name| Type::TypeVariable(name, vec![])),
            ),
        );
        candidate
            .params
            .iter()
            .skip(1)
            .zip(parameter_types)
            .all(|((_, contract), actual)| {
                let contract = substitute_trait_type_params(contract, &combined);
                crate::typechecker::subtyping::identical(&contract, actual)
            })
    })
}

fn class_type(class: &ClassTypeSignature) -> Type {
    if class.type_params.is_empty() {
        return Type::Class(class.fqn.clone(), MangledName::for_type(&class.fqn));
    }
    let parameters = Type::type_param_map(&class.type_params, &class.trait_bounds);
    Type::GenericClass {
        fqn: class.fqn.clone(),
        mangled_name: MangledName::for_type(&class.fqn),
        type_args: class
            .type_param_variances
            .iter()
            .cloned()
            .zip(
                class
                    .type_params
                    .iter()
                    .map(|name| parameters[&name.0].clone()),
            )
            .collect(),
    }
}

fn inherited_bounds(
    bounds: &TraitBounds,
    substitution: &BTreeMap<TypeParamName, Type>,
    original_method_params: &[TypeParamName],
    method_params: &[TypeParamName],
) -> TraitBounds {
    let mut result = TraitBounds::empty();
    for (parameter, requirements) in bounds.iter() {
        let target = if let Some(index) = original_method_params
            .iter()
            .position(|name| name == parameter)
        {
            &method_params[index]
        } else {
            match substitution.get(parameter) {
                Some(Type::TypeVariable(name, _) | Type::GenericParam(name, _, _)) => name,
                Some(_) => continue,
                None => parameter,
            }
        };
        let mut entry = TraitBounds::empty();
        entry.insert(parameter.clone(), requirements.clone());
        let renamed =
            rename_method_bounds(&entry, original_method_params, method_params, substitution);
        for (_, requirements) in renamed.iter() {
            result.insert(target.clone(), requirements.clone());
        }
    }
    result
}
