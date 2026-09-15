use crate::common::diagnostics::Diagnostics;
use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName, TypeParamName, Variance};
use crate::parser::ast::Expr;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

use super::Inference;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;

impl Inference<'_> {
    pub(super) fn infer_async_do(&mut self, body: &Expr, span: &Span) -> TypedExpr {
        // Nested computations never contribute awaits to their parent's discovery.
        let discovery = self.async_discovery.take();
        let result = self.infer_async_computation(body, span);
        self.async_discovery = discovery;
        result
    }

    fn infer_async_computation(&mut self, body: &Expr, span: &Span) -> TypedExpr {
        let Some(context) = self.async_expression_context(body, span) else {
            return self.error_expr(span);
        };
        let expected = self
            .expected_type
            .replace(Type::Function(vec![], Box::new(context.clone())));
        let closure = self.infer_closure(true, &[], body, span);
        self.expected_type = expected;

        // Keep the async closure boundary for await lowering, and wrap it
        // explicitly so ByName coercion does not introduce a second thunk.
        let deferred = TypedExpr {
            ty: Type::GenericNewtype {
                fqn: Fqn::from_dotted("standard.prelude.ByName").unwrap(),
                type_args: vec![(Variance::Covariant, context.clone())],
                concrete_inner_type: Box::new(closure.ty.clone()),
            },
            kind: TypedExprKind::NewtypeCreate {
                value: Box::new(closure),
            },
            span: span.clone(),
        };

        let trace = async_source_location(span);
        let awaitable = Fqn::from_dotted("standard.prelude.Awaitable").unwrap();
        let Some((method, return_type)) = self.resolve_trait_impl_method_for_type(
            &context,
            &awaitable,
            "defer",
            &[&deferred.ty, &trace.ty],
        ) else {
            self.diagnostics.error(
                span.clone(),
                format!("cannot resolve Awaitable.defer for '{context}'"),
            );
            return self.error_expr(span);
        };
        TypedExpr {
            kind: TypedExprKind::ImplFunctionCall {
                trait_fqn: method.trait_fqn,
                trait_type_params: method.trait_type_params,
                for_type: method.for_type,
                method_name: method.method_name,
                args: vec![deferred, trace],
                method_type_params: method.method_type_params,
            },
            ty: return_type,
            span: span.clone(),
        }
    }

    fn async_expression_context(&mut self, body: &Expr, span: &Span) -> Option<Type> {
        let expected = self.expected_type.clone();
        let expected_inner = expected
            .as_ref()
            .and_then(|ty| self.resolve_awaitable_value_type(ty));
        if let (Some(context), Some(_)) = (&expected, &expected_inner)
            && !context.contains_type_variable()
            && !self
                .unresolved_method_type_params
                .iter()
                .any(|name| context.contains_type_parameter_named(name))
        {
            return expected;
        }
        let (success, operands) = self.discover_async_body(body, expected_inner.as_ref());
        if success.is_error() {
            self.diagnostics.error(
                span.clone(),
                "cannot infer async do body; add an Awaitable type annotation",
            );
            return None;
        }
        if let (Some(context), Some(inner)) = (expected, expected_inner) {
            let mut substitution = TypeParamSubstitution::new();
            substitution.unify(&inner, &success);
            let partially_bound = apply_substitution(&substitution, &context);
            if partially_bound.contains_type_variable() {
                let inferred = self.combine_async_operands(&operands, &success, span)?;
                substitution.unify(&context, &inferred);
            }
            let resolved = apply_substitution(&substitution, &context);
            if resolved.contains_type_variable() {
                self.diagnostics.error(
                    span.clone(),
                    "cannot infer async do Awaitable parameters; add a type annotation",
                );
                return None;
            }
            return Some(resolved);
        }
        self.combine_async_operands(&operands, &success, span)
    }

    fn combine_async_operands(
        &mut self,
        operands: &[Type],
        success: &Type,
        span: &Span,
    ) -> Option<Type> {
        let mut context = None;
        for operand in operands {
            let Some(candidate) = self.rebind_awaitable(operand, success) else {
                self.diagnostics.error(
                    span.clone(),
                    format!("cannot resolve Awaitable.Rebind for '{operand}'"),
                );
                return None;
            };
            context = match context {
                None => Some(candidate),
                Some(previous) => match self.least_upper_bound(&previous, &candidate) {
                    Some(combined) => Some(combined),
                    None => {
                        self.diagnostics.error(span.clone(), format!(
                            "incompatible Awaitable contexts '{previous}' and '{candidate}' in async do; add an explicit type annotation"
                        ));
                        return None;
                    }
                },
            };
        }
        let Some(context) = context else {
            self.diagnostics.error(
                span.clone(),
                "async do requires an Awaitable type context when no await determines its type",
            );
            return None;
        };
        if self.resolve_awaitable_value_type(&context).as_ref() != Some(success) {
            self.diagnostics.error(
                span.clone(),
                format!("inferred async do type '{context}' must implement Awaitable<{success}>"),
            );
            return None;
        }
        Some(context)
    }

    /// Run discovery in a separate inference state so speculative scopes,
    /// generated functions, type references and diagnostics cannot escape.
    fn discover_async_body(&self, body: &Expr, expected_inner: Option<&Type>) -> (Type, Vec<Type>) {
        let mut diagnostics = Diagnostics::new();
        let mut discovery = Inference::new(
            self.package_path.clone(),
            self.current_file.clone(),
            self.registry,
            self.import_scope,
            &mut diagnostics,
        );
        discovery.scopes = self.scopes.clone();
        discovery.current_type_params = self.current_type_params.clone();
        discovery.type_param_counter = self.type_param_counter;
        discovery.class_type_defs = self.class_type_defs.clone();
        discovery.container_name = self.container_name.clone();
        discovery.current_module_name = self.current_module_name.clone();
        discovery.typechecking_class = self.typechecking_class.clone();
        discovery.unresolved_method_type_params = self.unresolved_method_type_params.clone();
        discovery.async_discovery = Some(Vec::new());
        let unresolved = Type::TypeVariable(TypeParamName("$asyncContext".into()), vec![]);
        discovery.async_return_type = Some(unresolved.clone());
        discovery.function_return_type = Some(unresolved);
        // Preserve known structure (for example a closure's parameter types)
        // even when its result still contains inference variables. Individual
        // expression handlers discard only the unresolved portions they cannot use.
        discovery.expected_type = expected_inner.cloned();
        discovery.push_scope();
        let success = discovery.infer_expr(body).ty;
        (success, discovery.async_discovery.take().unwrap())
    }

    /// Resolve the trait's map result to project Rebind without assuming a
    /// wrapper name, success-parameter position, or an error type parameter.
    pub(super) fn rebind_awaitable(&mut self, context: &Type, success: &Type) -> Option<Type> {
        let inner = self.resolve_awaitable_value_type(context)?;
        let callback = Type::Function(vec![inner], Box::new(success.clone()));
        let trace = source_location_type();
        let awaitable = Fqn::from_dotted("standard.prelude.Awaitable").unwrap();
        self.resolve_trait_impl_method_for_type(context, &awaitable, "map", &[&callback, &trace])
            .map(|(_, rebound)| rebound)
    }
}

fn source_location_type() -> Type {
    let fqn = Fqn::from_dotted("standard.prelude.SourceLocation").unwrap();
    Type::Record(fqn.clone(), MangledName::for_type(&fqn))
}

fn async_source_location(span: &Span) -> TypedExpr {
    let string = |value: String| TypedExpr {
        kind: TypedExprKind::StringLiteral(value),
        ty: Type::String,
        span: span.clone(),
    };
    let number = |value: usize| TypedExpr {
        kind: TypedExprKind::Int32Literal(value as i32),
        ty: Type::Int32,
        span: span.clone(),
    };
    TypedExpr {
        kind: TypedExprKind::RecordCreate {
            fqn: Fqn::from_dotted("standard.prelude.SourceLocation").unwrap(),
            fields: vec![
                ("file".into(), string(span.file.to_string())),
                ("line".into(), number(span.line as usize)),
                ("column".into(), number(span.column as usize)),
                ("functionName".into(), string("<async do>".into())),
            ],
            type_params: vec![],
        },
        ty: source_location_type(),
        span: span.clone(),
    }
}
