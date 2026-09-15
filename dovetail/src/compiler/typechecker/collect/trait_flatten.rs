//! Pass 1b: flatten trait `extends` hierarchies.
//!
//! Each local trait's `TraitSignature` is rewritten so its member vectors hold
//! the *flattened* set — its own members plus every super's members
//! (transitively), substituted into this trait's type params and tagged with
//! their `origin` (the ultimately-declaring trait). Downstream code — impl
//! completeness, membership checks, class `implements`, bound member lookup —
//! then works on inherited members unchanged. Dependency traits were already
//! flattened when their package was collected.
//!
//! Also enforced here: cycle detection, the interface-extends-only-interfaces
//! rule (interface-objects-design §6), and the appendix §1.2 same-name rules.

use std::collections::{BTreeMap, BTreeSet};

use crate::common::types::Fqn;
use crate::typechecker::registry::{
    TraitSignature, TraitSuperRef, canonical_method_signature, instantiate_trait_method,
    same_method_parameters,
};
use crate::typechecker::types::Type;

use super::Collector;
use super::implements::substitute_trait_type_params;

/// The outcome of comparing a trait's own member against an inherited member
/// with the same name — the appendix §1.2 rules.
enum MergeOutcome {
    /// Same signature, no body: redeclaring an inherited member is redundant — error.
    Redundant,
    /// Same signature WITH a body: a default-implementation override — the
    /// member keeps the inherited origin (same vtable slot) but this trait's
    /// default body wins for implementors that omit it.
    Override,
    /// Same parameters, different return type — error.
    ReturnConflict,
    /// Different parameters: both members exist.
    Distinct,
    /// One member takes `self` and the other does not: a static and an
    /// instance member with the same name are different kinds of member, and
    /// merging them would give one the other's vtable slot.
    SelfMismatch,
}

fn param_types(params: &[(String, Type)]) -> Vec<&Type> {
    params
        .iter()
        .filter(|(n, _)| n != "self")
        .map(|(_, t)| t)
        .collect()
}

fn takes_self(params: &[(String, Type)]) -> bool {
    params.first().is_some_and(|(n, _)| n == "self")
}

fn merge_own_member(
    own_params: &[(String, Type)],
    own_return: &Type,
    own_has_default: bool,
    inherited_params: &[(String, Type)],
    inherited_return: &Type,
) -> MergeOutcome {
    // `param_types` strips `self`, so self-ness must be compared separately —
    // otherwise `foo()` and `foo(self)` look like the same member.
    if takes_self(own_params) != takes_self(inherited_params) {
        return MergeOutcome::SelfMismatch;
    }
    if param_types(own_params) != param_types(inherited_params) {
        return MergeOutcome::Distinct;
    }
    if own_return == inherited_return {
        if own_has_default {
            MergeOutcome::Override
        } else {
            MergeOutcome::Redundant
        }
    } else {
        MergeOutcome::ReturnConflict
    }
}

impl Collector<'_> {
    /// Flatten every local trait. DFS so a local super is flattened before
    /// its extenders; dependency supers are already flat.
    pub(super) fn flatten_traits(&mut self) {
        let local: Vec<Fqn> = self
            .package_registry
            .traits()
            .map(|t| t.fqn.clone())
            .collect();
        let mut done: BTreeSet<Fqn> = BTreeSet::new();
        let mut visiting: Vec<Fqn> = Vec::new();
        for fqn in local {
            self.flatten_one(&fqn, &mut visiting, &mut done);
        }
    }

    fn flatten_one(&mut self, fqn: &Fqn, visiting: &mut Vec<Fqn>, done: &mut BTreeSet<Fqn>) {
        if done.contains(fqn) {
            return;
        }
        if visiting.contains(fqn) {
            // Cycle: report it at the trait that closes the loop and treat it
            // as having no supers so collection continues (errors accumulate).
            let path: Vec<String> = visiting
                .iter()
                .skip_while(|f| *f != fqn)
                .map(|f| format!("'{}'", f.symbol))
                .chain(std::iter::once(format!("'{}'", fqn.symbol)))
                .collect();
            let span = self.package_registry.get_trait(fqn).map(|t| t.span.clone());
            if let Some(span) = span {
                self.diagnostics.error(
                    span,
                    format!("'extends' cycle detected: {}", path.join(" -> ")),
                );
            }
            done.insert(fqn.clone());
            // Drop the supers so the cycle doesn't recurse forever.
            if let Some(mut sig) = self.package_registry.get_trait(fqn).cloned() {
                sig.supers.clear();
                self.package_registry.replace_trait(fqn.clone(), sig);
            }
            return;
        }

        let Some(sig) = self.package_registry.get_trait(fqn).cloned() else {
            done.insert(fqn.clone());
            return;
        };
        if sig.supers.is_empty() {
            done.insert(fqn.clone());
            return;
        }

        visiting.push(fqn.clone());
        // Flatten local supers first (dependency supers are already flat).
        let super_refs: Vec<TraitSuperRef> = sig.supers.clone();
        for super_ref in &super_refs {
            if super_ref.fqn.package == self.package_path {
                self.flatten_one(&super_ref.fqn, visiting, done);
            }
        }
        visiting.pop();

        // Re-fetch: a cycle involving this trait may have cleared its supers.
        let Some(mut sig) = self.package_registry.get_trait(fqn).cloned() else {
            done.insert(fqn.clone());
            return;
        };
        if sig.supers.is_empty() {
            done.insert(fqn.clone());
            return;
        }

        let mut closure: Vec<(Fqn, Vec<Type>)> = Vec::new();
        let mut closure_conflicts: BTreeSet<Fqn> = BTreeSet::new();
        let mut inherited_methods: Vec<crate::typechecker::registry::TraitMethodSig> = Vec::new();
        let mut inherited_properties: Vec<crate::typechecker::registry::TraitPropertySig> =
            Vec::new();
        let mut inherited_assocs: Vec<crate::typechecker::registry::AssociatedTypeSig> = Vec::new();

        for super_ref in &sig.supers {
            let Some(super_sig) = self
                .package_registry
                .lookup_trait(&super_ref.fqn, &self.package_path)
                .or_else(|| {
                    self.dependency_registry
                        .lookup_trait(&super_ref.fqn, &self.package_path)
                })
                .cloned()
            else {
                continue;
            };

            // Interface refinement: an interface may extend only interfaces.
            if sig.is_interface && !super_sig.is_interface {
                self.diagnostics.error(
                    super_ref.span.clone(),
                    format!(
                        "an interface may extend only interfaces; '{}' is a trait",
                        super_ref.fqn.symbol,
                    ),
                );
                continue;
            }

            // Substitution: the super's type params ↦ this ref's type args
            // (already in terms of the extender's params). `Self` stays `Self`.
            let sub: BTreeMap<crate::common::types::TypeParamName, Type> = super_sig
                .type_params
                .iter()
                .cloned()
                .zip(super_ref.type_args.iter().cloned())
                .collect();

            // Closure: the direct super plus its (already substituted) closure.
            let mut add_closure_entry =
                |entry_fqn: &Fqn,
                 entry_args: Vec<Type>,
                 diagnostics: &mut crate::common::diagnostics::Diagnostics| {
                    if let Some((_, existing_args)) = closure.iter().find(|(f, _)| f == entry_fqn) {
                        if *existing_args != entry_args
                            && closure_conflicts.insert(entry_fqn.clone())
                        {
                            diagnostics.error(
                            super_ref.span.clone(),
                            format!(
                                "trait '{}' is inherited more than once with conflicting type arguments",
                                entry_fqn.symbol,
                            ),
                        );
                        }
                        return;
                    }
                    closure.push((entry_fqn.clone(), entry_args));
                };
            add_closure_entry(
                &super_ref.fqn,
                super_ref.type_args.clone(),
                self.diagnostics,
            );
            for (closure_fqn, closure_args) in &super_sig.super_closure {
                let substituted: Vec<Type> = closure_args
                    .iter()
                    .map(|t| substitute_trait_type_params(t, &sub))
                    .collect();
                add_closure_entry(closure_fqn, substituted, self.diagnostics);
            }

            // Inherit the super's flattened members, substituted, with origins.
            for m in &super_sig.methods {
                let (origin_fqn, origin_args) = match &m.origin {
                    Some((f, args)) => (
                        f.clone(),
                        args.iter()
                            .map(|t| substitute_trait_type_params(t, &sub))
                            .collect(),
                    ),
                    None => (super_ref.fqn.clone(), super_ref.type_args.clone()),
                };
                let mut inherited = instantiate_trait_method(m, &sub);
                inherited.span = super_ref.span.clone();
                inherited.origin = Some((origin_fqn, origin_args));
                inherited_methods.push(inherited);
            }
            for p in &super_sig.properties {
                let (origin_fqn, origin_args) = match &p.origin {
                    Some((f, args)) => (
                        f.clone(),
                        args.iter()
                            .map(|t| substitute_trait_type_params(t, &sub))
                            .collect(),
                    ),
                    None => (super_ref.fqn.clone(), super_ref.type_args.clone()),
                };
                inherited_properties.push(crate::typechecker::registry::TraitPropertySig {
                    name: p.name.clone(),
                    params: p
                        .params
                        .iter()
                        .map(|(n, t)| (n.clone(), substitute_trait_type_params(t, &sub)))
                        .collect(),
                    return_type: substitute_trait_type_params(&p.return_type, &sub),
                    span: super_ref.span.clone(),
                    origin: Some((origin_fqn, origin_args)),
                    default_source: p.default_source.clone(),
                });
            }
            for a in &super_sig.associated_types {
                let (origin_fqn, origin_args) = match &a.origin {
                    Some((f, args)) => (
                        f.clone(),
                        args.iter()
                            .map(|t| substitute_trait_type_params(t, &sub))
                            .collect(),
                    ),
                    None => (super_ref.fqn.clone(), super_ref.type_args.clone()),
                };
                inherited_assocs.push(crate::typechecker::registry::AssociatedTypeSig {
                    name: a.name.clone(),
                    span: super_ref.span.clone(),
                    type_params: a.type_params.clone(),
                    origin: Some((origin_fqn, origin_args)),
                });
            }
        }

        // Merge inherited members against own declarations (§1.2) and against
        // each other (diamond dedupe / cross-super conflicts).
        self.merge_inherited(
            &mut sig,
            inherited_methods,
            inherited_properties,
            inherited_assocs,
        );
        sig.super_closure = closure;
        self.package_registry.replace_trait(fqn.clone(), sig);
        done.insert(fqn.clone());
    }

    fn merge_inherited(
        &mut self,
        sig: &mut TraitSignature,
        inherited_methods: Vec<crate::typechecker::registry::TraitMethodSig>,
        inherited_properties: Vec<crate::typechecker::registry::TraitPropertySig>,
        inherited_assocs: Vec<crate::typechecker::registry::AssociatedTypeSig>,
    ) {
        // Associated types first: same-name conflicts are errors regardless of shape.
        for inherited in inherited_assocs {
            if let Some(own) = sig
                .associated_types
                .iter()
                .find(|a| a.origin.is_none() && a.name == inherited.name)
            {
                let origin = inherited
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                self.diagnostics.error(
                    own.span.clone(),
                    format!(
                        "associated type '{}' is already inherited from trait '{}'",
                        inherited.name, origin,
                    ),
                );
                continue;
            }
            if let Some(existing) = sig
                .associated_types
                .iter()
                .find(|a| a.origin.is_some() && a.name == inherited.name)
            {
                if existing.origin == inherited.origin {
                    continue; // diamond — one copy suffices
                }
                let a = existing
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                let b = inherited
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                self.diagnostics.error(
                    inherited.span.clone(),
                    format!(
                        "inherited associated types '{}' from traits '{}' and '{}' conflict",
                        inherited.name, a, b,
                    ),
                );
                continue;
            }
            sig.associated_types.push(inherited);
        }

        for inherited in inherited_methods {
            // vs own declarations
            if let Some(own_pos) = sig
                .methods
                .iter()
                .position(|m| m.origin.is_none() && m.name == inherited.name)
            {
                let own = &sig.methods[own_pos];
                let origin_name = inherited
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                let (own_params, own_result) = canonical_method_signature(own);
                let (inherited_params, inherited_result) = canonical_method_signature(&inherited);
                let outcome = if own.type_params.len() != inherited.type_params.len()
                    && takes_self(&own.params) == takes_self(&inherited.params)
                {
                    MergeOutcome::Distinct
                } else {
                    merge_own_member(
                        &own_params,
                        &own_result,
                        own.default_source.is_some(),
                        &inherited_params,
                        &inherited_result,
                    )
                };
                match outcome {
                    MergeOutcome::Redundant => {
                        self.diagnostics.error(
                            own.span.clone(),
                            format!(
                                "method '{}' with the same signature is already inherited from trait '{}'",
                                inherited.name, origin_name,
                            ),
                        );
                        continue;
                    }
                    MergeOutcome::Override => {
                        // The redeclaration provides a default override: the
                        // member keeps the inherited origin (same vtable slot)
                        // and this trait's default body wins for implementors
                        // that omit the member. Drop the inherited copy.
                        sig.methods[own_pos].origin = inherited.origin.clone();
                        continue;
                    }
                    MergeOutcome::ReturnConflict => {
                        self.diagnostics.error(
                            own.span.clone(),
                            format!(
                                "method '{}' conflicts with member inherited from trait '{}': same parameters but different return type",
                                inherited.name, origin_name,
                            ),
                        );
                        continue;
                    }
                    MergeOutcome::Distinct => {}
                    MergeOutcome::SelfMismatch => {
                        self.diagnostics.error(
                            own.span.clone(),
                            format!(
                                "member '{}' conflicts with the one inherited from trait '{}': one takes 'self' and the other does not",
                                inherited.name, origin_name,
                            ),
                        );
                        continue;
                    }
                }
            }
            // vs own properties (members share one namespace)
            if sig
                .properties
                .iter()
                .any(|p| p.origin.is_none() && p.name == inherited.name)
            {
                let origin_name = inherited
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                self.diagnostics.error(
                    sig.span.clone(),
                    format!(
                        "property '{}' conflicts with method inherited from trait '{}'",
                        inherited.name, origin_name,
                    ),
                );
                continue;
            }
            // vs inherited PROPERTIES (members share one namespace)
            if let Some(existing) = sig
                .properties
                .iter()
                .find(|p| p.origin.is_some() && p.name == inherited.name)
            {
                let a = existing
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                let b = inherited
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                self.diagnostics.error(
                    inherited.span.clone(),
                    format!(
                        "inherited method '{}' from trait '{}' conflicts with property of the same name inherited from trait '{}'",
                        inherited.name, b, a,
                    ),
                );
                continue;
            }
            // vs already-inherited members
            if let Some(existing_pos) = sig
                .methods
                .iter()
                .position(|m| {
                    m.origin.is_some() && m.name == inherited.name && m.origin == inherited.origin
                })
                .or_else(|| {
                    sig.methods.iter().position(|m| {
                        m.origin.is_some()
                            && m.name == inherited.name
                            && same_method_parameters(m, &inherited)
                    })
                })
                .or_else(|| {
                    sig.methods.iter().position(|m| {
                        m.origin.is_some()
                            && m.name == inherited.name
                            && takes_self(&m.params) != takes_self(&inherited.params)
                    })
                })
            {
                let existing = &sig.methods[existing_pos];
                if existing.origin == inherited.origin {
                    // Diamond: one copy suffices — but the default may differ
                    // per path (an intermediate override). The overriding
                    // path's default wins ("nearest"); two DIFFERENT overrides
                    // through separate paths conflict — unless THIS trait's
                    // own override already absorbed the member, which
                    // disambiguates every path.
                    if existing.default_source.as_ref() == Some(&sig.fqn) {
                        continue;
                    }
                    if existing.default_source != inherited.default_source {
                        let origin_fqn = inherited.origin.as_ref().map(|(f, _)| f.clone());
                        let existing_overrides = existing.default_source.is_some()
                            && existing.default_source != origin_fqn;
                        let inherited_overrides = inherited.default_source.is_some()
                            && inherited.default_source != origin_fqn;
                        match (existing_overrides, inherited_overrides) {
                            (true, false) => {}
                            (false, true) => {
                                sig.methods[existing_pos].default_source =
                                    inherited.default_source.clone();
                            }
                            (true, true) => {
                                let a = existing
                                    .default_source
                                    .as_ref()
                                    .map(|f| f.symbol.0.clone())
                                    .unwrap_or_default();
                                let b = inherited
                                    .default_source
                                    .as_ref()
                                    .map(|f| f.symbol.0.clone())
                                    .unwrap_or_default();
                                self.diagnostics.error(
                                    inherited.span.clone(),
                                    format!(
                                        "conflicting default overrides for '{}': inherited via traits '{}' and '{}'; override '{}' here to disambiguate",
                                        inherited.name, a, b, inherited.name,
                                    ),
                                );
                            }
                            (false, false) => {
                                // One path carries the origin default, the
                                // other none — keep the defaulted one.
                                if existing.default_source.is_none() {
                                    sig.methods[existing_pos].default_source =
                                        inherited.default_source.clone();
                                }
                            }
                        }
                    }
                    continue;
                }
                // Distinct origins, identical signature: legal (one inline
                // implementation feeds both slots) — unless the two carry
                // DIFFERENT default bodies, which the name-keyed default
                // machinery cannot distinguish. A same-signature override in
                // THIS trait disambiguates (absorbed here).
                if takes_self(&existing.params) == takes_self(&inherited.params)
                    && same_method_parameters(existing, &inherited)
                    && canonical_method_signature(existing).1
                        == canonical_method_signature(&inherited).1
                {
                    if existing.default_source.as_ref() == Some(&sig.fqn) {
                        continue; // this trait's own override covers both
                    }
                    if existing.default_source != inherited.default_source {
                        let a = existing
                            .origin
                            .as_ref()
                            .map(|(f, _)| f.symbol.0.clone())
                            .unwrap_or_default();
                        let b = inherited
                            .origin
                            .as_ref()
                            .map(|(f, _)| f.symbol.0.clone())
                            .unwrap_or_default();
                        self.diagnostics.error(
                            inherited.span.clone(),
                            format!(
                                "members '{}' inherited from traits '{}' and '{}' have different default implementations; override '{}' here to disambiguate",
                                inherited.name, a, b, inherited.name,
                            ),
                        );
                        continue;
                    }
                }
                let existing = &sig.methods[existing_pos];
                let a = existing
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                let b = inherited
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                if takes_self(&existing.params) == takes_self(&inherited.params)
                    && same_method_parameters(existing, &inherited)
                    && canonical_method_signature(existing).1
                        != canonical_method_signature(&inherited).1
                {
                    self.diagnostics.error(
                        inherited.span.clone(),
                        format!(
                            "inherited members '{}' from traits '{}' and '{}' conflict: same parameters but different return types",
                            inherited.name, a, b,
                        ),
                    );
                    continue;
                }
                if takes_self(&existing.params) != takes_self(&inherited.params) {
                    // A static and an instance member of the same name are
                    // different kinds of member: merging them would give one
                    // the other's vtable slot, and pushing BOTH would leave
                    // one name with two members that every name-keyed lookup
                    // resolves first-wins. Reject, like the own-vs-inherited
                    // `SelfMismatch` case.
                    self.diagnostics.error(
                        inherited.span.clone(),
                        format!(
                            "inherited members '{}' from traits '{}' and '{}' conflict: one takes 'self' and the other does not",
                            inherited.name, a, b,
                        ),
                    );
                    continue;
                }
            }
            sig.methods.push(inherited);
        }

        for inherited in inherited_properties {
            if let Some(own_pos) = sig
                .properties
                .iter()
                .position(|p| p.origin.is_none() && p.name == inherited.name)
            {
                let own = &sig.properties[own_pos];
                let origin_name = inherited
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                match merge_own_member(
                    &own.params,
                    &own.return_type,
                    own.default_source.is_some(),
                    &inherited.params,
                    &inherited.return_type,
                ) {
                    MergeOutcome::Override => {
                        sig.properties[own_pos].origin = inherited.origin.clone();
                        continue;
                    }
                    MergeOutcome::Redundant => {
                        self.diagnostics.error(
                            own.span.clone(),
                            format!(
                                "property '{}' with the same signature is already inherited from trait '{}'",
                                inherited.name, origin_name,
                            ),
                        );
                        continue;
                    }
                    MergeOutcome::ReturnConflict => {
                        self.diagnostics.error(
                            own.span.clone(),
                            format!(
                                "property '{}' conflicts with member inherited from trait '{}': same parameters but different return type",
                                inherited.name, origin_name,
                            ),
                        );
                        continue;
                    }
                    MergeOutcome::SelfMismatch => {
                        self.diagnostics.error(
                            own.span.clone(),
                            format!(
                                "property '{}' conflicts with the one inherited from trait '{}': one takes 'self' and the other does not",
                                inherited.name, origin_name,
                            ),
                        );
                        continue;
                    }
                    MergeOutcome::Distinct => {}
                }
            }
            if sig
                .methods
                .iter()
                .any(|m| m.origin.is_none() && m.name == inherited.name)
            {
                let origin_name = inherited
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                self.diagnostics.error(
                    sig.span.clone(),
                    format!(
                        "method '{}' conflicts with property inherited from trait '{}'",
                        inherited.name, origin_name,
                    ),
                );
                continue;
            }
            // vs inherited METHODS (members share one namespace)
            if let Some(existing) = sig
                .methods
                .iter()
                .find(|m| m.origin.is_some() && m.name == inherited.name)
            {
                let a = existing
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                let b = inherited
                    .origin
                    .as_ref()
                    .map(|(f, _)| f.symbol.0.clone())
                    .unwrap_or_default();
                self.diagnostics.error(
                    inherited.span.clone(),
                    format!(
                        "inherited property '{}' from trait '{}' conflicts with method of the same name inherited from trait '{}'",
                        inherited.name, b, a,
                    ),
                );
                continue;
            }
            if let Some(existing_pos) = sig
                .properties
                .iter()
                .position(|p| p.origin.is_some() && p.name == inherited.name)
            {
                let existing = &sig.properties[existing_pos];
                if existing.origin == inherited.origin {
                    // Diamond — same rules as methods: overriding path wins,
                    // two distinct overrides conflict, and this trait's own
                    // override absorbs every path.
                    if existing.default_source.as_ref() == Some(&sig.fqn) {
                        continue;
                    }
                    if existing.default_source != inherited.default_source {
                        let origin_fqn = inherited.origin.as_ref().map(|(f, _)| f.clone());
                        let existing_overrides = existing.default_source.is_some()
                            && existing.default_source != origin_fqn;
                        let inherited_overrides = inherited.default_source.is_some()
                            && inherited.default_source != origin_fqn;
                        match (existing_overrides, inherited_overrides) {
                            (true, false) => {}
                            (false, true) => {
                                sig.properties[existing_pos].default_source =
                                    inherited.default_source.clone();
                            }
                            (true, true) => {
                                let a = existing
                                    .default_source
                                    .as_ref()
                                    .map(|f| f.symbol.0.clone())
                                    .unwrap_or_default();
                                let b = inherited
                                    .default_source
                                    .as_ref()
                                    .map(|f| f.symbol.0.clone())
                                    .unwrap_or_default();
                                self.diagnostics.error(
                                    inherited.span.clone(),
                                    format!(
                                        "conflicting default overrides for '{}': inherited via traits '{}' and '{}'; override '{}' here to disambiguate",
                                        inherited.name, a, b, inherited.name,
                                    ),
                                );
                            }
                            (false, false) => {
                                if existing.default_source.is_none() {
                                    sig.properties[existing_pos].default_source =
                                        inherited.default_source.clone();
                                }
                            }
                        }
                    }
                    continue;
                }
                // Self-ness is part of a property's identity too: a static
                // `property tag` and an instance `property tag(self)` are
                // different members, and pushing both would leave one name
                // with two entries that name-keyed lookups resolve
                // first-wins (and impl-completeness would accept only one).
                if takes_self(&existing.params) != takes_self(&inherited.params) {
                    let a = existing
                        .origin
                        .as_ref()
                        .map(|(f, _)| f.symbol.0.clone())
                        .unwrap_or_default();
                    let b = inherited
                        .origin
                        .as_ref()
                        .map(|(f, _)| f.symbol.0.clone())
                        .unwrap_or_default();
                    self.diagnostics.error(
                        inherited.span.clone(),
                        format!(
                            "inherited properties '{}' from traits '{}' and '{}' conflict: one takes 'self' and the other does not",
                            inherited.name, a, b,
                        ),
                    );
                    continue;
                }
                if existing.return_type == inherited.return_type {
                    if existing.default_source.as_ref() == Some(&sig.fqn) {
                        continue;
                    }
                    if existing.default_source != inherited.default_source {
                        let a = existing
                            .origin
                            .as_ref()
                            .map(|(f, _)| f.symbol.0.clone())
                            .unwrap_or_default();
                        let b = inherited
                            .origin
                            .as_ref()
                            .map(|(f, _)| f.symbol.0.clone())
                            .unwrap_or_default();
                        self.diagnostics.error(
                            inherited.span.clone(),
                            format!(
                                "members '{}' inherited from traits '{}' and '{}' have different default implementations; override '{}' here to disambiguate",
                                inherited.name, a, b, inherited.name,
                            ),
                        );
                        continue;
                    }
                }
                let existing = &sig.properties[existing_pos];
                if existing.return_type != inherited.return_type {
                    let a = existing
                        .origin
                        .as_ref()
                        .map(|(f, _)| f.symbol.0.clone())
                        .unwrap_or_default();
                    let b = inherited
                        .origin
                        .as_ref()
                        .map(|(f, _)| f.symbol.0.clone())
                        .unwrap_or_default();
                    self.diagnostics.error(
                        inherited.span.clone(),
                        format!(
                            "inherited properties '{}' from traits '{}' and '{}' conflict: different types",
                            inherited.name, a, b,
                        ),
                    );
                    continue;
                }
            }
            sig.properties.push(inherited);
        }
    }
}
