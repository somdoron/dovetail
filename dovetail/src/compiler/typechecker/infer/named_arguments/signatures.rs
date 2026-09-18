use super::super::generics::apply_substitution;
use super::super::type_param_substitution::TypeParamSubstitution;
use super::{Inference, Parameters, SymbolKind};
use crate::common::diagnostics::Diagnostics;
use crate::common::types::{Fqn, SymbolName, TypeParamName};
use crate::parser::ast::{Expr, TypeExpr};
use crate::typechecker::types::{TraitBounds, Type};

impl Inference<'_> {
    pub(super) fn named_call_signatures(&mut self, expression: &Expr) -> Vec<Parameters> {
        match expression {
            Expr::FunctionCall {
                name, type_args, ..
            } => {
                if self.lookup_variable(&name.value).is_some() {
                    return vec![];
                }
                if let Some(fqn) = self.resolve_fqn(&name.value, SymbolKind::Function) {
                    let mut parameters: Vec<_> = self
                        .registry
                        .lookup_function(&fqn, &self.package_path, &self.current_file)
                        .unwrap_or_default()
                        .iter()
                        .map(|sig| sig.params.clone())
                        .collect();
                    if let Some(defs) = self
                        .registry
                        .lookup_generic_function(&fqn, &self.package_path)
                    {
                        parameters.extend(defs.iter().map(|def| def.params.clone()));
                    }
                    return parameters;
                }
                let class_fqn = self
                    .resolve_fqn(&name.value, SymbolKind::Class)
                    .or_else(|| {
                        let mut diagnostics = Diagnostics::new();
                        self.named_call_probe(&mut diagnostics)
                            .resolve_type_name(&name.value, type_args, &name.span)
                            .and_then(|ty| ty.try_to_fqn())
                    });
                class_fqn
                    .and_then(|fqn| self.registry.get_class_type(&fqn))
                    .map(|class| {
                        vec![
                            class
                                .constructor_params
                                .iter()
                                .map(|p| (p.name.clone(), p.ty.clone()))
                                .collect(),
                        ]
                    })
                    .unwrap_or_default()
            }
            Expr::MethodCall {
                receiver,
                receiver_type_args,
                method,
                ..
            } => self.named_method_signatures(receiver, receiver_type_args, &method.value),
            _ => vec![],
        }
    }

    fn named_method_signatures(
        &mut self,
        receiver: &Expr,
        receiver_type_args: &[TypeExpr],
        method: &str,
    ) -> Vec<Parameters> {
        let symbol = SymbolName(method.into());
        if let Expr::Identifier(name, _) = receiver {
            if self.lookup_variable(name).is_none() {
                if let Some(module) = self.resolve_module_name(name) {
                    let mut result: Vec<_> = module
                        .functions
                        .get(&symbol)
                        .into_iter()
                        .flatten()
                        .map(|sig| sig.params.clone())
                        .collect();
                    result.extend(
                        module
                            .generic_members
                            .lookup(&symbol)
                            .into_iter()
                            .flatten()
                            .map(|sig| sig.params.clone()),
                    );
                    if !result.is_empty() {
                        return result;
                    }
                }
                if let Some(fqn) = self.resolve_trait_fqn(name) {
                    let mut diagnostics = Diagnostics::new();
                    let types = self
                        .named_call_probe(&mut diagnostics)
                        .resolve_type_args(receiver_type_args)
                        .unwrap_or_default();
                    return self.trait_named_signatures(&fqn, &types, method, false);
                }
                if let Some(import) = self.import_scope.lookup(name)
                    && let crate::typechecker::imports::ImportTarget::Extension(fqn) =
                        &import.target
                {
                    return self
                        .registry
                        .lookup_extension_blocks_by_fqn(fqn)
                        .iter()
                        .flat_map(|block| &block.methods)
                        .filter(|sig| sig.name == symbol)
                        .map(|sig| sig.params.clone())
                        .collect();
                }
                if let Some(fqn) = self.resolve_fqn(name, SymbolKind::Class) {
                    let signatures = self.class_named_signatures(&fqn, &symbol, false);
                    if !signatures.is_empty() {
                        return signatures;
                    }
                }
            }
            if name == "super"
                && let Some(binding) = self.lookup_variable("self")
                && let Some(parent) = binding
                    .ty
                    .try_to_fqn()
                    .and_then(|fqn| self.registry.get_class_type(&fqn))
                    .and_then(|class| class.parent_class.clone())
            {
                return self.class_named_signatures(&parent, &symbol, true);
            }
        }
        if let Some(functions) = self.resolve_qualified_call(receiver, method) {
            return functions.iter().map(|sig| sig.params.clone()).collect();
        }
        let mut diagnostics = Diagnostics::new();
        let shadowed =
            matches!(receiver, Expr::Identifier(name, _) if self.lookup_variable(name).is_some());
        let static_type = if shadowed {
            None
        } else {
            let mut probe = self.named_call_probe(&mut diagnostics);
            match receiver {
                Expr::Identifier(name, span) => {
                    probe.resolve_type_name(name, receiver_type_args, span)
                }
                _ => probe.try_expr_as_type_for_static_dispatch(receiver),
            }
        };
        if let Some(ty) = static_type.filter(|_| !diagnostics.has_errors()) {
            return self.static_named_signatures(&ty, &symbol);
        }
        let previous = self.expected_type.take();
        let typed_receiver = self.infer_expr(receiver);
        self.expected_type = previous;
        let signatures = self.instance_named_signatures(&typed_receiver.ty, &symbol);
        self.named_call_receiver = Some(typed_receiver);
        signatures
    }

    fn static_named_signatures(&self, receiver: &Type, method: &SymbolName) -> Vec<Parameters> {
        if let Type::GenericParam(_, bounds, _) | Type::TypeVariable(_, bounds) = receiver {
            return bounds
                .iter()
                .filter_map(|bound| bound.named())
                .flat_map(|bound| {
                    self.trait_named_signatures(
                        &bound.trait_fqn,
                        &bound.type_args,
                        &method.0,
                        false,
                    )
                })
                .filter(|parameters| parameters.first().is_none_or(|(name, _)| name != "self"))
                .collect();
        }
        let Some(fqn) = receiver.try_to_fqn() else {
            return vec![];
        };
        self.lookup_named_extension_methods(&fqn, method)
            .into_iter()
            .filter_map(|(block, method)| {
                self.applicable_named_parameters(
                    receiver,
                    &block.for_type,
                    &block.type_params,
                    &block.trait_bounds,
                    &method.params,
                )
            })
            .chain(
                self.lookup_all_generic_extension_methods(receiver, method)
                    .into_iter()
                    .filter_map(|(block, method)| {
                        self.applicable_named_parameters(
                            receiver,
                            &block.for_type,
                            &block.type_params,
                            &block.trait_bounds,
                            &method.params,
                        )
                    }),
            )
            .chain(
                self.registry
                    .find_impl_method(&fqn, method)
                    .into_iter()
                    .filter_map(|(block, method)| {
                        self.applicable_named_parameters(
                            receiver,
                            &block.for_type,
                            &block.type_params,
                            &block.trait_bounds,
                            &method.params,
                        )
                    }),
            )
            .collect()
    }

    fn trait_named_signatures(
        &self,
        fqn: &Fqn,
        type_args: &[Type],
        method: &str,
        implicit_receiver: bool,
    ) -> Vec<Parameters> {
        self.registry
            .get_trait(fqn)
            .into_iter()
            .flat_map(|declaration| {
                let types: Vec<_> = type_args
                    .iter()
                    .map(|ty| self.scoped_bound_type(ty))
                    .collect();
                let substitution =
                    TypeParamSubstitution::from_pairs(&declaration.type_params, &types);
                declaration
                    .methods
                    .iter()
                    .filter(move |sig| sig.name == method)
                    .map(move |sig| {
                        explicit_parameters(
                            &substitute_parameters(&sig.params, &substitution),
                            implicit_receiver,
                        )
                    })
            })
            .collect()
    }

    fn class_named_signatures(
        &self,
        fqn: &Fqn,
        method: &SymbolName,
        instance: bool,
    ) -> Vec<Parameters> {
        self.class_named_signatures_with_substitution(
            fqn,
            method,
            instance,
            &TypeParamSubstitution::new(),
        )
    }

    fn class_named_signatures_with_substitution(
        &self,
        fqn: &Fqn,
        method: &SymbolName,
        instance: bool,
        substitution: &TypeParamSubstitution,
    ) -> Vec<Parameters> {
        let Some(class) = self.registry.get_class_type(fqn) else {
            return vec![];
        };
        let methods = if instance {
            &class.instance_methods
        } else {
            &class.static_methods
        };
        let generics = if instance {
            &class.generic_instance_methods
        } else {
            &class.generic_static_methods
        };
        let mut result: Vec<_> = methods
            .get(method)
            .into_iter()
            .flatten()
            .filter(|sig| !sig.is_property)
            .map(|sig| {
                explicit_parameters(&substitute_parameters(&sig.params, substitution), instance)
            })
            .collect();
        result.extend(generics.get(method).into_iter().flatten().map(|sig| {
            explicit_parameters(&substitute_parameters(&sig.params, substitution), instance)
        }));
        if let Some(parent) = &class.parent_class {
            let mut parent_substitution = TypeParamSubstitution::new();
            if let Some(parent_type) = &class.parent_type_expr {
                let parent_type = apply_substitution(substitution, parent_type);
                parent_substitution = self.class_receiver_substitution(&parent_type);
            }
            for inherited in self.class_named_signatures_with_substitution(
                parent,
                method,
                instance,
                &parent_substitution,
            ) {
                if !result.iter().any(|own| {
                    own.iter()
                        .map(|(_, ty)| ty)
                        .eq(inherited.iter().map(|(_, ty)| ty))
                }) {
                    result.push(inherited);
                }
            }
        }
        result
    }

    fn instance_named_signatures(&self, receiver: &Type, method: &SymbolName) -> Vec<Parameters> {
        match receiver {
            Type::GenericParam(_, bounds, _) | Type::TypeVariable(_, bounds) => {
                return bounds
                    .iter()
                    .filter_map(|bound| bound.named())
                    .flat_map(|bound| {
                        if bound.is_class_bound() {
                            self.class_named_signatures(&bound.trait_fqn, method, true)
                        } else {
                            self.trait_named_signatures(
                                &bound.trait_fqn,
                                &bound.type_args,
                                &method.0,
                                true,
                            )
                        }
                    })
                    .collect();
            }
            Type::InterfaceObject { traits, .. } => {
                return traits
                    .iter()
                    .flat_map(|component| {
                        self.trait_named_signatures(
                            &component.trait_fqn,
                            &component.trait_type_args,
                            &method.0,
                            true,
                        )
                    })
                    .collect();
            }
            _ => {}
        }
        let Some(fqn) = receiver.try_to_fqn() else {
            return vec![];
        };
        let class = self.class_named_signatures_with_substitution(
            &fqn,
            method,
            true,
            &self.class_receiver_substitution(receiver),
        );
        if !class.is_empty() {
            return class;
        }
        let mut result = Vec::new();
        if let Some(module) = self.registry.lookup_module(&fqn) {
            result.extend(
                module
                    .functions
                    .get(method)
                    .into_iter()
                    .flatten()
                    .filter(|sig| sig.params.first().is_some_and(|(name, _)| name == "self"))
                    .map(|sig| explicit_parameters(&sig.params, true)),
            );
            result.extend(
                module
                    .generic_members
                    .lookup(method)
                    .into_iter()
                    .flatten()
                    .filter(|sig| sig.params.first().is_some_and(|(name, _)| name == "self"))
                    .map(|sig| explicit_parameters(&sig.params, true)),
            );
        }
        if !result.is_empty() {
            return result;
        }
        result.extend(
            self.lookup_named_extension_methods(&fqn, method)
                .iter()
                .filter_map(|(block, sig)| {
                    self.applicable_named_parameters(
                        receiver,
                        &block.for_type,
                        &block.type_params,
                        &block.trait_bounds,
                        &sig.params,
                    )
                })
                .map(|parameters| explicit_parameters(&parameters, true)),
        );
        result.extend(
            self.lookup_all_generic_extension_methods(receiver, method)
                .iter()
                .filter_map(|(block, sig)| {
                    self.applicable_named_parameters(
                        receiver,
                        &block.for_type,
                        &block.type_params,
                        &block.trait_bounds,
                        &sig.params,
                    )
                })
                .map(|parameters| explicit_parameters(&parameters, true)),
        );
        if !result.is_empty() {
            return result;
        }
        result.extend(
            self.registry
                .find_impl_method(&fqn, method)
                .iter()
                .filter_map(|(block, sig)| {
                    self.applicable_named_parameters(
                        receiver,
                        &block.for_type,
                        &block.type_params,
                        &block.trait_bounds,
                        &sig.params,
                    )
                })
                .map(|parameters| explicit_parameters(&parameters, true)),
        );
        result
    }
    fn class_receiver_substitution(&self, receiver: &Type) -> TypeParamSubstitution {
        if let Type::GenericClass { fqn, type_args, .. } = receiver
            && let Some(class) = self.registry.get_class_type(fqn)
        {
            let types: Vec<_> = type_args.iter().map(|(_, ty)| ty.clone()).collect();
            return TypeParamSubstitution::from_pairs(&class.type_params, &types);
        }
        TypeParamSubstitution::new()
    }

    /// A receiver family can have implementations for several concrete applications.
    /// Only declarations applicable to this receiver contribute argument names.
    fn applicable_named_parameters(
        &self,
        receiver: &Type,
        for_type: &Type,
        type_params: &[TypeParamName],
        bounds: &TraitBounds,
        parameters: &[(String, Type)],
    ) -> Option<Parameters> {
        let mut substitution = TypeParamSubstitution::new();
        if !substitution.unify(for_type, receiver) {
            return None;
        }
        if let Some(concrete_types) = substitution.resolve_type_params(type_params)
            && !self
                .unsatisfied_trait_bounds(bounds, type_params, &concrete_types)
                .is_empty()
        {
            return None;
        }
        Some(
            parameters
                .iter()
                .map(|(name, ty)| (name.clone(), apply_substitution(&substitution, ty)))
                .collect(),
        )
    }
}

fn explicit_parameters(parameters: &[(String, Type)], implicit_receiver: bool) -> Parameters {
    let skip = usize::from(
        implicit_receiver && parameters.first().is_some_and(|(name, _)| name == "self"),
    );
    parameters[skip..].to_vec()
}

fn substitute_parameters(
    parameters: &[(String, Type)],
    substitution: &TypeParamSubstitution,
) -> Parameters {
    parameters
        .iter()
        .map(|(name, ty)| (name.clone(), apply_substitution(substitution, ty)))
        .collect()
}
