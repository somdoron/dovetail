use std::collections::BTreeMap;

use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName};
use crate::typechecker::registry::{ClassTypeSignature, Registry};
use crate::typechecker::types::Type;

use super::collect::substitute_trait_type_params as apply_type_substitution;

/// A method's own generic arity remains part of its declaration identity even
/// when those parameters do not occur in its value parameter types.
pub(crate) fn template_name(
    fqn: &Fqn,
    parameters: &[impl std::fmt::Display],
    method_parameter_count: usize,
) -> MangledName {
    let name = MangledName::for_function(fqn, parameters);
    if method_parameter_count == 0 {
        name
    } else {
        MangledName(format!("{name}$methodParams{method_parameter_count}"))
    }
}

struct MemberContract {
    name: SymbolName,
    parameters: Vec<(String, Type)>,
    method_parameter_count: usize,
    is_property: bool,
}

/// Select a class declaration by the instantiated trait signature, retaining
/// template arguments so the normal specialization pass can generate its body.
pub(crate) fn resolve_template(
    registry: &Registry,
    trait_fqn: &Fqn,
    trait_parameters: &[Type],
    receiver: &Type,
    member: &SymbolName,
    method_parameters: &[Type],
) -> Option<(MangledName, Vec<Type>)> {
    let contract = member_contract(
        registry,
        trait_fqn,
        trait_parameters,
        receiver,
        member,
        method_parameters,
    )?;
    let mut current = receiver.clone();
    while let Some(class) = current
        .try_to_fqn()
        .and_then(|fqn| registry.get_class_type(&fqn))
    {
        let class_parameters: Vec<_> = match &current {
            Type::GenericClass { type_args, .. } => {
                type_args.iter().map(|(_, ty)| ty.clone()).collect()
            }
            _ => vec![],
        };
        if let Some(template) =
            class_template(class, &class_parameters, &contract, method_parameters)
        {
            return Some(template);
        }
        current = crate::typechecker::subtyping::class_parent(registry, &current)?;
    }
    None
}

fn member_contract(
    registry: &Registry,
    trait_fqn: &Fqn,
    trait_parameters: &[Type],
    receiver: &Type,
    member: &SymbolName,
    method_parameters: &[Type],
) -> Option<MemberContract> {
    let signature = registry.get_trait(trait_fqn)?;
    let method = signature
        .methods
        .iter()
        .find(|method| signature.method_dispatch_name(method) == *member);
    let property = signature
        .properties
        .iter()
        .find(|property| property.name == member.0);
    let (name, parameters, own_parameters) = match (method, property) {
        (Some(method), _) => (&method.name, &method.params, method.type_params.as_slice()),
        (_, Some(property)) => (&property.name, &property.params, &[][..]),
        _ => return None,
    };
    if own_parameters.len() != method_parameters.len() {
        return None;
    }
    let mut substitution: BTreeMap<_, _> = signature
        .type_params
        .iter()
        .cloned()
        .zip(trait_parameters.iter().cloned())
        .collect();
    substitution.extend(
        own_parameters
            .iter()
            .cloned()
            .zip(method_parameters.iter().cloned()),
    );
    substitution.insert(TypeParamName("Self".to_string()), receiver.clone());
    Some(MemberContract {
        name: SymbolName(name.clone()),
        parameters: parameters
            .iter()
            .map(|(name, ty)| (name.clone(), apply_type_substitution(ty, &substitution)))
            .collect(),
        method_parameter_count: own_parameters.len(),
        is_property: method.is_none(),
    })
}

fn class_template(
    class: &ClassTypeSignature,
    class_parameters: &[Type],
    contract: &MemberContract,
    method_parameters: &[Type],
) -> Option<(MangledName, Vec<Type>)> {
    let class_substitution: BTreeMap<_, _> = class
        .type_params
        .iter()
        .cloned()
        .zip(class_parameters.iter().cloned())
        .collect();
    // Instantiating a generic trait method must not turn it into an unrelated
    // non-generic overload that happens to take the same concrete argument.
    if contract.method_parameter_count == 0 {
        for method in class
            .instance_methods
            .get(&contract.name)
            .into_iter()
            .flatten()
            .chain(
                class
                    .static_methods
                    .get(&contract.name)
                    .into_iter()
                    .flatten(),
            )
        {
            if !method.is_abstract_method
                && method.is_property == contract.is_property
                && parameters_match(&method.params, &contract.parameters, &class_substitution)
            {
                return Some((method.mangled_name.clone(), vec![]));
            }
        }
    }
    for method in class
        .generic_instance_methods
        .get(&contract.name)
        .into_iter()
        .flatten()
        .chain(
            class
                .generic_static_methods
                .get(&contract.name)
                .into_iter()
                .flatten(),
        )
    {
        let mut substitution = class_substitution.clone();
        substitution.extend(
            method
                .method_type_params
                .iter()
                .cloned()
                .zip(method_parameters.iter().cloned()),
        );
        if method.is_abstract_method
            || method.is_property != contract.is_property
            || method.method_type_params.len() != contract.method_parameter_count
            || !parameters_match(&method.params, &contract.parameters, &substitution)
        {
            continue;
        }
        let fqn = Fqn {
            package: class.fqn.package.clone(),
            symbol: SymbolName(format!("{}.{}", class.fqn.symbol, contract.name)),
        };
        let raw_parameters: Vec<_> = method.params.iter().map(|(_, ty)| ty).collect();
        let arguments = class_parameters
            .iter()
            .chain(method_parameters)
            .cloned()
            .collect();
        return Some((
            template_name(&fqn, &raw_parameters, method.method_type_params.len()),
            arguments,
        ));
    }
    None
}

fn parameters_match(
    actual: &[(String, Type)],
    expected: &[(String, Type)],
    substitution: &BTreeMap<TypeParamName, Type>,
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|((name, actual), (expected_name, expected))| {
                if name == "self" || expected_name == "self" {
                    return name == expected_name;
                }
                crate::typechecker::subtyping::identical(
                    &apply_type_substitution(actual, substitution),
                    expected,
                )
            })
}
