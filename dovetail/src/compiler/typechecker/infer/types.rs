use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName, PackagePath, SymbolName, Variance};
use crate::parser::ast::TypeExpr;
use crate::typechecker::imports::ImportTarget;
use crate::typechecker::registry::{EnumTypeSignature, RecordTypeSignature};
use crate::typechecker::types::{Type, TypeReference};

use super::Inference;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;

pub(crate) fn is_byname_fqn(fqn: &Fqn) -> bool {
    fqn.package == PackagePath(vec!["standard".into(), "prelude".into()])
        && fqn.symbol == SymbolName("ByName".into())
}

/// The kind of symbol being resolved by `resolve_fqn`.
pub(super) enum SymbolKind {
    /// A function (non-generic or generic).
    Function,
    /// A global variable.
    Global,
    /// A record type (generic or non-generic).
    Record,
    /// An enum type.
    Enum,
    /// A class type (generic or non-generic).
    Class,
    /// A module.
    Module,
    /// A newtype.
    Newtype,
    /// A generic type alias.
    TypeAlias,
    /// A trait.
    Trait,
}

impl Inference<'_> {
    /// Resolve a bare name to its FQN: (1) import scope symbol, (2) current module, (3) same-package.
    /// Works for any symbol kind — functions, globals, record types, generic records.
    pub(super) fn resolve_fqn(&self, name: &str, kind: SymbolKind) -> Option<Fqn> {
        // 1. Check import scope for symbol import
        if let Some(resolved) = self.import_scope.lookup(name)
            && let ImportTarget::Symbol(ref fqn) = resolved.target
            && self.symbol_exists(fqn, &kind)
        {
            return Some(fqn.clone());
        }

        // 2. Check within current module or class (if any)
        if let Some(ref module_name) = self.container_name {
            let qualified = SymbolName(format!("{}.{}", module_name, name));
            let fqn = Fqn {
                package: self.package_path.clone(),
                symbol: qualified,
            };
            if self.symbol_exists(&fqn, &kind) {
                return Some(fqn);
            }
        }

        // 3. Same-package lookup
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(name.to_string()),
        };
        if self.symbol_exists(&fqn, &kind) {
            return Some(fqn);
        }

        // 4. Prelude fallback — check standard.prelude
        let prelude_fqn = Fqn {
            package: PackagePath(vec!["standard".into(), "prelude".into()]),
            symbol: SymbolName(name.to_string()),
        };
        if self.symbol_exists(&prelude_fqn, &kind) {
            Some(prelude_fqn)
        } else {
            None
        }
    }

    /// Check whether a symbol of the given kind exists at the given FQN.
    fn symbol_exists(&self, fqn: &Fqn, kind: &SymbolKind) -> bool {
        match kind {
            SymbolKind::Function => {
                self.registry
                    .lookup_function(fqn, &self.package_path, &self.current_file)
                    .is_some()
                    || self
                        .registry
                        .lookup_generic_function(fqn, &self.package_path)
                        .is_some()
            }
            SymbolKind::Global => {
                if self
                    .registry
                    .lookup_global(fqn, &self.package_path, &self.current_file)
                    .is_some()
                {
                    return true;
                }
                // Check if this is a generic module global (e.g., fqn.symbol = "Box.count")
                if let Some(dot_pos) = fqn.symbol.0.rfind('.') {
                    let module_name = &fqn.symbol.0[..dot_pos];
                    let global_name = &fqn.symbol.0[dot_pos + 1..];
                    let module_fqn = Fqn {
                        package: fqn.package.clone(),
                        symbol: SymbolName(module_name.to_string()),
                    };
                    if let Some(module_info) = self.registry.lookup_module(&module_fqn) {
                        return module_info
                            .generic_globals
                            .contains_key(&SymbolName(global_name.to_string()));
                    }
                }
                false
            }
            SymbolKind::Record => self
                .registry
                .lookup_record_type(fqn, &self.package_path, &self.current_file)
                .is_some(),
            SymbolKind::Enum => self
                .registry
                .lookup_enum_type(fqn, &self.package_path, &self.current_file)
                .is_some(),
            SymbolKind::Class => self
                .registry
                .lookup_class_type(fqn, &self.package_path)
                .is_some(),
            SymbolKind::Module => self.registry.lookup_module(fqn).is_some(),
            SymbolKind::Newtype => self
                .registry
                .lookup_newtype_type(fqn, &self.package_path, &self.current_file)
                .is_some(),
            SymbolKind::TypeAlias => self
                .registry
                .lookup_type_alias(fqn, &self.package_path, &self.current_file)
                .is_some(),
            SymbolKind::Trait => self
                .registry
                .lookup_trait(fqn, &self.package_path)
                .is_some(),
        }
    }
    /// Resolve a trait name to its FQN via imports or same-package lookup.
    pub(super) fn resolve_trait_fqn(&self, name: &str) -> Option<Fqn> {
        // 1. Check import scope
        if let Some(resolved) = self.import_scope.lookup(name)
            && let ImportTarget::Symbol(ref fqn) = resolved.target
            && self
                .registry
                .lookup_trait(fqn, &self.package_path)
                .is_some()
        {
            return Some(fqn.clone());
        }

        // 2. Same-package lookup
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(name.to_string()),
        };
        if self
            .registry
            .lookup_trait(&fqn, &self.package_path)
            .is_some()
        {
            return Some(fqn);
        }

        // 3. Prelude fallback
        let prelude_fqn = Fqn {
            package: PackagePath(vec!["standard".into(), "prelude".into()]),
            symbol: SymbolName(name.to_string()),
        };
        if self
            .registry
            .lookup_trait(&prelude_fqn, &self.package_path)
            .is_some()
        {
            return Some(prelude_fqn);
        }

        None
    }

    /// Returns true if `actual` is assignable to `expected`.
    /// Handles: Never → any, Error suppression, structural comparison for generic types
    /// with variance, Array, Tuple, InterfaceObject identity, and implicit concrete → trait
    /// object coercion when the concrete type implements the trait.
    #[allow(clippy::only_used_in_recursion)]
    pub(super) fn is_assignable(&self, expected: &Type, actual: &Type) -> bool {
        if actual == expected
            || actual.is_error()
            || expected.is_error()
            || actual.is_never()
            || expected.is_any()
        {
            return true;
        }
        if matches!(
            (expected, actual),
            (Type::AssociatedProjection(_), Type::AssociatedProjection(_))
        ) {
            return crate::typechecker::subtyping::identical(expected, actual);
        }
        // TypeParameter identity: same name → assignable (used in generic bodies)
        if let (
            Type::TypeVariable(a, _) | Type::GenericParam(a, _, _),
            Type::TypeVariable(b, _) | Type::GenericParam(b, _, _),
        ) = (expected, actual)
        {
            return a == b;
        }
        // Newtype identity: same FQN → assignable (inner type may differ during collection)
        if let (Type::Newtype(fqn_e, _), Type::Newtype(fqn_a, _)) = (expected, actual) {
            return fqn_e == fqn_a;
        }
        // Structural comparison for GenericRecord types: match FQN + type args with variance.
        if let (
            Type::GenericRecord {
                fqn: fqn_e,
                type_args: args_e,
                ..
            },
            Type::GenericRecord {
                fqn: fqn_a,
                type_args: args_a,
                ..
            },
        ) = (expected, actual)
        {
            if fqn_e != fqn_a || args_e.len() != args_a.len() {
                return false;
            }
            return args_e
                .iter()
                .zip(args_a.iter())
                .all(|((v_e, e), (_, a))| self.check_variance(*v_e, e, a));
        }
        // Structural comparison for GenericEnum types: match FQN + type args with variance.
        if let (
            Type::GenericEnum {
                fqn: fqn_e,
                type_args: args_e,
                ..
            },
            Type::GenericEnum {
                fqn: fqn_a,
                type_args: args_a,
                ..
            },
        ) = (expected, actual)
        {
            if fqn_e != fqn_a || args_e.len() != args_a.len() {
                return false;
            }
            return args_e
                .iter()
                .zip(args_a.iter())
                .all(|((v_e, e), (_, a))| self.check_variance(*v_e, e, a));
        }
        // Interface object → interface object: superset → subset. Every expected
        // component must be matched by an actual component with the same FQN and
        // pairwise-assignable type args ((A and B) <: A, (A and B and C) <: (A and B)).
        if let (
            Type::InterfaceObject {
                traits: traits_e, ..
            },
            Type::InterfaceObject {
                traits: traits_a, ..
            },
        ) = (expected, actual)
        {
            // Component type args are INVARIANT: a generic interface's args
            // feed erased vtable slots where values flow both directions, so
            // `Sink<Cat>` is not a `Sink<Animal>` (and vice versa).
            // With `extends`, an actual component also covers an expected one
            // in its super closure: a `B`-object is an `A`-object.
            return traits_e.iter().all(|ce| {
                traits_a.iter().any(|ca| {
                    let args = if ce.trait_fqn == ca.trait_fqn {
                        Some(ca.trait_type_args.clone())
                    } else {
                        self.registry.super_closure_args(
                            &ca.trait_fqn,
                            &ca.trait_type_args,
                            &ce.trait_fqn,
                        )
                    };
                    args.is_some_and(|args| {
                        args.len() == ce.trait_type_args.len()
                            && args
                                .iter()
                                .zip(&ce.trait_type_args)
                                .all(|(a, e)| self.is_assignable(e, a) && self.is_assignable(a, e))
                    })
                })
            });
        }
        if actual.is_class_type() && expected.is_class_type() {
            return crate::typechecker::subtyping::is_subtype(self.registry, actual, expected);
        }
        // Structural comparison for Newtype: same FQN.
        if let (Type::Newtype(fqn_e, _), Type::Newtype(fqn_a, _)) = (expected, actual) {
            return fqn_e == fqn_a;
        }
        // Structural comparison for GenericNewtype: same FQN + type args with variance
        if let (
            Type::GenericNewtype {
                fqn: fqn_e,
                type_args: args_e,
                ..
            },
            Type::GenericNewtype {
                fqn: fqn_a,
                type_args: args_a,
                ..
            },
        ) = (expected, actual)
            && fqn_e == fqn_a
        {
            return args_e.len() == args_a.len()
                && args_e
                    .iter()
                    .zip(args_a.iter())
                    .all(|((v_e, e), (_, a))| self.check_variance(*v_e, e, a));
        }
        // Different wrappers may still support the T -> ByName<T>
        // conversion below, including when T is itself a newtype.
        // Structural comparison for Array types.
        //
        // `Array<T>` is INVARIANT in `T`, so the element types must agree in both
        // directions. Recursing one way only made arrays covariant, which is
        // unsound for a mutable container: `Array<Dog>` passed as `Array<Animal>`
        // lets the callee store an `Animal` into it, and the read back through the
        // `Array<Dog>` alias traps. The same hole let `Array<Never>` — what
        // `Array.empty()` infers with no context — flow into any `Array<T>`,
        // where it reached codegen and produced invalid WASM.
        if let (Type::Array(elem_e), Type::Array(elem_a)) = (expected, actual) {
            return self.is_assignable(elem_e, elem_a) && self.is_assignable(elem_a, elem_e);
        }
        if let (Type::TupleExtend(left_e, right_e), Type::TupleExtend(left_a, right_a)) =
            (expected, actual)
        {
            // Only the right element can widen without changing the tuple's outer shape.
            return crate::typechecker::subtyping::identical(left_e, left_a)
                && self.is_assignable(right_e, right_a);
        }
        // Structural comparison for Tuple types
        if let (Type::Tuple(types_e, _), Type::Tuple(types_a, _)) = (expected, actual) {
            if types_e.len() != types_a.len() {
                return false;
            }
            return types_e
                .iter()
                .zip(types_a.iter())
                .all(|(e, a)| self.is_assignable(e, a));
        }
        // Structural comparison for Function types: contravariant in params, covariant in return
        if let (Type::Function(params_e, ret_e), Type::Function(params_a, ret_a)) =
            (expected, actual)
        {
            if params_e.len() != params_a.len() {
                return false;
            }
            // Params are contravariant: each expected param must be assignable FROM actual param
            let params_ok = params_e
                .iter()
                .zip(params_a.iter())
                .all(|(e, a)| self.is_assignable_in_function_position(a, e));
            // Return is covariant: actual return must be assignable TO expected return
            let ret_ok = self.is_assignable_in_function_position(ret_e, ret_a);
            return params_ok && ret_ok;
        }
        // Bounds carry full parent arguments, not just nominal ancestry.
        if matches!(actual, Type::TypeVariable(..) | Type::GenericParam(..))
            && crate::typechecker::subtyping::is_subtype(self.registry, actual, expected)
        {
            return true;
        }
        // Implicit coercion: concrete type → interface object type. The concrete
        // type must satisfy every component of the (possibly intersected) set.
        // A type parameter satisfies a component via its declared bounds
        // (type_satisfies_trait checks them), so `T where T: I` coerces to `I`.
        if let Type::InterfaceObject { traits, .. } = expected
            && traits
                .iter()
                .all(|c| self.type_satisfies_trait(&c.trait_fqn, &c.trait_type_args, actual, 0))
        {
            return true;
        }
        // Implicit coercion: T → ByName<T> (auto-wrapping at call sites)
        // ByName<T> inner type is () => T; the type arg is T itself.
        if let Type::GenericNewtype { fqn, type_args, .. } = expected
            && is_byname_fqn(fqn)
            && type_args.len() == 1
        {
            let inner_t = &type_args[0].1;
            return self.is_assignable(inner_t, actual);
        }
        false
    }

    /// Check assignability for a single type argument position given its variance.
    fn check_variance(&self, variance: Variance, expected: &Type, actual: &Type) -> bool {
        if actual.is_error() || expected.is_error() {
            return true;
        }
        crate::typechecker::subtyping::argument(self.registry, variance, expected, actual)
    }

    /// Compute the least upper bound (common supertype) of two types. Used by
    /// match-arm / if-else unification when neither arm's type is directly
    /// assignable to the other but they share a structural shape. For
    /// `GenericClass<T₁, T₂, …>` / `GenericEnum` / `GenericRecord` with the
    /// same FQN, computes the LUB per type-arg using the arg's variance.
    /// Returns `None` if no LUB exists (incomparable types).
    pub(super) fn least_upper_bound(&self, a: &Type, b: &Type) -> Option<Type> {
        if self.is_assignable(b, a) {
            return Some(b.clone());
        }
        if self.is_assignable(a, b) {
            return Some(a.clone());
        }
        match (a, b) {
            (
                Type::GenericClass {
                    fqn: fa,
                    mangled_name,
                    type_args: aa,
                },
                Type::GenericClass {
                    fqn: fb,
                    type_args: ab,
                    ..
                },
            ) if fa == fb && aa.len() == ab.len() => {
                let new_args = self.lub_type_args(aa, ab)?;
                Some(Type::GenericClass {
                    fqn: fa.clone(),
                    mangled_name: mangled_name.clone(),
                    type_args: new_args,
                })
            }
            (
                Type::GenericEnum {
                    fqn: fa,
                    mangled_name,
                    type_args: aa,
                },
                Type::GenericEnum {
                    fqn: fb,
                    type_args: ab,
                    ..
                },
            ) if fa == fb && aa.len() == ab.len() => {
                let new_args = self.lub_type_args(aa, ab)?;
                Some(Type::GenericEnum {
                    fqn: fa.clone(),
                    mangled_name: mangled_name.clone(),
                    type_args: new_args,
                })
            }
            (
                Type::GenericRecord {
                    fqn: fa,
                    mangled_name,
                    type_args: aa,
                },
                Type::GenericRecord {
                    fqn: fb,
                    type_args: ab,
                    ..
                },
            ) if fa == fb && aa.len() == ab.len() => {
                let new_args = self.lub_type_args(aa, ab)?;
                Some(Type::GenericRecord {
                    fqn: fa.clone(),
                    mangled_name: mangled_name.clone(),
                    type_args: new_args,
                })
            }
            _ => None,
        }
    }

    /// LUB over a pair of `(Variance, Type)` arg lists. Covariant args use
    /// `least_upper_bound`; contravariant would need `greatest_lower_bound`
    /// (not implemented — bail with None); invariant args must be equal.
    fn lub_type_args(
        &self,
        aa: &[(Variance, Type)],
        ab: &[(Variance, Type)],
    ) -> Option<Vec<(Variance, Type)>> {
        let mut out = Vec::with_capacity(aa.len());
        for ((va, ta), (_, tb)) in aa.iter().zip(ab.iter()) {
            let combined = match va {
                // Interface-object positions inside generic args cannot be
                // unified by coercion (no per-element reification) — require
                // exact equality, mirroring check_variance. Never yields to
                // the other side (empty branch of an if/match).
                Variance::Covariant
                    if (ta.contains_interface_object() || tb.contains_interface_object())
                        && !matches!(ta, Type::Never | Type::Error)
                        && !matches!(tb, Type::Never | Type::Error) =>
                {
                    if ta == tb {
                        ta.clone()
                    } else {
                        return None;
                    }
                }
                Variance::Covariant => self.least_upper_bound(ta, tb)?,
                Variance::Invariant => {
                    if ta == tb {
                        ta.clone()
                    } else {
                        return None;
                    }
                }
                Variance::Contravariant => {
                    // Greatest lower bound not implemented. If either type
                    // is assignable from the other (asymmetric subtype),
                    // pick the narrower one; otherwise bail.
                    if self.is_assignable(ta, tb) {
                        tb.clone()
                    } else if self.is_assignable(tb, ta) {
                        ta.clone()
                    } else {
                        return None;
                    }
                }
            };
            out.push((*va, combined));
        }
        Some(out)
    }

    /// Check that `actual` (rhs) is assignable to `expected` (lhs).
    /// Emits a diagnostic on mismatch.
    pub(super) fn check_assignable(&mut self, span: Span, expected: &Type, actual: &Type) {
        if !self.is_assignable(expected, actual) {
            self.diagnostics.error(
                span,
                format!("type mismatch: expected '{}', found '{}'", expected, actual),
            );
            return;
        }
        // A concrete → interface coercion whose target is provided only by
        // several distinct sub-trait impls (extends) has no principled choice
        // of implementation — require a direct impl.
        if let Type::InterfaceObject { traits, .. } = expected
            && !matches!(actual, Type::InterfaceObject { .. })
        {
            for c in traits {
                if let Some(providers) =
                    self.ambiguous_trait_providers(&c.trait_fqn, &c.trait_type_args, actual)
                {
                    let names: Vec<String> = providers
                        .iter()
                        .map(|f| format!("'{}'", f.symbol))
                        .collect();
                    self.diagnostics.error(
                            span.clone(),
                            format!(
                                "ambiguous implementations of trait '{}' for type '{}': provided by both {}; implement '{}' directly to disambiguate",
                                c.trait_fqn.symbol, actual, names.join(" and "),
                                c.trait_fqn.symbol,
                            ),
                        );
                }
            }
        }
    }

    /// Resolve a type expression to a concrete type.
    /// Assignability for a type appearing inside a function type's params or
    /// return. Interface-object conversions (concrete → interface, intersection
    /// → subset) are value coercions that build a new fat pointer — nothing can
    /// reify them for a function *value*, so positions inside function types
    /// require interface-object types to match exactly.
    fn is_assignable_in_function_position(&self, expected: &Type, actual: &Type) -> bool {
        // Exact match whenever an interface object appears at any depth —
        // tuples/containers inside a function type can no more reify a
        // fat-pointer conversion than the top level can.
        if !matches!(actual, Type::Never | Type::Error)
            && !matches!(expected, Type::Never | Type::Error)
            && (expected.contains_interface_object() || actual.contains_interface_object())
        {
            return expected == actual;
        }
        self.is_assignable(expected, actual)
    }

    pub(super) fn resolve_type_expr(&mut self, type_expr: &TypeExpr) -> Type {
        match type_expr {
            TypeExpr::TupleExtend(left, right, _) => {
                Type::tuple_extend(self.resolve_type_expr(left), self.resolve_type_expr(right))
            }
            TypeExpr::Tuple(type_exprs, _span) => {
                let types: Vec<Type> = type_exprs
                    .iter()
                    .map(|te| self.resolve_type_expr(te))
                    .collect();
                if types.iter().any(|t| t.is_error()) {
                    return Type::Error;
                }
                let mn = MangledName::for_tuple(&types);
                Type::Tuple(types, mn)
            }
            TypeExpr::Named(named) => {
                // A type-annotation position is strict: a generic type
                // referenced without type args is an error. We detect this
                // before delegating so the user gets a clear "needs type
                // arguments" message rather than a downstream "type
                // mismatch: T vs concrete" once the template TypeVariable
                // leaks into the let-binding/return-type check.
                if named.type_args.is_empty()
                    && let Some(expected) = self.expected_type_param_count(&named.name.value)
                    && expected > 0
                {
                    self.diagnostics.error(
                        named.span.clone(),
                        format!(
                            "expected {expected} type argument(s) for '{}', found 0",
                            named.name.value
                        ),
                    );
                    return Type::Error;
                }

                if let Some(ty) =
                    self.resolve_type_name(&named.name.value, &named.type_args, &named.span)
                {
                    self.record_type_reference(&ty, &named.name.span);
                    return ty;
                }
                // None means: unknown name, OR type-argument count mismatch.
                self.report_named_type_error(named);
                Type::Error
            }
            TypeExpr::Function(param_exprs, ret_expr, _span) => {
                let param_types: Vec<Type> = param_exprs
                    .iter()
                    .map(|te| self.resolve_type_expr(te))
                    .collect();
                let ret_type = self.resolve_type_expr(ret_expr);
                if param_types.iter().any(|t| t.is_error()) || ret_type.is_error() {
                    return Type::Error;
                }
                Type::Function(param_types, Box::new(ret_type))
            }
            TypeExpr::Intersection(components) => self.resolve_intersection_type_expr(components),
        }
    }

    /// Resolve an intersection type expression (`A and B`) to an
    /// interface-object type over all components. Accumulates errors; any
    /// failing component makes the whole type `Type::Error`.
    fn resolve_intersection_type_expr(
        &mut self,
        components: &[crate::parser::ast::NamedType],
    ) -> Type {
        let mut resolved: Vec<(Fqn, Vec<Type>)> = Vec::new();
        let mut failed = false;
        for named in components {
            match self.resolve_intersection_component(named) {
                Some(component) => resolved.push(component),
                None => failed = true,
            }
        }
        // Same interface twice with different type args is contradictory;
        // exact duplicates are deduped by the constructor.
        for i in 0..resolved.len() {
            for j in (i + 1)..resolved.len() {
                if resolved[i].0 == resolved[j].0 && resolved[i].1 != resolved[j].1 {
                    self.diagnostics.error(
                        components[j].span.clone(),
                        format!(
                            "interface '{}' appears more than once in intersection with different type arguments",
                            resolved[j].0.symbol.0
                        ),
                    );
                    failed = true;
                }
            }
        }
        if failed {
            return Type::Error;
        }
        Type::interface_intersection(resolved)
    }

    /// Resolve one component of an intersection type expression. Emits an
    /// error and returns `None` for unknown names, non-interfaces, and arity
    /// mismatches.
    fn resolve_intersection_component(
        &mut self,
        named: &crate::parser::ast::NamedType,
    ) -> Option<(Fqn, Vec<Type>)> {
        let Some(trait_fqn) = self.resolve_trait_fqn(&named.name.value) else {
            // Distinguish a known non-interface type from a truly unknown name.
            let known_non_trait = self
                .resolve_type_name(&named.name.value, &[], &named.span)
                .is_some_and(|t| !t.is_error());
            let message = if known_non_trait {
                format!(
                    "'{}' is not an interface; intersection components must be interfaces",
                    named.name.value
                )
            } else {
                format!("unknown type: '{}'", named.name.value)
            };
            self.diagnostics.error(named.span.clone(), message);
            return None;
        };
        let Some(sig) = self
            .registry
            .lookup_trait(&trait_fqn, &self.package_path)
            .cloned()
        else {
            self.diagnostics.error(
                named.span.clone(),
                format!("unknown type: '{}'", named.name.value),
            );
            return None;
        };
        if !sig.is_interface {
            self.diagnostics.error(
                named.name.span.clone(),
                format!(
                    "trait '{}' cannot be used as a type; declare it as an 'interface' to use it as an object type",
                    trait_fqn.symbol.0
                ),
            );
            return None;
        }
        if named.type_args.len() != sig.type_params.len() {
            self.diagnostics.error(
                named.span.clone(),
                format!(
                    "expected {} type argument(s) for '{}', found {}",
                    sig.type_params.len(),
                    named.name.value,
                    named.type_args.len()
                ),
            );
            return None;
        }
        let args: Vec<Type> = named
            .type_args
            .iter()
            .map(|ta| self.resolve_type_expr(ta))
            .collect();
        if args.iter().any(|t| t.is_error()) {
            return None;
        }
        Some((trait_fqn, args))
    }

    /// Record a type reference for LSP navigation, only for types with navigable definitions.
    fn record_type_reference(&mut self, ty: &Type, span: &Span) {
        let is_navigable = matches!(
            ty,
            Type::Record(..)
                | Type::Enum(..)
                | Type::Class(..)
                | Type::Newtype(..)
                | Type::GenericRecord { .. }
                | Type::GenericEnum { .. }
                | Type::GenericClass { .. }
                | Type::GenericNewtype { .. }
                | Type::InterfaceObject { .. }
        );
        if is_navigable {
            self.type_references.push(TypeReference {
                ty: ty.clone(),
                span: span.clone(),
            });
        }
    }

    /// Resolve a slice of type expressions into concrete types.
    /// Returns `None` if any type argument resolves to an error.
    pub(super) fn resolve_type_args(&mut self, type_args: &[TypeExpr]) -> Option<Vec<Type>> {
        let resolved: Vec<Type> = type_args
            .iter()
            .map(|ta| self.resolve_type_expr(ta))
            .collect();
        if resolved.iter().any(|t| t.is_error()) {
            None
        } else {
            Some(resolved)
        }
    }

    /// How many type parameters a named type expects. Used by the
    /// type-annotation strict check to reject bare generic references
    /// before they decay into template `TypeVariable`s.
    ///
    /// Returns:
    /// - `Some(0)` for primitives, type parameters in scope, non-generic
    ///   records/enums/classes/newtypes/type-aliases, and plain traits.
    /// - `Some(N)` for generic types with N type parameters (including
    ///   `Array` with N=1).
    /// - `None` if the name doesn't resolve to any known type.
    fn expected_type_param_count(&self, name: &str) -> Option<usize> {
        // Type parameter in scope.
        let tp_name = crate::common::types::TypeParamName(name.to_string());
        if self.current_type_params.contains_key(&tp_name) {
            return Some(0);
        }
        if Type::from_primitive(name).is_some() {
            return Some(0);
        }
        if name == "Array" {
            return Some(1);
        }
        if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Record) {
            return self
                .registry
                .lookup_record_type(&fqn, &self.package_path, &self.current_file)
                .map(|d| d.type_params.len());
        }
        if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Enum) {
            return self
                .registry
                .lookup_enum_type(&fqn, &self.package_path, &self.current_file)
                .map(|d| d.type_params.len());
        }
        if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Class) {
            return self
                .registry
                .lookup_class_type(&fqn, &self.package_path)
                .map(|d| d.type_params.len());
        }
        if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Newtype) {
            return self
                .registry
                .lookup_newtype_type(&fqn, &self.package_path, &self.current_file)
                .map(|d| d.type_params.len());
        }
        if let Some(fqn) = self.resolve_fqn(name, SymbolKind::TypeAlias) {
            return self
                .registry
                .lookup_type_alias(&fqn, &self.package_path, &self.current_file)
                .map(|s| s.type_params.len());
        }
        if let Some(trait_fqn) = self.resolve_trait_fqn(name) {
            return self
                .registry
                .lookup_trait(&trait_fqn, &self.package_path)
                .map(|s| s.type_params.len());
        }
        None
    }

    /// Emit a focused diagnostic when `resolve_type_name` returns `None`
    /// for a `TypeExpr::Named`. Distinguishes "unknown type" from
    /// "wrong type-argument count for a known generic".
    fn report_named_type_error(&mut self, named: &crate::parser::ast::NamedType) {
        let name = &named.name.value;
        let got = named.type_args.len();

        // Array (intrinsic): exactly 1 type arg required.
        if name == "Array" {
            self.diagnostics.error(
                named.span.clone(),
                format!("expected 1 type argument for 'Array', found {got}"),
            );
            return;
        }

        // Look up the type in each kind to see if it exists and how many
        // type params it expects.
        let known: Option<usize> = if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Record) {
            self.registry
                .lookup_record_type(&fqn, &self.package_path, &self.current_file)
                .map(|d| d.type_params.len())
        } else if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Enum) {
            self.registry
                .lookup_enum_type(&fqn, &self.package_path, &self.current_file)
                .map(|d| d.type_params.len())
        } else if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Class) {
            self.registry
                .lookup_class_type(&fqn, &self.package_path)
                .map(|d| d.type_params.len())
        } else if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Newtype) {
            self.registry
                .lookup_newtype_type(&fqn, &self.package_path, &self.current_file)
                .map(|d| d.type_params.len())
        } else if let Some(fqn) = self.resolve_fqn(name, SymbolKind::TypeAlias) {
            self.registry
                .lookup_type_alias(&fqn, &self.package_path, &self.current_file)
                .map(|s| s.type_params.len())
        } else if let Some(trait_fqn) = self.resolve_trait_fqn(name) {
            self.registry
                .lookup_trait(&trait_fqn, &self.package_path)
                .map(|s| s.type_params.len())
        } else {
            None
        };

        match known {
            Some(expected) if expected != got => {
                self.diagnostics.error(
                    named.span.clone(),
                    format!("expected {expected} type argument(s) for '{name}', found {got}"),
                );
            }
            _ => {
                self.diagnostics
                    .error(named.span.clone(), format!("unknown type: '{name}'"));
            }
        }
    }

    /// Resolve a type name to its full `Type`, honoring type arguments.
    ///
    /// Contract:
    /// - `type_args` must match what the named type expects:
    ///   - Non-generic kinds (primitives, type parameters, non-generic
    ///     records/enums/classes/newtypes, plain interface objects) require
    ///     `type_args` to be empty.
    ///   - Generic kinds require `type_args.len() == declared_type_params.len()`
    ///     (or arity 1 for `Array`).
    /// - On mismatch (wrong count, unknown name, unresolvable type arg),
    ///   returns `None`. This function does NOT emit diagnostics; callers
    ///   (`resolve_type_expr` etc.) emit them.
    ///
    /// `span` is used when the function delegates to instantiation helpers
    /// that report trait-bound failures.
    pub(super) fn resolve_type_name(
        &mut self,
        name: &str,
        type_args: &[TypeExpr],
        span: &Span,
    ) -> Option<Type> {
        if let Some((root, member)) = name.split_once('.')
            && let Some(receiver) = self
                .current_type_params
                .get(&crate::common::types::TypeParamName(root.to_string()))
                .cloned()
        {
            let parameters: Vec<_> = type_args
                .iter()
                .map(|ty| self.resolve_type_expr(ty))
                .collect();
            return Some(
                match crate::typechecker::associated_types::resolve_reference(
                    &receiver,
                    member,
                    parameters,
                    &[self.registry],
                ) {
                    Ok(ty) => ty,
                    Err(message) => {
                        self.diagnostics.error(span.clone(), message);
                        Type::Error
                    }
                },
            );
        }
        if let Some(Type::AssociatedProjection(projection)) = self
            .current_type_params
            .get(&crate::common::types::TypeParamName(name.to_string()))
            .cloned()
        {
            if projection.parameters.len() != type_args.len() {
                return None;
            }
            let mut projection = *projection;
            projection.parameters = type_args
                .iter()
                .map(|ty| self.resolve_type_expr(ty))
                .collect();
            return Some(projection.into_type());
        }
        // 0. Type parameter in scope (e.g. `T` inside a generic function body).
        //    No type args — type parameters aren't themselves generic here.
        if type_args.is_empty() {
            let tp_name = crate::common::types::TypeParamName(name.to_string());
            if let Some(ty) = self.current_type_params.get(&tp_name).cloned() {
                return Some(ty);
            }
        }

        // 1. Primitive — no type args allowed.
        if let Some(ty) = Type::from_primitive(name) {
            return if type_args.is_empty() { Some(ty) } else { None };
        }

        // 2. `Array` is intrinsic-generic with arity 1.
        //    With no type args supplied, return the template form
        //    (`Array<TypeVariable("T")>`) so callers like static-method
        //    dispatch can proceed; the typechecker is responsible for
        //    later binding T from context (expected type, argument types).
        //    If the element fails to resolve, propagate `Type::Error`
        //    inside the Array rather than returning `None` (so the
        //    caller's "unknown type" branch doesn't double-report).
        if name == "Array" {
            if type_args.is_empty() {
                return Some(Type::Array(Box::new(Type::TypeVariable(
                    crate::common::types::TypeParamName("T".to_string()),
                    vec![],
                ))));
            }
            if type_args.len() != 1 {
                return None;
            }
            let elem = self.resolve_type_expr(&type_args[0]);
            return Some(Type::Array(Box::new(elem)));
        }

        // Helper: when a type's args fail to resolve (any element is
        // `Type::Error`), the recursive `resolve_type_expr` calls already
        // reported the inner diagnostic — surface a top-level `Type::Error`
        // rather than `None`, so the caller doesn't double-report.
        let resolve_args_or_error = |args: &[TypeExpr], me: &mut Self| -> Vec<Type> {
            args.iter().map(|ta| me.resolve_type_expr(ta)).collect()
        };

        // Helper: build TypeVariable placeholders for a list of type params,
        // carrying each param's bounds from the trait-bounds table.
        let template_args = |type_params: &[crate::common::types::TypeParamName],
                             trait_bounds: &crate::typechecker::types::TraitBounds|
         -> Vec<Type> {
            type_params
                .iter()
                .map(|tp| {
                    let bounds = trait_bounds.get(tp).cloned().unwrap_or_default();
                    Type::TypeVariable(tp.clone(), bounds)
                })
                .collect()
        };

        // 3. Records (registered as `Type::Record(fqn, mn)` whether generic
        //    or not; generic-ness lives in `record_types`).
        if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Record)
            && let Some(def) = self
                .registry
                .lookup_record_type(&fqn, &self.package_path, &self.current_file)
                .cloned()
        {
            if def.type_params.is_empty() {
                return if type_args.is_empty() {
                    self.registry
                        .lookup_type(&fqn, &self.package_path, &self.current_file)
                        .cloned()
                } else {
                    None
                };
            }
            let resolved_args = if type_args.is_empty() {
                // Template form — caller resolves T from context.
                template_args(&def.type_params, &def.trait_bounds)
            } else {
                if type_args.len() != def.type_params.len() {
                    return None;
                }
                let args = resolve_args_or_error(type_args, self);
                if args.iter().any(|t| t.is_error()) {
                    return Some(Type::Error);
                }
                args
            };
            return Some(self.resolve_generic_record(&fqn, &def, &resolved_args, span));
        }

        // 4. Enums.
        if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Enum)
            && let Some(def) = self
                .registry
                .lookup_enum_type(&fqn, &self.package_path, &self.current_file)
                .cloned()
        {
            if def.type_params.is_empty() {
                return if type_args.is_empty() {
                    self.registry
                        .lookup_type(&fqn, &self.package_path, &self.current_file)
                        .cloned()
                } else {
                    None
                };
            }
            let resolved_args = if type_args.is_empty() {
                template_args(&def.type_params, &def.trait_bounds)
            } else {
                if type_args.len() != def.type_params.len() {
                    return None;
                }
                let args = resolve_args_or_error(type_args, self);
                if args.iter().any(|t| t.is_error()) {
                    return Some(Type::Error);
                }
                args
            };
            return Some(self.resolve_generic_enum_type(&fqn, &def, &resolved_args));
        }

        // 5. Classes.
        if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Class)
            && let Some(def) = self
                .registry
                .lookup_class_type(&fqn, &self.package_path)
                .cloned()
        {
            if def.type_params.is_empty() {
                return if type_args.is_empty() {
                    self.registry
                        .lookup_type(&fqn, &self.package_path, &self.current_file)
                        .cloned()
                } else {
                    None
                };
            }
            let resolved_args = if type_args.is_empty() {
                template_args(&def.type_params, &def.trait_bounds)
            } else {
                if type_args.len() != def.type_params.len() {
                    return None;
                }
                let args = resolve_args_or_error(type_args, self);
                if args.iter().any(|t| t.is_error()) {
                    return Some(Type::Error);
                }
                args
            };
            return Some(self.infer_generic_class(&fqn, &def, &resolved_args, span));
        }

        // 6. Newtypes.
        if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Newtype)
            && let Some(sig) = self
                .registry
                .lookup_newtype_type(&fqn, &self.package_path, &self.current_file)
                .cloned()
        {
            if sig.type_params.is_empty() {
                return if type_args.is_empty() {
                    self.registry
                        .lookup_type(&fqn, &self.package_path, &self.current_file)
                        .cloned()
                } else {
                    None
                };
            }
            let resolved_args = if type_args.is_empty() {
                template_args(&sig.type_params, &sig.trait_bounds)
            } else {
                if type_args.len() != sig.type_params.len() {
                    return None;
                }
                let args = resolve_args_or_error(type_args, self);
                if args.iter().any(|t| t.is_error()) {
                    return Some(Type::Error);
                }
                args
            };
            return Some(self.resolve_generic_newtype(&fqn, &sig, &resolved_args, span));
        }

        // 7. Type aliases — covers both generic and non-generic.
        if let Some(alias_fqn) = self.resolve_fqn(name, SymbolKind::TypeAlias)
            && let Some(alias_sig) = self
                .registry
                .lookup_type_alias(&alias_fqn, &self.package_path, &self.current_file)
                .cloned()
        {
            if alias_sig.type_params.is_empty() {
                if !type_args.is_empty() {
                    return None;
                }
                return Some(alias_sig.expanded_type);
            }
            if type_args.len() != alias_sig.type_params.len() {
                return None;
            }
            let resolved_args = resolve_args_or_error(type_args, self);
            if resolved_args.iter().any(|t| t.is_error()) {
                return Some(Type::Error);
            }
            self.check_trait_bounds(
                &alias_sig.trait_bounds,
                &alias_sig.type_params,
                &resolved_args,
                span,
            );
            let substitution =
                TypeParamSubstitution::from_pairs(&alias_sig.type_params, &resolved_args);
            return Some(apply_substitution(&substitution, &alias_sig.expanded_type));
        }

        // 8. Interfaces → interface object (supports generic interfaces).
        // A plain trait is bound-only and may not appear in type position.
        if let Some(trait_fqn) = self.resolve_trait_fqn(name)
            && let Some(sig) = self
                .registry
                .lookup_trait(&trait_fqn, &self.package_path)
                .cloned()
        {
            if type_args.len() != sig.type_params.len() {
                return None;
            }
            if !sig.is_interface {
                self.diagnostics.error(
                        span.clone(),
                        format!(
                            "trait '{}' cannot be used as a type; declare it as an 'interface' to use it as an object type",
                            trait_fqn.symbol.0
                        ),
                    );
                return Some(Type::Error);
            }
            let trait_type_args = resolve_args_or_error(type_args, self);
            if trait_type_args.iter().any(|t| t.is_error()) {
                return Some(Type::Error);
            }
            return Some(Type::interface_object(trait_fqn, trait_type_args));
        }

        None
    }

    /// Resolve a bare record type name: (1) import scope symbol, (2) same-package.
    pub(super) fn resolve_record_type(&self, name: &str) -> Option<RecordTypeSignature> {
        let fqn = self.resolve_fqn(name, SymbolKind::Record)?;
        self.registry
            .lookup_record_type(&fqn, &self.package_path, &self.current_file)
            .cloned()
    }

    /// Resolve a bare enum type name: (1) import scope symbol, (2) same-package.
    pub(super) fn resolve_enum_type(&self, name: &str) -> Option<EnumTypeSignature> {
        let fqn = self.resolve_fqn(name, SymbolKind::Enum)?;
        self.registry
            .lookup_enum_type(&fqn, &self.package_path, &self.current_file)
            .cloned()
    }
}
