//! Symbolic partitions of sealed leaves. These describe declared types, never
//! the set of instantiations which happen to be constructed in a final module.
use crate::common::types::{Fqn, MangledName, TypeParamName, Variance};
use crate::typechecker::{registry::Registry, subtyping, types::Type};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Constraint {
    actual: Type,
    expected: Type,
    positive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClassRegion {
    pub ty: Type,
    constraints: Vec<Constraint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Proof {
    Proven,
    Disproven,
    Unknown,
}

fn variable(t: &Type) -> bool {
    matches!(t, Type::TypeVariable(..) | Type::GenericParam(..))
}

/// Reduce structured subtype queries to a conjunction of atomic relations.
fn reduce(registry: &Registry, actual: &Type, expected: &Type) -> Vec<Constraint> {
    if subtyping::is_subtype(registry, actual, expected) {
        return vec![];
    }
    if actual.is_class_type() && expected.is_class_type() {
        let fqn = expected.try_to_fqn().expect("class name");
        let Some(projected) = subtyping::project_class(registry, actual, &fqn) else {
            return vec![Constraint {
                actual: Type::Any,
                expected: Type::Never,
                positive: true,
            }];
        };
        if &projected != actual {
            return reduce(registry, &projected, expected);
        }
    }
    let params = match (actual, expected) {
        (
            Type::GenericClass {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericClass {
                fqn: e,
                type_args: ea,
                ..
            },
        )
        | (
            Type::GenericRecord {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericRecord {
                fqn: e,
                type_args: ea,
                ..
            },
        )
        | (
            Type::GenericEnum {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericEnum {
                fqn: e,
                type_args: ea,
                ..
            },
        )
        | (
            Type::GenericNewtype {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericNewtype {
                fqn: e,
                type_args: ea,
                ..
            },
        ) if a == e && aa.len() == ea.len() => Some((aa, ea)),
        _ => None,
    };
    if let Some((aa, ea)) = params {
        let mut out = vec![];
        for ((_, a), (variance, e)) in aa.iter().zip(ea) {
            // A different, symbolic argument may have prevented the whole type
            // relation from being proven above. Preserve each argument relation
            // already established by the shared representation rules, including
            // widening an existing interface object to Any.
            if subtyping::argument(registry, *variance, e, a) {
                continue;
            }
            if (a.contains_interface_object() || e.contains_interface_object())
                && !a.is_never()
                && !e.is_never()
            {
                if a.contains_type_parameter() || e.contains_type_parameter() {
                    // Symbolic arguments can later become Never, which the
                    // representation relation permits without exact identity.
                    // Keep that disjunction opaque instead of strengthening it
                    // to equality and discarding possible leaf instantiations.
                    return vec![Constraint {
                        actual: actual.clone(),
                        expected: expected.clone(),
                        positive: true,
                    }];
                }
                // Match representation-preserving generic argument restrictions.
                out.extend(reduce(registry, a, e));
                out.extend(reduce(registry, e, a));
            } else {
                if *variance != Variance::Contravariant {
                    out.extend(reduce(registry, a, e));
                }
                if *variance != Variance::Covariant {
                    out.extend(reduce(registry, e, a));
                }
            }
        }
        return out;
    }
    match (actual, expected) {
        (Type::Array(a), Type::Array(e)) => {
            let mut out = reduce(registry, a, e);
            out.extend(reduce(registry, e, a));
            return out;
        }
        (Type::Tuple(a, _), Type::Tuple(e, _)) if a.len() == e.len() => {
            return a
                .iter()
                .zip(e)
                .flat_map(|(a, e)| reduce(registry, a, e))
                .collect();
        }
        (Type::Function(a, ar), Type::Function(e, er)) if a.len() == e.len() => {
            let mut out: Vec<_> = a
                .iter()
                .zip(e)
                .flat_map(|(a, e)| reduce(registry, e, a))
                .collect();
            out.extend(reduce(registry, ar, er));
            return out;
        }
        _ => {}
    }
    vec![Constraint {
        actual: actual.clone(),
        expected: expected.clone(),
        positive: true,
    }]
}

impl ClassRegion {
    pub fn new(registry: &Registry, leaf: &Fqn, subject: &Type) -> Option<Self> {
        let sig = registry.get_class_type(leaf)?;
        let params: Vec<_> = sig
            .type_params
            .iter()
            .zip(&sig.type_param_variances)
            .map(|(name, v)| {
                (
                    *v,
                    Type::TypeVariable(TypeParamName(format!("$coverage${leaf}${name}")), vec![]),
                )
            })
            .collect();
        let ty = if params.is_empty() {
            Type::Class(leaf.clone(), MangledName::for_type(leaf))
        } else {
            Type::GenericClass {
                fqn: leaf.clone(),
                mangled_name: MangledName::for_type(leaf),
                type_args: params.clone(),
            }
        };
        let mut constraints = reduce(registry, &ty, subject);
        let substitution =
            crate::typechecker::infer::type_param_substitution::TypeParamSubstitution::from_pairs(
                &sig.type_params,
                &params.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>(),
            );
        for (name, (_, param)) in sig.type_params.iter().zip(&params) {
            for bound in sig
                .trait_bounds
                .get(name)
                .into_iter()
                .flatten()
                .filter_map(|b| b.named())
            {
                if bound.kind != crate::typechecker::types::BoundKind::SubtypeOf {
                    continue;
                }
                let parent = registry.get_class_type(&bound.trait_fqn)?;
                let expected = if parent.type_params.is_empty() {
                    Type::Class(
                        bound.trait_fqn.clone(),
                        MangledName::for_type(&bound.trait_fqn),
                    )
                } else {
                    Type::GenericClass {
                        fqn: bound.trait_fqn.clone(),
                        mangled_name: MangledName::for_type(&bound.trait_fqn),
                        type_args: parent
                            .type_param_variances
                            .iter()
                            .cloned()
                            .zip(bound.type_args.iter().map(|t| {
                                crate::typechecker::infer::generics::apply_substitution(
                                    &substitution,
                                    t,
                                )
                            }))
                            .collect(),
                    }
                };
                constraints.extend(reduce(registry, param, &expected));
            }
        }
        let region = Self { ty, constraints };
        (region.feasibility(registry) != Proof::Disproven).then_some(region)
    }

    fn entails(&self, registry: &Registry, actual: &Type, expected: &Type) -> bool {
        let mut reachable = vec![actual.clone()];
        let mut index = 0;
        while index < reachable.len() {
            let current = reachable[index].clone();
            index += 1;
            if subtyping::is_subtype(registry, &current, expected) {
                return true;
            }
            for edge in self.constraints.iter().filter(|c| c.positive) {
                if subtyping::is_subtype(registry, &current, &edge.actual)
                    && !reachable
                        .iter()
                        .any(|t| subtyping::identical(t, &edge.expected))
                {
                    reachable.push(edge.expected.clone());
                }
            }
        }
        false
    }

    fn feasibility(&self, registry: &Registry) -> Proof {
        let basic = self.basic_feasibility(registry);
        if basic != Proof::Unknown {
            return basic;
        }
        // A non-generic sealed upper bound has a finite type domain, including
        // abstract classes themselves and Never (both are valid type arguments).
        // Generic or open descendants keep the domain symbolic.
        for bound in self.constraints.iter().filter(|c| c.positive) {
            let Type::TypeVariable(name, _) = &bound.actual else {
                continue;
            };
            let Type::Class(root, _) = &bound.expected else {
                continue;
            };
            if !registry.get_class_type(root).is_some_and(|c| c.is_sealed) {
                continue;
            }
            let mut candidates = vec![Type::Never];
            let mut finite = true;
            for (fqn, sig) in registry.all_class_types() {
                let ty = Type::Class(fqn.clone(), MangledName::for_type(fqn));
                if subtyping::project_class(registry, &ty, root).is_none() {
                    continue;
                }
                if !sig.type_params.is_empty() || (!sig.is_sealed && !sig.is_final) {
                    finite = false;
                    break;
                }
                candidates.push(ty);
            }
            if !finite {
                continue;
            }
            let possible = candidates.into_iter().any(|candidate| {
                let substitution = crate::typechecker::infer::type_param_substitution::TypeParamSubstitution::from_pairs(
                    std::slice::from_ref(name), &[candidate],
                );
                let mut constraints = Vec::new();
                for c in &self.constraints {
                    let actual = crate::typechecker::infer::generics::apply_substitution(&substitution, &c.actual);
                    let expected = crate::typechecker::infer::generics::apply_substitution(&substitution, &c.expected);
                    if c.positive {
                        constraints.extend(reduce(registry, &actual, &expected));
                    } else {
                        constraints.push(Constraint { actual, expected, positive: false });
                    }
                }
                Self { ty: self.ty.clone(), constraints }.basic_feasibility(registry) != Proof::Disproven
            });
            if !possible {
                return Proof::Disproven;
            }
        }
        Proof::Unknown
    }

    fn basic_feasibility(&self, registry: &Registry) -> Proof {
        for c in &self.constraints {
            if !c.positive && self.entails(registry, &c.actual, &c.expected) {
                return Proof::Disproven;
            }
            if c.positive
                && !c.actual.contains_type_parameter()
                && !c.expected.contains_type_parameter()
                && !subtyping::is_subtype(registry, &c.actual, &c.expected)
            {
                return Proof::Disproven;
            }
        }
        // A concrete lower bound cannot flow through variables into an incompatible upper bound.
        for lower in self
            .constraints
            .iter()
            .filter(|c| c.positive && !c.actual.contains_type_parameter())
        {
            for upper in self
                .constraints
                .iter()
                .filter(|c| c.positive && !c.expected.contains_type_parameter())
            {
                if self.entails(registry, &lower.expected, &upper.actual)
                    && !subtyping::is_subtype(registry, &lower.actual, &upper.expected)
                {
                    return Proof::Disproven;
                }
            }
        }
        if self.constraints.iter().all(|c| {
            !variable(&c.actual)
                && !variable(&c.expected)
                && !c.actual.contains_type_parameter()
                && !c.expected.contains_type_parameter()
        }) {
            Proof::Proven
        } else {
            Proof::Unknown
        }
    }

    pub fn covered_by(&self, registry: &Registry, target: &Type) -> bool {
        reduce(registry, &self.ty, target)
            .iter()
            .all(|c| self.entails(registry, &c.actual, &c.expected))
    }

    /// Split on each conjunct, retaining both possible outcomes. Unknown regions
    /// remain in the matrix and therefore cannot produce a false coverage proof.
    pub fn split(self, registry: &Registry, target: &Type) -> Vec<Self> {
        let tests = reduce(registry, &self.ty, target);
        let mut matching = self;
        let mut out = vec![];
        for test in tests {
            let mut missing = matching.clone();
            missing.constraints.push(Constraint {
                positive: false,
                ..test.clone()
            });
            if missing.feasibility(registry) != Proof::Disproven {
                out.push(missing);
            }
            matching.constraints.push(test);
            if matching.feasibility(registry) == Proof::Disproven {
                return out;
            }
        }
        out.push(matching);
        out
    }

    pub fn describe(&self) -> String {
        let mut names = Vec::new();
        let mut values = Vec::new();
        for bound in self.constraints.iter().filter(|c| c.positive) {
            let Type::TypeVariable(name, _) = &bound.actual else {
                continue;
            };
            if !bound.expected.contains_type_parameter()
                && self.constraints.iter().any(|other| {
                    other.positive
                        && subtyping::identical(&other.actual, &bound.expected)
                        && subtyping::identical(&other.expected, &bound.actual)
                })
            {
                names.push(name.clone());
                values.push(bound.expected.clone());
            }
        }
        let substitution =
            crate::typechecker::infer::type_param_substitution::TypeParamSubstitution::from_pairs(
                &names, &values,
            );
        let concrete =
            crate::typechecker::infer::generics::apply_substitution(&substitution, &self.ty);
        if !concrete.contains_type_parameter() {
            return concrete.to_string();
        }
        let render = |ty: &Type| {
            let mut text = ty.to_string();
            if let Type::GenericClass { type_args, .. } = &self.ty {
                for (_, param) in type_args {
                    if let Type::TypeVariable(name, _) = param {
                        text = text.replace(&name.0, name.0.rsplit('$').next().unwrap_or(&name.0));
                    }
                }
            }
            text
        };
        let constraints: Vec<_> = self
            .constraints
            .iter()
            .map(|c| {
                format!(
                    "{} {} {}",
                    render(&c.actual),
                    if c.positive { "<:" } else { "!<:" },
                    render(&c.expected)
                )
            })
            .collect();
        if constraints.is_empty() {
            render(&self.ty)
        } else {
            format!("{} where {}", render(&self.ty), constraints.join(", "))
        }
    }
}
