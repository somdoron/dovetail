//! Per-instantiation keys for concrete types.
//!
//! A interface object's WASM *type* is shared per-trait (de-monomorphized), but each impl
//! *instantiation* needs its own vtable instance — its methods are monomorphized by the concrete
//! type args. A generic impl class's erased mangled name collapses `Foo<A>` and `Foo<B>` to one
//! canonical name, so it can't distinguish them; [`instance_key`] does, by encoding the full
//! instantiation structurally.

use crate::common::types::{Fqn, Variance};
use crate::typechecker::types::Type;

/// A per-instantiation key for a concrete type, used to key interface-object vtable globals and wrapper
/// functions. Encodes the full instantiation (every FQN and, recursively, every type argument), so
/// distinct instantiations of a generic impl class get distinct keys. Built by [`instance_key`],
/// which encodes the type structurally (a unique tag per `Type` variant, delimited args) rather
/// than via `Display` — so it is unambiguous and stable.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub(super) struct InstanceKey(String);

impl std::fmt::Display for InstanceKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Structurally encode a concrete type into its per-instantiation [`InstanceKey`].
pub(super) fn instance_key(ty: &Type) -> InstanceKey {
    let mut s = String::new();
    encode_instance_key(&mut s, ty);
    InstanceKey(s)
}

fn encode_fqn(out: &mut String, fqn: &Fqn) {
    for seg in &fqn.package.0 {
        out.push_str(seg);
        out.push('.');
    }
    out.push_str(&fqn.symbol.0);
}

/// Encode a generic named type: `<tag>`fqn`(`arg₀`,`arg₁`…)`.
fn encode_generic(out: &mut String, tag: &str, fqn: &Fqn, type_args: &[(Variance, Type)]) {
    out.push_str(tag);
    encode_fqn(out, fqn);
    out.push('(');
    for (_variance, arg) in type_args {
        encode_instance_key(out, arg);
        out.push(',');
    }
    out.push_str(");");
}

/// Append a structural, unambiguous encoding of `ty` to `out`. Each variant leads with a distinct
/// tag and brackets its sub-parts, so two structurally different types can never produce the same
/// bytes. Concrete impl-receiver types never carry type parameters, but the type-parameter arms are
/// handled defensively for completeness.
fn encode_instance_key(out: &mut String, ty: &Type) {
    match ty {
        Type::AssociatedProjection(projection) => {
            out.push_str("associated(");
            encode_fqn(out, &projection.trait_fqn);
            out.push_str(&format!(";{};{};", projection.member, projection.trait_parameters.len()));
            for parameter in projection.types() { encode_instance_key(out, parameter); }
            out.push(')');
        },
        Type::TupleProjection(receiver, kind) => { out.push_str(&format!("projection:{kind:?};")); encode_instance_key(out, receiver); }
        Type::TupleExtend(left, right) => { out.push_str("extend;"); encode_instance_key(out, left); encode_instance_key(out, right); }
        Type::Unit => out.push_str("u;"),
        Type::Bool => out.push_str("b;"),
        Type::String => out.push_str("s;"),
        Type::Char => out.push_str("c;"),
        Type::Int8 => out.push_str("i8;"),
        Type::Int16 => out.push_str("i16;"),
        Type::Int32 => out.push_str("i32;"),
        Type::Int64 => out.push_str("i64;"),
        Type::Uint8 => out.push_str("U8;"),
        Type::Uint16 => out.push_str("U16;"),
        Type::Uint32 => out.push_str("U32;"),
        Type::Uint64 => out.push_str("U64;"),
        Type::Uint128 => out.push_str("U128;"),
        Type::Float32 => out.push_str("f32;"),
        Type::Float64 => out.push_str("f64;"),
        Type::Never => out.push_str("!;"),
        Type::Any => out.push_str("*;"),
        Type::Error => out.push_str("err;"),
        Type::SelfType => out.push_str("self;"),
        Type::Record(fqn, _) => { out.push('R'); encode_fqn(out, fqn); out.push(';'); }
        Type::Enum(fqn, _) => { out.push('E'); encode_fqn(out, fqn); out.push(';'); }
        Type::Class(fqn, _) => { out.push('C'); encode_fqn(out, fqn); out.push(';'); }
        Type::GenericRecord { fqn, type_args, .. } => encode_generic(out, "gR", fqn, type_args),
        Type::GenericEnum { fqn, type_args, .. } => encode_generic(out, "gE", fqn, type_args),
        Type::GenericClass { fqn, type_args, .. } => encode_generic(out, "gC", fqn, type_args),
        Type::GenericNewtype { fqn, type_args, .. } => encode_generic(out, "gN", fqn, type_args),
        Type::Newtype(fqn, inner) => { out.push('N'); encode_fqn(out, fqn); out.push('{'); encode_instance_key(out, inner); out.push_str("};"); }
        Type::Array(elem) => { out.push('['); encode_instance_key(out, elem); out.push_str("];"); }
        Type::Tuple(elems, _) => {
            out.push('(');
            for e in elems { encode_instance_key(out, e); }
            out.push_str(");");
        }
        Type::Function(params, ret) => {
            out.push('F');
            out.push('(');
            for p in params { encode_instance_key(out, p); }
            out.push_str("->");
            encode_instance_key(out, ret);
            out.push_str(");");
        }
        Type::InterfaceObject { traits, .. } => {
            // Len-1 encoding is byte-identical to the historical single-trait
            // form; intersections join component encodings with '&'.
            out.push('O');
            for (i, c) in traits.iter().enumerate() {
                if i > 0 { out.push('&'); }
                encode_fqn(out, &c.trait_fqn);
                out.push('(');
                for t in &c.trait_type_args { encode_instance_key(out, t); }
                out.push(')');
            }
            out.push(';');
        }
        Type::TypeVariable(name, _) => { out.push('V'); out.push_str(&name.0); out.push(';'); }
        Type::GenericParam(name, _, id) => {
            out.push('P');
            out.push_str(&name.0);
            out.push('#');
            out.push_str(&id.to_string());
            out.push(';');
        }
        Type::TypeConstructor { name, type_args } => {
            out.push('K');
            out.push_str(&name.0);
            out.push('(');
            for t in type_args { encode_instance_key(out, t); }
            out.push_str(");");
        }
    }
}
