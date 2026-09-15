//! Reject `static` fields/globals on generic classes and generic modules whose declared
//! type references the container's type parameters.
//!
//! Why: under full type erasure, a generic class shares one WASM struct (and therefore one
//! WASM global per static declaration) across all instantiations. A `T`-typed static would
//! need different storage per instantiation, which the layout doesn't accommodate. For
//! generic modules the implementation happens to work via global monomorphization, but the
//! user-facing semantics ("static" = one canonical storage) are nonsensical when the module
//! is parameterized — there's no canonical `Foo` when `Foo<T>` has type parameters. Reject
//! both for language hygiene.
//!
//! Statics with concrete types (e.g., `static var count: Int32 = 0` on a generic class) are
//! fine — they don't reference the container's type parameters.

use crate::common::diagnostics::Diagnostics;
use crate::common::types::TypeParamName;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::Type;

/// Walk the registry and report errors for any static (class or module) whose type contains
/// a type parameter from its enclosing generic container.
pub fn check_generic_static_rules(registry: &Registry, diagnostics: &mut Diagnostics) {
    // Generic classes.
    for (class_fqn, class_sig) in registry.all_class_types() {
        if class_sig.type_params.is_empty() {
            continue;
        }
        for (member_name, def) in &class_sig.generic_static_globals {
            let Some(ref ty) = def.ty else { continue };
            if let Some(offending) = first_offending_type_param(ty, &class_sig.type_params) {
                diagnostics.error(
                    def.body.span(),
                    format!(
                        "static `{}` on generic class `{}` references type parameter `{}`; \
                         statics on generic types must have a type independent of the type parameters",
                        member_name.0, class_fqn, offending.0,
                    ),
                );
            }
        }
    }

    // Generic modules.
    for (module_fqn, module_info) in registry.all_modules() {
        // A module is generic iff its type_param_variances vector is non-empty (parallel to
        // the underlying generic class/enum/record's type_params). We don't have direct
        // access to the module's `type_params` here, so we infer them from any generic
        // global's `type_params` field. If no generic globals exist, the module isn't
        // generic at the type-parameter level.
        let Some(any_def) = module_info.generic_globals.values().next() else {
            continue;
        };
        let module_type_params = &any_def.type_params;
        if module_type_params.is_empty() {
            continue;
        }
        for (member_name, def) in &module_info.generic_globals {
            if let Some(offending) = first_offending_type_param(&def.ty, module_type_params) {
                diagnostics.error(
                    def.body.span(),
                    format!(
                        "global `{}` on generic module `{}` references type parameter `{}`; \
                         globals on generic modules must have a type independent of the type parameters",
                        member_name.0, module_fqn, offending.0,
                    ),
                );
            }
        }
    }
}

/// Return the first type parameter from `type_params` that appears anywhere in `ty`, or
/// `None` if `ty` is independent of all of them.
fn first_offending_type_param<'a>(
    ty: &Type,
    type_params: &'a [TypeParamName],
) -> Option<&'a TypeParamName> {
    type_params
        .iter()
        .find(|tp| ty.contains_type_parameter_named(tp))
}
