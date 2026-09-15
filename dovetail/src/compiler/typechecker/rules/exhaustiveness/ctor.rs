//! Constructors, constructor sets, and the type-directed lowering of
//! `TypedPattern` into the matrix representation.

use std::collections::{BTreeMap, BTreeSet};

use crate::common::types::{Fqn, MangledName};
use crate::typechecker::infer::generics::apply_substitution;
use crate::typechecker::infer::type_param_substitution::TypeParamSubstitution;
use crate::typechecker::registry::{Registry, VariantPayload};
use crate::typechecker::types::{Type, TypeDef, TypedExpr, TypedExprKind, TypedPattern};

use super::matrix::Pat;

/// Metadata was unavailable, so no claim can be made about this match.
/// Propagates all the way out and suppresses the diagnostic entirely — a
/// missing lookup must never become a false "non-exhaustive".
#[derive(Debug, Clone, Copy)]
pub(super) struct Bail;

/// A value constructor for a column of the pattern matrix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ctor {
    /// An enum variant, tuple-form or record-form — both flatten to one
    /// positional payload list.
    Variant { name: String, index: u32 },
    /// The single constructor of a record, tuple, or newtype.
    Single,
    /// A `case x: C` type test. Only a constructor when the column is a sealed
    /// class; `fqn` may name a sealed intermediate, which covers its leaves.
    ClassTest(Type),
    /// A leaf of a sealed hierarchy. Only ever appears in a constructor *set*.
    ClassLeaf(super::class_regions::ClassRegion),
    Bool(bool),
    /// A literal drawn from an effectively infinite domain. Never completes a set.
    Lit(LitValue),
}

/// Concrete literal values, compared structurally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LitValue {
    Bool(bool),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    Uint8(u8),
    Uint16(u16),
    Uint32(u32),
    Uint64(u64),
    Uint128(u128),
    Float32(u32), // bits, for equality
    Float64(u64), // bits, for equality
    String(String),
    Char(char),
}

impl std::fmt::Display for LitValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LitValue::Bool(v) => write!(f, "{v}"),
            LitValue::Int8(v) => write!(f, "{v}"),
            LitValue::Int16(v) => write!(f, "{v}"),
            LitValue::Int32(v) => write!(f, "{v}"),
            LitValue::Int64(v) => write!(f, "{v}"),
            LitValue::Uint8(v) => write!(f, "{v}"),
            LitValue::Uint16(v) => write!(f, "{v}"),
            LitValue::Uint32(v) => write!(f, "{v}"),
            LitValue::Uint64(v) => write!(f, "{v}"),
            LitValue::Uint128(v) => write!(f, "{v}"),
            LitValue::Float32(bits) => write!(f, "{}", f32::from_bits(*bits)),
            LitValue::Float64(bits) => write!(f, "{}", f64::from_bits(*bits)),
            LitValue::String(v) => write!(f, "\"{v}\""),
            LitValue::Char(v) => write!(f, "'{v}'"),
        }
    }
}

/// The constructors a column's type can take.
pub(super) enum CtorSet {
    /// Complete and enumerable. The order here is the order missing cases are
    /// reported in, so it must stay stable.
    Finite(Vec<Ctor>),
    /// Effectively infinite (integers, floats, strings, chars) or structurally
    /// opaque (`Any`, interface objects, arrays, functions, type parameters,
    /// non-sealed classes). Only a wildcard row can cover it.
    Opaque,
    /// Metadata unavailable — abort the check.
    Unknown,
}

pub(super) struct PatCx<'a> {
    types: &'a BTreeMap<MangledName, TypeDef>,
    pub(super) registry: &'a Registry,
    pub(super) list_fqn: Option<Fqn>,
    /// `collect_sealed_leaves` is O(all classes); it is called from the inner
    /// `covers` loop, so memoize it.
    sealed_leaves: std::cell::RefCell<BTreeMap<Fqn, BTreeSet<Fqn>>>,
}

impl<'a> PatCx<'a> {
    pub(super) fn new(types: &'a BTreeMap<MangledName, TypeDef>, registry: &'a Registry) -> Self {
        Self {
            types,
            registry,
            list_fqn: Fqn::from_dotted("standard.prelude.List"),
            sealed_leaves: std::cell::RefCell::new(BTreeMap::new()),
        }
    }

    /// True when the type is a sealed class, whose leaves form a finite set.
    fn sealed_class_fqn(&self, ty: &Type) -> Option<Fqn> {
        let fqn = match ty {
            Type::Class(fqn, _) | Type::GenericClass { fqn, .. } => fqn,
            _ => return None,
        };
        match self.registry.get_class_type(fqn) {
            Some(sig) if sig.is_sealed => Some(fqn.clone()),
            _ => None,
        }
    }

    /// Leaf (final) descendants of a sealed class, memoized. Carries a visited
    /// set so a malformed class graph cannot loop.
    pub(super) fn sealed_leaves(&self, fqn: &Fqn) -> BTreeSet<Fqn> {
        if let Some(hit) = self.sealed_leaves.borrow().get(fqn) {
            return hit.clone();
        }
        let mut visited = BTreeSet::new();
        let leaves = self.collect_leaves(fqn, &mut visited);
        self.sealed_leaves
            .borrow_mut()
            .insert(fqn.clone(), leaves.clone());
        leaves
    }

    fn collect_leaves(&self, sealed_fqn: &Fqn, visited: &mut BTreeSet<Fqn>) -> BTreeSet<Fqn> {
        let mut leaves = BTreeSet::new();
        if !visited.insert(sealed_fqn.clone()) {
            return leaves;
        }
        for (fqn, sig) in self.registry.all_class_types() {
            if sig.parent_class.as_ref() == Some(sealed_fqn) {
                if sig.is_sealed && sig.is_abstract {
                    leaves.extend(self.collect_leaves(fqn, visited));
                } else {
                    // Final, or non-final (rejected elsewhere by class_rules but
                    // listed here so the error message names it).
                    leaves.insert(fqn.clone());
                }
            }
        }
        leaves
    }

    /// Variants of an enum type, with payload types substituted for the
    /// scrutinee's type arguments.
    ///
    /// Registry first: it carries payload types and type parameters for generic
    /// and dependency-package enums alike, while `types` only holds this
    /// package's monomorphized definitions.
    pub(super) fn enum_variants(&self, ty: &Type) -> Result<Vec<(String, Vec<Type>)>, Bail> {
        let (fqn, mangled, type_args): (&Fqn, &MangledName, Vec<Type>) = match ty {
            Type::Enum(fqn, mn) => (fqn, mn, Vec::new()),
            Type::GenericEnum {
                fqn,
                mangled_name,
                type_args,
            } => (
                fqn,
                mangled_name,
                type_args.iter().map(|(_, t)| t.clone()).collect(),
            ),
            _ => return Err(Bail),
        };

        if let Some(sig) = self.registry.get_enum_type(fqn) {
            if !type_args.is_empty() && sig.type_params.len() != type_args.len() {
                return Err(Bail);
            }
            let sub = (!type_args.is_empty())
                .then(|| TypeParamSubstitution::from_pairs(&sig.type_params, &type_args));
            return Ok(sig
                .variants
                .iter()
                .map(|(name, payload)| {
                    let flat: Vec<Type> = match payload {
                        VariantPayload::None => vec![],
                        VariantPayload::Tuple(ts) => ts.clone(),
                        VariantPayload::Record(fs) => fs.iter().map(|(_, t)| t.clone()).collect(),
                    };
                    let flat = match &sub {
                        Some(s) => flat.iter().map(|t| apply_substitution(s, t)).collect(),
                        None => flat,
                    };
                    (name.clone(), flat)
                })
                .collect());
        }

        // Fallback: this package's already-substituted concrete definition.
        if let Some(TypeDef::Enum(e)) = self.types.get(mangled) {
            return Ok(e
                .variants
                .iter()
                .map(|v| (v.name.to_string(), v.payload_types.clone()))
                .collect());
        }

        Err(Bail)
    }

    /// Field types of a record type, substituted for its type arguments.
    fn record_fields(&self, ty: &Type) -> Result<Vec<Type>, Bail> {
        let (fqn, type_args): (&Fqn, Vec<Type>) = match ty {
            Type::Record(fqn, _) => (fqn, Vec::new()),
            Type::GenericRecord { fqn, type_args, .. } => {
                (fqn, type_args.iter().map(|(_, t)| t.clone()).collect())
            }
            _ => return Err(Bail),
        };
        let sig = self.registry.get_record_type(fqn).ok_or(Bail)?;
        if !type_args.is_empty() && sig.type_params.len() != type_args.len() {
            return Err(Bail);
        }
        let sub = (!type_args.is_empty())
            .then(|| TypeParamSubstitution::from_pairs(&sig.type_params, &type_args));
        Ok(sig
            .fields
            .iter()
            .map(|(_, t)| match &sub {
                Some(s) => apply_substitution(s, t),
                None => t.clone(),
            })
            .collect())
    }

    fn newtype_inner(&self, ty: &Type) -> Option<Type> {
        match ty {
            Type::Newtype(_, inner) => Some((**inner).clone()),
            Type::GenericNewtype {
                concrete_inner_type,
                ..
            } => Some((**concrete_inner_type).clone()),
            _ => None,
        }
    }

    /// The constructors a column of this type can take.
    pub(super) fn ctor_set(&self, ty: &Type) -> CtorSet {
        match ty {
            Type::Bool => CtorSet::Finite(vec![Ctor::Bool(true), Ctor::Bool(false)]),
            Type::Enum(..) | Type::GenericEnum { .. } => match self.enum_variants(ty) {
                Ok(variants) => CtorSet::Finite(
                    variants
                        .into_iter()
                        .enumerate()
                        .map(|(i, (name, _))| Ctor::Variant {
                            name,
                            index: i as u32,
                        })
                        .collect(),
                ),
                Err(_) => CtorSet::Unknown,
            },
            Type::Record(..) | Type::GenericRecord { .. } => match self.record_fields(ty) {
                Ok(_) => CtorSet::Finite(vec![Ctor::Single]),
                Err(_) => CtorSet::Unknown,
            },
            Type::Tuple(..) => CtorSet::Finite(vec![Ctor::Single]),
            Type::Newtype(..) | Type::GenericNewtype { .. } => {
                CtorSet::Finite(vec![Ctor::Single])
            }
            Type::Class(..) | Type::GenericClass { .. } => match self.sealed_class_fqn(ty) {
                Some(fqn) => {
                    let leaves = self.sealed_leaves(&fqn);
                    if leaves.is_empty() {
                        // A sealed class with no subclasses: nothing can
                        // construct it, but say nothing rather than demand a
                        // case for a value that cannot exist.
                        CtorSet::Unknown
                    } else {
                        CtorSet::Finite(leaves.iter().filter_map(|leaf|super::class_regions::ClassRegion::new(self.registry,leaf,ty)).map(Ctor::ClassLeaf).collect())
                    }
                }
                // Non-sealed classes stay open: only a wildcard covers them.
                None => CtorSet::Opaque,
            },
            // Infinite domains and structurally opaque types. `Uint8` is finite
            // in principle but is deliberately left opaque — enumerating 256
            // constructors would be a behaviour change with no demand.
            _ => CtorSet::Opaque,
        }
    }

    pub(super) fn partitioned_ctor_set(&self, ty: &Type, patterns: &[Ctor]) -> CtorSet {
        let CtorSet::Finite(cases) = self.ctor_set(ty) else { return self.ctor_set(ty); };
        let mut out = Vec::new();
        for case in cases {
            if let Ctor::ClassLeaf(region) = case {
                let mut regions = vec![region];
                for pattern in patterns {
                    if let Ctor::ClassTest(target) = pattern {
                        regions = regions.into_iter().flat_map(|r|r.split(self.registry,target)).collect();
                        if regions.len() > 1024 { return CtorSet::Opaque; }
                    }
                }
                out.extend(regions.into_iter().map(Ctor::ClassLeaf));
            } else { out.push(case); }
        }
        CtorSet::Finite(out)
    }

    /// The column types produced by specializing `ty` on `ctor`.
    pub(super) fn ctor_field_types(&self, ty: &Type, ctor: &Ctor) -> Result<Vec<Type>, Bail> {
        match ctor {
            Ctor::Variant { name, .. } => {
                let variants = self.enum_variants(ty)?;
                variants
                    .into_iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, payload)| payload)
                    .ok_or(Bail)
            }
            Ctor::Single => match ty {
                Type::Record(..) | Type::GenericRecord { .. } => self.record_fields(ty),
                Type::Tuple(elems, _) => Ok(elems.clone()),
                _ => match self.newtype_inner(ty) {
                    Some(inner) => Ok(vec![inner]),
                    None => Err(Bail),
                },
            },
            // A class test binds the value but exposes no columns.
            Ctor::ClassTest(_) | Ctor::ClassLeaf(_) => Ok(vec![]),
            Ctor::Bool(_) | Ctor::Lit(_) => Ok(vec![]),
        }
    }

    /// Lowers a typed pattern against the type of the column it sits in.
    ///
    /// Type-directed rather than pattern-directed: whether `TypeAnnotated` is a
    /// constructor or a wildcard depends on whether the column is a sealed
    /// class.
    pub(super) fn lower(&self, pat: &TypedPattern, ty: &Type) -> Result<Pat, Bail> {
        match pat {
            TypedPattern::Wildcard | TypedPattern::Variable(..) => Ok(Pat::Wild),

            TypedPattern::TypeAnnotated { ty: annot, .. } => {
                if ty.is_any() || ty.is_class_type()
                    || (matches!(ty, Type::GenericRecord { .. } | Type::GenericEnum { .. })
                        && !crate::typechecker::subtyping::is_subtype(self.registry, ty, annot))
                {
                    Ok(Pat::Ctor { ctor: Ctor::ClassTest(annot.clone()), fields: vec![] })
                } else {
                    Ok(Pat::Wild)
                }
            }

            TypedPattern::Literal(expr) => {
                let lit = lit_value(expr).ok_or(Bail)?;
                match (ty, &lit) {
                    (Type::Bool, LitValue::Bool(v)) => Ok(Pat::Ctor {
                        ctor: Ctor::Bool(*v),
                        fields: vec![],
                    }),
                    _ => Ok(Pat::Ctor {
                        ctor: Ctor::Lit(lit),
                        fields: vec![],
                    }),
                }
            }

            TypedPattern::EnumVariant {
                variant_name,
                variant_index,
                payload_patterns,
                ..
            } => {
                let ctor = Ctor::Variant {
                    name: variant_name.clone(),
                    index: *variant_index,
                };
                let field_types = self.ctor_field_types(ty, &ctor)?;
                if payload_patterns.len() != field_types.len() {
                    return Err(Bail);
                }
                let fields = payload_patterns
                    .iter()
                    .zip(field_types.iter())
                    .map(|(p, t)| self.lower(p, t))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Pat::Ctor { ctor, fields })
            }

            TypedPattern::EnumVariantRecord {
                variant_name,
                variant_index,
                field_patterns,
                ..
            } => {
                let ctor = Ctor::Variant {
                    name: variant_name.clone(),
                    index: *variant_index,
                };
                let field_types = self.ctor_field_types(ty, &ctor)?;
                let fields = self.lower_by_index(field_patterns, &field_types)?;
                Ok(Pat::Ctor { ctor, fields })
            }

            TypedPattern::Record { fields, .. } => {
                let field_types = self.record_fields(ty)?;
                let lowered = self.lower_by_index(fields, &field_types)?;
                Ok(Pat::Ctor {
                    ctor: Ctor::Single,
                    fields: lowered,
                })
            }

            TypedPattern::Tuple {
                element_patterns, ..
            } => {
                let Type::Tuple(elems, _) = ty else {
                    return Err(Bail);
                };
                if element_patterns.len() != elems.len() {
                    return Err(Bail);
                }
                let fields = element_patterns
                    .iter()
                    .zip(elems.iter())
                    .map(|(p, t)| self.lower(p, t))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Pat::Ctor {
                    ctor: Ctor::Single,
                    fields,
                })
            }

            TypedPattern::Newtype { inner_pattern, .. } => {
                let inner_ty = self.newtype_inner(ty).ok_or(Bail)?;
                Ok(Pat::Ctor {
                    ctor: Ctor::Single,
                    fields: vec![self.lower(inner_pattern, &inner_ty)?],
                })
            }
        }
    }

    /// Lowers positionally-indexed field patterns, filling unmentioned
    /// positions with wildcards.
    fn lower_by_index(
        &self,
        fields: &[crate::typechecker::types::TypedFieldPattern],
        field_types: &[Type],
    ) -> Result<Vec<Pat>, Bail> {
        let mut out = vec![Pat::Wild; field_types.len()];
        for f in fields {
            let idx = f.field_index as usize;
            let ty = field_types.get(idx).ok_or(Bail)?;
            out[idx] = self.lower(&f.pattern, ty)?;
        }
        Ok(out)
    }
}

/// The literal value of a pattern expression, or `None` when it is not one this
/// analysis can compare — which aborts the check rather than panicking.
pub(super) fn lit_value(expr: &TypedExpr) -> Option<LitValue> {
    Some(match &expr.kind {
        TypedExprKind::BoolLiteral(v) => LitValue::Bool(*v),
        TypedExprKind::Int8Literal(v) => LitValue::Int8(*v),
        TypedExprKind::Int16Literal(v) => LitValue::Int16(*v),
        TypedExprKind::Int32Literal(v) => LitValue::Int32(*v),
        TypedExprKind::Int64Literal(v) => LitValue::Int64(*v),
        TypedExprKind::Uint8Literal(v) => LitValue::Uint8(*v),
        TypedExprKind::Uint16Literal(v) => LitValue::Uint16(*v),
        TypedExprKind::Uint32Literal(v) => LitValue::Uint32(*v),
        TypedExprKind::Uint64Literal(v) => LitValue::Uint64(*v),
        TypedExprKind::Uint128Literal(v) => LitValue::Uint128(*v),
        TypedExprKind::Float32Literal(v) => LitValue::Float32(v.to_bits()),
        TypedExprKind::Float64Literal(v) => LitValue::Float64(v.to_bits()),
        TypedExprKind::StringLiteral(v) => LitValue::String(v.clone()),
        TypedExprKind::CharLiteral(v) => LitValue::Char(*v),
        _ => return None,
    })
}
