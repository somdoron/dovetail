use std::path::Path;

use tower_lsp::lsp_types::Location;

use crate::common::span::Span;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{Type, TypedModule};

use super::diagnostics::{file_path_to_uri, span_to_range};
use super::position::NodeAtPosition;

/// Resolve a node to its definition location.
pub fn goto_definition(
    node: &NodeAtPosition,
    typed_module: &TypedModule,
    registry: &Registry,
    workspace_root: &Path,
) -> Option<Location> {
    match node {
        NodeAtPosition::FunctionCall { name, .. } | NodeAtPosition::FunctionRef { name, .. } => {
            let func = super::source_functions::get(typed_module, name)?;
            span_to_location(&func.span, workspace_root)
        }
        NodeAtPosition::MethodRef { method_name, .. } => {
            let func = super::source_functions::get(typed_module, method_name)?;
            span_to_location(&func.span, workspace_root)
        }
        NodeAtPosition::FieldAccess { receiver_type, .. } => {
            let fqn = type_to_fqn(receiver_type)?;
            type_fqn_to_location(&fqn, registry, workspace_root)
        }
        NodeAtPosition::GlobalRef { name, .. } => {
            let global = typed_module.globals.get(name)?;
            span_to_location(&global.span, workspace_root)
        }
        NodeAtPosition::RecordCreate { fqn, .. } => {
            let sig = registry.get_record_type(fqn)?;
            span_to_location(&sig.span, workspace_root)
        }
        NodeAtPosition::EnumCreate { fqn, .. } => {
            let sig = registry.get_enum_type(fqn)?;
            span_to_location(&sig.span, workspace_root)
        }
        NodeAtPosition::ClassNew { mangled_name, .. } => {
            // Find class FQN via the TypeDef
            if let Some(crate::typechecker::types::TypeDef::Class(class_def)) =
                typed_module.types.get(mangled_name)
            {
                type_fqn_to_location(&class_def.fqn, registry, workspace_root)
            } else {
                None
            }
        }
        NodeAtPosition::TypeRef { ty, .. } => {
            let fqn = type_to_fqn(ty)?;
            type_fqn_to_location(&fqn, registry, workspace_root)
        }
        // Local variables and let bindings are not cross-file navigable
        NodeAtPosition::VarRef { .. } | NodeAtPosition::Let { .. } => None,
        // Fallback: try to navigate to the expression's type definition
        NodeAtPosition::TypedExpr { ty, .. } => {
            let fqn = type_to_fqn(ty)?;
            type_fqn_to_location(&fqn, registry, workspace_root)
        }
    }
}

/// Navigate to the definition of the expression's type.
pub fn goto_type_definition(
    node: &NodeAtPosition,
    registry: &Registry,
    workspace_root: &Path,
) -> Option<Location> {
    let ty = match node {
        NodeAtPosition::FunctionCall { .. } => return None, // return type is not a named type
        NodeAtPosition::FunctionRef { ty, .. } => ty,
        NodeAtPosition::MethodRef { .. } => return None,
        NodeAtPosition::FieldAccess { span: _, receiver_type: _, field_name: _ } => {
            // The expression type is the field type — we need it from the node
            return None;
        }
        NodeAtPosition::VarRef { ty, .. } => ty,
        NodeAtPosition::GlobalRef { ty, .. } => ty,
        NodeAtPosition::RecordCreate { fqn, .. } => {
            return type_fqn_to_location(fqn, registry, workspace_root);
        }
        NodeAtPosition::EnumCreate { fqn, .. } => {
            return type_fqn_to_location(fqn, registry, workspace_root);
        }
        NodeAtPosition::ClassNew { mangled_name, .. } => {
            // Extract FQN from the mangled name by looking up class types
            for (_, sig) in registry.all_class_types() {
                if crate::common::types::MangledName::for_type(&sig.fqn) == *mangled_name {
                    return type_fqn_to_location(&sig.fqn, registry, workspace_root);
                }
            }
            return None;
        }
        NodeAtPosition::TypeRef { ty, .. } => ty,
        NodeAtPosition::Let { ty, .. } => ty,
        NodeAtPosition::TypedExpr { ty, .. } => ty,
    };

    let fqn = type_to_fqn(ty)?;
    type_fqn_to_location(&fqn, registry, workspace_root)
}

/// Convert a Span to an LSP Location.
fn span_to_location(span: &Span, workspace_root: &Path) -> Option<Location> {
    let uri = file_path_to_uri(workspace_root, &span.file)?;
    Some(Location {
        uri,
        range: span_to_range(span),
    })
}

/// Look up a type's declaration span from its FQN in the registry.
fn type_fqn_to_location(
    fqn: &crate::common::types::Fqn,
    registry: &Registry,
    workspace_root: &Path,
) -> Option<Location> {
    // Try record, enum, class, trait, newtype, type alias in order
    if let Some(sig) = registry.get_record_type(fqn) {
        return span_to_location(&sig.span, workspace_root);
    }
    if let Some(sig) = registry.get_enum_type(fqn) {
        return span_to_location(&sig.span, workspace_root);
    }
    if let Some(sig) = registry.get_class_type(fqn) {
        return span_to_location(&sig.span, workspace_root);
    }
    if let Some(sig) = registry.get_trait(fqn) {
        return span_to_location(&sig.span, workspace_root);
    }
    if let Some(sig) = registry.get_newtype_type(fqn) {
        return span_to_location(&sig.span, workspace_root);
    }
    None
}

/// Extract an FQN from a type, skipping primitives.
fn type_to_fqn(ty: &Type) -> Option<crate::common::types::Fqn> {
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
        // Primitives and other types don't have navigable definitions
        _ => None,
    }
}
