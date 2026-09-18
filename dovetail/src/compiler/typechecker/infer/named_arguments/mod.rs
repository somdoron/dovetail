//! Bind source argument labels before positional call inference.
mod metadata;
mod signatures;
mod validation;
use super::{Inference, types::SymbolKind};
use crate::common::diagnostics::Diagnostics;
use crate::common::span::Span;
use crate::common::types::{Fqn, SymbolName};
use crate::parser::ast::Expr;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

type Parameters = Vec<(String, Type)>;

#[derive(Clone)]
struct ArgumentBinding {
    names: Vec<String>,
    signatures: Vec<Parameters>,
    /// Parameter slot to source argument index.
    order: Vec<usize>,
}

#[derive(Clone)]
pub(super) struct NamedCallContext {
    span: Span,
    names: Vec<String>,
    receiver: Option<TypedExpr>,
    member: String,
}

impl Inference<'_> {
    pub(super) fn infer_named_call(&mut self, expression: &Expr) -> Option<TypedExpr> {
        let source = arguments(expression)?;
        if !source
            .iter()
            .any(|arg| matches!(arg, Expr::NamedArgument { .. }))
        {
            return None;
        }
        let span = expression.span();
        let member = match expression {
            Expr::FunctionCall { name, .. } => name.value.clone(),
            Expr::MethodCall { method, .. } => method.value.clone(),
            _ => unreachable!(),
        };
        let enclosing_receiver = self.named_call_receiver.take();
        let signatures = self.named_call_signatures(expression);
        let receiver = std::mem::replace(&mut self.named_call_receiver, enclosing_receiver);
        let bindings = match bind_signatures(source, signatures) {
            Ok(bindings) => bindings,
            Err(message) => {
                self.diagnostics.error(span.clone(), message);
                return Some(self.error_expr(&span));
            }
        };
        let Some(binding) = self.select_named_overload(expression, bindings, &receiver, &member)
        else {
            return Some(self.error_expr(&span));
        };
        let previous = self.named_call_context.replace(NamedCallContext {
            span: span.clone(),
            names: binding.names.clone(),
            receiver,
            member: member.clone(),
        });
        let call = self.infer_expr(&positional_call(expression, &binding.order));
        self.named_call_context = previous;
        let parameter_types = self.named_parameter_types(&call, &binding);
        self.validate_named_deferred_arguments(&call, &parameter_types);
        Some(preserve_named_arguments(
            call,
            source,
            binding,
            parameter_types,
            member,
            span,
        ))
    }

    fn select_named_overload(
        &mut self,
        expression: &Expr,
        mut bindings: Vec<ArgumentBinding>,
        receiver: &Option<TypedExpr>,
        member: &str,
    ) -> Option<ArgumentBinding> {
        if bindings.len() == 1 {
            return bindings.pop();
        }
        let span = expression.span();
        let mut successful = Vec::new();
        let mut failed_diagnostics = Vec::new();
        for binding in bindings {
            let call = positional_call(expression, &binding.order);
            let mut diagnostics = Diagnostics::new();
            let mut probe = self.named_call_probe(&mut diagnostics);
            probe.named_call_context = Some(NamedCallContext {
                span: span.clone(),
                names: binding.names.clone(),
                receiver: receiver.clone(),
                member: member.into(),
            });
            let typed = probe.infer_expr(&call);
            if !typed.ty.is_error() && !diagnostics.has_errors() {
                successful.push(binding);
            } else if failed_diagnostics.is_empty() {
                failed_diagnostics = diagnostics.iter().cloned().collect();
            }
        }
        match successful.len() {
            1 => successful.pop(),
            0 => {
                self.diagnostics.extend(&failed_diagnostics);
                if failed_diagnostics.is_empty() {
                    self.diagnostics
                        .error(span, "no matching overload for named arguments");
                }
                None
            }
            _ => {
                self.diagnostics.error(
                    span,
                    "ambiguous call: multiple overloads accept these named arguments",
                );
                None
            }
        }
    }

    pub(super) fn cached_named_receiver(&self, expression: &Expr) -> Option<TypedExpr> {
        self.named_call_context
            .as_ref()?
            .receiver
            .as_ref()
            .filter(|receiver| receiver.span == expression.span())
            .cloned()
    }
    pub(super) fn infer_extends_arguments(
        &mut self,
        parent: &Type,
        source: &[Expr],
        span: &Span,
    ) -> (Vec<TypedExpr>, Vec<usize>) {
        let Some(class) = parent
            .try_to_fqn()
            .and_then(|fqn| self.registry.get_class_type(&fqn))
        else {
            return (
                source.iter().map(|arg| self.infer_expr(arg)).collect(),
                (0..source.len()).collect(),
            );
        };
        let substitution = match parent {
            Type::GenericClass { type_args, .. } => {
                super::type_param_substitution::TypeParamSubstitution::from_pairs(
                    &class.type_params,
                    &type_args
                        .iter()
                        .map(|(_, ty)| ty.clone())
                        .collect::<Vec<_>>(),
                )
            }
            _ => super::type_param_substitution::TypeParamSubstitution::new(),
        };
        let parameters: Parameters = class
            .constructor_params
            .iter()
            .map(|param| {
                (
                    param.name.clone(),
                    super::generics::apply_substitution(&substitution, &param.ty),
                )
            })
            .collect();
        let order = match bind(source, &parameters) {
            Ok(order) => order,
            Err(message) => {
                self.diagnostics.error(span.clone(), message);
                return (vec![], vec![]);
            }
        };
        let mut evaluation_order = vec![0; order.len()];
        let arguments = order
            .iter()
            .enumerate()
            .map(|(slot, &index)| {
                evaluation_order[index] = slot;
                let expression = match &source[index] {
                    Expr::NamedArgument { value, .. } => value.as_ref(),
                    expression => expression,
                };
                let previous = self.expected_type.replace(parameters[slot].1.clone());
                let typed = self.infer_expr(expression);
                self.expected_type = previous;
                // Generic superclass argument assignability is checked after
                // specialization, as it is for positional extends arguments.
                if !parameters[slot].1.contains_type_parameter() {
                    self.check_assignable(expression.span(), &parameters[slot].1, &typed.ty);
                }
                typed
            })
            .collect();
        (arguments, evaluation_order)
    }

    /// Nested calls must never inherit the enclosing call's label filter.
    pub(super) fn named_signature_allowed(&self, parameters: &[(String, Type)]) -> bool {
        let Some(context) = &self.named_call_context else {
            return true;
        };
        if self.current_expr_span.as_ref() != Some(&context.span) {
            return true;
        }
        let names: Vec<_> = parameters
            .iter()
            .filter(|(name, _)| name != "self")
            .map(|(name, _)| name)
            .collect();
        names
            .iter()
            .copied()
            .eq(context.names.iter().filter(|name| name.as_str() != "self"))
    }

    pub(super) fn named_trait_implementation_allowed(
        &self,
        trait_fqn: &Fqn,
        dispatch_name: &SymbolName,
    ) -> bool {
        let Some(context) = &self.named_call_context else {
            return true;
        };
        if self.current_expr_span.as_ref() != Some(&context.span) {
            return true;
        }
        let Some(signature) = self.registry.get_trait(trait_fqn) else {
            return true;
        };
        let Some(declaration) = signature
            .methods
            .iter()
            .find(|method| signature.method_dispatch_name(method) == *dispatch_name)
        else {
            return true;
        };
        declaration.name != context.member || self.named_signature_allowed(&declaration.params)
    }

    fn named_call_probe<'a>(&'a self, diagnostics: &'a mut Diagnostics) -> Inference<'a> {
        Inference {
            scopes: self.scopes.clone(),
            class_type_defs: self.class_type_defs.clone(),
            current_type_params: self.current_type_params.clone(),
            type_param_counter: self.type_param_counter,
            expected_type: self.expected_type.clone(),
            loop_depth: self.loop_depth,
            function_return_type: self.function_return_type.clone(),
            async_return_type: self.async_return_type.clone(),
            block_wrapped_error: self.block_wrapped_error.clone(),
            container_name: self.container_name.clone(),
            current_module_name: self.current_module_name.clone(),
            unresolved_method_type_params: self.unresolved_method_type_params.clone(),
            typechecking_class: self.typechecking_class.clone(),
            ..Inference::new(
                self.package_path.clone(),
                self.current_file.clone(),
                self.registry,
                self.import_scope,
                diagnostics,
            )
        }
    }
}

fn bind_signatures(
    source: &[Expr],
    signatures: Vec<Parameters>,
) -> Result<Vec<ArgumentBinding>, String> {
    if has_conflicting_names(&signatures) {
        return Err("ambiguous parameter names from competing declarations; use explicit trait or extension qualification".into());
    }
    let mut bindings: Vec<ArgumentBinding> = Vec::new();
    let mut errors = Vec::new();
    for parameters in signatures {
        match bind(source, &parameters) {
            Ok(order) => {
                let names: Vec<_> = parameters.iter().map(|(name, _)| name.clone()).collect();
                if let Some(binding) = bindings.iter_mut().find(|binding| binding.names == names) {
                    binding.signatures.push(parameters);
                } else {
                    bindings.push(ArgumentBinding {
                        names,
                        order,
                        signatures: vec![parameters],
                    });
                }
            }
            Err(error) => {
                if !errors.contains(&error) {
                    errors.push(error);
                }
            }
        }
    }
    if !bindings.is_empty() {
        return Ok(bindings);
    }
    if errors.is_empty() {
        Err("named arguments require a declared function, method, or class constructor; function values and positional payload constructors have no argument names".into())
    } else {
        Err(errors.join("; "))
    }
}

fn has_conflicting_names(signatures: &[Parameters]) -> bool {
    signatures.iter().enumerate().any(|(index, first)| {
        signatures[..index].iter().any(|second| {
            first
                .iter()
                .map(|(_, ty)| ty)
                .eq(second.iter().map(|(_, ty)| ty))
                && !first
                    .iter()
                    .map(|(name, _)| name)
                    .eq(second.iter().map(|(name, _)| name))
        })
    })
}

fn preserve_named_arguments(
    call: TypedExpr,
    source: &[Expr],
    binding: ArgumentBinding,
    parameter_types: Vec<Type>,
    source_name: String,
    span: Span,
) -> TypedExpr {
    if call.ty.is_error() {
        return call;
    }
    // This intrinsic consumes a required literal at compile time and has
    // no runtime operands to reorder or evaluate.
    if matches!(
        &call.kind,
        TypedExprKind::IntrinsicCall {
            intrinsic: crate::typechecker::types::IntrinsicKind::ResourceBytes { .. },
            ..
        }
    ) {
        return call;
    }
    let mut argument_order = vec![0; binding.order.len()];
    let mut named_parameters = Vec::new();
    for (parameter, &source_index) in binding.order.iter().enumerate() {
        argument_order[source_index] = parameter;
        if matches!(source[source_index], Expr::NamedArgument { .. }) {
            named_parameters.push((parameter, source[source_index].span()));
        }
    }
    let ty = call.ty.clone();
    TypedExpr {
        kind: TypedExprKind::NamedCall {
            call: Box::new(call),
            argument_order,
            parameter_names: binding.names,
            parameter_types,
            named_parameters,
            source_name,
        },
        ty,
        span,
    }
}

/// Map parameter slots to source argument indices, without evaluating expressions.
fn bind(arguments: &[Expr], parameters: &[(String, Type)]) -> Result<Vec<usize>, String> {
    let mut slots = vec![None; parameters.len()];
    let mut named = false;
    for (source, argument) in arguments.iter().enumerate() {
        let slot = if let Expr::NamedArgument { name, .. } = argument {
            named = true;
            parameters
                .iter()
                .position(|(parameter, _)| parameter == &name.value)
                .ok_or_else(|| format!("unknown argument name '{}'", name.value))?
        } else {
            if named {
                return Err("positional arguments must precede named arguments".into());
            }
            if source >= parameters.len() {
                return Err("too many arguments".into());
            }
            source
        };
        if parameters[slot].0 == "self" && matches!(argument, Expr::NamedArgument { .. }) {
            return Err("an explicit receiver must be passed positionally".into());
        }
        if slots[slot].replace(source).is_some() {
            return Err(format!(
                "parameter '{}' is supplied more than once",
                parameters[slot].0
            ));
        }
    }
    slots
        .into_iter()
        .enumerate()
        .map(|(slot, source)| {
            source.ok_or_else(|| format!("missing argument for parameter '{}'", parameters[slot].0))
        })
        .collect()
}

fn arguments(expression: &Expr) -> Option<&[Expr]> {
    match expression {
        Expr::FunctionCall { args, .. } | Expr::MethodCall { args, .. } => Some(args),
        _ => None,
    }
}

fn positional_call(expression: &Expr, order: &[usize]) -> Expr {
    let mut result = expression.clone();
    let source = arguments(expression).unwrap();
    let reordered = order
        .iter()
        .map(|&index| match &source[index] {
            Expr::NamedArgument { value, .. } => (**value).clone(),
            value => value.clone(),
        })
        .collect();
    match &mut result {
        Expr::FunctionCall { args, .. } | Expr::MethodCall { args, .. } => *args = reordered,
        _ => unreachable!(),
    }
    result
}
