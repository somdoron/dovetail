use std::collections::BTreeMap;

use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName, TypeParamName, Variance};
use crate::parser::ast::TypeExpr;

use crate::typechecker::types::Type;

use super::Collector;

/// The kind of generic type being resolved during collection.
enum GenericTypeKind {
    Record,
    Enum,
}

impl Collector<'_> {
    /// Resolve a type expression to a concrete type using the registry.
    /// Gate for a trait name in type position: only an `interface` may be used
    /// as an object type; a plain trait is bound-only. Emits an error and
    /// returns false when the trait is not an interface.
    fn check_interface_in_type_position(&mut self, trait_fqn: &Fqn, span: &Span) -> bool {
        let is_interface = self
            .package_registry
            .lookup_trait(trait_fqn, &self.package_path)
            .or_else(|| {
                self.dependency_registry
                    .lookup_trait(trait_fqn, &self.package_path)
            })
            .is_some_and(|sig| sig.is_interface);
        if !is_interface {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "trait '{}' cannot be used as a type; declare it as an 'interface' to use it as an object type",
                    trait_fqn.symbol.0
                ),
            );
        }
        is_interface
    }

    /// Resolve one component of an intersection type expression to
    /// `(interface FQN, resolved type args)`. Emits an error and returns `None`
    /// for unknown names, non-interfaces, and arity mismatches.
    fn resolve_intersection_component(
        &mut self,
        named: &crate::parser::ast::NamedType,
        type_params_map: Option<&BTreeMap<String, Type>>,
    ) -> Option<(Fqn, Vec<Type>)> {
        let trait_fqn = self.resolve_name_to_fqn(&named.name.value, |fqn| {
            self.package_registry
                .lookup_trait(fqn, &self.package_path)
                .is_some()
                || self
                    .dependency_registry
                    .lookup_trait(fqn, &self.package_path)
                    .is_some()
        });
        let Some(trait_fqn) = trait_fqn else {
            let known_non_trait = self
                .resolve_name_to_fqn(&named.name.value, |fqn| {
                    self.package_registry.lookup_type_by_fqn(fqn).is_some()
                        || self.dependency_registry.lookup_type_by_fqn(fqn).is_some()
                })
                .is_some();
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
        if !self.check_interface_in_type_position(&trait_fqn, &named.name.span) {
            return None;
        }
        let expected_count = self
            .package_registry
            .lookup_trait(&trait_fqn, &self.package_path)
            .or_else(|| {
                self.dependency_registry
                    .lookup_trait(&trait_fqn, &self.package_path)
            })
            .map(|sig| sig.type_params.len())
            .unwrap_or(0);
        if named.type_args.len() != expected_count {
            self.diagnostics.error(
                named.span.clone(),
                format!(
                    "expected {} type argument(s) for '{}', found {}",
                    expected_count,
                    named.name.value,
                    named.type_args.len()
                ),
            );
            return None;
        }
        let args: Vec<Type> = named
            .type_args
            .iter()
            .map(|ta| match type_params_map {
                Some(map) => self.resolve_type_expr_with_type_params(ta, map),
                None => self.resolve_type_expr(ta),
            })
            .collect();
        if args.iter().any(|t| t.is_error()) {
            return None;
        }
        Some((trait_fqn, args))
    }

    /// Resolve an intersection type expression (`A and B`) to an
    /// interface-object type over all components. Accumulates errors; any
    /// failing component makes the whole type `Type::Error`.
    fn resolve_intersection_type_expr(
        &mut self,
        components: &[crate::parser::ast::NamedType],
        type_params_map: Option<&BTreeMap<String, Type>>,
    ) -> Type {
        let mut resolved: Vec<(Fqn, Vec<Type>)> = Vec::new();
        let mut failed = false;
        for named in components {
            match self.resolve_intersection_component(named, type_params_map) {
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
                if let Some(ty) = Type::from_primitive(&named.name.value) {
                    return ty;
                }

                // Handle Array<T> named syntax
                if named.name.value == "Array" {
                    if named.type_args.len() != 1 {
                        self.diagnostics.error(
                            named.span.clone(),
                            format!(
                                "expected 1 type argument for 'Array', found {}",
                                named.type_args.len()
                            ),
                        );
                        return Type::Error;
                    }
                    let elem = self.resolve_type_expr(&named.type_args[0]);
                    return Type::Array(Box::new(elem));
                }

                // Check for generic record/enum with type args
                if !named.type_args.is_empty() {
                    return self.resolve_generic_type_expr(named, &BTreeMap::new());
                }

                if let Some(fqn) = self.resolve_name_to_fqn(&named.name.value, |fqn| {
                    self.package_registry.lookup_type_by_fqn(fqn).is_some()
                        || self.dependency_registry.lookup_type_by_fqn(fqn).is_some()
                }) {
                    if let Some(ty) = self.package_registry.lookup_type_by_fqn(&fqn) {
                        ty.clone()
                    } else if let Some(ty) = self.dependency_registry.lookup_type_by_fqn(&fqn) {
                        ty.clone()
                    } else {
                        Type::Error
                    }
                } else if let Some(trait_fqn) = self.resolve_name_to_fqn(&named.name.value, |fqn| {
                    self.package_registry
                        .lookup_trait(fqn, &self.package_path)
                        .is_some()
                        || self
                            .dependency_registry
                            .lookup_trait(fqn, &self.package_path)
                            .is_some()
                }) {
                    if self.check_interface_in_type_position(&trait_fqn, &named.name.span) {
                        Type::interface_object(trait_fqn, vec![])
                    } else {
                        Type::Error
                    }
                } else {
                    self.diagnostics.error(
                        named.span.clone(),
                        format!("unknown type: '{}'", named.name.value),
                    );
                    Type::Error
                }
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
            TypeExpr::Intersection(components) => {
                self.resolve_intersection_type_expr(components, None)
            }
        }
    }

    /// Resolve a type expression, checking type parameter names first.
    /// Used for generic function parameter/return types.
    pub(super) fn resolve_type_expr_with_type_params(
        &mut self,
        type_expr: &TypeExpr,
        type_params_map: &BTreeMap<String, Type>,
    ) -> Type {
        match type_expr {
            TypeExpr::TupleExtend(left, right, _) => Type::tuple_extend(
                self.resolve_type_expr_with_type_params(left, type_params_map),
                self.resolve_type_expr_with_type_params(right, type_params_map),
            ),
            TypeExpr::Tuple(type_exprs, _span) => {
                let types: Vec<Type> = type_exprs
                    .iter()
                    .map(|te| self.resolve_type_expr_with_type_params(te, type_params_map))
                    .collect();
                if types.iter().any(|t| t.is_error()) {
                    return Type::Error;
                }
                let mn = MangledName::for_tuple(&types);
                Type::Tuple(types, mn)
            }
            TypeExpr::Named(named) => {
                if let Some((root, member)) = named.name.value.split_once('.')
                    && let Some(receiver) = type_params_map.get(root)
                {
                    let arguments: Vec<_> = named
                        .type_args
                        .iter()
                        .map(|ty| self.resolve_type_expr_with_type_params(ty, type_params_map))
                        .collect();
                    return match crate::typechecker::associated_types::resolve_reference(
                        receiver,
                        member,
                        arguments,
                        &[&self.package_registry, self.dependency_registry],
                    ) {
                        Ok(ty) => ty,
                        Err(message) => {
                            self.diagnostics.error(named.span.clone(), message);
                            Type::Error
                        }
                    };
                }

                // Check type params first
                if let Some(ty) = type_params_map.get(&named.name.value) {
                    if named.type_args.is_empty() {
                        return ty.clone();
                    }
                    // GAT reference: resolve type args and create TypeConstructor
                    let resolved_args: Vec<Type> = named
                        .type_args
                        .iter()
                        .map(|te| self.resolve_type_expr_with_type_params(te, type_params_map))
                        .collect();
                    return Type::TypeConstructor {
                        name: TypeParamName(named.name.value.clone()),
                        type_args: resolved_args,
                    };
                }

                // Handle Array<T> named syntax
                if named.name.value == "Array" {
                    if named.type_args.len() != 1 {
                        self.diagnostics.error(
                            named.span.clone(),
                            format!(
                                "expected 1 type argument for 'Array', found {}",
                                named.type_args.len()
                            ),
                        );
                        return Type::Error;
                    }
                    let elem = self
                        .resolve_type_expr_with_type_params(&named.type_args[0], type_params_map);
                    return Type::Array(Box::new(elem));
                }

                // Check for generic record/enum with type args (e.g. Box<T>)
                if !named.type_args.is_empty() {
                    return self.resolve_generic_type_expr(named, type_params_map);
                }

                // Check if name is a trait → interface object type
                if let Some(trait_fqn) = self.resolve_name_to_fqn(&named.name.value, |fqn| {
                    self.package_registry
                        .lookup_trait(fqn, &self.package_path)
                        .is_some()
                        || self
                            .dependency_registry
                            .lookup_trait(fqn, &self.package_path)
                            .is_some()
                }) {
                    if self.check_interface_in_type_position(&trait_fqn, &named.name.span) {
                        return Type::interface_object(trait_fqn, vec![]);
                    }
                    return Type::Error;
                }

                // Fall back to normal resolution
                self.resolve_type_expr(type_expr)
            }
            TypeExpr::Function(param_exprs, ret_expr, _span) => {
                let param_types: Vec<Type> = param_exprs
                    .iter()
                    .map(|te| self.resolve_type_expr_with_type_params(te, type_params_map))
                    .collect();
                let ret_type = self.resolve_type_expr_with_type_params(ret_expr, type_params_map);
                if param_types.iter().any(|t| t.is_error()) || ret_type.is_error() {
                    return Type::Error;
                }
                Type::Function(param_types, Box::new(ret_type))
            }
            TypeExpr::Intersection(components) => {
                self.resolve_intersection_type_expr(components, Some(type_params_map))
            }
        }
    }

    /// Resolve a generic type expression like `Box<Int32>` or `Option<T>`.
    /// Produces `Type::GenericRecord`, `Type::GenericEnum`, `Type::GenericClass`, or `Type::GenericNewtype`.
    fn resolve_generic_type_expr(
        &mut self,
        named: &crate::parser::ast::NamedType,
        type_params_map: &BTreeMap<String, Type>,
    ) -> Type {
        // Look up the generic record definition via import-aware resolution
        let resolved_fqn = self.resolve_name_to_fqn(&named.name.value, |fqn| {
            self.package_registry
                .lookup_generic_record_by_fqn(fqn, &self.package_path)
                .is_some()
                || self
                    .dependency_registry
                    .lookup_generic_record_by_fqn(fqn, &self.package_path)
                    .is_some()
        });

        if let Some(fqn) = resolved_fqn {
            let def = self
                .package_registry
                .lookup_generic_record_by_fqn(&fqn, &self.package_path)
                .or_else(|| {
                    self.dependency_registry
                        .lookup_generic_record_by_fqn(&fqn, &self.package_path)
                });
            if let Some(def) = def {
                let def = def.clone();
                return self.finish_resolve_generic_type_expr(
                    named,
                    type_params_map,
                    &fqn,
                    &def.type_params,
                    &def.type_param_variances,
                    GenericTypeKind::Record,
                );
            }
        }

        // Try generic enum definition
        let resolved_enum_fqn = self.resolve_name_to_fqn(&named.name.value, |fqn| {
            self.package_registry
                .lookup_generic_enum_by_fqn(fqn, &self.package_path)
                .is_some()
                || self
                    .dependency_registry
                    .lookup_generic_enum_by_fqn(fqn, &self.package_path)
                    .is_some()
        });

        if let Some(fqn) = resolved_enum_fqn {
            let def = self
                .package_registry
                .lookup_generic_enum_by_fqn(&fqn, &self.package_path)
                .or_else(|| {
                    self.dependency_registry
                        .lookup_generic_enum_by_fqn(&fqn, &self.package_path)
                });
            if let Some(def) = def {
                let def = def.clone();
                return self.finish_resolve_generic_type_expr(
                    named,
                    type_params_map,
                    &fqn,
                    &def.type_params,
                    &def.type_param_variances,
                    GenericTypeKind::Enum,
                );
            }
        }

        // Try generic class
        let resolved_class_fqn = self.resolve_name_to_fqn(&named.name.value, |fqn| {
            self.package_registry
                .lookup_class_type(fqn, &self.package_path)
                .is_some_and(|sig| !sig.type_params.is_empty())
                || self
                    .dependency_registry
                    .lookup_class_type(fqn, &self.package_path)
                    .is_some_and(|sig| !sig.type_params.is_empty())
        });

        if let Some(fqn) = resolved_class_fqn {
            let sig = self
                .package_registry
                .lookup_class_type(&fqn, &self.package_path)
                .or_else(|| {
                    self.dependency_registry
                        .lookup_class_type(&fqn, &self.package_path)
                });
            if let Some(sig) = sig {
                let type_params = sig.type_params.clone();
                let variances = sig.type_param_variances.clone();

                // Validate type arg count
                if named.type_args.len() != type_params.len() {
                    self.diagnostics.error(
                        named.span.clone(),
                        format!(
                            "expected {} type argument(s) for '{}', found {}",
                            type_params.len(),
                            named.name.value,
                            named.type_args.len()
                        ),
                    );
                    return Type::Error;
                }

                // Resolve type arguments with variance
                let resolved_type_args: Vec<Type> = named
                    .type_args
                    .iter()
                    .map(|ta| {
                        if type_params_map.is_empty() {
                            self.resolve_type_expr(ta)
                        } else {
                            self.resolve_type_expr_with_type_params(ta, type_params_map)
                        }
                    })
                    .collect();

                let mangled = MangledName::for_type(&fqn);
                let type_args_with_variance: Vec<(crate::common::types::Variance, Type)> =
                    variances
                        .iter()
                        .zip(resolved_type_args.iter())
                        .map(|(v, t)| (*v, t.clone()))
                        .collect();
                return Type::GenericClass {
                    fqn,
                    mangled_name: mangled,
                    type_args: type_args_with_variance,
                };
            }
        }

        // Try generic newtype (e.g. `Wrapper<Int32>`)
        let resolved_newtype_fqn = self.resolve_name_to_fqn(&named.name.value, |fqn| {
            self.package_registry
                .lookup_generic_newtype_by_fqn(fqn, &self.package_path)
                .is_some()
                || self
                    .dependency_registry
                    .lookup_generic_newtype_by_fqn(fqn, &self.package_path)
                    .is_some()
        });

        if let Some(fqn) = resolved_newtype_fqn {
            let sig = self
                .package_registry
                .lookup_generic_newtype_by_fqn(&fqn, &self.package_path)
                .or_else(|| {
                    self.dependency_registry
                        .lookup_generic_newtype_by_fqn(&fqn, &self.package_path)
                });
            if let Some(sig) = sig {
                let type_params = sig.type_params.clone();
                let variances = sig.type_param_variances.clone();
                let inner_type = sig.inner_type.clone();
                return self.finish_resolve_generic_newtype_expr(
                    named,
                    type_params_map,
                    &fqn,
                    &type_params,
                    &variances,
                    &inner_type,
                );
            }
        }

        // Try generic type alias (e.g. `type Maybe<T> = Option<T>`)
        let resolved_alias_fqn = self.resolve_name_to_fqn(&named.name.value, |fqn| {
            self.package_registry
                .lookup_generic_type_alias_by_fqn(fqn)
                .is_some()
                || self
                    .dependency_registry
                    .lookup_generic_type_alias_by_fqn(fqn)
                    .is_some()
        });

        if let Some(fqn) = resolved_alias_fqn {
            let alias_sig = self
                .package_registry
                .lookup_generic_type_alias_by_fqn(&fqn)
                .or_else(|| {
                    self.dependency_registry
                        .lookup_generic_type_alias_by_fqn(&fqn)
                });
            if let Some(alias_sig) = alias_sig {
                let alias_sig = alias_sig.clone();

                // Validate type arg count
                if named.type_args.len() != alias_sig.type_params.len() {
                    self.diagnostics.error(
                        named.span.clone(),
                        format!(
                            "expected {} type argument(s) for '{}', found {}",
                            alias_sig.type_params.len(),
                            named.name.value,
                            named.type_args.len()
                        ),
                    );
                    return Type::Error;
                }

                // Resolve type arguments
                let resolved_type_args: Vec<Type> = named
                    .type_args
                    .iter()
                    .map(|ta| {
                        if type_params_map.is_empty() {
                            self.resolve_type_expr(ta)
                        } else {
                            self.resolve_type_expr_with_type_params(ta, type_params_map)
                        }
                    })
                    .collect();

                // Build substitution map: type_param_name → concrete type
                let substitution: BTreeMap<String, Type> = alias_sig
                    .type_params
                    .iter()
                    .zip(resolved_type_args.iter())
                    .map(|(tp, ty)| (tp.0.clone(), ty.clone()))
                    .collect();

                // Substitute type params in the expanded type
                return substitute_type_params_in(&alias_sig.expanded_type, &substitution);
            }
        }

        // Try generic trait → interface object type
        if let Some(trait_fqn) = self.resolve_name_to_fqn(&named.name.value, |fqn| {
            self.package_registry
                .lookup_trait(fqn, &self.package_path)
                .is_some()
                || self
                    .dependency_registry
                    .lookup_trait(fqn, &self.package_path)
                    .is_some()
        }) {
            let sig = self
                .package_registry
                .lookup_trait(&trait_fqn, &self.package_path)
                .or_else(|| {
                    self.dependency_registry
                        .lookup_trait(&trait_fqn, &self.package_path)
                });
            if let Some(sig) = sig {
                let expected_count = sig.type_params.len();
                if named.type_args.len() != expected_count {
                    self.diagnostics.error(
                        named.span.clone(),
                        format!(
                            "expected {} type argument(s) for trait '{}', found {}",
                            expected_count,
                            named.name.value,
                            named.type_args.len()
                        ),
                    );
                    return Type::Error;
                }
                let trait_type_args: Vec<Type> = named
                    .type_args
                    .iter()
                    .map(|ta| {
                        if type_params_map.is_empty() {
                            self.resolve_type_expr(ta)
                        } else {
                            self.resolve_type_expr_with_type_params(ta, type_params_map)
                        }
                    })
                    .collect();
                if !self.check_interface_in_type_position(&trait_fqn, &named.name.span) {
                    return Type::Error;
                }
                return Type::interface_object(trait_fqn, trait_type_args);
            }
        }

        self.diagnostics.error(
            named.span.clone(),
            format!("unknown generic type: '{}'", named.name.value),
        );
        Type::Error
    }

    /// Finish resolving a generic type expression (shared by record and enum paths).
    fn finish_resolve_generic_type_expr(
        &mut self,
        named: &crate::parser::ast::NamedType,
        type_params_map: &BTreeMap<String, Type>,
        fqn: &crate::common::types::Fqn,
        type_params: &[crate::common::types::TypeParamName],
        type_param_variances: &[Variance],
        kind: GenericTypeKind,
    ) -> Type {
        // Validate type arg count
        if named.type_args.len() != type_params.len() {
            self.diagnostics.error(
                named.span.clone(),
                format!(
                    "expected {} type argument(s) for '{}', found {}",
                    type_params.len(),
                    named.name.value,
                    named.type_args.len()
                ),
            );
            return Type::Error;
        }

        // Resolve type arguments and pair with variances
        let resolved_type_args: Vec<(Variance, Type)> = named
            .type_args
            .iter()
            .enumerate()
            .map(|(i, ta)| {
                let ty = if type_params_map.is_empty() {
                    self.resolve_type_expr(ta)
                } else {
                    self.resolve_type_expr_with_type_params(ta, type_params_map)
                };
                let variance = type_param_variances
                    .get(i)
                    .copied()
                    .unwrap_or(Variance::Invariant);
                (variance, ty)
            })
            .collect();

        // Erased mangled name — all instantiations share one canonical TypeDef.
        let mangled = MangledName::for_type(fqn);
        match kind {
            GenericTypeKind::Record => Type::GenericRecord {
                fqn: fqn.clone(),
                mangled_name: mangled,
                type_args: resolved_type_args,
            },
            GenericTypeKind::Enum => Type::GenericEnum {
                fqn: fqn.clone(),
                mangled_name: mangled,
                type_args: resolved_type_args,
            },
        }
    }

    /// Finish resolving a generic newtype expression.
    /// Unlike records/enums (which are just references), newtypes embed their resolved
    /// inner type directly. We compute `concrete_inner_type` by substituting type params
    /// in the newtype's template inner type.
    fn finish_resolve_generic_newtype_expr(
        &mut self,
        named: &crate::parser::ast::NamedType,
        type_params_map: &BTreeMap<String, Type>,
        fqn: &crate::common::types::Fqn,
        type_params: &[crate::common::types::TypeParamName],
        type_param_variances: &[Variance],
        template_inner_type: &Type,
    ) -> Type {
        if named.type_args.len() != type_params.len() {
            self.diagnostics.error(
                named.span.clone(),
                format!(
                    "expected {} type argument(s) for '{}', found {}",
                    type_params.len(),
                    named.name.value,
                    named.type_args.len()
                ),
            );
            return Type::Error;
        }

        let resolved_type_args: Vec<(Variance, Type)> = named
            .type_args
            .iter()
            .enumerate()
            .map(|(i, ta)| {
                let ty = if type_params_map.is_empty() {
                    self.resolve_type_expr(ta)
                } else {
                    self.resolve_type_expr_with_type_params(ta, type_params_map)
                };
                let variance = type_param_variances
                    .get(i)
                    .copied()
                    .unwrap_or(Variance::Invariant);
                (variance, ty)
            })
            .collect();

        // Substitute type params in the inner type.
        // When type args contain TypeParameter (e.g. inside a generic context),
        // the result will contain TypeParameter types — correct for templates.
        let substitution: BTreeMap<String, Type> = type_params
            .iter()
            .zip(resolved_type_args.iter())
            .map(|(tp, (_, ty))| (tp.0.clone(), ty.clone()))
            .collect();
        let concrete_inner = substitute_type_params_in(template_inner_type, &substitution);

        Type::GenericNewtype {
            fqn: fqn.clone(),
            type_args: resolved_type_args,
            concrete_inner_type: Box::new(concrete_inner),
        }
    }
}

/// Recursively substitute type parameters in a type.
/// Replaces `Type::TypeVariable`/`GenericParam(name, _)` with the concrete type from the substitution map.
pub(super) fn substitute_type_params_in(ty: &Type, substitution: &BTreeMap<String, Type>) -> Type {
    match ty {
        Type::TypeVariable(name, _) | Type::GenericParam(name, _, _) => {
            if let Some(concrete) = substitution.get(&name.0) {
                concrete.clone()
            } else {
                ty.clone()
            }
        }
        Type::Array(elem) => Type::Array(Box::new(substitute_type_params_in(elem, substitution))),
        Type::TupleExtend(left, right) => Type::tuple_extend(
            substitute_type_params_in(left, substitution),
            substitute_type_params_in(right, substitution),
        ),
        Type::TupleProjection(receiver, kind) => {
            Type::tuple_projection(substitute_type_params_in(receiver, substitution), *kind)
        }
        Type::AssociatedProjection(projection) => projection
            .map(|ty| substitute_type_params_in(ty, substitution))
            .into_type(),
        Type::Tuple(types, _mn) => {
            let substituted: Vec<Type> = types
                .iter()
                .map(|t| substitute_type_params_in(t, substitution))
                .collect();
            let mn = MangledName::for_tuple(&substituted);
            Type::Tuple(substituted, mn)
        }
        Type::GenericRecord { fqn, type_args, .. } => {
            let substituted_args: Vec<(Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, substitute_type_params_in(t, substitution)))
                .collect();
            Type::GenericRecord {
                fqn: fqn.clone(),
                mangled_name: MangledName::for_type(fqn),
                type_args: substituted_args,
            }
        }
        Type::GenericEnum { fqn, type_args, .. } => {
            let substituted_args: Vec<(Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, substitute_type_params_in(t, substitution)))
                .collect();
            Type::GenericEnum {
                fqn: fqn.clone(),
                mangled_name: MangledName::for_type(fqn),
                type_args: substituted_args,
            }
        }
        Type::GenericClass {
            fqn,
            type_args: type_params,
            ..
        } => {
            let substituted_params = type_params
                .iter()
                .map(|(variance, ty)| (*variance, substitute_type_params_in(ty, substitution)))
                .collect();
            Type::GenericClass {
                fqn: fqn.clone(),
                mangled_name: MangledName::for_type(fqn),
                type_args: substituted_params,
            }
        }
        Type::InterfaceObject { traits, .. } => Type::interface_intersection(
            traits
                .iter()
                .map(|c| {
                    (
                        c.trait_fqn.clone(),
                        c.trait_type_args
                            .iter()
                            .map(|t| substitute_type_params_in(t, substitution))
                            .collect(),
                    )
                })
                .collect(),
        ),
        Type::Newtype(fqn, inner) => Type::Newtype(
            fqn.clone(),
            Box::new(substitute_type_params_in(inner, substitution)),
        ),
        Type::GenericNewtype {
            fqn,
            type_args,
            concrete_inner_type,
        } => {
            let substituted_args: Vec<(Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, substitute_type_params_in(t, substitution)))
                .collect();
            let substituted_inner = substitute_type_params_in(concrete_inner_type, substitution);
            Type::GenericNewtype {
                fqn: fqn.clone(),
                type_args: substituted_args,
                concrete_inner_type: Box::new(substituted_inner),
            }
        }
        Type::Function(params, ret) => {
            let substituted_params: Vec<Type> = params
                .iter()
                .map(|t| substitute_type_params_in(t, substitution))
                .collect();
            let substituted_ret = substitute_type_params_in(ret, substitution);
            Type::Function(substituted_params, Box::new(substituted_ret))
        }
        // Primitive types, Record, Enum, Never, Any, SelfType, Error — no substitution needed
        _ => ty.clone(),
    }
}
