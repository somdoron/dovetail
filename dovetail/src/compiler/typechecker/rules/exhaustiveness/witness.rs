//! Rendering an uncovered value back into something the user recognises.

use crate::typechecker::types::Type;

use super::ctor::{Ctor, PatCx};

/// An example value not covered by any arm.
#[derive(Debug, Clone)]
pub(super) enum Witness {
    Wild,
    Ctor { ctor: Ctor, fields: Vec<Witness> },
}

/// Renders a witness, using the column type to pick the notation.
///
/// Rendering is driven by type rather than by how the arms were written: a
/// witness is synthesized from the constructor set, so it has no source pattern
/// to inherit notation from. That also means a hand-written `case Cons(h, t)`
/// still gets list notation in the message, which is what the value is.
pub(super) fn render(cx: &PatCx<'_>, w: &Witness, ty: &Type) -> String {
    if let Some(list_fqn) = &cx.list_fqn {
        let is_list = match ty {
            Type::Enum(fqn, _) | Type::GenericEnum { fqn, .. } => fqn == list_fqn,
            _ => false,
        };
        if is_list {
            if let Some(rendered) = render_list(cx, w, ty) {
                return rendered;
            }
        }
    }
    render_plain(cx, w, ty)
}

/// Walks a `Cons` spine into `[a, b]`, or `[a, ...]` when the tail is unknown.
/// Returns `None` if the witness is not spine-shaped, so the caller falls back.
fn render_list(cx: &PatCx<'_>, w: &Witness, ty: &Type) -> Option<String> {
    let elem_ty = match ty {
        Type::GenericEnum { type_args, .. } => type_args.first().map(|(_, t)| t.clone()),
        _ => None,
    };
    let mut parts: Vec<String> = Vec::new();
    let mut current = w;
    loop {
        match current {
            Witness::Ctor { ctor, fields } => match ctor {
                Ctor::Variant { name, .. } if name == "Nil" => {
                    return Some(format!("[{}]", parts.join(", ")));
                }
                Ctor::Variant { name, .. } if name == "Cons" && fields.len() == 2 => {
                    parts.push(match &elem_ty {
                        Some(t) => render(cx, &fields[0], t),
                        None => "_".to_string(),
                    });
                    current = &fields[1];
                }
                _ => return None,
            },
            // An unconstrained tail: any longer list also escapes.
            Witness::Wild => {
                parts.push("...".to_string());
                return Some(format!("[{}]", parts.join(", ")));
            }
        }
    }
}

fn render_plain(cx: &PatCx<'_>, w: &Witness, ty: &Type) -> String {
    match w {
        Witness::Wild => "_".to_string(),
        Witness::Ctor { ctor, fields } => match ctor {
            Ctor::Bool(v) => v.to_string(),
            Ctor::Lit(v) => v.to_string(),
            Ctor::ClassLeaf(region) => region.describe(),
            Ctor::ClassTest(ty) => ty.to_string(),
            Ctor::Variant { name, .. } => {
                if fields.is_empty() {
                    name.clone()
                } else {
                    let field_types = cx.ctor_field_types(ty, ctor).unwrap_or_default();
                    let rendered = render_fields(cx, fields, &field_types);
                    format!("{name}({})", rendered.join(", "))
                }
            }
            Ctor::Single => {
                let field_types = cx.ctor_field_types(ty, ctor).unwrap_or_default();
                let rendered = render_fields(cx, fields, &field_types);
                match ty {
                    Type::Tuple(..) => format!("({})", rendered.join(", ")),
                    Type::Record(fqn, _) | Type::GenericRecord { fqn, .. } => {
                        format!("{} {{ {} }}", fqn.symbol.0, rendered.join(", "))
                    }
                    Type::Newtype(fqn, _) | Type::GenericNewtype { fqn, .. } => {
                        format!("{}({})", fqn.symbol.0, rendered.join(", "))
                    }
                    _ => rendered.join(", "),
                }
            }
        },
    }
}

fn render_fields(cx: &PatCx<'_>, fields: &[Witness], field_types: &[Type]) -> Vec<String> {
    fields
        .iter()
        .enumerate()
        .map(|(i, f)| match field_types.get(i) {
            Some(t) => render(cx, f, t),
            None => "_".to_string(),
        })
        .collect()
}
