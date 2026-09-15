use crate::common::diagnostics::Diagnostics;
use crate::typechecker::registry::{Registry, TraitSignature};

/// Enforce object safety on every `interface` declaration.
///
/// An interface is a trait whose methods can all be dispatched through a
/// vtable, so its name may appear in type position (interface object). The
/// check runs at the declaration — the error lands on the offending member,
/// not at a use site. Plain traits are exempt: they are bound-only and keep
/// full type-class power.
pub(super) fn check_object_safety(package_registry: &Registry, diagnostics: &mut Diagnostics) {
    for trait_sig in package_registry.traits() {
        if trait_sig.is_interface {
            check_interface(trait_sig, diagnostics);
        }
    }
}

fn check_interface(trait_sig: &TraitSignature, diagnostics: &mut Diagnostics) {
    let interface_name = &trait_sig.fqn.symbol.0;

    // Associated types (and GATs) cannot be recovered from an erased receiver.
    for assoc in &trait_sig.associated_types {
        diagnostics.error(
            assoc.span.clone(),
            format!(
                "interface '{}' cannot declare associated type '{}'; use a trait",
                interface_name, assoc.name
            ),
        );
    }

    for method in &trait_sig.methods {
        // A vtable dispatches on a receiver; a method without `self` has
        // nothing to dispatch on.
        let takes_self = method.params.first().is_some_and(|(name, _)| name == "self");
        if !takes_self {
            diagnostics.error(
                method.span.clone(),
                format!(
                    "method '{}' on interface '{}' must take self; put static functions on a trait",
                    method.name, interface_name
                ),
            );
        }

        // A monomorphizing AOT compiler cannot instantiate a generic method
        // reached through an erased receiver.
        if !method.type_params.is_empty() {
            diagnostics.error(
                method.span.clone(),
                format!(
                    "method '{}' on interface '{}' cannot be generic",
                    method.name, interface_name
                ),
            );
        }

        // `Self` outside the receiver or return type would require two values
        // to share a concrete type unknowable behind a fat pointer. `Self` as
        // the return type is fine — it is observed as the interface type.
        check_self_in_params(
            &method.params,
            &method.name,
            "method",
            interface_name,
            &method.span,
            diagnostics,
        );

        // A *bare* `Self` return is re-boxed to the interface at the vtable
        // boundary; `Self` nested inside another type (`Option<Self>`, ...)
        // cannot be, so it is rejected.
        check_self_in_return(
            &method.return_type,
            &method.name,
            "method",
            interface_name,
            &method.span,
            diagnostics,
        );
    }

    for property in &trait_sig.properties {
        let takes_self = property.params.first().is_some_and(|(name, _)| name == "self");
        if !takes_self {
            diagnostics.error(
                property.span.clone(),
                format!(
                    "property '{}' on interface '{}' must take self; put static properties on a trait",
                    property.name, interface_name
                ),
            );
        }

        check_self_in_params(
            &property.params,
            &property.name,
            "property",
            interface_name,
            &property.span,
            diagnostics,
        );

        check_self_in_return(
            &property.return_type,
            &property.name,
            "property",
            interface_name,
            &property.span,
            diagnostics,
        );
    }
}

fn check_self_in_return(
    return_type: &crate::typechecker::types::Type,
    member_name: &str,
    member_kind: &str,
    interface_name: &str,
    span: &crate::common::span::Span,
    diagnostics: &mut Diagnostics,
) {
    use crate::typechecker::types::Type;
    if !matches!(return_type, Type::SelfType) && return_type.contains_self_type() {
        diagnostics.error(
            span.clone(),
            format!(
                "{} '{}' on interface '{}' returns a type containing 'Self'; \
                 only a bare Self return is supported (it is observed as the interface type)",
                member_kind, member_name, interface_name
            ),
        );
    }
}

fn check_self_in_params(
    params: &[(String, crate::typechecker::types::Type)],
    member_name: &str,
    member_kind: &str,
    interface_name: &str,
    span: &crate::common::span::Span,
    diagnostics: &mut Diagnostics,
) {
    let has_self_typed_param = params
        .iter()
        .filter(|(name, _)| name != "self")
        .any(|(_, ty)| ty.contains_self_type());
    if has_self_typed_param {
        diagnostics.error(
            span.clone(),
            format!(
                "{} '{}' on interface '{}' has a parameter of type 'Self'; \
                 Self may appear only as the receiver or the return type",
                member_kind, member_name, interface_name
            ),
        );
    }
}
