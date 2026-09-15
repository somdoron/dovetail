//! WIT type → Dovetail type projection. The single source of truth for the
//! mapping: bindgen prints these sketches as source text, and (later phases
//! of) codegen walk the real typechecked `Type` in lockstep with the WIT
//! type, so no second name-resolution mechanism exists.

use wit_parser::{Resolve, Type, TypeDefKind};

use super::BindgenError;
use super::names;

/// Structural sketch of the Dovetail type a WIT type projects to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DovetailTypeSketch {
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    Float32,
    Float64,
    Char,
    String,
    Unit,
    Array(Box<DovetailTypeSketch>),
    Option(Box<DovetailTypeSketch>),
    Result(Box<DovetailTypeSketch>, Box<DovetailTypeSketch>),
    Tuple(Vec<DovetailTypeSketch>),
    /// A generated named type (record/enum/variant/flags/resource newtype),
    /// by its projected Dovetail short name.
    Named(String),
}

impl DovetailTypeSketch {
    /// Render as Dovetail source text (type position).
    pub fn render(&self) -> String {
        match self {
            DovetailTypeSketch::Bool => "Bool".into(),
            DovetailTypeSketch::Int8 => "Int8".into(),
            DovetailTypeSketch::Int16 => "Int16".into(),
            DovetailTypeSketch::Int32 => "Int32".into(),
            DovetailTypeSketch::Int64 => "Int64".into(),
            DovetailTypeSketch::Uint8 => "Uint8".into(),
            DovetailTypeSketch::Uint16 => "Uint16".into(),
            DovetailTypeSketch::Uint32 => "Uint32".into(),
            DovetailTypeSketch::Uint64 => "Uint64".into(),
            DovetailTypeSketch::Float32 => "Float32".into(),
            DovetailTypeSketch::Float64 => "Float64".into(),
            DovetailTypeSketch::Char => "Char".into(),
            DovetailTypeSketch::String => "String".into(),
            DovetailTypeSketch::Unit => "Unit".into(),
            DovetailTypeSketch::Array(inner) => format!("Array<{}>", inner.render()),
            DovetailTypeSketch::Option(inner) => format!("Option<{}>", inner.render()),
            DovetailTypeSketch::Result(ok, err) => {
                format!("Result<{}, {}>", ok.render(), err.render())
            }
            DovetailTypeSketch::Tuple(items) => {
                let inner: Vec<String> = items.iter().map(|t| t.render()).collect();
                format!("({})", inner.join(", "))
            }
            DovetailTypeSketch::Named(name) => name.clone(),
        }
    }
}

/// Project a WIT type. Named types must be declared in the same interface
/// (the v1 no-foreign-`use` restriction is enforced by the caller walking
/// interface types; here a named type simply projects to its Dovetail name).
pub fn map_wit_type(resolve: &Resolve, ty: &Type) -> Result<DovetailTypeSketch, BindgenError> {
    Ok(match ty {
        Type::Bool => DovetailTypeSketch::Bool,
        Type::U8 => DovetailTypeSketch::Uint8,
        Type::U16 => DovetailTypeSketch::Uint16,
        Type::U32 => DovetailTypeSketch::Uint32,
        Type::U64 => DovetailTypeSketch::Uint64,
        Type::S8 => DovetailTypeSketch::Int8,
        Type::S16 => DovetailTypeSketch::Int16,
        Type::S32 => DovetailTypeSketch::Int32,
        Type::S64 => DovetailTypeSketch::Int64,
        Type::F32 => DovetailTypeSketch::Float32,
        Type::F64 => DovetailTypeSketch::Float64,
        Type::Char => DovetailTypeSketch::Char,
        Type::String => DovetailTypeSketch::String,
        Type::ErrorContext => {
            return Err(BindgenError {
                message: "WIT type `error-context` is not supported yet".to_string(),
            });
        }
        Type::Id(id) => {
            let def = &resolve.types[*id];
            match &def.kind {
                TypeDefKind::List(inner) => {
                    DovetailTypeSketch::Array(Box::new(map_wit_type(resolve, inner)?))
                }
                TypeDefKind::Option(inner) => {
                    DovetailTypeSketch::Option(Box::new(map_wit_type(resolve, inner)?))
                }
                TypeDefKind::Result(r) => {
                    let ok = match &r.ok {
                        Some(t) => map_wit_type(resolve, t)?,
                        None => DovetailTypeSketch::Unit,
                    };
                    let err = match &r.err {
                        Some(t) => map_wit_type(resolve, t)?,
                        None => DovetailTypeSketch::Unit,
                    };
                    DovetailTypeSketch::Result(Box::new(ok), Box::new(err))
                }
                TypeDefKind::Tuple(t) => {
                    let items = t
                        .types
                        .iter()
                        .map(|t| map_wit_type(resolve, t))
                        .collect::<Result<Vec<_>, _>>()?;
                    DovetailTypeSketch::Tuple(items)
                }
                TypeDefKind::Record(_)
                | TypeDefKind::Variant(_)
                | TypeDefKind::Enum(_)
                | TypeDefKind::Flags(_)
                | TypeDefKind::Resource => {
                    let name = def.name.as_ref().ok_or_else(|| BindgenError {
                        message: "anonymous named WIT type has no name".to_string(),
                    })?;
                    DovetailTypeSketch::Named(names::pascal(name))
                }
                // `own<r>` / `borrow<r>` — both project to the resource's
                // handle newtype; ownership is documentation at this layer.
                TypeDefKind::Handle(handle) => {
                    let resource_id = match handle {
                        wit_parser::Handle::Own(id) | wit_parser::Handle::Borrow(id) => *id,
                    };
                    let def = &resolve.types[resource_id];
                    let name = def.name.as_ref().ok_or_else(|| BindgenError {
                        message: "resource type has no name".to_string(),
                    })?;
                    DovetailTypeSketch::Named(names::pascal(name))
                }
                // Type alias: project the target.
                TypeDefKind::Type(inner) => map_wit_type(resolve, inner)?,
                TypeDefKind::Future(_) | TypeDefKind::Stream(_) => {
                    return Err(BindgenError {
                        message: format!(
                            "WIT type `{}` uses async types (future/stream), \
                             which are not supported yet",
                            def.name.as_deref().unwrap_or("<anonymous>")
                        ),
                    });
                }
                TypeDefKind::Unknown => {
                    return Err(BindgenError {
                        message: "unresolved WIT type".to_string(),
                    });
                }
                other => {
                    return Err(BindgenError {
                        message: format!("WIT type kind `{}` is not supported yet", other.as_str()),
                    });
                }
            }
        }
    })
}
