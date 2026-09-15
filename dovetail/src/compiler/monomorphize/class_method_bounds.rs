use std::collections::BTreeMap;

use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName};
use crate::typechecker::infer::generic_bounds_satisfied;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind, TypedFunction, VtableSlot};

pub(super) fn specialize_vtable_method(
    registry: &Registry,
    slot: &VtableSlot,
    template: &TypedFunction,
    substitution: &BTreeMap<TypeParamName, Type>,
    name: MangledName,
) -> TypedFunction {
    let mut template = template.clone();
    if !method_available(registry, slot, &template, substitution) {
        // The class still needs its stable virtual layout, but this member cannot
        // be called at this instantiation. Do not specialize its invalid body.
        template.body = TypedExpr {
            kind: TypedExprKind::Panic {
                message: Box::new(TypedExpr {
                    kind: TypedExprKind::StringLiteral("unavailable constrained method".to_string()),
                    ty: Type::String,
                    span: template.span.clone(),
                }),
            },
            ty: Type::Never,
            span: template.span.clone(),
        };
    }
    super::substitute::substitute_types_in_function(&template, substitution, name)
}

fn method_available(
    registry: &Registry,
    slot: &VtableSlot,
    template: &TypedFunction,
    substitution: &BTreeMap<TypeParamName, Type>,
) -> bool {
    let Some((owner, method)) = slot.impl_fqn.symbol.0.rsplit_once('.') else { return true };
    let owner = Fqn { package: slot.impl_fqn.package.clone(), symbol: SymbolName(owner.to_string()) };
    let Some(class) = registry.get_class_type(&owner) else { return true };
    let Some(methods) = class.generic_instance_methods.get(&SymbolName(method.to_string())) else { return true };
    let Some(definition) = methods.iter().find(|definition| {
        let parameters: Vec<_> = definition.params.iter().map(|(_, ty)| ty).collect();
        MangledName::for_function(&slot.impl_fqn, &parameters) == template.name
    }) else { return true };
    let Some(arguments) = template.type_params.iter().map(|name| substitution.get(name).cloned())
        .collect::<Option<Vec<_>>>() else { return true };
    generic_bounds_satisfied(registry, &owner.package, &definition.trait_bounds, &template.type_params, &arguments)
}
