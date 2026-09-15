use std::path::Path;

use tower_lsp::lsp_types::Location;

use crate::common::types::Fqn;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{Type, TypeDef, TypedModule};

use super::diagnostics::{file_path_to_uri, span_to_range};
use super::position::NodeAtPosition;

/// Find implementations of a trait or subclasses of a class.
pub fn goto_implementation(
    node: &NodeAtPosition,
    typed_module: &TypedModule,
    registry: &Registry,
    workspace_root: &Path,
) -> Vec<Location> {
    let target_fqn = match node {
        // Type references → extract FQN
        NodeAtPosition::RecordCreate { fqn, .. } | NodeAtPosition::EnumCreate { fqn, .. } => {
            fqn.clone()
        }
        NodeAtPosition::ClassNew { mangled_name, .. } => {
            if let Some(TypeDef::Class(class_def)) = typed_module.types.get(mangled_name) {
                class_def.fqn.clone()
            } else {
                return Vec::new();
            }
        }
        // Function/method → check if it's a trait method
        NodeAtPosition::FunctionCall { name, .. }
        | NodeAtPosition::FunctionRef { name, .. }
        | NodeAtPosition::MethodRef {
            method_name: name, ..
        } => {
            if let Some(func) = super::source_functions::get(typed_module, name) {
                // Check if this function belongs to a trait by examining its name
                // Trait impl methods have mangled names like "trait_fqn.type_fqn.method"
                // Try to find a class owning this method for override lookup
                if let Some(class_fqn) = find_class_owning_method(registry, name) {
                    let method_short = extract_method_short_name(&name.0);
                    return find_method_overrides(
                        &class_fqn,
                        &method_short,
                        typed_module,
                        registry,
                        workspace_root,
                    );
                }
                // Try extracting FQN from the function span's file to see if it's a trait
                // For now, just use the function's vtable_self_type if present
                if let Some(self_type) = &func.vtable_self_type {
                    // This is a virtual method; find overrides
                    let method_short = extract_method_short_name(&name.0);
                    if let Some(fqn) = type_to_fqn(self_type) {
                        return find_method_overrides(
                            &fqn,
                            &method_short,
                            typed_module,
                            registry,
                            workspace_root,
                        );
                    }
                }
                return Vec::new();
            }
            return Vec::new();
        }
        // For typed expressions with a type, try to extract FQN
        NodeAtPosition::TypeRef { ty, .. }
        | NodeAtPosition::VarRef { ty, .. }
        | NodeAtPosition::GlobalRef { ty, .. }
        | NodeAtPosition::Let { ty, .. }
        | NodeAtPosition::TypedExpr { ty, .. } => match type_to_fqn(ty) {
            Some(fqn) => fqn,
            None => return Vec::new(),
        },
        NodeAtPosition::FieldAccess { .. } => return Vec::new(),
    };

    // Check if the target is a trait → find all implementations
    if registry.get_trait(&target_fqn).is_some() {
        return find_trait_implementations(&target_fqn, registry, workspace_root);
    }

    // Check if the target is a class → find all subclasses
    if registry.get_class_type(&target_fqn).is_some() {
        return find_subclasses(&target_fqn, registry, workspace_root);
    }

    Vec::new()
}

/// Find all trait implementation blocks for a given trait.
fn find_trait_implementations(
    trait_fqn: &Fqn,
    registry: &Registry,
    workspace_root: &Path,
) -> Vec<Location> {
    let mut locations = Vec::new();

    for block in registry.all_implement_blocks() {
        if block.trait_fqn == *trait_fqn
            && let Some(uri) = file_path_to_uri(workspace_root, &block.span.file)
        {
            locations.push(Location {
                uri,
                range: span_to_range(&block.span),
            });
        }
    }

    locations
}

/// Find all subclasses of a given class.
fn find_subclasses(parent_fqn: &Fqn, registry: &Registry, workspace_root: &Path) -> Vec<Location> {
    let mut locations = Vec::new();

    for (child_fqn, sig) in registry.all_class_types() {
        if child_fqn == parent_fqn {
            continue;
        }
        if registry.class_is_subtype(child_fqn, parent_fqn)
            && let Some(uri) = file_path_to_uri(workspace_root, &sig.span.file)
        {
            locations.push(Location {
                uri,
                range: span_to_range(&sig.span),
            });
        }
    }

    locations
}

/// Find overrides of a method in subclasses.
fn find_method_overrides(
    class_fqn: &Fqn,
    method_name: &str,
    typed_module: &TypedModule,
    registry: &Registry,
    workspace_root: &Path,
) -> Vec<Location> {
    let mut locations = Vec::new();

    // Walk all class type defs looking for vtable entries that override this method
    for type_def in typed_module.types.values() {
        if let TypeDef::Class(class_def) = type_def {
            if class_def.fqn == *class_fqn {
                continue;
            }
            // Check if this class is a subtype
            if !registry.class_is_subtype(&class_def.fqn, class_fqn) {
                continue;
            }
            // Check vtable_methods for an override of the target method
            for slot in &class_def.vtable_methods {
                if slot.method_name.0 == method_name {
                    // This class overrides the method — find the function span.
                    let vt_mangled = crate::common::types::MangledName::for_function(
                        &slot.impl_fqn,
                        &slot.param_types,
                    );
                    if let Some(func) = super::source_functions::get(typed_module, &vt_mangled)
                        && let Some(uri) = file_path_to_uri(workspace_root, &func.span.file)
                    {
                        locations.push(Location {
                            uri,
                            range: span_to_range(&func.span),
                        });
                    }
                }
            }
        }
    }

    locations
}

/// Find the class that owns a given method (by mangled name).
fn find_class_owning_method(
    registry: &Registry,
    method_mangled: &crate::common::types::MangledName,
) -> Option<Fqn> {
    for (class_fqn, sig) in registry.all_class_types() {
        for methods in sig.instance_methods.values() {
            for method_sig in methods {
                if method_sig.mangled_name == *method_mangled {
                    return Some(class_fqn.clone());
                }
            }
        }
    }
    None
}

/// Extract the short method name from a mangled name string.
/// E.g., "pkg.Class.method(Int32)" → "method"
fn extract_method_short_name(mangled: &str) -> String {
    // Strip param list if present
    let base = mangled.split('(').next().unwrap_or(mangled);
    // Take the last segment after the last dot
    base.rsplit('.').next().unwrap_or(base).to_string()
}

/// Extract FQN from a Type.
fn type_to_fqn(ty: &Type) -> Option<Fqn> {
    match ty {
        Type::Record(fqn, _)
        | Type::Enum(fqn, _)
        | Type::Class(fqn, _)
        | Type::GenericRecord { fqn, .. }
        | Type::GenericEnum { fqn, .. }
        | Type::GenericClass { fqn, .. }
        | Type::Newtype(fqn, _)
        | Type::GenericNewtype { fqn, .. } => Some(fqn.clone()),
        Type::InterfaceObject { traits, .. } => Some(traits[0].trait_fqn.clone()),
        _ => None,
    }
}
