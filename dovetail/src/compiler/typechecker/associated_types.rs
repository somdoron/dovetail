//! Symbolic associated types and normalization against the current package registry.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::infer::generics::apply_substitution;
use super::infer::type_param_substitution::TypeParamSubstitution;
use super::registry::Registry;
use super::types::{NamedTraitBound, Type};
use crate::common::types::Fqn;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AssociatedProjection {
    pub receiver: Type,
    pub trait_fqn: Fqn,
    pub trait_parameters: Vec<Type>,
    pub member: String,
    pub parameters: Vec<Type>,
}

impl AssociatedProjection {
    pub fn types(&self) -> impl Iterator<Item = &Type> {
        std::iter::once(&self.receiver)
            .chain(&self.trait_parameters)
            .chain(&self.parameters)
    }

    pub fn map(&self, mut transform: impl FnMut(&Type) -> Type) -> Self {
        Self {
            receiver: transform(&self.receiver),
            trait_fqn: self.trait_fqn.clone(),
            trait_parameters: self.trait_parameters.iter().map(&mut transform).collect(),
            member: self.member.clone(),
            parameters: self.parameters.iter().map(transform).collect(),
        }
    }

    pub fn into_type(self) -> Type {
        if !self.receiver.contains_type_parameter() && !matches!(self.receiver, Type::SelfType) {
            CONCRETE_PROJECTIONS.with(|encountered| encountered.set(true));
        }
        let registry = ENVIRONMENT.with(|environment| environment.borrow().clone());
        let Some(registry) = registry else {
            return Type::AssociatedProjection(Box::new(self));
        };
        if self.parameters.is_empty() {
            if let Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) = &self.receiver
            {
                for bound in bounds.iter().filter_map(|bound| bound.named()) {
                    let arguments = if bound.trait_fqn == self.trait_fqn {
                        Some(bound.type_args.clone())
                    } else {
                        registry.super_closure_args(
                            &bound.trait_fqn,
                            &bound.type_args,
                            &self.trait_fqn,
                        )
                    };
                    if arguments.is_some_and(|arguments| {
                        arguments.len() == self.trait_parameters.len()
                            && arguments
                                .iter()
                                .zip(&self.trait_parameters)
                                .all(|(a, b)| super::subtyping::identical(a, b))
                    }) {
                        if let Some(equality) = bound.associated_types.get(&self.member) {
                            return equality.clone();
                        }
                    }
                }
            }
        }
        if self.receiver.contains_type_parameter() || matches!(self.receiver, Type::SelfType) {
            return Type::AssociatedProjection(Box::new(self));
        }
        let recursive =
            ACTIVE.with(|active| active.borrow().contains(&self) || active.borrow().len() >= 64);
        if recursive {
            CYCLES.with(|cycles| cycles.set(cycles.get().wrapping_add(1)));
            ERRORS.with(|errors| {
                errors.borrow_mut().push(format!(
                    "cyclic associated type '{}.{}'",
                    self.receiver, self.member
                ))
            });
            return self.unresolved_type();
        }
        let previous_cycles = CYCLES.with(Cell::get);
        ACTIVE.with(|active| active.borrow_mut().push(self.clone()));
        let result = self.normalize(&registry).unwrap_or_else(|| {
            ERRORS.with(|errors| {
                errors.borrow_mut().push(format!(
                    "cannot resolve associated type '{}.{}'",
                    self.receiver, self.member
                ))
            });
            self.unresolved_type()
        });
        ACTIVE.with(|active| {
            active.borrow_mut().pop();
        });
        // A provisional cycle must preserve the whole reference, not a partly
        // expanded Array<projection> that would grow on every collection round.
        if DEFER_FAILURES.with(Cell::get) && CYCLES.with(Cell::get) != previous_cycles {
            return self.unresolved_type();
        }
        result
    }

    fn unresolved_type(&self) -> Type {
        if DEFER_FAILURES.with(Cell::get) {
            Type::AssociatedProjection(Box::new(self.clone()))
        } else {
            Type::Error
        }
    }

    fn normalize(&self, registry: &Registry) -> Option<Type> {
        let receiver_fqn = self.receiver.try_to_fqn()?;
        let mut direct = Vec::new();
        let mut inherited = Vec::new();
        for (block, route) in registry.find_providing_impl_blocks(&self.trait_fqn, &receiver_fqn) {
            let mut substitution = TypeParamSubstitution::new();
            if !substitution.unify(&block.for_type, &self.receiver) {
                continue;
            }
            let application = route
                .as_ref()
                .map(|(_, parameters)| parameters)
                .unwrap_or(&block.trait_type_args);
            if application.len() != self.trait_parameters.len()
                || !application
                    .iter()
                    .zip(&self.trait_parameters)
                    .all(|(expected, actual)| substitution.unify(expected, actual))
            {
                continue;
            }
            let Some(mut substitution) =
                super::infer::complete_impl_substitution(registry, block, substitution)
            else {
                continue;
            };
            let Some((parameters, definition)) = block.associated_type_defs.get(&self.member)
            else {
                continue;
            };
            if parameters.len() != self.parameters.len() {
                continue;
            }
            // Associated-definition parameters shadow block parameters. Apply a
            // single simultaneous substitution so replacement types cannot be captured.
            for (parameter, argument) in parameters.iter().zip(&self.parameters) {
                substitution.insert(parameter.clone(), argument.clone());
            }
            let result = apply_substitution(&substitution, definition);
            if route.is_none() {
                direct.push(result);
            } else {
                inherited.push(result);
            }
        }
        let candidates = if direct.is_empty() { inherited } else { direct };
        if candidates.len() == 1 {
            candidates.into_iter().next()
        } else {
            None
        }
    }
}

/// Resolve one associated member against the evidence on a generic receiver.
pub(crate) fn from_bound(
    receiver: &Type,
    bound: &NamedTraitBound,
    member: &str,
    parameters: Vec<Type>,
    registry: &Registry,
) -> Option<Type> {
    let signature = registry.get_trait(&bound.trait_fqn)?;
    let associated = signature
        .associated_types
        .iter()
        .find(|associated| associated.name == member)?;
    if associated.type_params.len() != parameters.len() {
        return None;
    }
    if parameters.is_empty() {
        if let Some(equality) = bound.associated_types.get(member) {
            return Some(equality.clone());
        }
    }
    Some(projection_from_bound(receiver, bound, member, parameters, registry)?.into_type())
}

fn projection_from_bound(
    receiver: &Type,
    bound: &NamedTraitBound,
    member: &str,
    parameters: Vec<Type>,
    registry: &Registry,
) -> Option<AssociatedProjection> {
    let signature = registry.get_trait(&bound.trait_fqn)?;
    let associated = signature
        .associated_types
        .iter()
        .find(|associated| associated.name == member)?;
    if associated.type_params.len() != parameters.len() {
        return None;
    }
    let (trait_fqn, trait_parameters) = if let Some((origin, arguments)) = &associated.origin {
        let substitution =
            TypeParamSubstitution::from_pairs(&signature.type_params, &bound.type_args);
        (
            origin.clone(),
            arguments
                .iter()
                .map(|ty| apply_substitution(&substitution, ty))
                .collect(),
        )
    } else {
        (bound.trait_fqn.clone(), bound.type_args.clone())
    };
    Some(AssociatedProjection {
        receiver: receiver.clone(),
        trait_fqn,
        trait_parameters,
        member: member.to_string(),
        parameters,
    })
}

/// Resolve a source-level projection before normalization. Two distinct
/// declarations remain ambiguous even if both happen to normalize to Int32.
pub(crate) fn resolve_reference(
    receiver: &Type,
    member: &str,
    parameters: Vec<Type>,
    registries: &[&Registry],
) -> Result<Type, String> {
    let bounds = match receiver {
        Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) => bounds.as_slice(),
        _ => &[],
    };
    let mut candidates: Vec<(AssociatedProjection, Type)> = Vec::new();
    let mut arities = std::collections::BTreeSet::new();
    for bound in bounds.iter().filter_map(|bound| bound.named()) {
        let Some(registry) = registries
            .iter()
            .find(|registry| registry.get_trait(&bound.trait_fqn).is_some())
        else {
            continue;
        };
        let signature = registry.get_trait(&bound.trait_fqn).unwrap();
        let Some(associated) = signature
            .associated_types
            .iter()
            .find(|associated| associated.name == member)
        else {
            continue;
        };
        arities.insert(associated.type_params.len());
        let Some(projection) =
            projection_from_bound(receiver, bound, member, parameters.clone(), registry)
        else {
            continue;
        };
        if candidates
            .iter()
            .any(|(existing, _)| *existing == projection)
        {
            continue;
        }
        let ty = from_bound(receiver, bound, member, parameters.clone(), registry).unwrap();
        candidates.push((projection, ty));
    }
    if candidates.len() == 1 {
        return Ok(candidates.pop().unwrap().1);
    }
    let name = format!("{receiver}.{member}");
    if candidates.len() > 1 {
        return Err(format!(
            "associated type '{name}' is ambiguous between trait bounds"
        ));
    }
    if arities.is_empty() {
        return Err(format!(
            "no bound on '{receiver}' declares associated type '{member}'"
        ));
    }
    Err(format!(
        "associated type '{name}' expects a type parameter count in {arities:?}, found {}",
        parameters.len()
    ))
}

thread_local! {
    static CYCLES: Cell<usize> = const { Cell::new(0) };
    static DEFER_FAILURES: Cell<bool> = const { Cell::new(false) };
    static CONCRETE_PROJECTIONS: Cell<bool> = const { Cell::new(false) };
    static ENVIRONMENT: RefCell<Option<Rc<Registry>>> = const { RefCell::new(None) };
    static ACTIVE: RefCell<Vec<AssociatedProjection>> = const { RefCell::new(Vec::new()) };
    static ERRORS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn with_registry<R>(operation: impl FnOnce(&Registry) -> R) -> Option<R> {
    let registry = ENVIRONMENT.with(|environment| environment.borrow().clone());
    registry.as_deref().map(operation)
}

/// Substitution is shared by inference and specialization. A scoped environment
/// lets both normalize projections without keeping registry references in types.
/// Restoring all state on drop also isolates nested compilations and LSP recovery.
pub(crate) struct NormalizationScope {
    previous_cycles: usize,
    previous_defer_failures: bool,
    previous_concrete_projections: bool,
    previous: Option<Rc<Registry>>,
    active: Vec<AssociatedProjection>,
    errors: Vec<String>,
}

impl NormalizationScope {
    pub fn install(registry: &Registry) -> Self {
        Self::with_environment(Some(Rc::new(registry.clone())), false)
    }

    /// Keep unresolved definitions intact while newly discovered implementation
    /// targets are added. A final strict pass reports any remaining failures.
    pub fn provisional(registry: &Registry) -> Self {
        Self::with_environment(Some(Rc::new(registry.clone())), true)
    }

    /// Observe concrete projections during provisional collection without
    /// trying to resolve them against an incomplete implementation registry.
    pub fn collecting() -> Self {
        Self::with_environment(None, true)
    }

    fn with_environment(registry: Option<Rc<Registry>>, defer_failures: bool) -> Self {
        Self {
            previous_cycles: CYCLES.with(|cycles| cycles.replace(0)),
            previous_defer_failures: DEFER_FAILURES.with(|defer| defer.replace(defer_failures)),
            previous_concrete_projections: CONCRETE_PROJECTIONS
                .with(|encountered| encountered.replace(false)),
            previous: ENVIRONMENT.with(|environment| environment.replace(registry)),
            active: ACTIVE.with(|active| active.replace(Vec::new())),
            errors: ERRORS.with(|errors| errors.replace(Vec::new())),
        }
    }

    pub fn encountered_concrete_projections(&self) -> bool {
        CONCRETE_PROJECTIONS.with(Cell::get)
    }

    pub fn errors(&self) -> Vec<String> {
        ERRORS.with(|errors| errors.replace(Vec::new()))
    }
}

impl Drop for NormalizationScope {
    fn drop(&mut self) {
        CYCLES.with(|cycles| cycles.set(self.previous_cycles));
        DEFER_FAILURES.with(|defer| defer.set(self.previous_defer_failures));
        CONCRETE_PROJECTIONS
            .with(|encountered| encountered.set(self.previous_concrete_projections));
        ENVIRONMENT.with(|environment| environment.replace(self.previous.take()));
        ACTIVE.with(|active| active.replace(std::mem::take(&mut self.active)));
        ERRORS.with(|errors| errors.replace(std::mem::take(&mut self.errors)));
    }
}
