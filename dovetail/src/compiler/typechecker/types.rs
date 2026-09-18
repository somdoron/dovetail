use std::collections::BTreeMap;

use crate::common::span::Span;
use crate::common::types::{
    Fqn, InterfaceMemberName, MangledName, PackagePath, SymbolName, TypeParamName, VarName,
    Variance, Visibility,
};

/// Shape-dependent tuple projection, normalized after substitution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TupleProjection {
    Init,
    Last,
}

pub fn is_tuple_constraint(fqn: &Fqn) -> bool {
    fqn.package.to_string() == "standard.prelude" && fqn.symbol.0 == "Tuple"
}

/// The kind of a type parameter bound.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BoundKind {
    /// The type parameter must implement the given trait.
    HasTrait,
    /// The type parameter must be a subtype of the given class or primitive.
    SubtypeOf,
}

/// A single type parameter bound: either a trait bound or a nominal subtype bound.
/// E.g., `From<Int32>` has trait_fqn = From, type_args = [Int32], kind = HasTrait.
/// `Animal` with kind = SubtypeOf means "must be a subtype of class Animal".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NamedTraitBound {
    pub associated_types: BTreeMap<String, Type>,
    pub trait_fqn: Fqn,
    pub type_args: Vec<Type>,
    pub kind: BoundKind,
}

impl NamedTraitBound {
    /// Recover a primitive subtype bound without confusing a user class with
    /// the same short name for a prelude primitive.
    pub fn primitive_subtype(&self) -> Option<Type> {
        if self.kind != BoundKind::SubtypeOf {
            return None;
        }
        let primitive = Type::from_primitive(&self.trait_fqn.symbol.0)?;
        (primitive.to_fqn() == self.trait_fqn).then_some(primitive)
    }

    pub fn is_class_bound(&self) -> bool {
        self.kind == BoundKind::SubtypeOf && self.primitive_subtype().is_none()
    }
}

/// Class category bounds have no nominal identity or type arguments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TraitBound {
    Named(NamedTraitBound),
    IsClass,
}

impl TraitBound {
    pub fn named(&self) -> Option<&NamedTraitBound> {
        match self {
            Self::Named(bound) => Some(bound),
            Self::IsClass => None,
        }
    }
}

/// Trait bounds map: type parameter → required trait bounds.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct TraitBounds(BTreeMap<TypeParamName, Vec<TraitBound>>);

impl TraitBounds {
    /// An empty set of trait bounds.
    pub fn empty() -> Self {
        Self(BTreeMap::new())
    }

    /// Get the trait bounds for a type parameter.
    pub fn get(&self, tp: &TypeParamName) -> Option<&Vec<TraitBound>> {
        self.0.get(tp)
    }

    /// Iterate over all (type_param, trait_bounds) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&TypeParamName, &Vec<TraitBound>)> {
        self.0.iter()
    }

    /// Insert bounds for a type parameter, appending to any existing bounds.
    pub fn insert(&mut self, tp: TypeParamName, bounds: Vec<TraitBound>) {
        let existing = self.0.entry(tp).or_default();
        for bound in bounds {
            if let TraitBound::Named(incoming) = &bound
                && let Some(TraitBound::Named(current)) = existing.iter_mut().find(|b| {
                    b.named().is_some_and(|b| {
                        b.trait_fqn == incoming.trait_fqn
                            && b.type_args.len() == incoming.type_args.len()
                            && b.type_args
                                .iter()
                                .zip(&incoming.type_args)
                                .all(|(left, right)| super::subtyping::identical(left, right))
                            && b.kind == incoming.kind
                            && incoming.associated_types.iter().all(|(n, t)| {
                                b.associated_types
                                    .get(n)
                                    .is_none_or(|previous| super::subtyping::identical(previous, t))
                            })
                    })
                })
            {
                current
                    .associated_types
                    .extend(incoming.associated_types.clone());
                continue;
            }
            if !existing.contains(&bound) {
                existing.push(bound);
            }
        }
    }

    /// Find contradictory equalities for the same trait application.
    pub fn conflicting_associated_binding(
        &self,
        tp: &TypeParamName,
        incoming: &[TraitBound],
    ) -> Option<String> {
        let bounds: Vec<_> = self
            .get(tp)
            .into_iter()
            .flatten()
            .chain(incoming)
            .filter_map(TraitBound::named)
            .collect();
        for (i, left) in bounds.iter().enumerate() {
            for right in &bounds[i + 1..] {
                if left.trait_fqn != right.trait_fqn
                    || left.type_args.len() != right.type_args.len()
                    || !left
                        .type_args
                        .iter()
                        .zip(&right.type_args)
                        .all(|(left, right)| super::subtyping::identical(left, right))
                {
                    continue;
                }
                for (name, ty) in &left.associated_types {
                    if right
                        .associated_types
                        .get(name)
                        .is_some_and(|other| !super::subtyping::identical(other, ty))
                    {
                        return Some(name.clone());
                    }
                }
            }
        }
        None
    }

    /// Merge another set of trait bounds into this one.
    pub fn merge(&mut self, other: &TraitBounds) {
        for (tp, bounds) in other.iter() {
            self.insert(tp.clone(), bounds.clone());
        }
    }

    /// Check if there are no bounds.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// One interface in an interface-object type: the interface's FQN plus its
/// type arguments (empty for a non-generic interface).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceComponent {
    pub trait_fqn: Fqn,
    pub trait_type_args: Vec<Type>,
}

/// Representation of types in the type system.
///
/// Custom `PartialEq` and `Hash` implementations ignore `concrete_inner_type` in
/// `GenericNewtype` — it is a cached/derived value from `fqn + type_args` and should
/// not affect type identity.
#[derive(Debug, Clone)]
pub enum Type {
    /// The unit type.
    Unit,
    /// The boolean type.
    Bool,
    /// The string type.
    String,
    /// The character type (Unicode code point).
    Char,
    /// Signed integer types.
    Int8,
    Int16,
    Int32,
    Int64,
    /// Unsigned integer types.
    Uint8,
    Uint16,
    Uint32,
    Uint64,
    /// Unsigned 128-bit integer. A first-class integer primitive represented as a
    /// width-2 `[i64, i64]` (lo, hi) flattened run; boxes to a dedicated `$Uint128`
    /// struct with raw `i64` fields (never the generic anyref tuple).
    Uint128,
    /// Floating-point types.
    Float32,
    Float64,
    /// A non-generic record type, identified by its FQN and MangledName.
    Record(Fqn, MangledName),
    /// A non-generic enum type, identified by its FQN and MangledName.
    Enum(Fqn, MangledName),
    /// A generic record type (monomorphized instantiation).
    /// Each type argument carries its variance annotation from the declaration.
    GenericRecord {
        fqn: Fqn,
        mangled_name: MangledName,
        type_args: Vec<(Variance, Type)>,
    },
    /// A generic enum type (monomorphized instantiation).
    /// Each type argument carries its variance annotation from the declaration.
    GenericEnum {
        fqn: Fqn,
        mangled_name: MangledName,
        type_args: Vec<(Variance, Type)>,
    },
    /// A tuple type: `(Int32, Bool)`.
    Tuple(Vec<Type>, MangledName),
    /// Deferred append while the left operand has unknown outer shape.
    TupleExtend(Box<Type>, Box<Type>),
    /// Deferred init/last result for a tuple whose arity is not known yet.
    TupleProjection(Box<Type>, TupleProjection),
    AssociatedProjection(Box<super::associated_types::AssociatedProjection>),
    /// The bottom type — produced by expressions that never return (e.g. `panic`).
    /// Compatible with any expected type.
    Never,
    /// The top type — any value can be assigned to Any.
    /// Primitives are boxed (wrapped in GC structs) when stored as Any.
    Any,
    /// An array type: `Array<T>`.
    Array(Box<Type>),
    /// A type variable from the collect/registry phase. Placeholder for unification —
    /// must be resolved before appearing in the typed AST.
    TypeVariable(TypeParamName, Vec<TraitBound>),
    /// A generic type parameter from the inference phase. Has a unique ID for identity.
    /// Lives in the typed AST and is substituted by monomorphize.
    GenericParam(TypeParamName, Vec<TraitBound>, u32),
    /// The `Self` type — placeholder in trait method signatures for the implementing type.
    SelfType,
    /// An interface object type — dynamic dispatch via vtable. Carries a
    /// non-empty set of interfaces (one component for a plain interface
    /// object, several for an intersection `A and B`), **sorted by FQN and
    /// deduped**, plus the precomputed mangled name for the whole set.
    /// Construct only via [`Type::interface_object`] / [`Type::interface_intersection`]
    /// so the invariant holds.
    InterfaceObject {
        traits: Vec<InterfaceComponent>,
        mangled_name: MangledName,
    },
    /// A non-generic class type, identified by its FQN and MangledName.
    Class(Fqn, MangledName),
    /// A generic class type (monomorphized instantiation).
    /// Each type argument carries its variance annotation from the declaration.
    GenericClass {
        fqn: Fqn,
        mangled_name: MangledName,
        type_args: Vec<(Variance, Type)>,
    },
    /// A newtype wrapper: `newtype Cents = Int32`.
    /// Transparent in codegen (no WASM struct type), but distinct in the type system.
    Newtype(Fqn, Box<Type>),
    /// A generic newtype wrapper: `newtype Wrapper<T> = T`.
    /// Each type argument carries its variance annotation from the declaration.
    /// The concrete_inner_type is the inner type with type params substituted.
    GenericNewtype {
        fqn: Fqn,
        type_args: Vec<(Variance, Type)>,
        concrete_inner_type: Box<Type>,
    },
    /// A function type: `(Int32, String) => Bool` or `Int32 => Int32`.
    /// Param types and return type. Function values will be heap-allocated closures.
    Function(Vec<Type>, Box<Type>),
    /// A type constructor: a type-level function that takes type arguments and produces a type.
    /// E.g. `Rebind<U>` in a trait method signature.
    /// Intermediate form — expanded to concrete types during impl substitution.
    TypeConstructor {
        name: TypeParamName,
        type_args: Vec<Type>,
    },
    /// Sentinel for error recovery — avoids cascading type errors.
    Error,
}

impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Type::Unit, Type::Unit)
            | (Type::Bool, Type::Bool)
            | (Type::String, Type::String)
            | (Type::Char, Type::Char)
            | (Type::Int8, Type::Int8)
            | (Type::Int16, Type::Int16)
            | (Type::Int32, Type::Int32)
            | (Type::Int64, Type::Int64)
            | (Type::Uint8, Type::Uint8)
            | (Type::Uint16, Type::Uint16)
            | (Type::Uint32, Type::Uint32)
            | (Type::Uint64, Type::Uint64)
            | (Type::Uint128, Type::Uint128)
            | (Type::Float32, Type::Float32)
            | (Type::Float64, Type::Float64)
            | (Type::Never, Type::Never)
            | (Type::Any, Type::Any)
            | (Type::SelfType, Type::SelfType)
            | (Type::Error, Type::Error) => true,
            (Type::Record(f1, m1), Type::Record(f2, m2)) => f1 == f2 && m1 == m2,
            (Type::Enum(f1, m1), Type::Enum(f2, m2)) => f1 == f2 && m1 == m2,
            (
                Type::GenericRecord {
                    fqn: f1,
                    mangled_name: m1,
                    type_args: a1,
                },
                Type::GenericRecord {
                    fqn: f2,
                    mangled_name: m2,
                    type_args: a2,
                },
            ) => f1 == f2 && m1 == m2 && a1 == a2,
            (
                Type::GenericEnum {
                    fqn: f1,
                    mangled_name: m1,
                    type_args: a1,
                },
                Type::GenericEnum {
                    fqn: f2,
                    mangled_name: m2,
                    type_args: a2,
                },
            ) => f1 == f2 && m1 == m2 && a1 == a2,
            (Type::AssociatedProjection(a), Type::AssociatedProjection(b)) => a == b,
            (Type::TupleProjection(a, k), Type::TupleProjection(b, l)) => k == l && a == b,
            (Type::TupleExtend(a, b), Type::TupleExtend(c, d)) => a == c && b == d,
            (Type::Tuple(t1, m1), Type::Tuple(t2, m2)) => t1 == t2 && m1 == m2,
            (Type::Array(e1), Type::Array(e2)) => e1 == e2,
            (Type::TypeVariable(n1, b1), Type::TypeVariable(n2, b2)) => n1 == n2 && b1 == b2,
            (Type::GenericParam(n1, b1, id1), Type::GenericParam(n2, b2, id2)) => {
                n1 == n2 && b1 == b2 && id1 == id2
            }
            (
                Type::InterfaceObject {
                    traits: t1,
                    mangled_name: m1,
                },
                Type::InterfaceObject {
                    traits: t2,
                    mangled_name: m2,
                },
            ) => t1 == t2 && m1 == m2,
            (Type::Class(f1, m1), Type::Class(f2, m2)) => f1 == f2 && m1 == m2,
            (
                Type::GenericClass {
                    fqn: f1,
                    mangled_name: m1,
                    type_args: a1,
                },
                Type::GenericClass {
                    fqn: f2,
                    mangled_name: m2,
                    type_args: a2,
                },
            ) => f1 == f2 && m1 == m2 && a1 == a2,
            (Type::Newtype(f1, i1), Type::Newtype(f2, i2)) => f1 == f2 && i1 == i2,
            // GenericNewtype: ignore concrete_inner_type (it's a cached/derived value)
            (
                Type::GenericNewtype {
                    fqn: f1,
                    type_args: a1,
                    ..
                },
                Type::GenericNewtype {
                    fqn: f2,
                    type_args: a2,
                    ..
                },
            ) => f1 == f2 && a1 == a2,
            (Type::Function(p1, r1), Type::Function(p2, r2)) => p1 == p2 && r1 == r2,
            (
                Type::TypeConstructor {
                    name: n1,
                    type_args: a1,
                },
                Type::TypeConstructor {
                    name: n2,
                    type_args: a2,
                },
            ) => n1 == n2 && a1 == a2,
            _ => false,
        }
    }
}

impl Eq for Type {}

impl std::hash::Hash for Type {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Type::Unit
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Int64
            | Type::Uint8
            | Type::Uint16
            | Type::Uint32
            | Type::Uint64
            | Type::Uint128
            | Type::Float32
            | Type::Float64
            | Type::Never
            | Type::Any
            | Type::SelfType
            | Type::Error => {}
            Type::Record(f, m) | Type::Enum(f, m) | Type::Class(f, m) => {
                f.hash(state);
                m.hash(state);
            }
            Type::GenericRecord {
                fqn,
                mangled_name,
                type_args,
            }
            | Type::GenericEnum {
                fqn,
                mangled_name,
                type_args,
            }
            | Type::GenericClass {
                fqn,
                mangled_name,
                type_args,
            } => {
                fqn.hash(state);
                mangled_name.hash(state);
                type_args.hash(state);
            }
            Type::AssociatedProjection(projection) => projection.hash(state),
            Type::TupleProjection(receiver, kind) => {
                receiver.hash(state);
                kind.hash(state);
            }
            Type::TupleExtend(left, right) => {
                left.hash(state);
                right.hash(state);
            }
            Type::Tuple(types, mn) => {
                types.hash(state);
                mn.hash(state);
            }
            Type::Array(elem) => elem.hash(state),
            Type::TypeVariable(name, bounds) => {
                name.hash(state);
                bounds.hash(state);
            }
            Type::GenericParam(name, bounds, id) => {
                name.hash(state);
                bounds.hash(state);
                id.hash(state);
            }
            Type::InterfaceObject {
                traits,
                mangled_name,
            } => {
                traits.hash(state);
                mangled_name.hash(state);
            }
            Type::Newtype(fqn, inner) => {
                fqn.hash(state);
                inner.hash(state);
            }
            // GenericNewtype: ignore concrete_inner_type (cached/derived value)
            Type::GenericNewtype { fqn, type_args, .. } => {
                fqn.hash(state);
                type_args.hash(state);
            }
            Type::Function(params, ret) => {
                params.hash(state);
                ret.hash(state);
            }
            Type::TypeConstructor { name, type_args } => {
                name.hash(state);
                type_args.hash(state);
            }
        }
    }
}

impl Type {
    /// Whether this type is statically guaranteed to be a class reference.
    pub fn is_class_reference(&self) -> bool {
        match self {
            Self::Class(..) | Self::GenericClass { .. } => true,
            Self::TypeVariable(_, bounds) | Self::GenericParam(_, bounds, _) => {
                bounds.iter().any(|bound| match bound {
                    TraitBound::IsClass => true,
                    TraitBound::Named(named) => named.is_class_bound(),
                })
            }
            _ => false,
        }
    }

    /// Build a name → TypeVariable map from type params and optional trait bounds.
    pub fn type_param_map(
        type_params: &[TypeParamName],
        trait_bounds: &TraitBounds,
    ) -> BTreeMap<String, Type> {
        type_params
            .iter()
            .map(|tp| {
                let bounds: Vec<TraitBound> = trait_bounds.get(tp).cloned().unwrap_or_default();
                (tp.0.clone(), Type::TypeVariable(tp.clone(), bounds))
            })
            .collect()
    }

    /// Construct a `InterfaceObject` type with precomputed mangled name. The mangled name is
    /// **per-trait** (type args dropped): interface objects are de-monomorphized to one WASM type per
    /// trait. Type-level identity still distinguishes instantiations via `trait_type_args` (see
    /// `PartialEq`); only the codegen key is shared.
    pub fn interface_object(trait_fqn: Fqn, trait_type_args: Vec<Type>) -> Self {
        let mangled_name = MangledName::for_interface_object_per_interface(&trait_fqn);
        Type::InterfaceObject {
            traits: vec![InterfaceComponent {
                trait_fqn,
                trait_type_args,
            }],
            mangled_name,
        }
    }

    /// Construct an interface-object type over a set of interfaces — a plain
    /// interface object for one component, an intersection (`A and B`) for
    /// several. Sorts by FQN and dedups exact duplicates so identity is
    /// order-insensitive; same-FQN components with different type args must be
    /// rejected before calling this.
    pub fn interface_intersection(components: Vec<(Fqn, Vec<Type>)>) -> Self {
        let mut components: Vec<InterfaceComponent> = components
            .into_iter()
            .map(|(trait_fqn, trait_type_args)| InterfaceComponent {
                trait_fqn,
                trait_type_args,
            })
            .collect();
        components.sort_by(|a, b| a.trait_fqn.cmp(&b.trait_fqn));
        components.dedup();
        debug_assert!(!components.is_empty());
        let fqns: Vec<Fqn> = components.iter().map(|c| c.trait_fqn.clone()).collect();
        let mangled_name = MangledName::for_interface_object_set(&fqns);
        Type::InterfaceObject {
            traits: components,
            mangled_name,
        }
    }

    /// Try to resolve a primitive type by name.
    pub fn from_primitive(name: &str) -> Option<Type> {
        match name {
            "Unit" => Some(Type::Unit),
            "Bool" => Some(Type::Bool),
            "String" => Some(Type::String),
            "Char" => Some(Type::Char),
            "Int8" => Some(Type::Int8),
            "Int16" => Some(Type::Int16),
            "Int32" => Some(Type::Int32),
            "Int64" => Some(Type::Int64),
            "Uint8" => Some(Type::Uint8),
            "Uint16" => Some(Type::Uint16),
            "Uint32" => Some(Type::Uint32),
            "Uint64" => Some(Type::Uint64),
            "Uint128" => Some(Type::Uint128),
            "Float32" => Some(Type::Float32),
            "Float64" => Some(Type::Float64),
            "Never" => Some(Type::Never),
            "Any" => Some(Type::Any),
            _ => None,
        }
    }

    /// Returns true if this is any integer type (signed or unsigned).
    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Type::Int8
                | Type::Int16
                | Type::Int32
                | Type::Int64
                | Type::Uint8
                | Type::Uint16
                | Type::Uint32
                | Type::Uint64
                | Type::Uint128
        )
    }

    /// Returns true if this is any floating-point type.
    pub fn is_float(&self) -> bool {
        matches!(self, Type::Float32 | Type::Float64)
    }

    /// Returns true if this is any numeric type (integer or float).
    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    /// Returns true if this is the error sentinel type.
    pub fn is_error(&self) -> bool {
        matches!(self, Type::Error)
    }

    /// Returns true if this type or any nested type is `Type::Error`.
    pub fn contains_error(&self) -> bool {
        match self {
            Type::AssociatedProjection(projection) => projection.types().any(Type::contains_error),
            Type::Error => true,
            Type::Array(elem) => elem.contains_error(),
            Type::GenericRecord { type_args, .. } | Type::GenericEnum { type_args, .. } => {
                type_args.iter().any(|(_, t)| t.contains_error())
            }
            Type::GenericClass { type_args, .. } => {
                type_args.iter().any(|(_, t)| t.contains_error())
            }
            Type::GenericNewtype {
                type_args,
                concrete_inner_type,
                ..
            } => {
                type_args.iter().any(|(_, t)| t.contains_error())
                    || concrete_inner_type.contains_error()
            }
            Type::TupleProjection(receiver, _) => receiver.contains_error(),
            Type::TupleExtend(left, right) => left.contains_error() || right.contains_error(),
            Type::Tuple(types, _) => types.iter().any(|t| t.contains_error()),
            Type::Function(params, ret) => {
                params.iter().any(|t| t.contains_error()) || ret.contains_error()
            }
            _ => false,
        }
    }

    /// Returns true if this is the bottom type (Never).
    pub fn is_never(&self) -> bool {
        matches!(self, Type::Never)
    }

    /// Returns true if this is a reference type (heap-allocated in WASMGC).
    /// Reference types: Record, String, Never, Error.
    /// Primitives (Int*, Uint*, Float*, Bool, Unit, Char) are not reference types.
    /// TypeVariable/GenericParam are NOT included — they should be resolved before this is called.
    /// Returns true if this is the top type (Any).
    pub fn is_any(&self) -> bool {
        matches!(self, Type::Any)
    }

    /// Returns true if this is a class type (non-generic or generic).
    pub fn is_class_type(&self) -> bool {
        matches!(self, Type::Class(..) | Type::GenericClass { .. })
    }

    /// Whether the type's WASM representation is a single reference value (an `anyref`
    /// subtype) — i.e. it fits in one `anyref` slot without boxing. A tuple is **not** a
    /// reference type: it lowers to a flattened run of N values, not a single ref. (Its
    /// boxed form `(ref $Tuple_N)` is a reference, but the type itself is the flattened run.)
    pub fn is_reference_type(&self) -> bool {
        match self {
            Type::Record(..)
            | Type::Enum(..)
            | Type::Class(..)
            | Type::GenericRecord { .. }
            | Type::GenericEnum { .. }
            | Type::GenericClass { .. }
            | Type::InterfaceObject { .. }
            | Type::String
            | Type::Array(_)
            | Type::Function(..)
            | Type::Never
            | Type::Error
            | Type::Any => true,
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => inner.is_reference_type(),
            Type::AssociatedProjection(..)
            | Type::TupleProjection(..)
            | Type::Tuple(..)
            | Type::TupleExtend(..)
            | Type::Unit
            | Type::Bool
            | Type::Char
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Int64
            | Type::Uint8
            | Type::Uint16
            | Type::Uint32
            | Type::Uint64
            | Type::Uint128
            | Type::Float32
            | Type::Float64
            | Type::TypeVariable(_, _)
            | Type::GenericParam(_, _, _)
            | Type::SelfType
            | Type::TypeConstructor { .. } => false,
        }
    }

    /// Returns true if this type contains any `TypeVariable` or `GenericParam` anywhere (including nested).
    pub fn contains_type_parameter(&self) -> bool {
        match self {
            Type::AssociatedProjection(projection) => {
                projection.types().any(Type::contains_type_parameter)
            }
            Type::TypeVariable(_, _) | Type::GenericParam(_, _, _) => true,
            Type::Array(elem) => elem.contains_type_parameter(),
            Type::GenericRecord { type_args, .. }
            | Type::GenericEnum { type_args, .. }
            | Type::GenericClass { type_args, .. } => {
                type_args.iter().any(|(_, t)| t.contains_type_parameter())
            }
            Type::InterfaceObject { traits, .. } => traits.iter().any(|c| {
                c.trait_type_args
                    .iter()
                    .any(|t| t.contains_type_parameter())
            }),
            Type::Newtype(_, inner) => inner.contains_type_parameter(),
            Type::GenericNewtype { type_args, .. } => {
                // `concrete_inner_type` is a cached/derived value (see the type's
                // doc and its eq/hash, which ignore it). It may still carry the
                // definition's unsubstituted type params even when `type_args`
                // are concrete, so only the type args determine genericity.
                type_args.iter().any(|(_, t)| t.contains_type_parameter())
            }
            Type::TupleProjection(receiver, _) => receiver.contains_type_parameter(),
            Type::TupleExtend(left, right) => {
                left.contains_type_parameter() || right.contains_type_parameter()
            }
            Type::Tuple(types, _) => types.iter().any(|t| t.contains_type_parameter()),
            Type::Function(params, ret) => {
                params.iter().any(|t| t.contains_type_parameter()) || ret.contains_type_parameter()
            }
            Type::TypeConstructor { type_args, .. } => {
                type_args.iter().any(|t| t.contains_type_parameter())
            }
            _ => false,
        }
    }

    /// Whether Any or Never occurs anywhere in this type.
    pub fn contains_any_or_never(&self) -> bool {
        match self {
            Type::AssociatedProjection(projection) => {
                projection.types().any(Type::contains_any_or_never)
            }
            Type::Any | Type::Never => true,
            Type::Array(element) | Type::Newtype(_, element) => element.contains_any_or_never(),
            Type::GenericRecord { type_args, .. }
            | Type::GenericEnum { type_args, .. }
            | Type::GenericClass { type_args, .. }
            | Type::GenericNewtype { type_args, .. } => {
                type_args.iter().any(|(_, ty)| ty.contains_any_or_never())
            }
            Type::InterfaceObject { traits, .. } => traits.iter().any(|component| {
                component
                    .trait_type_args
                    .iter()
                    .any(Type::contains_any_or_never)
            }),
            Type::TupleProjection(receiver, _) => receiver.contains_any_or_never(),
            Type::TupleExtend(left, right) => {
                left.contains_any_or_never() || right.contains_any_or_never()
            }
            Type::Tuple(elements, _)
            | Type::TypeConstructor {
                type_args: elements,
                ..
            } => elements.iter().any(Type::contains_any_or_never),
            Type::Function(parameters, result) => {
                parameters.iter().any(Type::contains_any_or_never) || result.contains_any_or_never()
            }
            _ => false,
        }
    }

    /// Returns true if this type contains any unresolved `TypeVariable` anywhere (including nested).
    /// Unlike `contains_type_parameter`, this ignores `GenericParam` which represents resolved
    /// generic context type parameters.
    pub fn contains_type_variable(&self) -> bool {
        match self {
            Type::AssociatedProjection(projection) => {
                projection.types().any(Type::contains_type_variable)
            }
            Type::TypeVariable(_, _) => true,
            Type::GenericParam(_, _, _) => false,
            Type::Array(elem) => elem.contains_type_variable(),
            Type::GenericRecord { type_args, .. }
            | Type::GenericEnum { type_args, .. }
            | Type::GenericClass { type_args, .. } => {
                type_args.iter().any(|(_, t)| t.contains_type_variable())
            }
            Type::InterfaceObject { traits, .. } => traits
                .iter()
                .any(|c| c.trait_type_args.iter().any(|t| t.contains_type_variable())),
            Type::Newtype(_, inner) => inner.contains_type_variable(),
            Type::GenericNewtype {
                type_args,
                concrete_inner_type,
                ..
            } => {
                type_args.iter().any(|(_, t)| t.contains_type_variable())
                    || concrete_inner_type.contains_type_variable()
            }
            Type::TupleProjection(receiver, _) => receiver.contains_type_variable(),
            Type::TupleExtend(left, right) => {
                left.contains_type_variable() || right.contains_type_variable()
            }
            Type::Tuple(types, _) => types.iter().any(|t| t.contains_type_variable()),
            Type::Function(params, ret) => {
                params.iter().any(|t| t.contains_type_variable()) || ret.contains_type_variable()
            }
            Type::TypeConstructor { type_args, .. } => {
                type_args.iter().any(|t| t.contains_type_variable())
            }
            _ => false,
        }
    }

    /// Returns true if this type contains a type parameter with the given name.
    /// Used by the occurs check in unification to prevent infinite types like `T = Array<T>`.
    pub fn contains_type_parameter_named(&self, name: &TypeParamName) -> bool {
        match self {
            Type::AssociatedProjection(projection) => projection
                .types()
                .any(|ty| ty.contains_type_parameter_named(name)),
            Type::TypeVariable(n, _) | Type::GenericParam(n, _, _) => n == name,
            Type::Array(elem) => elem.contains_type_parameter_named(name),
            Type::GenericRecord { type_args, .. } | Type::GenericEnum { type_args, .. } => {
                type_args
                    .iter()
                    .any(|(_, t)| t.contains_type_parameter_named(name))
            }
            Type::GenericClass { type_args, .. } => type_args
                .iter()
                .any(|(_, t)| t.contains_type_parameter_named(name)),
            Type::InterfaceObject { traits, .. } => traits.iter().any(|c| {
                c.trait_type_args
                    .iter()
                    .any(|t| t.contains_type_parameter_named(name))
            }),
            Type::Newtype(_, inner) => inner.contains_type_parameter_named(name),
            Type::GenericNewtype {
                type_args,
                concrete_inner_type,
                ..
            } => {
                type_args
                    .iter()
                    .any(|(_, t)| t.contains_type_parameter_named(name))
                    || concrete_inner_type.contains_type_parameter_named(name)
            }
            Type::TupleProjection(receiver, _) => receiver.contains_type_parameter_named(name),
            Type::TupleExtend(left, right) => {
                left.contains_type_parameter_named(name)
                    || right.contains_type_parameter_named(name)
            }
            Type::Tuple(types, _) => types.iter().any(|t| t.contains_type_parameter_named(name)),
            Type::Function(params, ret) => {
                params.iter().any(|t| t.contains_type_parameter_named(name))
                    || ret.contains_type_parameter_named(name)
            }
            Type::TypeConstructor {
                name: atn,
                type_args,
            } => {
                atn == name
                    || type_args
                        .iter()
                        .any(|t| t.contains_type_parameter_named(name))
            }
            _ => false,
        }
    }

    /// Returns true if this type contains `SelfType` anywhere (including nested).
    pub fn contains_self_type(&self) -> bool {
        match self {
            Type::AssociatedProjection(projection) => {
                projection.types().any(Type::contains_self_type)
            }
            Type::SelfType => true,
            Type::Array(elem) => elem.contains_self_type(),
            Type::GenericRecord { type_args, .. } | Type::GenericEnum { type_args, .. } => {
                type_args.iter().any(|(_, t)| t.contains_self_type())
            }
            Type::GenericClass { type_args, .. } => {
                type_args.iter().any(|(_, t)| t.contains_self_type())
            }
            Type::InterfaceObject { traits, .. } => traits
                .iter()
                .any(|c| c.trait_type_args.iter().any(|t| t.contains_self_type())),
            Type::Newtype(_, inner) => inner.contains_self_type(),
            Type::GenericNewtype {
                type_args,
                concrete_inner_type,
                ..
            } => {
                type_args.iter().any(|(_, t)| t.contains_self_type())
                    || concrete_inner_type.contains_self_type()
            }
            Type::TupleProjection(receiver, _) => receiver.contains_self_type(),
            Type::TupleExtend(left, right) => {
                left.contains_self_type() || right.contains_self_type()
            }
            Type::Tuple(types, _) => types.iter().any(|t| t.contains_self_type()),
            Type::Function(params, ret) => {
                params.iter().any(|t| t.contains_self_type()) || ret.contains_self_type()
            }
            Type::TypeConstructor { type_args, .. } => {
                type_args.iter().any(|t| t.contains_self_type())
            }
            _ => false,
        }
    }

    /// Returns true if this is a signed integer type.
    pub fn is_signed_integer(&self) -> bool {
        matches!(self, Type::Int8 | Type::Int16 | Type::Int32 | Type::Int64)
    }

    /// Non-panicking version of `to_fqn()`. Returns `None` for `Error`, `TypeVariable`/`GenericParam`,
    /// `TypeConstructor`, and `SelfType` variants instead of panicking.
    /// The type segment used in mangled names of members of a *non-generic*
    /// implement block. A plain type yields its base FQN rendering —
    /// byte-identical to the historical `type_fqn` segment — while an
    /// instantiated generic appends its type args so sibling instantiations
    /// (`implement Tr for List<Int32>` vs `implement Tr for List<String>`)
    /// mangle distinctly. Generic blocks keep the bare-FQN segment and are
    /// distinguished by `with_type_args` instead.
    pub fn impl_segment(&self) -> String {
        match self {
            Type::GenericRecord { fqn, type_args, .. }
            | Type::GenericEnum { fqn, type_args, .. }
            | Type::GenericClass { fqn, type_args, .. } => {
                let args: Vec<String> = type_args.iter().map(|(_, t)| t.to_string()).collect();
                format!("{}<{}>", fqn, args.join(","))
            }
            Type::GenericNewtype { fqn, type_args, .. } => {
                let args: Vec<String> = type_args.iter().map(|(_, t)| t.to_string()).collect();
                format!("{}<{}>", fqn, args.join(","))
            }
            Type::Array(elem) => format!("{}<{}>", self.to_fqn(), elem),
            // Tuples and function types share a uniform base FQN ("Tuple2",
            // "Fn1") — element types must be rendered or sibling impl blocks
            // on e.g. `(Int32, Int32)` vs `(String, String)` collide.
            Type::Tuple(elems, _) => {
                let parts: Vec<String> = elems.iter().map(|t| t.to_string()).collect();
                format!("{}<{}>", self.to_fqn(), parts.join(","))
            }
            Type::Function(params, ret) => {
                let parts: Vec<String> = params.iter().map(|t| t.to_string()).collect();
                format!("{}<{}=>{}>", self.to_fqn(), parts.join(","), ret)
            }
            Type::TypeVariable(n, _) | Type::GenericParam(n, _, _) => n.0.clone(),
            _ => self.to_fqn().to_string(),
        }
    }

    pub fn try_to_fqn(&self) -> Option<Fqn> {
        match self {
            Type::TypeVariable(..)
            | Type::GenericParam(..)
            | Type::TypeConstructor { .. }
            | Type::SelfType
            | Type::TupleProjection(..)
            | Type::AssociatedProjection(..) => None,
            Type::TupleExtend(..) => Some(self.to_fqn()),
            // An intersection has no single FQN.
            Type::InterfaceObject { traits, .. } if traits.len() > 1 => None,
            _ => Some(self.to_fqn()),
        }
    }

    /// Returns a canonical FQN for this type, using `standard.prelude` as the package for primitives.
    /// Panics on Error — callers must guard against that before calling.
    pub fn to_fqn(&self) -> Fqn {
        let prelude = PackagePath(vec!["standard".into(), "prelude".into()]);
        let name = match self {
            Type::Unit => "Unit",
            Type::Bool => "Bool",
            Type::String => "String",
            Type::Char => "Char",
            Type::Int8 => "Int8",
            Type::Int16 => "Int16",
            Type::Int32 => "Int32",
            Type::Int64 => "Int64",
            Type::Uint8 => "Uint8",
            Type::Uint16 => "Uint16",
            Type::Uint32 => "Uint32",
            Type::Uint64 => "Uint64",
            Type::Uint128 => "Uint128",
            Type::Float32 => "Float32",
            Type::Float64 => "Float64",
            Type::Never => "Never",
            Type::Any => "Any",
            Type::Array(_) => "Array",
            Type::TupleProjection(..) => "TupleProjection",
            Type::AssociatedProjection(..) => "AssociatedProjection",
            Type::TupleExtend(..) => "TupleExtend",
            Type::Record(fqn, _)
            | Type::Enum(fqn, _)
            | Type::Class(fqn, _)
            | Type::GenericRecord { fqn, .. }
            | Type::GenericEnum { fqn, .. }
            | Type::GenericClass { fqn, .. }
            | Type::Newtype(fqn, _)
            | Type::GenericNewtype { fqn, .. } => return fqn.clone(),
            Type::Tuple(types, _) => {
                return Fqn {
                    package: PackagePath(vec![]),
                    symbol: SymbolName(format!("Tuple{}", types.len())),
                };
            }
            Type::Function(params, _) => {
                return Fqn {
                    package: PackagePath(vec![]),
                    symbol: SymbolName(format!("Fn{}", params.len())),
                };
            }
            Type::InterfaceObject { traits, .. } => {
                if traits.len() == 1 {
                    return traits[0].trait_fqn.clone();
                }
                panic!("to_fqn() called on intersection type '{}'", self)
            }
            Type::TypeVariable(name, _) | Type::GenericParam(name, _, _) => {
                panic!("to_fqn() called on type parameter '{}'", name)
            }
            Type::TypeConstructor { name, .. } => {
                panic!("to_fqn() called on TypeConstructor '{}'", name)
            }
            Type::SelfType => panic!("to_fqn() called on Self type"),
            Type::Error => {
                return Fqn {
                    package: PackagePath(vec![]),
                    symbol: SymbolName("<error>".into()),
                };
            }
        };
        Fqn {
            package: prelude,
            symbol: SymbolName(name.into()),
        }
    }

    /// Does this type contain an interface-object type at any depth?
    /// Used by variance/LUB guards: positions that cannot reify fat-pointer
    /// conversions require exact equality whenever an interface object is
    /// anywhere inside either side.
    pub fn contains_interface_object(&self) -> bool {
        match self {
            Type::AssociatedProjection(projection) => {
                projection.types().any(Type::contains_interface_object)
            }
            Type::InterfaceObject { .. } => true,
            Type::Array(elem) | Type::Newtype(_, elem) => elem.contains_interface_object(),
            Type::TupleProjection(receiver, _) => receiver.contains_interface_object(),
            Type::TupleExtend(left, right) => {
                left.contains_interface_object() || right.contains_interface_object()
            }
            Type::Tuple(types, _) => types.iter().any(|t| t.contains_interface_object()),
            Type::Function(params, ret) => {
                params.iter().any(|t| t.contains_interface_object())
                    || ret.contains_interface_object()
            }
            Type::GenericRecord { type_args, .. }
            | Type::GenericEnum { type_args, .. }
            | Type::GenericClass { type_args, .. }
            | Type::GenericNewtype { type_args, .. } => {
                type_args.iter().any(|(_, t)| t.contains_interface_object())
            }
            _ => false,
        }
    }

    /// Whether a deferred extension remains anywhere in the type.
    pub fn contains_tuple_extension(&self) -> bool {
        match self {
            Type::AssociatedProjection(projection) => {
                projection.types().any(Type::contains_tuple_extension)
            }
            Type::TupleProjection(receiver, _) => receiver.contains_tuple_extension(),
            Type::TupleExtend(..) => true,
            Type::Array(element) | Type::Newtype(_, element) => element.contains_tuple_extension(),
            Type::Tuple(elements, _)
            | Type::TypeConstructor {
                type_args: elements,
                ..
            } => elements.iter().any(Type::contains_tuple_extension),
            Type::Function(parameters, result) => {
                parameters.iter().any(Type::contains_tuple_extension)
                    || result.contains_tuple_extension()
            }
            Type::GenericRecord { type_args, .. }
            | Type::GenericEnum { type_args, .. }
            | Type::GenericClass { type_args, .. }
            | Type::GenericNewtype { type_args, .. } => type_args
                .iter()
                .any(|(_, ty)| ty.contains_tuple_extension()),
            Type::InterfaceObject { traits, .. } => traits.iter().any(|component| {
                component
                    .trait_type_args
                    .iter()
                    .any(Type::contains_tuple_extension)
            }),
            _ => false,
        }
    }

    pub fn is_recursive_tuple_head(&self, parameters: &[TypeParamName]) -> bool {
        let Type::TupleExtend(left, right) = self else {
            return false;
        };
        match (&**left, &**right) {
            (Type::TypeVariable(l, _), Type::TypeVariable(r, _)) => {
                parameters.contains(l) && parameters.contains(r) && left.is_tuple()
            }
            _ => false,
        }
    }

    /// A tuple-constrained extension has one unambiguous prefix/final split.
    pub fn split_tuple_extension(&self, actual: &Type) -> Option<(Type, Type)> {
        let Type::TupleExtend(left, _) = self else {
            return None;
        };
        if !left.is_tuple() {
            return None;
        }
        match actual {
            Type::Tuple(elements, _) if elements.len() >= 3 => Some((
                Type::tuple_projection(actual.clone(), TupleProjection::Init),
                Type::tuple_projection(actual.clone(), TupleProjection::Last),
            )),
            Type::TupleExtend(prefix, last) if prefix.is_tuple() => {
                Some(((**prefix).clone(), (**last).clone()))
            }
            _ => None,
        }
    }

    /// A provably decreasing obligation does not spend the cyclic-trait budget.
    pub fn has_tuple_subterm(&self, candidate: &Type) -> bool {
        let Type::Tuple(elements, _) = self else {
            return false;
        };
        elements.contains(candidate)
            || (elements.len() > 2
                && Type::tuple_projection(self.clone(), TupleProjection::Init) == *candidate)
    }

    /// Structural tuple evidence, independent of element trait implementations.
    pub fn is_tuple(&self) -> bool {
        match self {
            Type::Tuple(elements, _) => elements.len() >= 2,
            Type::TupleExtend(..) => true,
            Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) => bounds
                .iter()
                .filter_map(TraitBound::named)
                .any(|b| is_tuple_constraint(&b.trait_fqn)),
            _ => false,
        }
    }

    pub fn tuple_projection(receiver: Type, kind: TupleProjection) -> Type {
        match (&receiver, kind) {
            (Type::Error, _) => Type::Error,
            (Type::Tuple(elements, _), TupleProjection::Last) => {
                elements.last().cloned().unwrap_or(Type::Error)
            }
            (Type::Tuple(elements, _), TupleProjection::Init) if elements.len() == 2 => {
                elements[0].clone()
            }
            (Type::Tuple(elements, _), TupleProjection::Init) if elements.len() > 2 => {
                let prefix = elements[..elements.len() - 1].to_vec();
                let name = MangledName::for_tuple(&prefix);
                Type::Tuple(prefix, name)
            }
            (Type::TupleExtend(_, right), TupleProjection::Last) => (**right).clone(),
            (Type::TupleExtend(left, _), TupleProjection::Init) => (**left).clone(),
            _ => Type::TupleProjection(Box::new(receiver), kind),
        }
    }

    /// Normalize extension only when the left operand's outer shape is known.
    pub fn tuple_extend(left: Type, right: Type) -> Type {
        if left.is_error() || right.is_error() {
            return Type::Error;
        }
        let mut elements = match left {
            Type::Tuple(elements, _) => elements,
            Type::TypeVariable(..)
            | Type::GenericParam(..)
            | Type::SelfType
            | Type::TypeConstructor { .. }
            | Type::TupleExtend(..)
            | Type::TupleProjection(..)
            | Type::AssociatedProjection(..) => {
                return Type::TupleExtend(Box::new(left), Box::new(right));
            }
            _ => vec![left],
        };
        elements.push(right);
        let name = MangledName::for_tuple(&elements);
        Type::Tuple(elements, name)
    }

    /// Get the MangledName for a concrete type.
    pub fn mangled_name(&self) -> MangledName {
        match self {
            Type::AssociatedProjection(projection) => MangledName(format!(
                "$Associated${}${}${}${}${}",
                projection.receiver.mangled_name(),
                projection.trait_fqn,
                projection.member,
                projection
                    .trait_parameters
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
                projection
                    .parameters
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )),
            Type::TupleProjection(receiver, kind) => {
                MangledName(format!("$Tuple{kind:?}${}", receiver.mangled_name()))
            }
            Type::TupleExtend(left, right) => MangledName(format!(
                "$Extend${}${}",
                left.mangled_name(),
                right.mangled_name()
            )),
            Type::Class(_, mn)
            | Type::GenericClass {
                mangled_name: mn, ..
            }
            | Type::Record(_, mn)
            | Type::GenericRecord {
                mangled_name: mn, ..
            }
            | Type::Enum(_, mn)
            | Type::GenericEnum {
                mangled_name: mn, ..
            }
            | Type::Tuple(_, mn) => mn.clone(),
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => inner.mangled_name(),
            Type::Array(elem) => MangledName::for_array_type(&elem.mangled_name()),
            // Len-1 must stay byte-identical to the old `for_type(to_fqn())`
            // fallback; intersections join the component names with `&`.
            Type::InterfaceObject { traits, .. } => {
                let parts: Vec<String> = traits
                    .iter()
                    .map(|c| MangledName::for_type(&c.trait_fqn).0)
                    .collect();
                MangledName(parts.join("&"))
            }
            _ => MangledName::for_type(&self.to_fqn()),
        }
    }
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Unit => write!(f, "Unit"),
            Type::Bool => write!(f, "Bool"),
            Type::String => write!(f, "String"),
            Type::Char => write!(f, "Char"),
            Type::Int8 => write!(f, "Int8"),
            Type::Int16 => write!(f, "Int16"),
            Type::Int32 => write!(f, "Int32"),
            Type::Int64 => write!(f, "Int64"),
            Type::Uint8 => write!(f, "Uint8"),
            Type::Uint16 => write!(f, "Uint16"),
            Type::Uint32 => write!(f, "Uint32"),
            Type::Uint64 => write!(f, "Uint64"),
            Type::Uint128 => write!(f, "Uint128"),
            Type::Float32 => write!(f, "Float32"),
            Type::Float64 => write!(f, "Float64"),
            Type::Record(fqn, _)
            | Type::Enum(fqn, _)
            | Type::Class(fqn, _)
            | Type::Newtype(fqn, _) => write!(f, "{}", fqn.symbol),
            Type::GenericNewtype { fqn, type_args, .. }
            | Type::GenericRecord { fqn, type_args, .. }
            | Type::GenericEnum { fqn, type_args, .. } => {
                let args: Vec<String> = type_args.iter().map(|(_, t)| t.to_string()).collect();
                write!(f, "{}<{}>", fqn.symbol, args.join(", "))
            }
            Type::GenericClass { fqn, type_args, .. } => {
                let args: Vec<String> = type_args.iter().map(|(_, t)| t.to_string()).collect();
                write!(f, "{}<{}>", fqn.symbol, args.join(", "))
            }
            Type::AssociatedProjection(projection) => {
                write!(f, "{}.{}", projection.receiver, projection.member)?;
                if !projection.parameters.is_empty() {
                    write!(
                        f,
                        "<{}>",
                        projection
                            .parameters
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )?;
                }
                Ok(())
            }
            Type::TupleProjection(receiver, kind) => write!(f, "{receiver}.{kind:?}"),
            Type::TupleExtend(left, right) => write!(f, "({left} ~ {right})"),
            Type::Tuple(types, _) => {
                let parts: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "({})", parts.join(", "))
            }
            Type::InterfaceObject { traits, .. } => {
                let parts: Vec<String> = traits
                    .iter()
                    .map(|c| {
                        if c.trait_type_args.is_empty() {
                            c.trait_fqn.symbol.to_string()
                        } else {
                            let args: Vec<String> =
                                c.trait_type_args.iter().map(|t| t.to_string()).collect();
                            format!("{}<{}>", c.trait_fqn.symbol, args.join(", "))
                        }
                    })
                    .collect();
                write!(f, "{}", parts.join(" and "))
            }
            Type::Array(elem) => write!(f, "Array<{}>", elem),
            Type::Function(params, ret) => {
                if params.len() == 1 {
                    write!(f, "{} => {}", params[0], ret)
                } else {
                    let parts: Vec<String> = params.iter().map(|t| t.to_string()).collect();
                    write!(f, "({}) => {}", parts.join(", "), ret)
                }
            }
            Type::TypeVariable(name, _) | Type::GenericParam(name, _, _) => write!(f, "{}", name),
            Type::SelfType => write!(f, "Self"),
            Type::Never => write!(f, "Never"),
            Type::Any => write!(f, "Any"),
            Type::TypeConstructor { name, type_args } => {
                let args: Vec<String> = type_args.iter().map(|t| t.to_string()).collect();
                write!(f, "{}<{}>", name, args.join(", "))
            }
            Type::Error => write!(f, "<error>"),
        }
    }
}

/// A type-checked global variable declaration.
#[derive(Debug, Clone)]
pub struct TypedGlobal {
    pub visibility: Visibility,
    pub name: MangledName,
    pub mutable: bool,
    pub ty: Type,
    pub initializer: TypedExpr,
    pub span: Span,
    /// Non-empty for generic global templates. Empty for non-generic globals.
    pub type_params: Vec<TypeParamName>,
}

/// An array type definition for codegen — one per Array<T> specialization.
#[derive(Debug, Clone)]
pub struct ArrayTypeDef {
    pub mangled_name: MangledName,
    pub element_type: Type,
}

/// A single entry in a class's vtable layout.
///
/// Under full type erasure the canonical `ClassTypeDef` is shared by every instantiation —
/// there is no per-`type_args` ClassTypeDef. For generic classes the slot's `impl_fqn`
/// identifies the method that lives on the canonical (template) class; codegen derives the
/// concrete WASM function key at each instantiation by:
///
/// ```text
/// MangledName::for_function(&slot.impl_fqn, &slot.param_types)
///     .with_type_args(&type_args)
/// ```
///
/// For non-generic classes `type_args` is empty and the derived name resolves directly in
/// `module.functions`. `param_types[0]` is `self` (every vtable slot is an instance method).
/// `Type::TypeVariable` slots persist for generic class methods and lower to `anyref` via
/// `single_val_type`, producing the erased vtable slot signature shared across all
/// instantiations.
#[derive(Debug, Clone)]
pub struct VtableSlot {
    /// Keep property access distinct from a same-named method call.
    pub is_property: bool,

    /// Source-level method name (e.g., `"speak"`, `"get"`). For LSP, diagnostics, and
    /// matching overrides against parent slots.
    pub method_name: SymbolName,

    /// The class + method FQN that provides this implementation. For inherited slots this
    /// is the ancestor's method FQN; for overrides and new methods it's the current class's
    /// method FQN. Symbol format: `"ClassName.methodName"`.
    pub impl_fqn: Fqn,

    /// Param types of the implementation (including `self` at index 0).
    pub param_types: Vec<Type>,

    /// Return type of the implementation.
    pub return_type: Type,

    /// Type args to pass to the impl method, expressed in THIS class's type-param
    /// space. Codegen derives the concrete method key at each instantiation by
    /// substituting the class's type params with the instantiation's type args
    /// into this list, then `MangledName::for_function(impl_fqn, param_types)
    /// .with_type_args(<substituted>)`.
    ///
    /// For own methods and straight-through generic inheritance this is just the
    /// class's own type params (so the substitution yields the instantiation's
    /// type args — the pre-existing behavior). It differs only when a method is
    /// inherited from a *generic* ancestor bound to concrete/other types — e.g. a
    /// non-generic class extending `Parent<Concrete>` — where it carries the
    /// ancestor binding so the correctly-monomorphized method is found.
    pub impl_type_params: Vec<Type>,
}

/// A class type definition for codegen — all fields in physical order.
#[derive(Debug, Clone)]
pub struct ClassTypeDef {
    pub fqn: Fqn,
    pub mangled_name: MangledName,
    pub fields: Vec<ClassFieldDef>,
    pub is_final: bool,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub parent_mangled_name: Option<MangledName>,
    /// Resolved parent type (set when parent is a generic class, for monomorphize discovery).
    pub parent_type: Option<Type>,
    /// Vtable layout — one entry per virtual method slot. Parent entries come first
    /// (overrides replaced), then new child virtual methods. See `VtableSlot`.
    pub vtable_methods: Vec<VtableSlot>,
    /// The root of the inheritance chain (topmost class).
    pub hierarchy_root_mangled: MangledName,
    /// This class's constructor params (typed).
    pub constructor_params: Vec<TypedParam>,
    /// This class's initializer body statements (own let bindings + expressions).
    /// Emitted directly (not wrapped in Block) so locals stay in scope.
    pub initializer: Vec<TypedExpr>,
    /// Field names + types to push to stack after initializer runs.
    /// Order: [constructor_params..., let_bindings...]
    pub initializer_fields: Vec<(String, Type)>,
    /// Typed extends arg expressions (reference own constructor params).
    /// None for root classes.
    pub extends_args: Option<Vec<TypedExpr>>,
    /// Parameter slots in the order written in the extends clause.
    pub extends_argument_order: Vec<usize>,
    /// Non-empty for generic class templates (keyed by base mangled name).
    /// Empty for concrete (non-generic or instantiated) classes.
    pub type_params: Vec<TypeParamName>,
    /// Declaration span for LSP go-to-definition.
    pub span: Span,
}

/// A field in a class type definition.
#[derive(Debug, Clone)]
pub struct ClassFieldDef {
    pub name: String,
    pub ty: Type,
    pub visibility: Visibility,
    pub mutable: bool,
    pub declared_by: Fqn,
}

/// A interface object type definition for codegen — one per unique InterfaceObject coercion target.
/// Participates in the type dependency graph so ordering with user types is automatic.
#[derive(Debug, Clone)]
pub struct InterfaceObjectTypeDef {
    pub trait_fqn: Fqn,
    pub mangled_name: MangledName,
    /// Direct supers' per-trait `$IfaceObj$…` keys, in `extends` declaration
    /// order. The vtable struct holds one immutable non-null ref field per
    /// direct super BEFORE the own member slots, so a `B`-object upcasts to an
    /// `A`-object by statically extracting the nested `$Vtable$A` ref.
    pub supers: Vec<MangledName>,
    /// (member_name, non-self param types, return type) per OWN vtable slot
    /// (inherited members live in the nested super vtables). Field indices in
    /// the emitted struct are offset by `supers.len()`.
    pub vtable_members: Vec<(InterfaceMemberName, Vec<Type>, Type)>,
}

/// An intersection interface-object type definition for codegen: the fat
/// pointer struct `(data, ref $Vtable$A&B)` plus the set vtable struct whose
/// fields are refs to the component vtables (sorted by FQN).
#[derive(Debug, Clone)]
pub struct InterfaceIntersectionTypeDef {
    pub mangled_name: MangledName,
    /// (component FQN, component per-trait `$IfaceObj$…` key), sorted by FQN.
    pub components: Vec<(Fqn, MangledName)>,
}

/// A component key and its ordered (member, implementation, type parameters) slots.
pub type VtableMethodGroup = (
    MangledName,
    Vec<(InterfaceMemberName, MangledName, Vec<Type>)>,
);

/// A type definition — codegen emits one WASM type (or a run of them) per variant.
#[derive(Debug, Clone)]
#[allow(
    clippy::large_enum_variant,
    reason = "Keep compiler data inline without adding allocations to this representation."
)]
pub enum TypeDef {
    Record(RecordTypeDef),
    Enum(EnumTypeDef),
    Array(ArrayTypeDef),
    Class(ClassTypeDef),
    InterfaceObject(InterfaceObjectTypeDef),
    InterfaceIntersection(InterfaceIntersectionTypeDef),
}

impl TypeDef {
    /// The mangled name for this type definition.
    pub fn mangled_name(&self) -> &MangledName {
        match self {
            TypeDef::Record(rec) => &rec.mangled_name,
            TypeDef::Enum(e) => &e.mangled_name,
            TypeDef::Array(arr) => &arr.mangled_name,
            TypeDef::Class(cls) => &cls.mangled_name,
            TypeDef::InterfaceObject(to) => &to.mangled_name,
            TypeDef::InterfaceIntersection(toi) => &toi.mangled_name,
        }
    }

    /// Returns true if this TypeDef is a generic template (has unresolved type parameters).
    /// Templates are retained in module.types for LSP and rules, but skipped by codegen.
    pub fn is_generic_template(&self) -> bool {
        match self {
            TypeDef::Record(rec) => !rec.type_params.is_empty(),
            TypeDef::Enum(e) => !e.type_params.is_empty(),
            TypeDef::Class(cls) => !cls.type_params.is_empty(),
            _ => false,
        }
    }
}

/// A record type definition for codegen.
#[derive(Debug, Clone)]
pub struct RecordTypeDef {
    pub fqn: Fqn,
    pub mangled_name: MangledName,
    pub fields: Vec<(String, Type)>,
    /// Non-empty for generic record templates (keyed by base mangled name).
    /// Empty for concrete (non-generic or instantiated) records.
    pub type_params: Vec<TypeParamName>,
    /// Declaration span for LSP go-to-definition.
    pub span: Span,
}

/// An enum type definition for codegen.
#[derive(Debug, Clone)]
pub struct EnumTypeDef {
    pub fqn: Fqn,
    pub mangled_name: MangledName,
    /// Non-empty for generic enum templates (keyed by base mangled name).
    /// Empty for concrete (non-generic or instantiated) enums.
    pub type_params: Vec<TypeParamName>,
    pub variants: Vec<EnumVariantDef>,
    /// Declaration span for LSP go-to-definition.
    pub span: Span,
}

/// A variant in an enum type definition.
#[derive(Debug, Clone)]
pub struct EnumVariantDef {
    pub name: String,
    pub payload_types: Vec<Type>, // for both tuple and record, this is the flat list of types in declaration order
}

/// A type-checked test declaration.
#[derive(Debug, Clone)]
pub struct TypedTest {
    pub name: String,
    pub fqtn: String,
    pub package_path: PackagePath,
    pub return_type: Type,
    pub body: TypedExpr,
    pub mangled_name: MangledName,
    pub span: Span,
    /// None = no @skip; Some(None) = @skip; Some(Some(r)) = @skip("r")
    pub skip_reason: Option<Option<String>>,
    /// None = no @panics; Some(None) = @panics; Some(Some(m)) = @panics("m")
    pub expected_panic: Option<Option<String>>,
    /// None = no @timeout; Some(ms) = @timeout(ms)
    pub timeout_ms: Option<u64>,
}

/// A resolved type name reference with its source span.
/// Collected during inference for LSP navigation on type annotations.
#[derive(Debug, Clone)]
pub struct TypeReference {
    pub ty: Type,
    pub span: Span,
}

/// A typed module — the output of the typechecker for one package.
/// When packages are merged via `merge_from`, this becomes the combined typed AST
/// for an entire project. `main_function_fqn` is resolved after merging to identify
/// the entry point (None for libraries).
#[derive(Debug, Clone)]
pub struct TypedModule {
    pub main_function_fqn: Option<Fqn>,
    pub functions: BTreeMap<MangledName, TypedFunction>,
    pub globals: BTreeMap<MangledName, TypedGlobal>,
    pub types: BTreeMap<MangledName, TypeDef>,
    pub tests: Vec<TypedTest>,
    pub type_references: Vec<TypeReference>,
    pub implement_blocks: Vec<TypedImplementBlock>,
    pub extension_blocks: Vec<TypedExtensionBlock>,
    /// Embedded resource blobs declared via `Dovetail.toml` `resources = [...]`.
    /// Keyed by `(declaring_project_root, resource_name)`; carried through
    /// the pipeline so codegen can allocate a passive data segment for each.
    pub resources: BTreeMap<(PackagePath, String), Vec<u8>>,
    /// Default-body templates for trait members, keyed
    /// `MangledName::for_trait_default(trait, member)` with `Self` as a
    /// trait-bounded type variable. Kept OUTSIDE `functions` so they survive
    /// monomorphize's template strip: the post-strip safety nets
    /// (`elaborate_coercions` → `ensure_vtable_functions`) can still
    /// materialize a default per implementing type. Never emitted by codegen.
    pub default_templates: BTreeMap<MangledName, TypedFunction>,
    /// Generic helpers retained for implementation methods materialized after
    /// coercion. These are templates, never code generation inputs.
    pub function_templates: BTreeMap<MangledName, TypedFunction>,
    /// Direct (concrete → super-interface) vtable coercions synthesized by
    /// `elaborate_coercions` with no expression of their own: when a `$via$`-
    /// backed coercion re-boxes a bare-`Self` return into a super interface
    /// that the type ALSO implements directly, the direct impl must own the
    /// (type, super) re-box global regardless of whether any expression in
    /// the program coerces to that super. Codegen ingests these alongside the
    /// expression-scanned coercions; they emit globals/wrappers only.
    pub synthetic_interface_coercions: Vec<SyntheticInterfaceCoercion>,
    /// (concrete type, full via group key, direct group key) mappings for
    /// which the direct super vtable global serves a bare-`Self` re-box:
    /// recorded only when the type directly implements the SAME application
    /// of the super the via group backs. Without an entry, a via wrapper's
    /// re-box must use its own provider standalone — an unrelated direct
    /// coercion of a different application must not hijack it.
    pub direct_rebox_authorizations: Vec<(Type, MangledName, MangledName)>,
}

/// See `TypedModule::synthetic_interface_coercions`.
#[derive(Debug, Clone)]
pub struct SyntheticInterfaceCoercion {
    pub concrete_type: Type,
    pub interface_mangled_name: MangledName,
    pub vtable_methods: Vec<VtableMethodGroup>,
}

/// A type-checked implement block — all method bodies fully typed.
/// Lives in `TypedModule.implement_blocks`. Monomorphize expands these into concrete `TypedFunction` entries.
#[derive(Debug, Clone)]
pub struct TypedImplementBlock {
    pub trait_fqn: Fqn,
    pub type_fqn: Fqn,
    pub for_type: Type,
    pub type_params: Vec<TypeParamName>,
    pub trait_type_args: Vec<Type>,
    pub trait_bounds: TraitBounds,
    pub methods: Vec<TypedImplMethod>,
    pub properties: Vec<TypedImplMethod>,
    pub span: Span,
}

/// A method or property within a typed implement block.
#[derive(Debug, Clone)]
pub struct TypedImplMethod {
    pub name: SymbolName,
    pub method_type_params: Vec<TypeParamName>,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: TypedExpr,
    pub span: Span,
    pub is_async: bool,
    pub visibility: Visibility,
}

/// A type-checked extension block — all method bodies fully typed.
/// Lives in `TypedModule.extension_blocks`. Monomorphize expands these into concrete `TypedFunction` entries.
#[derive(Debug, Clone)]
pub struct TypedExtensionBlock {
    pub ext_fqn: Fqn,
    pub for_type: Type,
    pub type_params: Vec<TypeParamName>,
    pub trait_bounds: TraitBounds,
    pub methods: Vec<TypedExtMethod>,
    pub properties: Vec<TypedExtMethod>,
    pub span: Span,
}

/// A method or property within a typed extension block.
#[derive(Debug, Clone)]
pub struct TypedExtMethod {
    pub name: SymbolName,
    pub method_type_params: Vec<TypeParamName>,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: TypedExpr,
    pub span: Span,
    pub is_async: bool,
    pub visibility: Visibility,
}

/// Resolved reference to a trait impl method — used by desugar helper variants.
/// Desugar passes convert these into `ImplFunctionCall` nodes.
#[derive(Debug, Clone)]
pub struct ResolvedImplMethod {
    pub trait_fqn: Fqn,
    pub trait_type_params: Vec<Type>,
    pub for_type: Type,
    pub method_name: SymbolName,
    pub method_type_params: Vec<Type>,
}

impl TypedModule {
    /// Create an empty TypedModule with no main function.
    pub fn empty() -> Self {
        Self {
            main_function_fqn: None,
            functions: BTreeMap::new(),
            globals: BTreeMap::new(),
            types: BTreeMap::new(),
            tests: Vec::new(),
            type_references: Vec::new(),
            implement_blocks: Vec::new(),
            extension_blocks: Vec::new(),
            resources: BTreeMap::new(),
            default_templates: BTreeMap::new(),
            function_templates: BTreeMap::new(),
            synthetic_interface_coercions: Vec::new(),
            direct_rebox_authorizations: Vec::new(),
        }
    }

    /// Merge another module's functions, globals and types into this one.
    /// Keeps self.main_function_fqn unchanged.
    /// Keeps ALL functions and globals (public + internal) for codegen.
    pub fn merge_from(&mut self, other: TypedModule) {
        self.functions.extend(other.functions);
        self.function_templates.extend(other.function_templates);
        self.globals.extend(other.globals);
        self.types.extend(other.types);
        self.tests.extend(other.tests);
        self.type_references.extend(other.type_references);
        self.implement_blocks.extend(other.implement_blocks);
        self.extension_blocks.extend(other.extension_blocks);
        self.resources.extend(other.resources);
        self.default_templates.extend(other.default_templates);
        self.synthetic_interface_coercions
            .extend(other.synthetic_interface_coercions);
        self.direct_rebox_authorizations
            .extend(other.direct_rebox_authorizations);
    }
}

/// A type-checked function declaration.
#[derive(Debug, Clone)]
pub struct TypedFunction {
    pub visibility: Visibility,
    pub name: MangledName,
    /// Non-empty for generic function templates (before monomorphization).
    pub type_params: Vec<TypeParamName>,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: TypedExpr,
    pub span: Span,
    /// For virtual class methods: the hierarchy root class type used as WASM param 0 type.
    pub vtable_self_type: Option<Type>,
    /// Whether this function was declared with the `async` modifier.
    pub is_async: bool,
    /// Human-readable name for WASM name section (e.g., `a.add(Int32, Int32)`).
    pub display_name: String,
    /// Source-level function name from the Fqn symbol (e.g., `Async.sleep`, `main`).
    /// Used for stack traces and diagnostics without MangledName parsing.
    pub source_name: String,
}

/// A type-checked function parameter.
#[derive(Debug, Clone)]
pub struct TypedParam {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

/// A type-checked closure parameter.
#[derive(Debug, Clone)]
pub struct TypedClosureParam {
    pub name: VarName,
    pub ty: Type,
    pub span: Span,
}

/// A variable captured by a closure from an enclosing scope.
#[derive(Debug, Clone)]
pub struct CapturedVar {
    pub name: VarName,
    pub ty: Type,
    pub mutable: bool, // true → boxed; false → copied
}

/// A type-checked expression with its resolved type.
#[derive(Debug, Clone)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: Type,
    pub span: Span,
}

/// The kind of an intrinsic call (compiler built-in).
#[derive(Debug, Clone)]
pub enum IntrinsicKind {
    TupleProjection(TupleProjection),
    BinaryOperator(crate::parser::ast::BinOp),
    ClassIdentityEquals,
    ClassIdentityHash,
    // ── WASI p3 core (waitable sets, subtasks, async clock) ──
    P3WaitableSetNew,
    P3WaitableSetJoin,
    P3WaitableRemove,
    P3WaitableSetWait,
    P3WaitableSetPoll,
    P3ThreadYield,
    P3WaitableSetDrop,
    P3SubtaskDrop,
    P3SubtaskCancel,
    P3MonotonicNow,
    P3WaitForStart,
    P3WaitUntilStart,
    P3StdinOpen,
    P3StdinReadStart,
    P3StdinDropReadable,
    P3StdinReadResult,
    P3StdinDropResultFuture,
    P3StreamReadFinish,
    P3StreamDiscard,
    P3FsOpenAtStart,
    P3FsOpenAtFinish,
    P3FsStatStart,
    P3FsStatFinish,
    P3FsCreateDirectoryAtStart,
    P3FsUnlinkFileAtStart,
    P3FsRemoveDirectoryAtStart,
    P3FsUnitFinish,
    P3FsStatAtStart,
    P3FsSetSizeStart,
    P3FsSyncStart,
    P3FsSyncDataStart,
    P3FsAdviseStart,
    P3FsGetFlagsStart,
    P3FsGetFlagsFinish,
    P3FsGetTypeStart,
    P3FsGetTypeFinish,
    P3FsIsSameObjectStart,
    P3FsIsSameObjectFinish,
    P3FsMetadataHashStart,
    P3FsMetadataHashAtStart,
    P3FsMetadataHashFinish,
    P3FsLinkAtStart,
    P3FsReadlinkAtStart,
    P3FsReadlinkAtFinish,
    P3FsSetTimesStart,
    P3FsSetTimesAtStart,
    P3FsReadDirectory,
    P3FsEntryReadStart,
    P3FsEntryReadFinish,
    P3FsDropEntryReadable,
    P3FsDropEntryResult,
    P3TcpCreate,
    P3TcpBind,
    P3TcpConnectStart,
    P3TcpConnectFinish,
    P3TcpListen,
    P3TcpAcceptStart,
    P3TcpAcceptFinish,
    P3TcpDropAcceptStream,
    P3TcpSend,
    P3TcpReceive,
    P3TcpSendWriteStart,
    P3TcpReceiveReadStart,
    P3TcpDropSendWritable,
    P3TcpDropReceiveReadable,
    P3TcpDropSendResult,
    P3TcpDropReceiveResult,
    P3TcpCancelAcceptRead,
    P3TcpCancelSendWrite,
    P3TcpCancelReceiveRead,
    P3FsCancelRead,
    P3FsCancelWrite,
    P3FsCancelEntryRead,
    P3StdinCancelRead,
    P3StdoutOpen,
    P3StderrOpen,
    P3StdoutWriteStart,
    P3StderrWriteStart,
    P3StdoutCancelWrite,
    P3StderrCancelWrite,
    P3StdoutReadResult,
    P3StderrReadResult,
    P3TcpReadReceiveResult,
    P3TcpReadSendResult,
    P3FsReadReadResult,
    P3FsReadWriteResult,
    P3FsReadEntryResult,
    P3FsReadAppendResult,
    P3AsyncCallDiscard,
    P3DnsResolveStart,
    P3DnsResolveFinish,
    P3UdpCreate,
    P3UdpBind,
    P3UdpConnect,
    P3UdpDisconnect,
    P3UdpSendStart,
    P3UdpSendFinish,
    P3UdpReceiveStart,
    P3UdpReceiveFinish,
    P3UdpLocalAddress,
    P3UdpRemoteAddress,
    P3UdpClose,
    P3TcpLocalAddress,
    P3TcpRemoteAddress,
    P3TcpSetListenBacklogSize,
    // Socket options. Every getter is `result<scalar, error-code>` through a
    // retptr and every setter is `(self, value, retptr) -> ()`; the emit arms
    // differ only in the scalar's width and offset.
    P3TcpIsListening,
    // `get-address-family` on both socket types: a plain flat enum return
    // (0 = ipv4, 1 = ipv6), no result and no retptr — like `isListening`.
    P3TcpAddressFamily,
    P3UdpAddressFamily,
    P3TcpKeepAliveEnabled,
    P3TcpSetKeepAliveEnabled,
    P3TcpKeepAliveIdleTime,
    P3TcpSetKeepAliveIdleTime,
    P3TcpKeepAliveInterval,
    P3TcpSetKeepAliveInterval,
    P3TcpKeepAliveCount,
    P3TcpSetKeepAliveCount,
    P3TcpHopLimit,
    P3UdpUnicastHopLimit,
    P3UdpSetUnicastHopLimit,
    P3UdpReceiveBufferSize,
    P3UdpSetReceiveBufferSize,
    P3UdpSendBufferSize,
    P3UdpSetSendBufferSize,
    P3TcpSetHopLimit,
    P3TcpReceiveBufferSize,
    P3TcpSetReceiveBufferSize,
    P3TcpSendBufferSize,
    P3TcpSetSendBufferSize,
    P3TcpClose,
    P3FsSymlinkAtStart,
    P3FsRenameAtStart,
    P3FsReadViaStream,
    P3FsWriteViaStream,
    P3FsAppendViaStream,
    P3FsClose,
    P3FsStreamReadStart,
    P3FsStreamWriteStart,
    P3FsStreamAppendWriteStart,
    P3FsDropReadable,
    P3FsDropWritable,
    P3FsDropReadResult,
    P3FsDropWriteResult,
    P3FsDropAppendResult,
    StringUnsafeBytes,
    /// String.fromBytes(buf, start, length) -> String
    StringFromBytes,
    /// String.fromChar(c) -> String (UTF-8 encode)
    StringFromChar,
    ReadonlySliceMake,
    ReadonlySliceLength,
    ReadonlySliceGet,
    ReadonlySliceSlice,
    ReadonlySliceCopyTo,
    ArrayGet,
    ArraySet,
    ArrayLength,
    ArrayClone,
    ArrayFill,
    /// array.extend(value, newSize) -> Array<T>
    ArrayExtend,
    /// array.concat(other) -> Array<T>
    ArrayConcat,
    ArrayEmpty,
    /// array.copy(destination, sourceOffset, destinationOffset, length) -> Array<T>
    ArrayCopy,
    /// Numeric type conversion (e.g. `42.toInt64()`). Payload is the target type.
    NumericConvert(Type),
    /// Uint128.multiply(x: Uint64, y: Uint64) -> Uint128 — widening multiply, lowers
    /// directly to the single `i64.mul_wide_u` wide-arithmetic instruction.
    Uint128Multiply,
    /// Uint128.make(lo: Uint64, hi: Uint64) -> Uint128 — construct a 128-bit value
    /// from its low and high 64-bit words. Lowers to a no-op: the two `Uint64` args
    /// are already the flattened `[lo, hi]` stack representation of a `Uint128`.
    Uint128Make,
    /// Uint128.high -> Uint64 — extract the high 64-bit word. The flattened
    /// representation is `[lo, hi]` with `hi` on top of the stack, so this just
    /// drops the low word (via a scratch local) rather than shifting. (The low
    /// word, `Uint128.low`, is `toUint64()` — a single `Drop` of the high word.)
    Uint128High,
    /// Float64.toBits() -> Int64, Float32.toBits() -> Int32
    FloatToBits,
    /// Int64.bitsToFloat64() -> Float64, Int32.bitsToFloat32() -> Float32
    BitsToFloat,
    /// Math.floor(x) — floor of a float
    MathFloor,
    /// Math.trunc(x) — truncate toward zero
    MathTrunc,
    /// Math.abs(x) — absolute value of a float
    MathAbs,
    /// Math.fmod(x, y) — floating-point remainder
    MathFmod,
    /// Math.isNan(x) — test if NaN
    MathIsNan,
    /// Math.isInfinity(x) — test if ±Infinity
    MathIsInfinity,
    /// String.length — UTF-8 character count
    StringLength,
    /// String.byteLength — byte count of backing array
    StringByteLength,
    /// String.getChar(index) — get character at UTF-8 index
    StringGetChar,
    /// String.isAscii — true if string contains only ASCII characters
    StringIsAscii,
    /// debug(value: String) — print string to stdout
    DebugPrint,
    /// debug<T>(value: T) where T: Display — call format then print to stdout
    DebugPrintDisplay {
        format_method: MangledName,
    },
    /// WallClock.now() -> Instant
    WallClockNow,
    /// Random.bytes(len: Int64) -> Array<Uint8>
    RandomBytes,
    /// Random.int64() -> Int64
    RandomInt64,
    FsPreopensGetDirectories,
    CliTerminalStdin,
    CliTerminalStdout,
    CliTerminalStderr,
    CliGetEnvironment,
    CliGetArguments,
    CliInitialCwd,
    CliExit,
    // Console (blocking stdout/stderr output)
    ConsolePrint,
    ConsolePrintln,
    ConsoleEprint,
    ConsoleEprintln,
    // Resources (embedded binary blobs)
    /// `Resource.bytes(name: String): Array<Uint8>` — the literal `name`
    /// is resolved at typecheck time to a concrete resource registered in
    /// the calling project. Codegen lowers each call site to a single
    /// `array.new_data` over the corresponding passive data segment.
    ResourceBytes {
        resource_name: String,
        declaring_root: PackagePath,
    },
}

/// The kind of a typed expression.
#[derive(Debug, Clone)]
pub enum TypedExprKind {
    /// Source call metadata, retained until argument evaluation is lowered.
    NamedCall {
        call: Box<TypedExpr>,
        /// Explicit argument slots in written evaluation order (implicit receiver excluded).
        argument_order: Vec<usize>,
        /// Visible declaration's explicit parameter names, in declaration order.
        parameter_names: Vec<String>,
        /// Instantiated declared parameter types, aligned with parameter_names.
        parameter_types: Vec<Type>,
        /// Explicitly labelled parameter slots.
        named_parameters: Vec<(usize, Span)>,
        source_name: String,
    },
    UnitLiteral,
    BoolLiteral(bool),
    StringLiteral(String),
    CharLiteral(char),
    Int8Literal(i8),
    Int16Literal(i16),
    Int32Literal(i32),
    Int64Literal(i64),
    Uint8Literal(u8),
    Uint16Literal(u16),
    Uint32Literal(u32),
    Uint64Literal(u64),
    Uint128Literal(u128),
    Float32Literal(f32),
    Float64Literal(f64),
    BinaryOp {
        op: crate::parser::ast::BinOp,
        left: Box<TypedExpr>,
        right: Box<TypedExpr>,
    },
    UnaryOp {
        op: crate::parser::ast::UnaryOp,
        operand: Box<TypedExpr>,
    },
    Block(Vec<TypedExpr>),
    Panic {
        message: Box<TypedExpr>,
    },
    Assert {
        condition: Box<TypedExpr>,
        message: Option<Box<TypedExpr>>,
    },
    Let {
        name: VarName,
        mutable: bool,
        boxed: bool,
        var_ty: Type,
        value: Box<TypedExpr>,
    },
    VarRef {
        name: VarName,
        boxed: bool,
    },
    Assign {
        name: VarName,
        target_ty: Type,
        boxed: bool,
        value: Box<TypedExpr>,
    },
    GlobalRef {
        name: MangledName,
        /// Non-empty when referencing a generic global instantiation.
        type_params: Vec<Type>,
    },
    /// A reference to a named function as a first-class value.
    /// Produced when a function name is used in expression position without calling it.
    FunctionRef {
        name: MangledName,
        /// Non-empty when referencing a generic function instantiation.
        type_params: Vec<Type>,
    },
    /// A bound reference to an instance method as a first-class value.
    /// Captures the receiver (self) and the method identity.
    /// Type is `(non-self params) => return_type` — self is stripped.
    MethodRef {
        object: Box<TypedExpr>,
        method_name: MangledName,
        /// Non-empty when referencing a generic method instantiation.
        type_params: Vec<Type>,
    },
    GlobalAssign {
        name: MangledName,
        /// Non-empty when assigning to a generic global instantiation.
        type_params: Vec<Type>,
        value: Box<TypedExpr>,
    },
    FunctionCall {
        name: MangledName,
        args: Vec<TypedExpr>,
        /// Non-empty when calling a generic function instantiation.
        type_params: Vec<Type>,
    },
    If {
        condition: Box<TypedExpr>,
        then_branch: Box<TypedExpr>,
        else_branch: Option<Box<TypedExpr>>,
    },
    While {
        condition: Box<TypedExpr>,
        body: Box<TypedExpr>,
    },
    Break,
    Continue,
    Match {
        subject: Box<TypedExpr>,
        arms: Vec<TypedMatchArm>,
    },
    RecordCreate {
        fqn: Fqn,
        fields: Vec<(String, TypedExpr)>,
        /// Non-empty when the record is a generic instantiation. Carries the type params
        /// used at this creation site so monomorphize can produce the concrete TypeDef.
        type_params: Vec<Type>,
    },
    /// A tuple literal `(a, b, …)`. Structural and unparameterized — the element order *is* the
    /// representation (the expression's `ty` is the `Type::Tuple`). Distinct from `RecordCreate`
    /// because tuples are no longer records: they flatten to a run of WASM values.
    TupleLiteral {
        elements: Vec<TypedExpr>,
    },
    EnumCreate {
        fqn: Fqn,
        variant_name: String,
        args: Vec<TypedExpr>,
        /// Non-empty when the enum is a generic instantiation.
        type_params: Vec<Type>,
    },
    EnumVariantRecordCreate {
        fqn: Fqn,
        variant_name: String,
        args: Vec<TypedExpr>, // reordered to declaration order
        /// Non-empty when the enum is a generic instantiation.
        type_params: Vec<Type>,
    },
    FieldAccess {
        object: Box<TypedExpr>,
        field_name: String,
        field_index: u32,
        boxed: bool,
    },
    FieldAssign {
        object: Box<TypedExpr>,
        field_name: String,
        field_index: u32,
        value: Box<TypedExpr>,
        boxed: bool,
    },
    RecordWith {
        object: Box<TypedExpr>,
        fqn: Fqn,
        overrides: Vec<(String, u32, TypedExpr)>, // (field_name, field_index, value)
        /// Non-empty when the record is a generic instantiation.
        type_params: Vec<Type>,
    },
    ArrayLiteral {
        elements: Vec<TypedExpr>,
    },
    IntrinsicCall {
        intrinsic: IntrinsicKind,
        args: Vec<TypedExpr>,
    },
    /// Box a primitive value to Any for WASM codegen.
    /// Only emitted by the any-boxing pass; never produced by inference.
    BoxToAny {
        inner: Box<TypedExpr>,
    },
    /// Runtime type test: `e is T`. Value is Any, result is Bool.
    TypeTest {
        value: Box<TypedExpr>,
        target_type: Type,
    },
    /// Runtime type cast: `e as T`. Value is Any, result is target_type (traps on failure).
    TypeCast {
        value: Box<TypedExpr>,
        target_type: Type,
    },
    /// A destructuring let binding: `let (x, y) = expr`
    LetDestructure {
        pattern: TypedPattern,
        var_ty: Type,
        value: Box<TypedExpr>,
    },
    /// Class instantiation at call site.
    ClassNew {
        mangled_name: MangledName,
        args: Vec<TypedExpr>,
        /// Non-empty when the class is a generic instantiation.
        type_params: Vec<Type>,
    },
    /// Class struct creation from field values (no constructor). Used by variance cast synthesis.
    ClassStructCreate {
        target_mangled_name: MangledName,
        type_params: Vec<Type>,
        fields: Vec<TypedExpr>,
    },
    /// Virtual method call on a class instance (dispatched through vtable).
    ClassVirtualCall {
        /// The object to call the method on.
        object: Box<TypedExpr>,
        /// The vtable slot index for this method.
        vtable_slot: u32,
        /// Arguments including self (object is prepended as first arg in codegen).
        args: Vec<TypedExpr>,
    },
    /// Direct call to a parent class method via `super.method()`.
    ClassSuperCall {
        /// The mangled name of the parent's method implementation.
        method_mangled: MangledName,
        /// Arguments including self.
        args: Vec<TypedExpr>,
    },
    /// Newtype construction: `Cents(100)` — transparent wrapper.
    NewtypeCreate {
        value: Box<TypedExpr>,
    },
    /// Newtype unwrapping: `cents.value` — transparent unwrap.
    NewtypeValue {
        value: Box<TypedExpr>,
    },
    /// Coerce a concrete value to an interface object type (single interface
    /// or intersection). `interface_mangled_name` is the SET key (`$IfaceObj$A` /
    /// `$IfaceObj$A&B`); `vtable_methods` is grouped per component — one
    /// `(component per-trait key, slot entries)` group per interface, sorted.
    InterfaceObjectCoerce {
        inner: Box<TypedExpr>,
        interface_mangled_name: MangledName,
        concrete_type: Type,
        vtable_methods: Vec<VtableMethodGroup>,
    },
    /// Template version of InterfaceObjectCoerce — emitted in generic template bodies
    /// when the concrete type contains a TypeVariable/GenericParam. Monomorphize resolves this
    /// to a real InterfaceObjectCoerce after substitution provides a concrete type.
    /// `traits` mirrors the target set's components (sorted).
    TemplateInterfaceObjectCoerce {
        inner: Box<TypedExpr>,
        traits: Vec<(Fqn, Vec<Type>)>,
        concrete_type: Type,
    },
    /// Upcast an interface object to a (strict) subset of its components —
    /// `(A and B) → A`, `(A and B and C) → (A and B)`. Source set is
    /// `inner.ty`, target set is the node's own `ty`; codegen extracts the
    /// data field plus the needed component vtable refs and rebuilds the
    /// target fat pointer. Fully static — no runtime lookup.
    InterfaceObjectUpcast {
        inner: Box<TypedExpr>,
    },
    /// Call a method on a interface object receiver (dispatched by trait signature).
    InterfaceObjectMethodCall {
        interface_mangled_name: MangledName,
        method_name: String,
        member_name: InterfaceMemberName,
        receiver: Box<TypedExpr>,
        args: Vec<TypedExpr>,
    },
    /// `await expr` — extracts inner value T from Awaitable<T>.
    /// Only valid inside async function/closure. Desugared into andThen/map chains.
    Await {
        operand: Box<TypedExpr>,
        /// Enclosing async function's full return type (e.g. Async<FnT, E>).
        return_type: Type,
        /// Resolved impl method for andThen on this operand type.
        and_then_method: ResolvedImplMethod,
        /// Resolved impl method for map on this operand type.
        map_method: ResolvedImplMethod,
        /// MangledName for SourceLocation record type (used by desugar to create trace args).
        source_location_mn: MangledName,
    },
    /// `use expr` — scoped resource acquire/use/release for `Usable<T, E>`. The
    /// value of the expression is the resource of type `T`. Desugared in the
    /// `desugar_use` pass: the rest of the current block is captured as a
    /// closure and passed to the trait's `use` method along with an `errorF`
    /// for converting the resource's `E` into the enclosing context's `E2`.
    /// The compiler synthesizes `errorF` from a `From<E> for E2` impl or
    /// emits identity when `E == E2`; this info is recorded on the node.
    Use {
        operand: Box<TypedExpr>,
        /// The resource type `T` (also the type of the `use` expression itself).
        inner_type: Type,
        /// The resource impl's source error type `E`.
        source_error: Type,
        /// The enclosing context's expected error type `E2`. Equals `source_error`
        /// when no async context exists, in which case desugar emits an identity
        /// errorF closure.
        target_error: Type,
        /// `From<source_error> for target_error` impl, when conversion is needed
        /// (i.e. `source_error != target_error && source_error != Never`).
        /// `None` for identity or trivial Never-source cases.
        from_method: Option<ResolvedImplMethod>,
    },
    /// `for pattern in iterable do body` — placeholder for desugaring pass.
    /// Desugared into While(true) + Match(next(), Some→body, None→break) before codegen.
    ForLoop {
        pattern: TypedPattern,
        iterable: Box<TypedExpr>,
        iterator_method: ResolvedImplMethod,
        iterator_type: Type,
        element_type: Type,
        body: Box<TypedExpr>,
    },
    /// Wraps an async function body. Carries the resolved `succeed` method
    /// from the Awaitable trait impl. Consumed by the desugar_await pass.
    AsyncBlock {
        body: Box<TypedExpr>,
        succeed_method: ResolvedImplMethod,
    },
    /// `try expr` / `expr.orReturn` — placeholder for desugaring pass (Phase 6).
    /// Desugaring rewrites to: let tmp = operand; match unwrap(tmp) ...
    Try {
        operand: Box<TypedExpr>,
        /// Resolved impl method for `unwrap` to call during desugaring.
        unwrap_method: ResolvedImplMethod,
        /// The return type of the `unwrap` method: Result<T, OnFailure>.
        unwrap_return_type: Type,
        /// The enclosing function's return type (for the return expr in desugaring).
        return_type: Type,
        /// When Some, the error value is wrapped via `From::from()` before returning
        /// (e.g. converting `Result<Never, E>` → `Async<T, E>` in async functions).
        from_method: Option<ResolvedImplMethod>,
    },
    /// Internal return expression — exits the enclosing function with a value.
    /// Not surface syntax; produced only by the try/orReturn desugaring pass.
    Return {
        value: Box<TypedExpr>,
        /// The function's declared return type (for variance cast coercion point).
        return_type: Type,
    },
    /// A closure expression: `x => x + 1`
    Closure {
        params: Vec<TypedClosureParam>,
        body: Box<TypedExpr>,
        captures: Vec<CapturedVar>,
    },
    /// A call to a function-typed value: `f(args)` where `f` is a variable of `Type::Function`.
    /// Separate from `FunctionCall` because function values are runtime values, not statically resolved symbols.
    ClosureCall {
        callee: Box<TypedExpr>,
        args: Vec<TypedExpr>,
    },
    /// A call to a trait implementation method. Body lives in `TypedModule.implement_blocks`.
    /// Monomorphize resolves this to a concrete `FunctionCall`.
    ImplFunctionCall {
        trait_fqn: Fqn,
        trait_type_params: Vec<Type>,
        for_type: Type,
        method_name: SymbolName,
        args: Vec<TypedExpr>,
        method_type_params: Vec<Type>,
    },
    /// A trait implementation method used as a first-class value.
    /// Monomorphize resolves this to a concrete `FunctionRef`.
    ImplFunctionRef {
        trait_fqn: Fqn,
        trait_type_params: Vec<Type>,
        for_type: Type,
        method_name: SymbolName,
        method_type_params: Vec<Type>,
    },
    /// A call to an extension method. Body lives in `TypedModule.extension_blocks`.
    /// Monomorphize resolves this to a concrete `FunctionCall`.
    ExtFunctionCall {
        ext_fqn: Fqn,
        for_type: Type,
        method_name: SymbolName,
        args: Vec<TypedExpr>,
        type_params: Vec<Type>,
    },
    /// An extension method used as a first-class value.
    /// Monomorphize resolves this to a concrete `FunctionRef`.
    ExtFunctionRef {
        ext_fqn: Fqn,
        for_type: Type,
        method_name: SymbolName,
        type_params: Vec<Type>,
    },
}

/// A typed field pattern in a record pattern.
#[derive(Debug, Clone)]
pub struct TypedFieldPattern {
    pub field_name: String,
    pub field_index: u32,
    pub pattern: TypedPattern,
}

/// A typed pattern in a match expression.
#[derive(Debug, Clone)]
pub enum TypedPattern {
    Wildcard,
    Literal(Box<TypedExpr>),
    Variable(VarName, Type),
    /// Type-annotated pattern: `case b: Box<Int32> =>` — binding with type narrowing.
    TypeAnnotated {
        binding: VarName,
        ty: Type,
    },
    Record {
        ty: Type,
        fields: Vec<TypedFieldPattern>,
    },
    EnumVariant {
        enum_type: Type,
        variant_name: String,
        variant_index: u32,
        payload_patterns: Vec<TypedPattern>,
    },
    EnumVariantRecord {
        enum_type: Type,
        variant_name: String,
        variant_index: u32,
        field_patterns: Vec<TypedFieldPattern>,
    },
    Tuple {
        element_patterns: Vec<TypedPattern>,
        tuple_type: Type,
    },
    Newtype {
        newtype_ty: Type,
        inner_pattern: Box<TypedPattern>,
    },
}

/// A typed match arm.
#[derive(Debug, Clone)]
pub struct TypedMatchArm {
    pub pattern: TypedPattern,
    pub guard: Option<Box<TypedExpr>>,
    pub body: Box<TypedExpr>,
    pub span: Span,
}

/// Native implementations of the prelude's arithmetic and concatenation traits.
pub(crate) fn primitive_binary_operator(
    fqn: &Fqn,
    method: &str,
) -> Option<crate::parser::ast::BinOp> {
    use crate::parser::ast::BinOp;
    if fqn.package.to_string() != "standard.prelude" {
        return None;
    }
    if fqn.symbol.0 == "String" {
        return (method == "concat").then_some(BinOp::Concat);
    }
    if !matches!(
        fqn.symbol.0.as_str(),
        "Int8"
            | "Int16"
            | "Int32"
            | "Int64"
            | "Uint8"
            | "Uint16"
            | "Uint32"
            | "Uint64"
            | "Uint128"
            | "Float32"
            | "Float64"
    ) {
        return None;
    }
    match method {
        "add" => Some(BinOp::Add),
        "sub" => Some(BinOp::Sub),
        "mul" => Some(BinOp::Mul),
        "div" if fqn.symbol.0 != "Uint128" => Some(BinOp::Div),
        _ => None,
    }
}

/// The type segment for members of an implement block (see
/// `MangledName::for_impl_block_method`).
///
/// - Non-generic block: the for-type's `impl_segment` — sibling
///   instantiations (`Tr for List<Int32>` vs `Tr for List<String>`) mangle
///   distinctly; plain types stay byte-identical to the historical bare-FQN
///   segment.
/// - Generic block whose for-type is the *trivial application* of its own
///   type params in order (`<T> Tr for List<T>` — the overwhelmingly common
///   shape): the historical bare-FQN segment, byte-identical; the
///   instantiation is distinguished by `with_type_args`.
/// - Generic block with a *shaped* for-type (`<T> Tr for Pair<T, Int32>`):
///   the shaped `impl_segment` (variables render by name) — without it, two
///   sibling shaped blocks instantiated at the same bindings would collide
///   (`Pair<T, Int32>` and `Pair<T, String>` both at `T := Bool`).
pub fn impl_block_segment(for_type: &Type, type_params: &[TypeParamName]) -> String {
    if type_params.is_empty() {
        return for_type.impl_segment();
    }
    let arg_is_param = |t: &Type, p: &TypeParamName| matches!(t, Type::TypeVariable(n, _) | Type::GenericParam(n, _, _) if n == p);
    let trivial = match for_type {
        Type::GenericRecord { type_args, .. }
        | Type::GenericEnum { type_args, .. }
        | Type::GenericClass { type_args, .. }
        | Type::GenericNewtype { type_args, .. } => {
            type_args.len() == type_params.len()
                && type_args
                    .iter()
                    .zip(type_params.iter())
                    .all(|((_, t), p)| arg_is_param(t, p))
        }
        Type::Array(elem) => type_params.len() == 1 && arg_is_param(elem, &type_params[0]),
        // Shaped tuple/function for-types are never "trivial" — their shape
        // must appear in the segment so shaped siblings stay distinct.
        Type::Tuple(elems, _) => {
            elems.len() == type_params.len()
                && elems
                    .iter()
                    .zip(type_params.iter())
                    .all(|(t, p)| arg_is_param(t, p))
        }
        Type::Function(..) => false,
        _ => true,
    };
    if trivial {
        for_type
            .try_to_fqn()
            .map(|f| f.to_string())
            .unwrap_or_else(|| for_type.impl_segment())
    } else {
        for_type.impl_segment()
    }
}

/// Mangled name for a member of an implement block, aware of the block's
/// for-type shape (see `impl_block_segment`).
pub fn impl_member_mangled_name(
    trait_fqn: &Fqn,
    for_type: &Type,
    type_params: &[TypeParamName],
    method_name: &SymbolName,
    trait_type_args: &[Type],
) -> MangledName {
    let segment = impl_block_segment(for_type, type_params);
    MangledName::for_impl_block_method(trait_fqn, &segment, method_name, trait_type_args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_typed_function(name: &str) -> TypedFunction {
        TypedFunction {
            visibility: Visibility::Internal,
            name: MangledName(name.to_string()),
            type_params: vec![],
            params: vec![],
            return_type: Type::Unit,
            body: TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Unit,
                span: Span::point(std::sync::Arc::from("test"), 1, 1),
            },
            span: Span::point(std::sync::Arc::from("test"), 1, 1),
            vtable_self_type: None,
            is_async: false,
            display_name: name.to_string(),
            source_name: name.to_string(),
        }
    }

    #[test]
    fn test_typed_module_empty() {
        let module = TypedModule::empty();
        assert!(module.main_function_fqn.is_none());
        assert!(module.functions.is_empty());
    }

    #[test]
    fn test_typed_module_merge_from() {
        let mut root = TypedModule::empty();
        root.functions.insert(
            MangledName("a.main".to_string()),
            dummy_typed_function("a.main"),
        );

        let mut other = TypedModule::empty();
        other.functions.insert(
            MangledName("a.utils.helper".to_string()),
            dummy_typed_function("a.utils.helper"),
        );

        root.merge_from(other);

        // Both functions present
        assert_eq!(root.functions.len(), 2);
        assert!(
            root.functions
                .contains_key(&MangledName("a.main".to_string()))
        );
        assert!(
            root.functions
                .contains_key(&MangledName("a.utils.helper".to_string()))
        );
    }
}
