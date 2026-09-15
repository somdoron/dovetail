use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};

use crate::typechecker::registry::Registry;
use crate::typechecker::types::TypedModule;

use super::diagnostics::span_to_range;
use super::position::NodeAtPosition;

/// Produce hover content for a node at the cursor position.
pub fn hover_for_node(
    node: &NodeAtPosition,
    typed_module: &TypedModule,
    registry: &Registry,
) -> Option<Hover> {
    let (code, doc, range) = match node {
        NodeAtPosition::FunctionCall { name, span } => {
            let func = super::source_functions::get(typed_module, name)?;
            let sig = format_function_signature(func);
            let fqn_str = func.name.0.split('$').next().unwrap_or(&func.name.0);
            let doc = crate::common::types::Fqn::from_dotted(fqn_str)
                .and_then(|fqn| registry.lookup_doc_comment(&fqn).map(str::to_string));
            (sig, doc, span_to_range(span))
        }
        NodeAtPosition::FunctionRef { name, span, .. } => {
            let func = super::source_functions::get(typed_module, name)?;
            let sig = format_function_signature(func);
            let fqn_str = func.name.0.split('$').next().unwrap_or(&func.name.0);
            let doc = crate::common::types::Fqn::from_dotted(fqn_str)
                .and_then(|fqn| registry.lookup_doc_comment(&fqn).map(str::to_string));
            (sig, doc, span_to_range(span))
        }
        NodeAtPosition::MethodRef { method_name, span } => {
            let func = super::source_functions::get(typed_module, method_name)?;
            let sig = format_function_signature(func);
            (sig, None, span_to_range(span))
        }
        NodeAtPosition::FieldAccess { receiver_type, field_name, span } => {
            let fqn = type_to_fqn(receiver_type)?;
            let field_type = lookup_field_type(registry, &fqn, field_name)?;
            let code = format!("{}: {}", field_name, field_type);
            let doc = registry
                .lookup_sub_doc_comment(&fqn, field_name)
                .map(str::to_string);
            (code, doc, span_to_range(span))
        }
        NodeAtPosition::VarRef { name, ty, span } => {
            let code = format!("let {}: {}", name, ty);
            (code, None, span_to_range(span))
        }
        NodeAtPosition::GlobalRef { name, ty, span } => {
            let global = typed_module.globals.get(name);
            let mutability = if global.is_some_and(|g| g.mutable) {
                "let mutable "
            } else {
                "let "
            };
            let display_name = name.0.rsplit('.').next().unwrap_or(&name.0);
            let code = format!("{}{}: {}", mutability, display_name, ty);
            let fqn_str = name.0.split('$').next().unwrap_or(&name.0);
            let doc = crate::common::types::Fqn::from_dotted(fqn_str)
                .and_then(|fqn| registry.lookup_doc_comment(&fqn).map(str::to_string));
            (code, doc, span_to_range(span))
        }
        NodeAtPosition::RecordCreate { fqn, span } => {
            let sig = registry.get_record_type(fqn)?;
            let fields: Vec<String> = sig
                .fields
                .iter()
                .map(|(name, ty)| format!("  {}: {}", name, ty))
                .collect();
            let code = format!("record {}\n{}", fqn.symbol, fields.join("\n"));
            let doc = registry.lookup_doc_comment(fqn).map(str::to_string);
            (code, doc, span_to_range(span))
        }
        NodeAtPosition::EnumCreate { fqn, variant_name, span } => {
            let sig = registry.get_enum_type(fqn)?;
            let variant_info = sig
                .variants
                .iter()
                .find(|(name, _)| name == variant_name);
            let code = match variant_info {
                Some((name, payload)) => format!("{}.{}{}", fqn.symbol, name, format_variant_payload(payload)),
                None => format!("{}.{}", fqn.symbol, variant_name),
            };
            let doc = registry
                .lookup_sub_doc_comment(fqn, variant_name)
                .map(str::to_string);
            (code, doc, span_to_range(span))
        }
        NodeAtPosition::ClassNew { mangled_name, span } => {
            // Find the class type def in the typed module for constructor params
            if let Some(crate::typechecker::types::TypeDef::Class(class_def)) =
                typed_module.types.get(mangled_name)
            {
                let params: Vec<String> = class_def
                    .constructor_params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.ty))
                    .collect();
                let code = format!("class {}({})", class_def.fqn.symbol, params.join(", "));
                let doc = registry
                    .lookup_doc_comment(&class_def.fqn)
                    .map(str::to_string);
                (code, doc, span_to_range(span))
            } else {
                return None;
            }
        }
        NodeAtPosition::Let { name, ty, span } => {
            let code = format!("let {}: {}", name, ty);
            (code, None, span_to_range(span))
        }
        NodeAtPosition::TypeRef { ty, span } => {
            let code = format_type_hover(ty, registry);
            let doc = type_doc_comment(ty, registry);
            (code, doc, span_to_range(span))
        }
        NodeAtPosition::TypedExpr { ty, span } => {
            let code = format!(": {}", ty);
            (code, None, span_to_range(span))
        }
    };

    let mut value = format!("```dovetail\n{}\n```", code);
    if let Some(doc) = doc {
        value.push_str("\n\n---\n\n");
        value.push_str(&doc);
    }

    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range: Some(range),
    })
}

/// Format a function signature for hover display.
fn format_function_signature(func: &crate::typechecker::types::TypedFunction) -> String {
    let params: Vec<String> = func
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty))
        .collect();
    let async_prefix = if func.is_async { "async " } else { "" };
    let display = if func.display_name.is_empty() {
        func.name.0.split('$').next().unwrap_or(&func.name.0)
    } else {
        &func.display_name
    };
    let name = display.rsplit('.').next().unwrap_or(display);
    let type_params_str = if func.type_params.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = func.type_params.iter().map(|tp| tp.0.as_str()).collect();
        format!("<{}>", names.join(", "))
    };
    format!(
        "{}function {}{}({}): {}",
        async_prefix,
        name,
        type_params_str,
        params.join(", "),
        func.return_type,
    )
}

/// Extract an FQN from a type for field lookup.
fn type_to_fqn(ty: &crate::typechecker::types::Type) -> Option<crate::common::types::Fqn> {
    use crate::typechecker::types::Type;
    match ty {
        Type::Record(fqn, _)
        | Type::Enum(fqn, _)
        | Type::Class(fqn, _)
        | Type::GenericRecord { fqn, .. }
        | Type::GenericEnum { fqn, .. }
        | Type::GenericClass { fqn, .. }
        | Type::Newtype(fqn, _)
        | Type::GenericNewtype { fqn, .. } => Some(fqn.clone()),
        _ => None,
    }
}

/// Look up a field's type from a type's FQN in the registry.
fn lookup_field_type(
    registry: &Registry,
    fqn: &crate::common::types::Fqn,
    field_name: &str,
) -> Option<crate::typechecker::types::Type> {
    // Check record types
    if let Some(sig) = registry.get_record_type(fqn) {
        for (name, ty) in &sig.fields {
            if name == field_name {
                return Some(ty.clone());
            }
        }
    }
    // Check class types
    if let Some(sig) = registry.get_class_type(fqn) {
        for field in &sig.fields {
            if field.name == field_name {
                return Some(field.ty.clone());
            }
        }
    }
    None
}

/// Format a type reference for hover display.
fn format_type_hover(ty: &crate::typechecker::types::Type, registry: &Registry) -> String {
    use crate::typechecker::types::Type;
    match ty {
        Type::Record(fqn, _) | Type::GenericRecord { fqn, .. } => {
            if let Some(sig) = registry.get_record_type(fqn) {
                let fields: Vec<String> = sig
                    .fields
                    .iter()
                    .map(|(name, ty)| format!("  {}: {}", name, ty))
                    .collect();
                format!("record {}\n{}", fqn.symbol, fields.join("\n"))
            } else {
                format!("record {}", fqn.symbol)
            }
        }
        Type::Enum(fqn, _) | Type::GenericEnum { fqn, .. } => {
            format!("enum {}", fqn.symbol)
        }
        Type::Class(fqn, _) | Type::GenericClass { fqn, .. } => {
            format!("class {}", fqn.symbol)
        }
        Type::Newtype(fqn, _) | Type::GenericNewtype { fqn, .. } => {
            if let Some(sig) = registry.get_newtype_type(fqn) {
                format!("newtype {} = {}", fqn.symbol, sig.inner_type)
            } else {
                format!("newtype {}", fqn.symbol)
            }
        }
        Type::InterfaceObject { traits, .. } => {
            let keyword = match traits.first().and_then(|c| registry.get_trait(&c.trait_fqn)) {
                Some(sig) if sig.is_interface => "interface",
                _ => "trait",
            };
            let names: Vec<String> = traits.iter().map(|c| c.trait_fqn.symbol.to_string()).collect();
            format!("{} {}", keyword, names.join(" and "))
        }
        _ => format!("{}", ty),
    }
}

/// Extract a doc comment for a type reference.
fn type_doc_comment(ty: &crate::typechecker::types::Type, registry: &Registry) -> Option<String> {
    let fqn = type_to_fqn(ty)?;
    registry.lookup_doc_comment(&fqn).map(str::to_string)
}

/// Format a variant payload for hover display.
fn format_variant_payload(payload: &crate::typechecker::registry::VariantPayload) -> String {
    use crate::typechecker::registry::VariantPayload;
    match payload {
        VariantPayload::None => String::new(),
        VariantPayload::Tuple(types) => {
            let parts: Vec<String> = types.iter().map(|t| t.to_string()).collect();
            format!("({})", parts.join(", "))
        }
        VariantPayload::Record(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(name, ty)| format!("{}: {}", name, ty))
                .collect();
            format!(" {{ {} }}", parts.join(", "))
        }
    }
}
