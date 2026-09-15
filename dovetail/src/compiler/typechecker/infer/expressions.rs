use crate::common::span::{Span, Spanned};
use crate::common::types::{
    Fqn, InterfaceMemberName, MangledName, PackagePath, SymbolName, VarName, Visibility,
};
use crate::parser::ast::{BinOp, Expr, TypeExpr, UnaryOp};

use crate::typechecker::registry::{
    ExtMethodSignature, ExtensionBlockSignature, FunctionSignature, VariantPayload,
};
use crate::typechecker::types::{
    InterfaceComponent, IntrinsicKind, NamedTraitBound, ResolvedImplMethod, TraitBound, Type,
    TypeDef, TypedExpr, TypedExprKind, TypedMatchArm, TypedPattern,
};

use super::Inference;
use super::function_expressions::resolve_intrinsic_kind;
use super::generics::apply_substitution;

impl Inference<'_> {
    pub(super) fn infer_expr(&mut self, expr: &Expr) -> TypedExpr {
        let previous = self.current_expr_span.replace(expr.span());
        let typed = self.infer_expr_inner(expr);
        if previous.is_none() {
            self.check_generic_use_continuations(std::slice::from_ref(&typed), &typed.ty);
        }
        self.current_expr_span = previous;
        typed
    }

    fn infer_expr_inner(&mut self, expr: &Expr) -> TypedExpr {
        match expr {
            Expr::PrefixedLiteral {
                prefix,
                parts,
                span,
            } => self.infer_prefixed_literal(prefix, parts, span),
            Expr::ResolvedTypeRef(fqn, span) => {
                // Only ever legal as a method-call receiver, where step 0 of
                // `infer_method_call` consumes it before the receiver is
                // inferred as a value.
                self.diagnostics.error(
                    span.clone(),
                    format!("internal error: type reference `{fqn}` used as a value"),
                );
                TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                }
            }
            Expr::UnitLiteral(span) => TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Unit,
                span: span.clone(),
            },
            Expr::BoolLiteral(val, span) => TypedExpr {
                kind: TypedExprKind::BoolLiteral(*val),
                ty: Type::Bool,
                span: span.clone(),
            },
            Expr::StringLiteral(s, span) => TypedExpr {
                kind: TypedExprKind::StringLiteral(s.clone()),
                ty: Type::String,
                span: span.clone(),
            },
            Expr::CharLiteral(c, span) => TypedExpr {
                kind: TypedExprKind::CharLiteral(*c),
                ty: Type::Char,
                span: span.clone(),
            },

            // Signed integer literals
            Expr::Int8Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Int8Literal(*v),
                ty: Type::Int8,
                span: span.clone(),
            },
            Expr::Int16Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Int16Literal(*v),
                ty: Type::Int16,
                span: span.clone(),
            },
            Expr::Int32Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Int32Literal(*v),
                ty: Type::Int32,
                span: span.clone(),
            },
            Expr::Int64Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Int64Literal(*v),
                ty: Type::Int64,
                span: span.clone(),
            },

            // Unsigned integer literals
            Expr::Uint8Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Uint8Literal(*v),
                ty: Type::Uint8,
                span: span.clone(),
            },
            Expr::Uint16Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Uint16Literal(*v),
                ty: Type::Uint16,
                span: span.clone(),
            },
            Expr::Uint32Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Uint32Literal(*v),
                ty: Type::Uint32,
                span: span.clone(),
            },
            Expr::Uint64Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Uint64Literal(*v),
                ty: Type::Uint64,
                span: span.clone(),
            },
            Expr::ExactNumberLiteral(text, span) => self.infer_exact_number(text, span),
            Expr::Uint128Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Uint128Literal(*v),
                ty: Type::Uint128,
                span: span.clone(),
            },

            // Float literals
            Expr::Float32Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Float32Literal(*v),
                ty: Type::Float32,
                span: span.clone(),
            },
            Expr::Float64Literal(v, span) => TypedExpr {
                kind: TypedExprKind::Float64Literal(*v),
                ty: Type::Float64,
                span: span.clone(),
            },

            // Binary operator
            Expr::BinaryOp {
                op,
                left,
                right,
                span,
            } => {
                let typed_left = self.infer_expr(left);
                // The left operand's type is the expectation for the right one.
                // Most binary operators want both sides at the same type, and
                // without this a literal with no type of its own — `xs == []`,
                // whose `[]` would otherwise be `List<Never>` — fails the
                // same-type guard below.
                let saved_expected = self.expected_type.take();
                self.expected_type = if typed_left.ty.is_error() {
                    None
                } else {
                    self.operator_rhs_expectation(*op, &typed_left.ty)
                };
                let typed_right = self.infer_expr(right);
                self.expected_type = saved_expected;

                // Skip checking if either side is Error
                if typed_left.ty.is_error() || typed_right.ty.is_error() {
                    return TypedExpr {
                        kind: TypedExprKind::BinaryOp {
                            op: *op,
                            left: Box::new(typed_left),
                            right: Box::new(typed_right),
                        },
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                if *op == BinOp::TupleExtend {
                    let ty = Type::tuple_extend(typed_left.ty.clone(), typed_right.ty.clone());
                    return crate::typechecker::tuple_extension::lower(TypedExpr {
                        kind: TypedExprKind::BinaryOp {
                            op: *op,
                            left: Box::new(typed_left),
                            right: Box::new(typed_right),
                        },
                        ty,
                        span: span.clone(),
                    });
                }

                let operand_ty = &typed_left.ty;

                // Operator traits select an implementation before any native type checks.
                if let Some(lowered) = self.try_lower_op_to_trait(
                    *op,
                    operand_ty,
                    typed_left.clone(),
                    typed_right.clone(),
                    span,
                ) {
                    return lowered;
                }

                // Trait calls can have asymmetric operands. Built-in operations and
                // failed trait applications require matching operand types here.
                if typed_left.ty != typed_right.ty
                    && !typed_left.ty.is_never()
                    && !typed_right.ty.is_never()
                {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "binary '{}' requires operands of the same type, found '{}' and '{}'",
                            op, typed_left.ty, typed_right.ty
                        ),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::BinaryOp {
                            op: *op,
                            left: Box::new(typed_left),
                            right: Box::new(typed_right),
                        },
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                let result_ty = self.check_binary_op(*op, operand_ty, span.clone());

                TypedExpr {
                    kind: TypedExprKind::BinaryOp {
                        op: *op,
                        left: Box::new(typed_left),
                        right: Box::new(typed_right),
                    },
                    ty: result_ty,
                    span: span.clone(),
                }
            }

            // Unary operator
            Expr::UnaryOp { op, operand, span } => {
                let typed_operand = self.infer_expr(operand);

                if typed_operand.ty.is_error() {
                    return TypedExpr {
                        kind: TypedExprKind::UnaryOp {
                            op: *op,
                            operand: Box::new(typed_operand),
                        },
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                if *op == UnaryOp::Neg
                    && !typed_operand.ty.is_numeric()
                    && let Some(result) = self.lower_library_negation(&typed_operand, span)
                {
                    return result;
                }
                let result_ty = self.check_unary_op(*op, &typed_operand.ty, span.clone());

                TypedExpr {
                    kind: TypedExprKind::UnaryOp {
                        op: *op,
                        operand: Box::new(typed_operand),
                    },
                    ty: result_ty,
                    span: span.clone(),
                }
            }

            Expr::Block(block) => {
                self.push_scope();
                let saved_expected = self.expected_type.clone();
                // If the block's expected type is async-shaped (`Async<_, E>`),
                // expose its E to nested `use` expressions for From auto-conversion.
                let saved_wrapped_err = self.block_wrapped_error.clone();
                if let Some(exp) = &saved_expected
                    && let Some(err) = self.resolve_awaitable_error_type(exp)
                {
                    self.block_wrapped_error = Some(err);
                }
                let len = block.expressions.len();
                let typed_exprs: Vec<TypedExpr> = block
                    .expressions
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        if i + 1 < len {
                            // Non-last expressions: clear expected_type
                            self.expected_type = None;
                        } else {
                            // Last expression: restore parent expected_type
                            self.expected_type = saved_expected.clone();
                        }
                        self.infer_expr(e)
                    })
                    .collect();
                self.expected_type = saved_expected;
                self.block_wrapped_error = saved_wrapped_err;
                self.pop_scope();
                let ty = typed_exprs
                    .last()
                    .map(|e| e.ty.clone())
                    .unwrap_or(Type::Unit);
                self.check_generic_use_continuations(&typed_exprs, &ty);
                TypedExpr {
                    kind: TypedExprKind::Block(typed_exprs),
                    ty,
                    span: block.span.clone(),
                }
            }
            Expr::Panic { message, span } => {
                let typed_message = self.infer_expr(message);
                self.check_assignable(typed_message.span.clone(), &Type::String, &typed_message.ty);
                TypedExpr {
                    kind: TypedExprKind::Panic {
                        message: Box::new(typed_message),
                    },
                    ty: Type::Never,
                    span: span.clone(),
                }
            }
            Expr::Assert {
                condition,
                message,
                span,
            } => {
                let typed_condition = self.infer_expr(condition);
                self.check_assignable(
                    typed_condition.span.clone(),
                    &Type::Bool,
                    &typed_condition.ty,
                );
                let typed_message = message.as_ref().map(|msg| {
                    let typed_msg = self.infer_expr(msg);
                    self.check_assignable(typed_msg.span.clone(), &Type::String, &typed_msg.ty);
                    Box::new(typed_msg)
                });
                TypedExpr {
                    kind: TypedExprKind::Assert {
                        condition: Box::new(typed_condition),
                        message: typed_message,
                    },
                    ty: Type::Unit,
                    span: span.clone(),
                }
            }

            Expr::Let {
                name,
                mutable,
                type_annotation,
                value,
                span,
            } => {
                // Resolve type annotation first to provide expected_type hint
                let annotated_ty = type_annotation
                    .as_ref()
                    .map(|ta| self.resolve_type_expr(ta));

                // Set expected type before inferring value (enables inference for e.g. Array.empty())
                let prev_expected = self.expected_type.take();
                self.expected_type = annotated_ty.clone();
                let typed_value = self.infer_expr(value);
                self.expected_type = prev_expected;

                let var_ty = if let Some(expected) = annotated_ty {
                    self.check_assignable(typed_value.span.clone(), &expected, &typed_value.ty);
                    expected
                } else {
                    typed_value.ty.clone()
                };

                let var_name = VarName(name.value.clone());
                self.define_variable(var_name.clone(), var_ty.clone(), *mutable);

                TypedExpr {
                    kind: TypedExprKind::Let {
                        name: var_name,
                        mutable: *mutable,
                        boxed: false,
                        var_ty,
                        value: Box::new(typed_value),
                    },
                    ty: Type::Unit,
                    span: span.clone(),
                }
            }

            Expr::Identifier(name, span) => {
                if let Some(binding) = self.lookup_variable(name) {
                    TypedExpr {
                        kind: TypedExprKind::VarRef {
                            name: VarName(name.clone()),
                            boxed: false,
                        },
                        ty: binding.ty,
                        span: span.clone(),
                    }
                } else if let Some((mangled, ty, _mutable, global_type_args)) =
                    self.lookup_global(name)
                {
                    TypedExpr {
                        kind: TypedExprKind::GlobalRef {
                            name: mangled,
                            type_params: global_type_args,
                        },
                        ty,
                        span: span.clone(),
                    }
                } else if let Some(result) = self.try_resolve_bare_module_property(name, span) {
                    result
                } else if let Some(result) = self.try_resolve_bare_variant_identifier(name, span) {
                    result
                } else if let Some(result) = self.try_resolve_function_ref(name, span) {
                    result
                } else {
                    let msg = if let Some(fqn) = self.registry.suggest_import_for_global(name) {
                        format!(
                            "undefined variable: '{}'; try adding 'import {}'",
                            name, fqn
                        )
                    } else {
                        format!("undefined variable: '{}'", name)
                    };
                    self.diagnostics.error(span.clone(), msg);
                    TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    }
                }
            }

            Expr::Assignment {
                target,
                value,
                span,
            } => {
                // Handle array index assignment: arr[i] = v
                if let Expr::Index { object, index, .. } = target.as_ref() {
                    let typed_object = self.infer_expr(object);
                    let typed_index = self.infer_expr(index);
                    let typed_value = self.infer_expr(value);
                    return match &typed_object.ty {
                        Type::Array(elem) => {
                            self.check_assignable(
                                typed_index.span.clone(),
                                &Type::Int32,
                                &typed_index.ty,
                            );
                            self.check_assignable(typed_value.span.clone(), elem, &typed_value.ty);
                            TypedExpr {
                                kind: TypedExprKind::IntrinsicCall {
                                    intrinsic: IntrinsicKind::ArraySet,
                                    args: vec![typed_object, typed_index, typed_value],
                                },
                                ty: Type::Unit,
                                span: span.clone(),
                            }
                        }
                        Type::Error => TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        },
                        _ => self.infer_trait_index(
                            typed_object,
                            typed_index,
                            Some(typed_value),
                            span,
                        ),
                    };
                }

                // Handle module-qualified global assignment: Module.global = value
                // or Box<Int32>.count = value
                if let Expr::FieldAccess {
                    object,
                    object_type_params,
                    field,
                    ..
                } = target.as_ref()
                {
                    if let Expr::Identifier(name, _) = object.as_ref() {
                        if let Some(module_info) = self.resolve_module_name(name).cloned() {
                            let member_sym = SymbolName(field.value.clone());
                            // Check concrete globals (only for non-generic access)
                            if object_type_params.is_empty()
                                && let Some(sig) = module_info.globals.get(&member_sym)
                            {
                                if sig.visibility == Visibility::Private
                                    && sig.source_file != self.current_file
                                {
                                    self.diagnostics.error(
                                        span.clone(),
                                        format!(
                                            "no member '{}' found in module '{}'",
                                            field.value, name
                                        ),
                                    );
                                } else if !sig.mutable {
                                    self.diagnostics.error(
                                        span.clone(),
                                        format!(
                                            "cannot assign to immutable global '{}.{}'",
                                            name, field.value
                                        ),
                                    );
                                }
                                let prev_expected = self.expected_type.take();
                                self.expected_type = Some(sig.ty.clone());
                                let typed_value = self.infer_expr(value);
                                self.expected_type = prev_expected;
                                self.check_assignable(
                                    typed_value.span.clone(),
                                    &sig.ty,
                                    &typed_value.ty,
                                );
                                return TypedExpr {
                                    kind: TypedExprKind::GlobalAssign {
                                        name: sig.mangled_name.clone(),
                                        type_params: vec![],
                                        value: Box::new(typed_value),
                                    },
                                    ty: Type::Unit,
                                    span: span.clone(),
                                };
                            }
                            // Check generic globals (with explicit type args or bidirectional inference)
                            if let Some((mangled, ty, mutable, global_type_args)) = self
                                .resolve_generic_module_global(
                                    &module_info,
                                    &member_sym,
                                    object_type_params,
                                )
                            {
                                if !mutable {
                                    self.diagnostics.error(
                                        span.clone(),
                                        format!(
                                            "cannot assign to immutable global '{}.{}'",
                                            name, field.value
                                        ),
                                    );
                                }
                                let prev_expected = self.expected_type.take();
                                self.expected_type = Some(ty.clone());
                                let typed_value = self.infer_expr(value);
                                self.expected_type = prev_expected;
                                self.check_assignable(
                                    typed_value.span.clone(),
                                    &ty,
                                    &typed_value.ty,
                                );
                                return TypedExpr {
                                    kind: TypedExprKind::GlobalAssign {
                                        name: mangled,
                                        type_params: global_type_args,
                                        value: Box::new(typed_value),
                                    },
                                    ty: Type::Unit,
                                    span: span.clone(),
                                };
                            }
                        }

                        // Try class static global assignment: ClassName.field = value
                        if let Some(
                            Type::Class(class_fqn, _) | Type::GenericClass { fqn: class_fqn, .. },
                        ) = self.resolve_type_name(name, &[], span)
                        {
                            let global_fqn = Fqn {
                                package: class_fqn.package.clone(),
                                symbol: SymbolName(format!("{}.{}", class_fqn.symbol, field.value)),
                            };
                            // Concrete static global
                            if let Some(sig) = self
                                .registry
                                .lookup_global(&global_fqn, &self.package_path, &self.current_file)
                                .cloned()
                            {
                                if !self.check_class_field_visibility(
                                    sig.visibility,
                                    &class_fqn,
                                    span,
                                ) {
                                    let _typed_value = self.infer_expr(value);
                                    return TypedExpr {
                                        kind: TypedExprKind::UnitLiteral,
                                        ty: Type::Error,
                                        span: span.clone(),
                                    };
                                }
                                if !sig.mutable {
                                    self.diagnostics.error(
                                        span.clone(),
                                        format!(
                                            "cannot assign to immutable static field '{}.{}'",
                                            name, field.value
                                        ),
                                    );
                                }
                                let prev_expected = self.expected_type.take();
                                self.expected_type = Some(sig.ty.clone());
                                let typed_value = self.infer_expr(value);
                                self.expected_type = prev_expected;
                                self.check_assignable(
                                    typed_value.span.clone(),
                                    &sig.ty,
                                    &typed_value.ty,
                                );
                                return TypedExpr {
                                    kind: TypedExprKind::GlobalAssign {
                                        name: sig.mangled_name.clone(),
                                        type_params: vec![],
                                        value: Box::new(typed_value),
                                    },
                                    ty: Type::Unit,
                                    span: span.clone(),
                                };
                            }
                            // Generic static global
                            if let Some(class_sig) = self
                                .registry
                                .lookup_class_type(&class_fqn, &self.package_path)
                                .cloned()
                            {
                                let member_sym = SymbolName(field.value.clone());
                                if let Some(def) =
                                    class_sig.generic_static_globals.get(&member_sym).cloned()
                                {
                                    let type_args = if !object_type_params.is_empty() {
                                        if object_type_params.len() == def.type_params.len() {
                                            self.resolve_type_args(object_type_params)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    };
                                    if let Some(type_args) = type_args {
                                        let substitution = super::type_param_substitution::TypeParamSubstitution::from_pairs(&def.type_params, &type_args);
                                        let concrete_ty = if let Some(ref ty) = def.ty {
                                            apply_substitution(&substitution, ty)
                                        } else {
                                            Type::Error
                                        };
                                        let effective_fqn = Fqn {
                                            package: class_fqn.package.clone(),
                                            symbol: SymbolName(format!(
                                                "{}.{}",
                                                class_fqn.symbol, field.value
                                            )),
                                        };
                                        // Statics on generic classes share one storage across all instantiations
                                        // (the rules pass forbids the declared type from referencing the class's
                                        // type parameters, so every `Box<T>.field` resolves to the same global).
                                        let mangled = MangledName::for_global(&effective_fqn);

                                        if !self.check_class_field_visibility(
                                            def.visibility,
                                            &class_fqn,
                                            span,
                                        ) {
                                            let _typed_value = self.infer_expr(value);
                                            return TypedExpr {
                                                kind: TypedExprKind::UnitLiteral,
                                                ty: Type::Error,
                                                span: span.clone(),
                                            };
                                        }
                                        if !def.mutable {
                                            self.diagnostics.error(
                                                span.clone(),
                                                format!(
                                                    "cannot assign to immutable static field '{}.{}'",
                                                    name, field.value
                                                ),
                                            );
                                        }
                                        let prev_expected = self.expected_type.take();
                                        self.expected_type = Some(concrete_ty.clone());
                                        let typed_value = self.infer_expr(value);
                                        self.expected_type = prev_expected;
                                        self.check_assignable(
                                            typed_value.span.clone(),
                                            &concrete_ty,
                                            &typed_value.ty,
                                        );
                                        let _ = type_args;
                                        return TypedExpr {
                                            kind: TypedExprKind::GlobalAssign {
                                                name: mangled,
                                                type_params: vec![],
                                                value: Box::new(typed_value),
                                            },
                                            ty: Type::Unit,
                                            span: span.clone(),
                                        };
                                    }
                                }
                            }
                        }
                    }

                    // Class field assignment: self.field = value
                    let typed_object = self.infer_expr(object);
                    let (fqn, class_mangled) = match &typed_object.ty {
                        Type::Class(fqn, _mn) => (fqn.clone(), MangledName::for_type(fqn)),
                        Type::GenericClass {
                            fqn, mangled_name, ..
                        } => (fqn.clone(), mangled_name.clone()),
                        _ => {
                            self.diagnostics
                                .error(span.clone(), "invalid assignment target".to_string());
                            let _typed_value = self.infer_expr(value);
                            return TypedExpr {
                                kind: TypedExprKind::UnitLiteral,
                                ty: Type::Error,
                                span: span.clone(),
                            };
                        }
                    };

                    // Check typechecking_class first, then ClassTypeDef
                    let tc_class_fields = self
                        .typechecking_class
                        .as_ref()
                        .filter(|(mn, _)| {
                            *mn == class_mangled || *mn == MangledName::for_type(&fqn)
                        })
                        .map(|(_, fields)| fields.clone());

                    if fqn.package == self.package_path
                        || matches!(&typed_object.ty, Type::GenericClass { .. })
                    {
                        let fields = tc_class_fields.or_else(|| {
                            let cls_lookup = self
                                .class_type_defs
                                .get(&class_mangled)
                                .or_else(|| self.class_type_defs.get(&MangledName::for_type(&fqn)));
                            if let Some(TypeDef::Class(cls)) = cls_lookup {
                                if cls.type_params.is_empty() {
                                    Some(cls.fields.clone())
                                } else {
                                    // Template ClassTypeDef — apply type arg substitution to fields
                                    self.substitute_template_class_fields(cls, &typed_object.ty)
                                }
                            } else {
                                None
                            }
                        });
                        if let Some(fields) = fields {
                            for (idx, f) in fields.iter().enumerate() {
                                if f.name == field.value {
                                    if !self.is_field_accessible(f.visibility, &f.declared_by) {
                                        self.check_class_field_visibility(
                                            f.visibility,
                                            &f.declared_by,
                                            span,
                                        );
                                        return TypedExpr {
                                            kind: TypedExprKind::UnitLiteral,
                                            ty: Type::Error,
                                            span: span.clone(),
                                        };
                                    }
                                    if !f.mutable {
                                        self.diagnostics.error(
                                            span.clone(),
                                            format!(
                                                "cannot assign to immutable field '{}'",
                                                field.value
                                            ),
                                        );
                                    }
                                    let prev_expected = self.expected_type.take();
                                    self.expected_type = Some(f.ty.clone());
                                    let typed_value = self.infer_expr(value);
                                    self.expected_type = prev_expected;
                                    self.check_assignable(
                                        typed_value.span.clone(),
                                        &f.ty,
                                        &typed_value.ty,
                                    );
                                    return TypedExpr {
                                        kind: TypedExprKind::FieldAssign {
                                            object: Box::new(typed_object),
                                            field_name: field.value.clone(),
                                            field_index: idx as u32,
                                            value: Box::new(typed_value),
                                            boxed: false,
                                        },
                                        ty: Type::Unit,
                                        span: span.clone(),
                                    };
                                }
                            }
                        }
                    }

                    // Cross-package: use registry
                    if let Some(class_sig) = self
                        .registry
                        .lookup_class_type(&fqn, &self.package_path)
                        .cloned()
                    {
                        // Build type param substitution for generic classes
                        let substitution = if let Type::GenericClass { type_args, .. } =
                            &typed_object.ty
                        {
                            if !class_sig.type_params.is_empty() {
                                let concrete_types: Vec<Type> =
                                    type_args.iter().map(|(_, t)| t.clone()).collect();
                                Some(super::type_param_substitution::TypeParamSubstitution::from_pairs(&class_sig.type_params, &concrete_types))
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        let has_variance = class_sig
                            .type_param_variances
                            .iter()
                            .any(|v| *v != crate::common::types::Variance::Invariant);
                        for (idx, f) in class_sig.fields.iter().enumerate() {
                            if f.name == field.value {
                                if !self.check_class_field_visibility(f.visibility, &fqn, span) {
                                    return TypedExpr {
                                        kind: TypedExprKind::UnitLiteral,
                                        ty: Type::Error,
                                        span: span.clone(),
                                    };
                                }
                                if !f.mutable {
                                    self.diagnostics.error(
                                        span.clone(),
                                        format!(
                                            "cannot assign to immutable field '{}'",
                                            field.value
                                        ),
                                    );
                                }
                                let field_ty = if let Some(ref sub) = substitution {
                                    super::generics::apply_substitution(sub, &f.ty)
                                } else {
                                    f.ty.clone()
                                };
                                let boxed = has_variance && f.mutable;
                                let prev_expected = self.expected_type.take();
                                self.expected_type = Some(field_ty.clone());
                                let typed_value = self.infer_expr(value);
                                self.expected_type = prev_expected;
                                self.check_assignable(
                                    typed_value.span.clone(),
                                    &field_ty,
                                    &typed_value.ty,
                                );
                                return TypedExpr {
                                    kind: TypedExprKind::FieldAssign {
                                        object: Box::new(typed_object),
                                        field_name: field.value.clone(),
                                        field_index: idx as u32,
                                        value: Box::new(typed_value),
                                        boxed,
                                    },
                                    ty: Type::Unit,
                                    span: span.clone(),
                                };
                            }
                        }
                    }

                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "no field '{}' found on type {}",
                            field.value, typed_object.ty
                        ),
                    );
                    let _typed_value = self.infer_expr(value);
                    return TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                let name = match target.as_ref() {
                    Expr::Identifier(name, _) => name,
                    _ => {
                        self.diagnostics
                            .error(span.clone(), "invalid assignment target".to_string());
                        let _typed_value = self.infer_expr(value);
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                };

                // Check local variables first
                if let Some(binding) = self.lookup_variable(name) {
                    if !binding.mutable {
                        self.diagnostics.error(
                            span.clone(),
                            format!("cannot assign to immutable variable '{}'", name),
                        );
                    }
                    let prev_expected = self.expected_type.take();
                    self.expected_type = Some(binding.ty.clone());
                    let typed_value = self.infer_expr(value);
                    self.expected_type = prev_expected;
                    self.check_assignable(typed_value.span.clone(), &binding.ty, &typed_value.ty);
                    TypedExpr {
                        kind: TypedExprKind::Assign {
                            name: VarName(name.clone()),
                            target_ty: binding.ty.clone(),
                            boxed: false,
                            value: Box::new(typed_value),
                        },
                        ty: Type::Unit,
                        span: span.clone(),
                    }
                } else if let Some((mangled, ty, mutable, global_type_args)) =
                    self.lookup_global(name)
                {
                    // Global variable assignment
                    if !mutable {
                        self.diagnostics.error(
                            span.clone(),
                            format!("cannot assign to immutable global '{}'", name),
                        );
                    }
                    let prev_expected = self.expected_type.take();
                    self.expected_type = Some(ty.clone());
                    let typed_value = self.infer_expr(value);
                    self.expected_type = prev_expected;
                    self.check_assignable(typed_value.span.clone(), &ty, &typed_value.ty);
                    TypedExpr {
                        kind: TypedExprKind::GlobalAssign {
                            name: mangled,
                            type_params: global_type_args,
                            value: Box::new(typed_value),
                        },
                        ty: Type::Unit,
                        span: span.clone(),
                    }
                } else {
                    let msg = if let Some(fqn) = self.registry.suggest_import_for_global(name) {
                        format!(
                            "undefined variable: '{}'; try adding 'import {}'",
                            name, fqn
                        )
                    } else {
                        format!("undefined variable: '{}'", name)
                    };
                    self.diagnostics.error(span.clone(), msg);
                    let _typed_value = self.infer_expr(value);
                    TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    }
                }
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
                span,
            } => {
                let parent_expected = self.expected_type.clone();

                // Set expected Bool for condition
                self.expected_type = Some(Type::Bool);
                let typed_condition = self.infer_expr(condition);
                self.expected_type = parent_expected.clone();
                self.check_assignable(
                    typed_condition.span.clone(),
                    &Type::Bool,
                    &typed_condition.ty,
                );

                // Propagate parent expected to then branch
                self.expected_type = parent_expected.clone();
                let typed_then = self.infer_expr(then_branch);

                if let Some(else_expr) = else_branch {
                    // Propagate parent expected to else branch
                    self.expected_type = parent_expected.clone();
                    let mut typed_else = self.infer_expr(else_expr);
                    self.expected_type = parent_expected;

                    // Determine result type, accounting for Never and Error
                    let result_ty = if typed_then.ty.is_error() || typed_else.ty.is_error() {
                        Type::Error
                    } else if typed_then.ty.is_never() {
                        typed_else.ty.clone()
                    } else if let Some(expected @ Type::InterfaceObject { .. }) = self
                        .expected_type
                        .as_ref()
                        .filter(|e| {
                            matches!(e, Type::InterfaceObject { .. })
                                && !typed_else.ty.is_never()
                                && self.is_assignable(e, &typed_then.ty)
                                && self.is_assignable(e, &typed_else.ty)
                        })
                        .cloned()
                    {
                        // Annotated interface-object context: use it (see
                        // match_expression.rs — pairwise unification could
                        // otherwise pick a sub-interface and reroute a
                        // concrete branch's dispatch).
                        expected
                    } else if typed_else.ty.is_never()
                        || self.is_assignable(&typed_then.ty, &typed_else.ty)
                    {
                        typed_then.ty.clone()
                    } else if self.is_assignable(&typed_else.ty, &typed_then.ty) {
                        typed_else.ty.clone()
                    } else if let Some(lub) = self.least_upper_bound(&typed_then.ty, &typed_else.ty)
                    {
                        lub
                    } else {
                        self.diagnostics.error(
                            typed_else.span.clone(),
                            format!(
                                "type mismatch: expected '{}', found '{}'",
                                typed_then.ty, typed_else.ty
                            ),
                        );
                        Type::Error
                    };

                    // Re-infer branches whose type differs from result_ty
                    // to ensure consistent WASM types (e.g. Option<Never> → Option<String>).
                    let mut typed_then = typed_then;
                    if !result_ty.is_error() && !result_ty.is_never() {
                        if typed_then.ty != result_ty
                            && !typed_then.ty.is_error()
                            && !typed_then.ty.is_never()
                            && self.is_assignable(&result_ty, &typed_then.ty)
                        {
                            let saved = self.expected_type.take();
                            self.expected_type = Some(result_ty.clone());
                            typed_then = self.infer_expr(then_branch);
                            self.expected_type = saved;
                        }
                        if typed_else.ty != result_ty
                            && !typed_else.ty.is_error()
                            && !typed_else.ty.is_never()
                            && self.is_assignable(&result_ty, &typed_else.ty)
                        {
                            let saved = self.expected_type.take();
                            self.expected_type = Some(result_ty.clone());
                            typed_else = self.infer_expr(else_expr);
                            self.expected_type = saved;
                        }
                    }

                    TypedExpr {
                        kind: TypedExprKind::If {
                            condition: Box::new(typed_condition),
                            then_branch: Box::new(typed_then),
                            else_branch: Some(Box::new(typed_else)),
                        },
                        ty: result_ty,
                        span: span.clone(),
                    }
                } else {
                    self.expected_type = parent_expected;
                    // No else branch: then-branch must be Unit
                    if !typed_then.ty.is_error() && !typed_then.ty.is_never() {
                        self.check_assignable(typed_then.span.clone(), &Type::Unit, &typed_then.ty);
                    }

                    TypedExpr {
                        kind: TypedExprKind::If {
                            condition: Box::new(typed_condition),
                            then_branch: Box::new(typed_then),
                            else_branch: None,
                        },
                        ty: Type::Unit,
                        span: span.clone(),
                    }
                }
            }

            Expr::While {
                condition,
                body,
                span,
            } => {
                let prev_expected = self.expected_type.take();

                self.expected_type = Some(Type::Bool);
                let typed_condition = self.infer_expr(condition);
                self.check_assignable(
                    typed_condition.span.clone(),
                    &Type::Bool,
                    &typed_condition.ty,
                );

                self.expected_type = Some(Type::Unit);
                self.loop_depth += 1;
                let typed_body = self.infer_expr(body);
                self.loop_depth -= 1;
                self.expected_type = prev_expected;
                self.check_async_loop(Some(&typed_condition), &typed_body);

                // Body must be Unit (or Never/Error)
                if !typed_body.ty.is_error() && !typed_body.ty.is_never() {
                    self.check_assignable(typed_body.span.clone(), &Type::Unit, &typed_body.ty);
                }

                TypedExpr {
                    kind: TypedExprKind::While {
                        condition: Box::new(typed_condition),
                        body: Box::new(typed_body),
                    },
                    ty: Type::Unit,
                    span: span.clone(),
                }
            }

            Expr::Break(span) => {
                if self.loop_depth == 0 {
                    self.diagnostics
                        .error(span.clone(), "break outside of loop".to_string());
                }
                TypedExpr {
                    kind: TypedExprKind::Break,
                    ty: Type::Never,
                    span: span.clone(),
                }
            }

            Expr::Continue(span) => {
                if self.loop_depth == 0 {
                    self.diagnostics
                        .error(span.clone(), "continue outside of loop".to_string());
                }
                TypedExpr {
                    kind: TypedExprKind::Continue,
                    ty: Type::Never,
                    span: span.clone(),
                }
            }

            Expr::For {
                pattern,
                iterable,
                body,
                span,
            } => self.infer_for_expr(pattern, iterable, body, span),

            Expr::Match {
                subject,
                arms,
                span,
            } => self.infer_match_expr(subject, arms, span),

            Expr::FunctionCall {
                name,
                type_args,
                args,
                span,
            } => self.infer_bare_function_call(name, type_args, args, span),

            Expr::MethodCall {
                receiver,
                method,
                receiver_type_args,
                type_args,
                args,
                span,
            } => {
                self.infer_method_call(receiver, method, receiver_type_args, type_args, args, span)
            }

            Expr::FieldAccess {
                object,
                object_type_params,
                field,
                field_type_params,
                span,
            } => {
                self.infer_field_access(object, object_type_params, field, field_type_params, span)
            }

            Expr::RecordWith {
                object,
                fields,
                span,
            } => self.infer_record_with(object, fields, span),

            Expr::ArrayLiteral { elements, span } => self.infer_array_literal(elements, span),
            Expr::ListLiteral {
                elements,
                tail,
                span,
            } => self.infer_list_literal(elements, tail.as_deref(), span),

            Expr::RecordCreate {
                type_name,
                type_args,
                fields,
                span,
            } => self.infer_record_create(type_name, type_args, fields, span),

            Expr::Index {
                object,
                index,
                span,
            } => {
                let typed_object = self.infer_expr(object);
                let typed_index = self.infer_expr(index);
                let obj_ty = typed_object.ty.clone();
                match obj_ty {
                    Type::Array(elem) => {
                        self.check_assignable(
                            typed_index.span.clone(),
                            &Type::Int32,
                            &typed_index.ty,
                        );
                        TypedExpr {
                            kind: TypedExprKind::IntrinsicCall {
                                intrinsic: IntrinsicKind::ArrayGet,
                                args: vec![typed_object, typed_index],
                            },
                            ty: *elem,
                            span: span.clone(),
                        }
                    }
                    Type::String => {
                        self.check_assignable(
                            typed_index.span.clone(),
                            &Type::Int32,
                            &typed_index.ty,
                        );
                        TypedExpr {
                            kind: TypedExprKind::IntrinsicCall {
                                intrinsic: IntrinsicKind::StringGetChar,
                                args: vec![typed_object, typed_index],
                            },
                            ty: Type::Char,
                            span: span.clone(),
                        }
                    }
                    Type::Error => TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    },
                    _ => self.infer_trait_index(typed_object, typed_index, None, span),
                }
            }

            Expr::SliceIndex {
                object,
                start,
                end,
                inclusive,
                span,
            } => self.infer_slice_index(object, start.as_deref(), end.as_deref(), *inclusive, span),

            Expr::TupleLiteral { elements, span } => self.infer_tuple_literal(elements, span),

            Expr::LetDestructure {
                pattern,
                type_annotation,
                value,
                span,
            } => self.infer_let_destructure(pattern, type_annotation.as_ref(), value, span),

            Expr::Try { operand, span } | Expr::OrReturn { operand, span } => {
                self.infer_try_expr(operand, span)
            }

            Expr::Await { operand, span } => self.infer_await_expr(operand, span),

            Expr::Use { operand, span } => self.infer_use_expr(operand, span),

            Expr::Intrinsic(span) => {
                // Should never reach inference — intrinsic methods are skipped
                self.diagnostics.error(
                    span.clone(),
                    "intrinsic body can only appear in extension methods".to_string(),
                );
                TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                }
            }

            Expr::EnumVariantRecordCreate {
                type_name,
                variant_name,
                fields,
                span,
            } => self.infer_enum_variant_record_create(type_name, variant_name, fields, span),

            Expr::TypeTest {
                expr: inner,
                target,
                span,
            } => {
                let typed_inner = self.infer_expr(inner);

                if !typed_inner.ty.is_any()
                    && !typed_inner.ty.is_class_type()
                    && !typed_inner.ty.is_error()
                {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "'is' type test requires subject of type Any or a class type, found '{}'",
                            typed_inner.ty
                        ),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::BoolLiteral(false),
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                let target_type = self.resolve_type_expr(target);
                if target_type.is_error() {
                    return TypedExpr {
                        kind: TypedExprKind::BoolLiteral(false),
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                if target_type.is_any() || target_type.is_never() {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "'is' type test target must be a concrete type, found '{}'",
                            target_type
                        ),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::BoolLiteral(false),
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }
                // An interface-object TARGET would test fat-pointer identity,
                // not "implements" — reject like the matching `as` gate
                // (interface objects do not support downcasts either way).
                if matches!(target_type, Type::InterfaceObject { .. }) {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "'is' cannot test interface type '{}'; interface objects carry no runtime type information — test the concrete type instead",
                            target_type
                        ),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::BoolLiteral(false),
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                // When subject is a class type, validate the target is in the same hierarchy
                if typed_inner.ty.is_class_type() && !target_type.is_error() {
                    if !target_type.is_class_type() {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "'is' with class subject requires a class target type, found '{}'",
                                target_type
                            ),
                        );
                        return TypedExpr {
                            kind: TypedExprKind::BoolLiteral(false),
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                    if typed_inner.ty.is_error() || target_type.is_error() {
                        return TypedExpr {
                            kind: TypedExprKind::BoolLiteral(false),
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                    let subject_fqn = typed_inner.ty.try_to_fqn();
                    let target_fqn = target_type.try_to_fqn();
                    if crate::typechecker::subtyping::is_subtype(
                        self.registry,
                        &typed_inner.ty,
                        &target_type,
                    ) {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "'is' type test on '{}' for '{}' is always true — the subject is already a subtype of the target",
                                typed_inner.ty, target_type
                            ),
                        );
                        return TypedExpr {
                            kind: TypedExprKind::BoolLiteral(false),
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                    if let (Some(subject_fqn), Some(target_fqn)) = (&subject_fqn, &target_fqn)
                        && !self.registry.class_is_subtype(target_fqn, subject_fqn)
                        && !self.registry.class_is_subtype(subject_fqn, target_fqn)
                    {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "'is' type test between unrelated classes '{}' and '{}' will always be false",
                                typed_inner.ty, target_type
                            ),
                        );
                        return TypedExpr {
                            kind: TypedExprKind::BoolLiteral(false),
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                }

                TypedExpr {
                    kind: TypedExprKind::TypeTest {
                        value: Box::new(typed_inner),
                        target_type,
                    },
                    ty: Type::Bool,
                    span: span.clone(),
                }
            }

            Expr::TypeCast {
                expr: inner,
                target,
                span,
            } => {
                let typed_inner = self.infer_expr(inner);

                let target_type = self.resolve_type_expr(target);
                if target_type.is_error() {
                    return TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                // Any → InterfaceObject is not supported (codegen can't build vtable from Any)
                if typed_inner.ty.is_any() && matches!(target_type, Type::InterfaceObject { .. }) {
                    self.diagnostics.error(
                        span.clone(),
                        "'as' from Any to interface object is not supported".to_string(),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                // Concrete type → interface object: validate the type implements
                // every component of the (possibly intersected) set.
                if let Type::InterfaceObject { traits, .. } = &target_type {
                    if typed_inner.ty.is_error() {
                        return typed_inner;
                    }
                    let failing = traits.iter().find(|c| {
                        !self.type_satisfies_trait(
                            &c.trait_fqn,
                            &c.trait_type_args,
                            &typed_inner.ty,
                            0,
                        )
                    });
                    if failing.is_none() {
                        return TypedExpr {
                            kind: TypedExprKind::Block(vec![typed_inner]),
                            ty: target_type,
                            span: span.clone(),
                        };
                    } else {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "type '{}' does not implement trait '{}'",
                                typed_inner.ty,
                                failing
                                    .map(|c| Type::interface_object(
                                        c.trait_fqn.clone(),
                                        c.trait_type_args.clone()
                                    )
                                    .to_string())
                                    .unwrap_or_default()
                            ),
                        );
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                }

                if !typed_inner.ty.is_any()
                    && !typed_inner.ty.is_class_type()
                    && !typed_inner.ty.is_error()
                    && !typed_inner.ty.contains_type_parameter()
                    && !target_type.contains_type_parameter()
                {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "'as' type cast requires subject of type Any or a class type, found '{}'",
                            typed_inner.ty
                        ),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                if target_type.is_any() || target_type.is_never() {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "'as' type cast target must be a concrete type, found '{}'",
                            target_type
                        ),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }

                // When subject is a class type, validate the target is in the same hierarchy
                if typed_inner.ty.is_class_type() && !target_type.is_error() {
                    if !target_type.is_class_type() {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "'as' with class subject requires a class target type, found '{}'",
                                target_type
                            ),
                        );
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                    if typed_inner.ty.is_error() || target_type.is_error() {
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                    let subject_fqn = typed_inner.ty.try_to_fqn();
                    let target_fqn = target_type.try_to_fqn();
                    if crate::typechecker::subtyping::is_subtype(
                        self.registry,
                        &typed_inner.ty,
                        &target_type,
                    ) {
                        // Subject is already a subtype — no cast needed, just
                        // return the expression with the target type so the user
                        // can control the inferred type.
                        return TypedExpr {
                            kind: typed_inner.kind,
                            ty: target_type,
                            span: span.clone(),
                        };
                    }
                    if let (Some(subject_fqn), Some(target_fqn)) = (&subject_fqn, &target_fqn)
                        && !self.registry.class_is_subtype(target_fqn, subject_fqn)
                        && !self.registry.class_is_subtype(subject_fqn, target_fqn)
                    {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "'as' type cast between unrelated classes '{}' and '{}' will always fail",
                                typed_inner.ty, target_type
                            ),
                        );
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                }

                TypedExpr {
                    kind: TypedExprKind::TypeCast {
                        value: Box::new(typed_inner),
                        target_type: target_type.clone(),
                    },
                    ty: target_type,
                    span: span.clone(),
                }
            }

            Expr::AsyncDo { body, span } => self.infer_async_do(body, span),
            Expr::Closure {
                is_async,
                params,
                body,
                span,
            } => self.infer_closure(*is_async, params, body, span),
        }
    }

    pub(super) fn infer_array_literal(&mut self, elements: &[Expr], span: &Span) -> TypedExpr {
        if elements.is_empty() {
            // Try to infer element type from expected_type context
            if let Some(Type::Array(elem_ty)) = &self.expected_type {
                let elem_ty = (**elem_ty).clone();
                return TypedExpr {
                    kind: TypedExprKind::ArrayLiteral { elements: vec![] },
                    ty: Type::Array(Box::new(elem_ty)),
                    span: span.clone(),
                };
            }
            self.diagnostics.error(
                span.clone(),
                "cannot infer element type for empty array literal".to_string(),
            );
            return TypedExpr {
                kind: TypedExprKind::ArrayLiteral { elements: vec![] },
                ty: Type::Error,
                span: span.clone(),
            };
        }

        let typed_elements: Vec<TypedExpr> = elements.iter().map(|e| self.infer_expr(e)).collect();

        // Find the lowest common type using the same algorithm as match/if-else
        let mut elem_ty: Option<Type> = None;
        for typed_elem in &typed_elements {
            elem_ty = Some(match elem_ty {
                None => typed_elem.ty.clone(),
                Some(prev) => {
                    if prev.is_error() || typed_elem.ty.is_error() {
                        Type::Error
                    } else if prev.is_never() {
                        typed_elem.ty.clone()
                    } else if typed_elem.ty.is_never() || self.is_assignable(&prev, &typed_elem.ty)
                    {
                        prev
                    } else if self.is_assignable(&typed_elem.ty, &prev) {
                        typed_elem.ty.clone()
                    } else {
                        self.diagnostics.error(
                            typed_elem.span.clone(),
                            format!(
                                "type mismatch: expected '{}', found '{}'",
                                prev, typed_elem.ty
                            ),
                        );
                        Type::Error
                    }
                }
            });
        }

        let elem_ty = elem_ty.unwrap_or(Type::Error);
        let result_ty = if elem_ty.is_error() {
            Type::Error
        } else {
            Type::Array(Box::new(elem_ty))
        };

        TypedExpr {
            kind: TypedExprKind::ArrayLiteral {
                elements: typed_elements,
            },
            ty: result_ty,
            span: span.clone(),
        }
    }

    /// Infer a list literal: `[1, 2, 3]`, `[]`, or a cons chain `h :: t`.
    ///
    /// Mirrors `infer_array_literal`'s element-type join, then folds the result
    /// into a right-nested `List.Cons(.., List.Nil)` chain. Because `::` parses
    /// into this same node, `a :: b :: []` and `[a, b]` join identically — and
    /// a widening tail (`dog :: animals` where `Dog <: Animal`) is accepted,
    /// which a direct `List.Cons` desugar could not do (the enum-construction
    /// path binds `T` from the first payload and then requires the tail to
    /// match it exactly).
    pub(super) fn infer_list_literal(
        &mut self,
        elements: &[Expr],
        tail: Option<&Expr>,
        span: &Span,
    ) -> TypedExpr {
        let list_fqn = Fqn::from_dotted("standard.prelude.List").unwrap();
        let Some(list_sig) = self.registry.get_enum_type(&list_fqn).cloned() else {
            self.diagnostics.error(
                span.clone(),
                "list literals require 'standard.prelude.List'".to_string(),
            );
            return TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            };
        };

        if !elements.is_empty() || tail.is_none() {
            self.check_private_type_access(
                &list_sig.fqn,
                list_sig.construction_private,
                "enum",
                span,
                "construct",
            );
        }

        let expected_element = match &self.expected_type {
            Some(Type::GenericEnum { fqn, type_args, .. }) if *fqn == list_fqn => {
                type_args.first().map(|(_, ty)| ty.clone())
            }
            _ => None,
        };
        let typed_elements: Vec<TypedExpr> = elements
            .iter()
            .map(|e| {
                let saved = self.expected_type.take();
                self.expected_type = expected_element.clone();
                let typed = self.infer_expr(e);
                self.expected_type = saved;
                typed
            })
            .collect();

        // A `::` tail must itself be a list; its element type joins with the rest.
        let mut typed_tail: Option<TypedExpr> = None;
        let mut tail_elem_ty: Option<Type> = None;
        if let Some(tail_expr) = tail {
            let typed = self.infer_expr(tail_expr);
            match &typed.ty {
                Type::GenericEnum { fqn, type_args, .. } if *fqn == list_fqn => {
                    tail_elem_ty = type_args.first().map(|(_, t)| t.clone());
                }
                ty if ty.is_error() => tail_elem_ty = Some(Type::Error),
                ty => {
                    self.diagnostics.error(
                        typed.span.clone(),
                        format!("expected a 'List' on the right of '::', found '{ty}'"),
                    );
                    tail_elem_ty = Some(Type::Error);
                }
            }
            typed_tail = Some(typed);
        }

        // Lowest common type across the elements, same algorithm as arrays.
        let mut elem_ty: Option<Type> = expected_element;
        for typed_elem in &typed_elements {
            elem_ty = Some(match elem_ty {
                None => typed_elem.ty.clone(),
                Some(prev) => {
                    if prev.is_error() || typed_elem.ty.is_error() {
                        Type::Error
                    } else if prev.is_never() {
                        typed_elem.ty.clone()
                    } else if typed_elem.ty.is_never() || self.is_assignable(&prev, &typed_elem.ty)
                    {
                        prev
                    } else if self.is_assignable(&typed_elem.ty, &prev) {
                        typed_elem.ty.clone()
                    } else {
                        self.diagnostics.error(
                            typed_elem.span.clone(),
                            format!(
                                "type mismatch: expected '{}', found '{}'",
                                prev, typed_elem.ty
                            ),
                        );
                        Type::Error
                    }
                }
            });
        }

        // Fold the tail's element type in last, reporting at the tail's span in
        // list terms — the mismatch is between two lists, not two elements.
        if let Some(tail_ty) = tail_elem_ty {
            elem_ty = Some(match elem_ty {
                None => tail_ty,
                Some(prev) => {
                    if prev.is_error() || tail_ty.is_error() {
                        Type::Error
                    } else if prev.is_never() {
                        tail_ty
                    } else if tail_ty.is_never() || self.is_assignable(&prev, &tail_ty) {
                        prev
                    } else if self.is_assignable(&tail_ty, &prev) {
                        tail_ty
                    } else {
                        let tail_span = typed_tail
                            .as_ref()
                            .map(|t| t.span.clone())
                            .unwrap_or_else(|| span.clone());
                        self.diagnostics.error(
                            tail_span,
                            format!(
                                "type mismatch: expected 'List<{prev}>', found 'List<{tail_ty}>'"
                            ),
                        );
                        Type::Error
                    }
                }
            });
        }

        // An empty `[]` is `List<Never>`, which covariance makes assignable to
        // every `List<T>` — so unlike an empty array literal this needs no
        // annotation and reports nothing. `expected_type` only sharpens the
        // displayed type.
        let elem_ty = elem_ty.unwrap_or_else(|| match &self.expected_type {
            Some(Type::GenericEnum { fqn, type_args, .. }) if *fqn == list_fqn => type_args
                .first()
                .map(|(_, t)| t.clone())
                .unwrap_or(Type::Never),
            _ => Type::Never,
        });

        if elem_ty.is_error() {
            return TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            };
        }

        let list_ty =
            self.resolve_generic_enum_type(&list_fqn, &list_sig, std::slice::from_ref(&elem_ty));

        // Fold right-to-left into Cons cells, seeded by the tail or by Nil.
        // Every node carries `list_ty` so payload coercion can look up variant
        // payload types from it.
        let mut acc = typed_tail.unwrap_or_else(|| TypedExpr {
            kind: TypedExprKind::EnumCreate {
                fqn: list_fqn.clone(),
                variant_name: "Nil".to_string(),
                args: vec![],
                type_params: vec![elem_ty.clone()],
            },
            ty: list_ty.clone(),
            // The closing bracket: an empty-list diagnostic points at `[]`.
            span: span.clone(),
        });
        for elem in typed_elements.into_iter().rev() {
            let cell_span = elem.span.merge(&acc.span);
            acc = TypedExpr {
                kind: TypedExprKind::EnumCreate {
                    fqn: list_fqn.clone(),
                    variant_name: "Cons".to_string(),
                    args: vec![elem, acc],
                    type_params: vec![elem_ty.clone()],
                },
                ty: list_ty.clone(),
                span: cell_span,
            };
        }
        acc
    }

    /// Infer a tuple literal expression: `(1, true)`, `(10, 20, 30)`.
    fn infer_tuple_literal(&mut self, elements: &[Expr], span: &Span) -> TypedExpr {
        let expected_elem_types: Option<Vec<Type>> = match &self.expected_type {
            Some(Type::Tuple(elem_types, _)) if elem_types.len() == elements.len() => {
                Some(elem_types.clone())
            }
            _ => None,
        };

        let typed_elements: Vec<TypedExpr> = elements
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let prev_expected = self.expected_type.take();
                if let Some(ref etypes) = expected_elem_types {
                    self.expected_type = Some(etypes[i].clone());
                }
                let typed = self.infer_expr(e);
                self.expected_type = prev_expected;
                typed
            })
            .collect();

        // Prefer the expected element type for each slot when the inferred
        // element is assignable to it. Tuple types are nominal in WASM codegen
        // (the struct type and per-slot value types come from the element type
        // list), so a tuple literal assigned into a `(Any, X)` context must
        // materialize element 0 at `Any` — not at its narrower inferred type
        // like `(Any, Any)`. Without this, `(Any, X)` and `((Any, Any), X)`
        // become two distinct WASM GC struct types and the validator rejects
        // the mismatch. The element value is already a subtype of the expected
        // slot, so codegen boxes it into the (possibly erased) slot correctly.
        let types: Vec<Type> = typed_elements
            .iter()
            .enumerate()
            .map(|(i, e)| match &expected_elem_types {
                Some(etypes) if self.is_assignable(&etypes[i], &e.ty) => etypes[i].clone(),
                _ => e.ty.clone(),
            })
            .collect();

        if types.iter().any(|t| t.is_error()) {
            return TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            };
        }

        let mn = crate::common::types::MangledName::for_tuple(&types);

        TypedExpr {
            kind: TypedExprKind::TupleLiteral {
                elements: typed_elements,
            },
            ty: Type::Tuple(types, mn),
            span: span.clone(),
        }
    }

    fn infer_let_destructure(
        &mut self,
        pattern: &crate::parser::ast::Pattern,
        type_annotation: Option<&TypeExpr>,
        value: &Expr,
        span: &Span,
    ) -> TypedExpr {
        // Resolve optional type annotation
        let annotated_ty = type_annotation.map(|ta| self.resolve_type_expr(ta));

        // Set expected type before inferring value
        let prev_expected = self.expected_type.take();
        self.expected_type = annotated_ty.clone();
        let typed_value = self.infer_expr(value);
        self.expected_type = prev_expected;

        let value_ty = if let Some(expected) = annotated_ty {
            self.check_assignable(typed_value.span.clone(), &expected, &typed_value.ty);
            expected
        } else {
            typed_value.ty.clone()
        };

        // The value must be a tuple type
        let typed_pattern = match &value_ty {
            Type::Tuple(elem_types, mn) => {
                self.infer_tuple_destructure_pattern(pattern, elem_types, mn, &value_ty, span)
            }
            Type::Error => TypedPattern::Wildcard,
            _ => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "cannot destructure non-tuple type '{}' with tuple pattern",
                        value_ty
                    ),
                );
                TypedPattern::Wildcard
            }
        };

        TypedExpr {
            kind: TypedExprKind::LetDestructure {
                pattern: typed_pattern,
                var_ty: value_ty,
                value: Box::new(typed_value),
            },
            ty: Type::Unit,
            span: span.clone(),
        }
    }

    pub(super) fn infer_tuple_destructure_pattern(
        &mut self,
        pattern: &crate::parser::ast::Pattern,
        elem_types: &[Type],
        _mn: &crate::common::types::MangledName,
        tuple_type: &Type,
        span: &Span,
    ) -> crate::typechecker::types::TypedPattern {
        use crate::parser::ast::Pattern;
        use crate::typechecker::types::TypedPattern;

        match pattern {
            Pattern::Tuple(sub_pats, pat_span) => {
                if sub_pats.len() != elem_types.len() {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!(
                            "tuple pattern has {} elements but the tuple has {}",
                            sub_pats.len(),
                            elem_types.len()
                        ),
                    );
                    return TypedPattern::Wildcard;
                }

                let element_patterns: Vec<TypedPattern> = sub_pats
                    .iter()
                    .zip(elem_types.iter())
                    .map(|(sub_pat, elem_ty)| self.infer_sub_pattern(sub_pat, elem_ty))
                    .collect();

                TypedPattern::Tuple {
                    element_patterns,
                    tuple_type: tuple_type.clone(),
                }
            }
            _ => {
                self.diagnostics.error(
                    span.clone(),
                    "expected tuple pattern in let destructuring".to_string(),
                );
                TypedPattern::Wildcard
            }
        }
    }

    fn operator_rhs_expectation(&self, op: BinOp, lhs: &Type) -> Option<Type> {
        use super::generics::apply_substitution;
        use super::type_param_substitution::TypeParamSubstitution;
        let name = match op {
            BinOp::Add => "Add",
            BinOp::Sub => "Sub",
            BinOp::Mul => "Mul",
            BinOp::Div => "Div",
            BinOp::TupleExtend => return None,
            BinOp::Concat => "Concat",
            _ => return Some(lhs.clone()),
        };
        let trait_fqn = Fqn::from_dotted(&format!("standard.prelude.{name}")).unwrap();
        let mut candidates = Vec::new();
        if let Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) = lhs {
            for bound in bounds
                .iter()
                .filter_map(crate::typechecker::types::TraitBound::named)
            {
                if bound.trait_fqn == trait_fqn && bound.type_args.len() == 1 {
                    candidates.push(bound.type_args[0].clone());
                }
            }
        } else if let Some(fqn) = lhs.try_to_fqn() {
            for info in self.registry.find_impl_blocks(&trait_fqn, &fqn) {
                let mut sub = TypeParamSubstitution::new();
                if sub.unify(&info.for_type, lhs) && info.trait_type_args.len() == 1 {
                    let rhs = apply_substitution(&sub, &info.trait_type_args[0]);
                    if !rhs.contains_type_parameter() {
                        candidates.push(rhs);
                    }
                }
            }
        }
        candidates
            .first()
            .filter(|first| candidates.iter().all(|ty| ty == *first))
            .cloned()
    }

    /// Build the trait call result, deriving Bool for equality and ordering.
    /// Arithmetic and concatenation use the implementation's declared output.
    fn build_lowered_op_result(
        &self,
        op: BinOp,
        make_call: impl Fn(Vec<TypedExpr>) -> TypedExprKind,
        typed_left: TypedExpr,
        typed_right: TypedExpr,
        return_type: &Type,
        span: &Span,
    ) -> Option<TypedExpr> {
        match op {
            BinOp::Eq => Some(TypedExpr {
                kind: make_call(vec![typed_left, typed_right]),
                ty: Type::Bool,
                span: span.clone(),
            }),
            BinOp::Ne => {
                let equals_call = TypedExpr {
                    kind: make_call(vec![typed_left, typed_right]),
                    ty: Type::Bool,
                    span: span.clone(),
                };
                Some(TypedExpr {
                    kind: TypedExprKind::UnaryOp {
                        op: UnaryOp::Not,
                        operand: Box::new(equals_call),
                    },
                    ty: Type::Bool,
                    span: span.clone(),
                })
            }
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                let ordering_fqn = Fqn::from_dotted("standard.prelude.Ordering").unwrap();
                let ordering_type = self
                    .registry
                    .lookup_type_by_fqn(&ordering_fqn)
                    .expect("Ordering type must exist in prelude registry")
                    .clone();

                let compare_call = TypedExpr {
                    kind: make_call(vec![typed_left, typed_right]),
                    ty: ordering_type.clone(),
                    span: span.clone(),
                };

                let (variant_name, variant_index, positive) = match op {
                    BinOp::Lt => ("Less", 0u32, true),
                    BinOp::Gt => ("Greater", 2u32, true),
                    BinOp::Le => ("Greater", 2u32, false),
                    BinOp::Ge => ("Less", 0u32, false),
                    _ => unreachable!(),
                };

                Some(TypedExpr {
                    kind: TypedExprKind::Match {
                        subject: Box::new(compare_call),
                        arms: vec![
                            TypedMatchArm {
                                pattern: TypedPattern::EnumVariant {
                                    enum_type: ordering_type.clone(),
                                    variant_name: variant_name.to_string(),
                                    variant_index,
                                    payload_patterns: vec![],
                                },
                                guard: None,
                                body: Box::new(TypedExpr {
                                    kind: TypedExprKind::BoolLiteral(positive),
                                    ty: Type::Bool,
                                    span: span.clone(),
                                }),
                                span: span.clone(),
                            },
                            TypedMatchArm {
                                pattern: TypedPattern::Wildcard,
                                guard: None,
                                body: Box::new(TypedExpr {
                                    kind: TypedExprKind::BoolLiteral(!positive),
                                    ty: Type::Bool,
                                    span: span.clone(),
                                }),
                                span: span.clone(),
                            },
                        ],
                    },
                    ty: Type::Bool,
                    span: span.clone(),
                })
            }
            // Arithmetic and concatenation take the implementation's output type.
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Concat => Some(TypedExpr {
                kind: make_call(vec![typed_left, typed_right]),
                ty: return_type.clone(),
                span: span.clone(),
            }),
            _ => None,
        }
    }

    fn try_lower_op_to_trait(
        &mut self,
        op: BinOp,
        operand_ty: &Type,
        typed_left: TypedExpr,
        typed_right: TypedExpr,
        span: &Span,
    ) -> Option<TypedExpr> {
        // Each operator has one prelude trait and method.
        let (trait_name, method_name) = match op {
            BinOp::Eq | BinOp::Ne => ("Equatable", "equals"),
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => ("Comparable", "compare"),
            BinOp::Add => ("Add", "add"),
            BinOp::Sub => ("Sub", "sub"),
            BinOp::Mul => ("Mul", "mul"),
            BinOp::Div => ("Div", "div"),
            BinOp::Concat => ("Concat", "concat"),
            _ => return None,
        };

        let trait_fqn = Fqn::from_dotted(&format!("standard.prelude.{}", trait_name)).unwrap();

        // Type parameter with trait bound (e.g. T : Equatable): lower using ImplFunctionCall.
        // Monomorphize will resolve this to the concrete impl method after substitution.
        if let Type::TypeVariable(_name, bounds) | Type::GenericParam(_name, bounds, _) = operand_ty
        {
            let matching: Vec<_> = bounds
                .iter()
                .filter_map(crate::typechecker::types::TraitBound::named)
                .filter(|b| {
                    b.trait_fqn == trait_fqn
                        && (b.type_args.is_empty()
                            || (b.type_args.len() == 1
                                && self.is_assignable(&b.type_args[0], &typed_right.ty)
                                && self.is_assignable(&typed_right.ty, &b.type_args[0])))
                })
                .collect();
            if matching.len() > 1 {
                self.diagnostics.error(
                    span.clone(),
                    format!("ambiguous operator '{}' for '{}'", op, operand_ty),
                );
                return None;
            }
            if let Some(bound) = matching.first() {
                let return_type = if matches!(
                    op,
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
                ) {
                    Type::Bool
                } else if let Some(output) = bound.associated_types.get("Output") {
                    self.scoped_bound_type(output)
                } else {
                    self.diagnostics.error(span.clone(), format!("operator '{}' requires an associated type constraint: '{}<{}, Output = ResultType>'", op, trait_name, typed_right.ty));
                    return None;
                };
                return self.build_lowered_op_result(
                    op,
                    |args| TypedExprKind::ImplFunctionCall {
                        trait_fqn: trait_fqn.clone(),
                        trait_type_params: bound.type_args.clone(),
                        for_type: operand_ty.clone(),
                        method_name: SymbolName(method_name.to_string()),
                        args,
                        method_type_params: vec![],
                    },
                    typed_left,
                    typed_right,
                    &return_type,
                    span,
                );
            }
            return None;
        }

        // Primitive equality and ordering retain their native checks. Arithmetic
        // and concatenation require an implementation, including on primitives.
        match operand_ty {
            Type::Record(..)
            | Type::GenericRecord { .. }
            | Type::Enum(..)
            | Type::GenericEnum { .. }
            | Type::Class(_, _)
            | Type::GenericClass { .. }
            | Type::Tuple(..)
            | Type::TupleExtend(..)
            | Type::TupleProjection(..)
            | Type::Array(_)
            | Type::Newtype(_, _)
            | Type::GenericNewtype { .. } => {}
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
            | Type::TypeVariable(_, _)
            | Type::GenericParam(_, _, _)
            | Type::SelfType
            | Type::InterfaceObject { .. }
            | Type::Any
            | Type::Function(..)
            | Type::AssociatedProjection(_)
            | Type::TypeConstructor { .. }
            | Type::Error => {
                if !matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Concat
                ) {
                    return None;
                }
            }
        }

        let expected = self.expected_type.take();
        let resolved = self.resolve_trait_impl_method_for_type(
            operand_ty,
            &trait_fqn,
            method_name,
            &[&typed_right.ty],
        );
        self.expected_type = expected;
        let (resolved, return_type) = resolved?;

        if operand_ty.is_numeric() || matches!(operand_ty, Type::String) {
            return Some(TypedExpr {
                kind: TypedExprKind::BinaryOp {
                    op,
                    left: Box::new(typed_left),
                    right: Box::new(typed_right),
                },
                ty: return_type,
                span: span.clone(),
            });
        }

        self.build_lowered_op_result(
            op,
            |args| TypedExprKind::ImplFunctionCall {
                trait_fqn: resolved.trait_fqn.clone(),
                trait_type_params: resolved.trait_type_params.clone(),
                for_type: resolved.for_type.clone(),
                method_name: resolved.method_name.clone(),
                args,
                method_type_params: resolved.method_type_params.clone(),
            },
            typed_left,
            typed_right,
            &return_type,
            span,
        )
    }

    /// Validate a binary operator for the given operand type. Returns the result type.
    fn check_binary_op(
        &mut self,
        op: BinOp,
        operand_ty: &Type,
        span: crate::common::span::Span,
    ) -> Type {
        match op {
            BinOp::TupleExtend => unreachable!("extension is inferred from both operands"),
            BinOp::Eq | BinOp::Ne => {
                // Valid for: Bool, all integers, all floats, String, Char.
                // Arrays and other compound types are handled via the Equatable trait (try_lower_op_to_trait).
                if matches!(
                    operand_ty,
                    Type::Bool
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
                ) {
                    Type::Bool
                } else {
                    self.diagnostics.error(
                        span,
                        format!(
                            "operator '{}' is not supported for type '{}'; consider implementing 'Equatable'",
                            op, operand_ty
                        ),
                    );
                    Type::Error
                }
            }
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                // Valid for: all integers, all floats, Char, String.
                // Arrays and other compound types are handled via the Comparable trait (try_lower_op_to_trait).
                if operand_ty.is_numeric() || matches!(operand_ty, Type::Char | Type::String) {
                    Type::Bool
                } else {
                    self.diagnostics.error(
                        span,
                        format!(
                            "operator '{}' is not supported for type '{}'; consider implementing 'Comparable'",
                            op, operand_ty
                        ),
                    );
                    Type::Error
                }
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Concat => {
                let name = match op {
                    BinOp::Add => "Add",
                    BinOp::Sub => "Sub",
                    BinOp::Mul => "Mul",
                    BinOp::Div => "Div",
                    _ => "Concat",
                };
                let message = if matches!((op, operand_ty), (BinOp::Add, Type::String)) {
                    "String concatenation uses '++', not '+'".to_string()
                } else {
                    format!(
                        "operator '{}' is not supported for type '{}'; consider implementing '{}'",
                        op, operand_ty, name
                    )
                };
                self.diagnostics.error(span, message);
                Type::Error
            }
            BinOp::Rem => {
                // Valid for: all integers only (not floats). Uint128 has no divide instruction.
                if matches!(operand_ty, Type::Uint128) {
                    self.diagnostics.error(
                        span,
                        "operator '%' is not supported for type 'Uint128'".to_string(),
                    );
                    Type::Error
                } else if operand_ty.is_integer() {
                    operand_ty.clone()
                } else {
                    self.diagnostics.error(
                        span,
                        format!(
                            "operator '{}' is not supported for type '{}'",
                            op, operand_ty
                        ),
                    );
                    Type::Error
                }
            }
            BinOp::LogicalAnd | BinOp::LogicalOr => {
                if *operand_ty == Type::Bool {
                    Type::Bool
                } else {
                    self.diagnostics.error(
                        span,
                        format!(
                            "operator '{}' requires Bool operands, found '{}'",
                            op, operand_ty
                        ),
                    );
                    Type::Error
                }
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                // Valid for: all integers only (not floats, not bool)
                if operand_ty.is_integer() {
                    operand_ty.clone()
                } else {
                    self.diagnostics.error(
                        span,
                        format!(
                            "operator '{}' is not supported for type '{}'",
                            op, operand_ty
                        ),
                    );
                    Type::Error
                }
            }
        }
    }

    /// Validate a unary operator for the given operand type. Returns the result type.
    fn check_unary_op(
        &mut self,
        op: UnaryOp,
        operand_ty: &Type,
        span: crate::common::span::Span,
    ) -> Type {
        match op {
            UnaryOp::Neg => {
                // Valid for: signed integers, floats
                if operand_ty.is_signed_integer() || operand_ty.is_float() {
                    operand_ty.clone()
                } else {
                    self.diagnostics.error(
                        span,
                        format!(
                            "operator '{}' is not supported for type '{}'",
                            op, operand_ty
                        ),
                    );
                    Type::Error
                }
            }
            UnaryOp::Not => {
                // Valid for: Bool only
                if *operand_ty == Type::Bool {
                    Type::Bool
                } else {
                    self.diagnostics.error(
                        span,
                        format!(
                            "operator '{}' is not supported for type '{}'",
                            op, operand_ty
                        ),
                    );
                    Type::Error
                }
            }
            UnaryOp::BitNot => {
                // Valid for: all integers (signed and unsigned)
                if operand_ty.is_integer() {
                    operand_ty.clone()
                } else {
                    self.diagnostics.error(
                        span,
                        format!(
                            "operator '{}' is not supported for type '{}'",
                            op, operand_ty
                        ),
                    );
                    Type::Error
                }
            }
        }
    }

    /// Infer a field access expression.
    /// Handles all forms: `obj.field`, `obj.prop<T>`, `Box<Int32>.count`, `Box<Int32>.zero<Bool>`.
    /// `object_type_params` are type args on the receiver (non-empty for generic module access).
    /// `field_type_params` are type args on the property (non-empty for generic property access).
    pub(super) fn infer_field_access(
        &mut self,
        object: &Expr,
        object_type_params: &[TypeExpr],
        field: &Spanned<String>,
        field_type_params: &[TypeExpr],
        span: &Span,
    ) -> TypedExpr {
        // Check for module-qualified access: ModuleName.global or ModuleName.property
        if let Expr::Identifier(name, _) = object
            && let Some(module_info) = self.resolve_module_name(name).cloned()
        {
            let member_sym = SymbolName(field.value.clone());

            // Non-generic module path: try concrete globals and properties first
            if object_type_params.is_empty() {
                // Try global
                if let Some(sig) = module_info.globals.get(&member_sym)
                    && self.is_member_visible(
                        sig.visibility,
                        &module_info.fqn.package,
                        &sig.source_file,
                    )
                {
                    return TypedExpr {
                        ty: sig.ty.clone(),
                        kind: TypedExprKind::GlobalRef {
                            name: sig.mangled_name.clone(),
                            type_params: vec![],
                        },
                        span: span.clone(),
                    };
                }
                // Try function/property
                if let Some(overloads) = module_info.functions.get(&member_sym) {
                    let property = overloads.iter().find(|sig| {
                        sig.is_property
                            && sig.params.is_empty()
                            && self.is_member_visible(
                                sig.visibility,
                                &module_info.fqn.package,
                                &sig.source_file,
                            )
                    });
                    if let Some(prop_sig) = property {
                        if prop_sig.is_intrinsic
                            && let Some(intrinsic) =
                                super::function_expressions::resolve_intrinsic_kind(
                                    &module_info.fqn,
                                    &member_sym,
                                    &prop_sig.return_type,
                                )
                        {
                            return TypedExpr {
                                ty: prop_sig.return_type.clone(),
                                kind: TypedExprKind::IntrinsicCall {
                                    intrinsic,
                                    args: vec![],
                                },
                                span: span.clone(),
                            };
                        }
                        return TypedExpr {
                            ty: prop_sig.return_type.clone(),
                            kind: TypedExprKind::FunctionCall {
                                name: prop_sig.mangled_name.clone(),
                                args: vec![],
                                type_params: vec![],
                            },
                            span: span.clone(),
                        };
                    }

                    // Try function reference (Module.func as value)
                    if let Some(result) = self.try_resolve_overloaded_function_ref(
                        overloads,
                        &format!("{}.{}", name, field.value),
                        span,
                    ) {
                        return result;
                    }
                }
            }

            // Try generic module global (with explicit type args or bidirectional inference)
            if let Some((mangled, ty, _mutable, global_type_args)) =
                self.resolve_generic_module_global(&module_info, &member_sym, object_type_params)
            {
                return TypedExpr {
                    ty,
                    kind: TypedExprKind::GlobalRef {
                        name: mangled,
                        type_params: global_type_args,
                    },
                    span: span.clone(),
                };
            }
            // Only properties are evaluated by member access. A zero-argument
            // function must go through function-reference resolution below.
            let has_generic_property = module_info
                .generic_members
                .lookup_visible(&member_sym, &self.package_path, &self.current_file)
                .iter()
                .any(|def| def.is_property);
            let generic_candidates = if has_generic_property {
                self.resolve_generic_module_static_method(
                    &module_info,
                    &member_sym,
                    &[],
                    object_type_params,
                    field_type_params,
                    Some(span),
                )
            } else {
                vec![]
            };
            if !generic_candidates.is_empty() {
                let resolved = &generic_candidates[0];
                return match resolved {
                    super::ResolvedFunction::Regular {
                        mangled_name,
                        return_type,
                        type_args,
                    } => TypedExpr {
                        ty: return_type.clone(),
                        kind: TypedExprKind::FunctionCall {
                            name: mangled_name.clone(),
                            args: vec![],
                            type_params: type_args.clone(),
                        },
                        span: span.clone(),
                    },
                    _ => unreachable!(),
                };
            }
            if let Some(reference) = self.try_resolve_generic_module_function_ref(
                &module_info,
                &member_sym,
                object_type_params,
                field_type_params,
                span,
            ) {
                return reference;
            }
            // Check if a visible member with this name exists but didn't resolve.
            // Report helpful error instead of falling through to "no variant in enum".
            let member_sym_check = SymbolName(field.value.clone());
            let has_visible_member =
                module_info
                    .functions
                    .get(&member_sym_check)
                    .is_some_and(|overloads| {
                        overloads.iter().any(|sig| match sig.visibility {
                            Visibility::Public | Visibility::Protected => true,
                            Visibility::Internal => module_info.fqn.package == self.package_path,
                            Visibility::Private => sig.source_file == self.current_file,
                        })
                    })
                    || !module_info
                        .generic_members
                        .lookup_visible(&member_sym_check, &self.package_path, &self.current_file)
                        .is_empty();
            if has_visible_member {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "cannot resolve member '{}.{}'; check type arguments",
                        name, field.value
                    ),
                );
                return TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                };
            }
            // Member not in module — fall through to extension/other dispatch
        }

        // Try to resolve as an enum variant constructor: Enum.Variant (no payload)
        if let Expr::Identifier(name, _) = object
            && let Some(enum_sig) = self.resolve_enum_type(name)
        {
            if let Some((_, payload)) = enum_sig.variants.iter().find(|(v, _)| v == &field.value) {
                self.check_private_type_access(
                    &enum_sig.fqn,
                    enum_sig.construction_private,
                    "enum",
                    span,
                    "construct",
                );

                match payload {
                    VariantPayload::Tuple(types) => {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "variant '{}.{}' requires {} argument(s)",
                                name,
                                field.value,
                                types.len()
                            ),
                        );
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                    VariantPayload::Record(_) => {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "variant '{}.{}' requires record-style construction with {{ }}",
                                name, field.value
                            ),
                        );
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                    VariantPayload::None => {}
                }
                // Handle generic enums
                if !enum_sig.type_params.is_empty() {
                    // Infer type args from:
                    // 1. Explicit type args on the type (e.g., Async<WaiterId, Never>.MakeWaiter)
                    // 2. expected_type (bidirectional inference)
                    // 3. Covariant variance defaults (Never for out params)
                    let type_args: Vec<Type> = if !object_type_params.is_empty()
                        && object_type_params.len() == enum_sig.type_params.len()
                    {
                        object_type_params
                            .iter()
                            .map(|te| self.resolve_type_expr(te))
                            .collect()
                    } else {
                        match &self.expected_type {
                            Some(Type::GenericEnum {
                                fqn: exp_fqn,
                                type_args,
                                ..
                            }) if *exp_fqn == enum_sig.fqn => {
                                type_args.iter().map(|(_, t)| t.clone()).collect()
                            }
                            _ => {
                                // Default covariant params to Never
                                let sub =
                                    super::type_param_substitution::TypeParamSubstitution::new();
                                match sub.resolve_with_variance_defaults(
                                    &enum_sig.type_params,
                                    &enum_sig.type_param_variances,
                                ) {
                                    Some(args) => args,
                                    None => {
                                        self.diagnostics.error(
                                            span.clone(),
                                            format!(
                                                "cannot infer type arguments for generic enum '{}'",
                                                name
                                            ),
                                        );
                                        return TypedExpr {
                                            kind: TypedExprKind::UnitLiteral,
                                            ty: Type::Error,
                                            span: span.clone(),
                                        };
                                    }
                                }
                            }
                        }
                    };
                    let enum_ty =
                        self.resolve_generic_enum_type(&enum_sig.fqn, &enum_sig, &type_args);
                    return TypedExpr {
                        ty: enum_ty,
                        kind: TypedExprKind::EnumCreate {
                            fqn: enum_sig.fqn.clone(),
                            variant_name: field.value.clone(),
                            args: vec![],
                            type_params: type_args.clone(),
                        },
                        span: span.clone(),
                    };
                }
                let mangled_name = MangledName::for_type(&enum_sig.fqn);
                return TypedExpr {
                    ty: Type::Enum(enum_sig.fqn.clone(), mangled_name),
                    kind: TypedExprKind::EnumCreate {
                        fqn: enum_sig.fqn.clone(),
                        variant_name: field.value.clone(),
                        args: vec![],
                        type_params: vec![],
                    },
                    span: span.clone(),
                };
            }
            // Variant not found — fall through only if a trait impl
            // static method/property by this name exists, so that
            // `EnumName.from` resolves via `From<T> for EnumName`'s
            // `from` (handled in `try_resolve_static_property` below).
            // Mirrors the call-site fall-through in
            // function_expressions.rs:516-520. Without the existence
            // check, typos like `Color.Vermilion` would lose the
            // helpful "no variant" diagnostic.
            let member_sym = SymbolName(field.value.clone());
            if self
                .registry
                .find_impl_method(&enum_sig.fqn, &member_sym)
                .is_empty()
            {
                self.diagnostics.error(
                    span.clone(),
                    format!("no variant '{}' in enum '{}'", field.value, name),
                );
                return TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                };
            }
        }

        // Check for static property access: Type.property (before inferring object)
        // This must come before the object_type_params check since generic class
        // static properties may use explicit type args (e.g. Box<Int32>.tag)
        if let Expr::Identifier(name, _) = object
            && let Some(result) =
                self.try_resolve_static_property(name, field, object_type_params, span)
        {
            return result;
        }

        // Non-module paths only valid without object type params
        if !object_type_params.is_empty() {
            self.diagnostics.error(
                span.clone(),
                "type arguments on non-module field access".to_string(),
            );
            return TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            };
        }

        let typed_object = self.infer_expr(object);
        if typed_object.ty.is_tuple() && field_type_params.is_empty() {
            let projection = match field.value.as_str() {
                "init" => Some(crate::typechecker::types::TupleProjection::Init),
                "last" => Some(crate::typechecker::types::TupleProjection::Last),
                _ => None,
            };
            if let Some(kind) = projection {
                let ty = Type::tuple_projection(typed_object.ty.clone(), kind);
                return crate::typechecker::tuple_extension::lower(TypedExpr {
                    kind: TypedExprKind::IntrinsicCall {
                        intrinsic: IntrinsicKind::TupleProjection(kind),
                        args: vec![typed_object],
                    },
                    ty,
                    span: span.clone(),
                });
            }
        }

        // Dispatch by object type
        match &typed_object.ty {
            Type::Error => {
                return TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                };
            }

            Type::InterfaceObject { traits, .. } => {
                // Find which components declare the property: exactly one →
                // dispatch through it; several → ambiguous; none → concrete
                // pipeline fallback.
                let traits = traits.clone();
                let mut declaring: Vec<&InterfaceComponent> = traits
                    .iter()
                    .filter(|c| {
                        self.registry
                            .lookup_trait(&c.trait_fqn, &self.package_path)
                            .is_some_and(|sig| sig.properties.iter().any(|p| p.name == field.value))
                    })
                    .collect();
                // Same-origin members inherited into several components (via
                // `extends`) are one member, not an ambiguity — prefer the
                // origin component itself for a stable dispatch key.
                if declaring.len() > 1 {
                    // Origin identity is the full APPLICATION — fqn AND type
                    // args (see the method-call form of this dedup).
                    let member_origin = |c: &InterfaceComponent| -> Option<(Fqn, Vec<Type>)> {
                        let sig = self
                            .registry
                            .lookup_trait(&c.trait_fqn, &self.package_path)?;
                        let m = sig.properties.iter().find(|p| p.name == field.value)?;
                        Some(match m.origin.as_ref() {
                            Some((f, raw_args)) => {
                                let sub = crate::typechecker::infer::type_param_substitution::TypeParamSubstitution::from_pairs(
                                    &sig.type_params,
                                    &c.trait_type_args,
                                );
                                (
                                    f.clone(),
                                    raw_args
                                        .iter()
                                        .map(|t| super::generics::apply_substitution(&sub, t))
                                        .collect(),
                                )
                            }
                            None => (c.trait_fqn.clone(), c.trait_type_args.clone()),
                        })
                    };
                    let origins: Vec<Option<(Fqn, Vec<Type>)>> =
                        declaring.iter().map(|c| member_origin(c)).collect();
                    if origins.iter().all(|o| o.is_some())
                        && origins.windows(2).all(|w| w[0] == w[1])
                    {
                        let (origin_fqn, origin_args) = origins[0].clone().unwrap();
                        if let Some(pos) = declaring.iter().position(|c| {
                            c.trait_fqn == origin_fqn && c.trait_type_args == origin_args
                        }) {
                            let chosen = declaring[pos];
                            declaring = vec![chosen];
                        }
                    }
                }
                if declaring.len() > 1 {
                    let names: Vec<String> = declaring
                        .iter()
                        .map(|c| format!("'{}'", c.trait_fqn.symbol))
                        .collect();
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "ambiguous property '{}': declared by {}",
                            field.value,
                            names.join(" and ")
                        ),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }
                if let Some(component) = declaring.first() {
                    let trait_fqn = component.trait_fqn.clone();
                    let trait_type_args = component.trait_type_args.clone();
                    if let Some(result) = self.try_resolve_interface_object_property(
                        &typed_object,
                        &trait_fqn,
                        &trait_type_args,
                        field,
                        span,
                    ) {
                        return result;
                    }
                }
                // Property not in any component — try concrete pipeline
                if let Some(result) = self.resolve_concrete_type_property(
                    &typed_object,
                    field,
                    field_type_params,
                    span,
                ) {
                    return result;
                }
            }

            Type::Class(..) | Type::GenericClass { .. } => {
                if let Some(result) = self.try_resolve_class_field(&typed_object, field, span) {
                    return result;
                }
                if let Some(result) =
                    self.try_resolve_class_instance_property(&typed_object, field, span)
                {
                    return result;
                }
                if let Some(result) = self.resolve_concrete_type_property(
                    &typed_object,
                    field,
                    field_type_params,
                    span,
                ) {
                    return result;
                }
            }

            Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) => {
                let bounds = bounds.clone();
                if let Some(result) = self.try_resolve_type_param_class_bound_field(
                    &typed_object,
                    &bounds,
                    field,
                    span,
                ) {
                    return result;
                }
                if let Some(result) =
                    self.try_resolve_property_from_trait_bounds(&typed_object, &bounds, field, span)
                {
                    return result;
                }
            }

            _ => {
                if let Some(result) = self.resolve_concrete_type_property(
                    &typed_object,
                    field,
                    field_type_params,
                    span,
                ) {
                    return result;
                }
            }
        }

        // No field and no extension property found
        self.diagnostics.error(
            field.span.clone(),
            format!("no field '{}' on type '{}'", field.value, typed_object.ty),
        );
        TypedExpr {
            kind: TypedExprKind::UnitLiteral,
            ty: Type::Error,
            span: span.clone(),
        }
    }

    /// Try to resolve an instance property access on a type parameter via its
    /// trait bounds (mirror of `try_resolve_method_from_trait_bounds` for
    /// properties). Emits a deferred `ImplFunctionCall` that monomorphize
    /// resolves once the type parameter is substituted.
    /// Resolve an instance property against ONE trait bound of a type
    /// parameter. Shared by implicit dispatch (all bounds tried) and the
    /// explicit `TraitName.prop(receiver)` form (single bound).
    pub(super) fn try_resolve_property_from_bound(
        &mut self,
        typed_object: &TypedExpr,
        bound: &NamedTraitBound,
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        let trait_sig = self
            .registry
            .lookup_trait(&bound.trait_fqn, &self.package_path)
            .cloned()?;
        let prop = trait_sig.properties.iter().find(|p| {
            p.name == field.value && p.params.first().is_some_and(|(n, _)| n == "self")
        })?;
        let mut sub = super::type_param_substitution::TypeParamSubstitution::new()
            .with_self_type(typed_object.ty.clone());
        for (tp, arg) in trait_sig.type_params.iter().zip(bound.type_args.iter()) {
            sub.insert(tp.clone(), arg.clone());
        }
        for associated in &trait_sig.associated_types {
            let parameters = associated
                .type_params
                .iter()
                .map(|name| Type::TypeVariable(name.clone(), vec![]))
                .collect();
            if let Some(projection) = crate::typechecker::associated_types::from_bound(
                &typed_object.ty,
                bound,
                &associated.name,
                parameters,
                self.registry,
            ) {
                sub.insert(
                    crate::common::types::TypeParamName(associated.name.clone()),
                    projection,
                );
            }
        }
        let return_type = apply_substitution(&sub, &prop.return_type);
        Some(TypedExpr {
            kind: TypedExprKind::ImplFunctionCall {
                trait_fqn: bound.trait_fqn.clone(),
                trait_type_params: bound.type_args.clone(),
                for_type: typed_object.ty.clone(),
                method_name: SymbolName(field.value.clone()),
                args: vec![typed_object.clone()],
                method_type_params: vec![],
            },
            ty: return_type,
            span: span.clone(),
        })
    }

    fn try_resolve_property_from_trait_bounds(
        &mut self,
        typed_object: &TypedExpr,
        bounds: &[TraitBound],
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        // Resolve against every bound; a property declared by several bounds
        // is ambiguous (mirror of the method form's cross-bound check).
        // Keyed by bound INDEX (see the method form): two applications of one
        // trait are distinct bounds.
        let mut matches: Vec<(usize, TypedExpr)> = Vec::new();
        for (bound_idx, bound) in bounds.iter().enumerate() {
            let Some(bound) = bound.named() else {
                continue;
            };
            let Some(trait_sig) = self
                .registry
                .lookup_trait(&bound.trait_fqn, &self.package_path)
                .cloned()
            else {
                continue;
            };
            let Some(prop) = trait_sig.properties.iter().find(|p| {
                p.name == field.value && p.params.first().is_some_and(|(n, _)| n == "self")
            }) else {
                continue;
            };
            let mut sub = super::type_param_substitution::TypeParamSubstitution::new()
                .with_self_type(typed_object.ty.clone());
            for (tp, arg) in trait_sig.type_params.iter().zip(bound.type_args.iter()) {
                sub.insert(tp.clone(), arg.clone());
            }
            for associated in &trait_sig.associated_types {
                let parameters = associated
                    .type_params
                    .iter()
                    .map(|name| Type::TypeVariable(name.clone(), vec![]))
                    .collect();
                if let Some(projection) = crate::typechecker::associated_types::from_bound(
                    &typed_object.ty,
                    bound,
                    &associated.name,
                    parameters,
                    self.registry,
                ) {
                    sub.insert(
                        crate::common::types::TypeParamName(associated.name.clone()),
                        projection,
                    );
                }
            }
            let return_type = apply_substitution(&sub, &prop.return_type);
            matches.push((
                bound_idx,
                TypedExpr {
                    kind: TypedExprKind::ImplFunctionCall {
                        trait_fqn: bound.trait_fqn.clone(),
                        trait_type_params: bound.type_args.clone(),
                        for_type: typed_object.ty.clone(),
                        method_name: SymbolName(field.value.clone()),
                        args: vec![typed_object.clone()],
                        method_type_params: vec![],
                    },
                    ty: return_type,
                    span: span.clone(),
                },
            ));
        }
        if matches.len() > 1 {
            // `extends` origin dedup (see the method form): one inherited
            // declaration reached through several bounds is not ambiguous.
            if let Some(kept) = self.dedup_bound_matches_by_origin(
                &matches,
                bounds,
                |sig, member| {
                    sig.properties
                        .iter()
                        .find(|p| p.name == member)
                        .and_then(|p| p.origin.clone())
                },
                &field.value,
            ) {
                matches = vec![matches.swap_remove(kept)];
            }
        }
        if matches.len() > 1 {
            let names: Vec<String> = matches
                .iter()
                .map(|(i, _)| format!("'{}'", Self::bound_display(&bounds[*i])))
                .collect();
            self.diagnostics.error(
                span.clone(),
                format!(
                    "ambiguous property '{}' on type '{}': declared by trait {}",
                    field.value,
                    typed_object.ty,
                    names.join(" and trait "),
                ),
            );
            return Some(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }
        matches.into_iter().next().map(|(_, expr)| expr)
    }

    /// Try to resolve a field access on a TypeParameter via class bounds.
    /// Walks the class hierarchy for each SubtypeOf bound looking for a matching field.
    fn try_resolve_type_param_class_bound_field(
        &mut self,
        typed_object: &TypedExpr,
        bounds: &[TraitBound],
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        for bound in bounds
            .iter()
            .filter_map(crate::typechecker::types::TraitBound::named)
        {
            if bound.kind != crate::typechecker::types::BoundKind::SubtypeOf {
                continue;
            }
            let mut current_fqn = Some(bound.trait_fqn.clone());
            while let Some(fqn) = current_fqn.take() {
                let class_sig = match self.registry.lookup_class_type(&fqn, &self.package_path) {
                    Some(sig) => sig.clone(),
                    None => break,
                };
                for (idx, f) in class_sig.fields.iter().enumerate() {
                    if f.name == field.value {
                        if f.visibility == Visibility::Public {
                            return Some(TypedExpr {
                                kind: TypedExprKind::FieldAccess {
                                    object: Box::new(typed_object.clone()),
                                    field_name: field.value.clone(),
                                    field_index: idx as u32,
                                    boxed: false,
                                },
                                ty: f.ty.clone(),
                                span: span.clone(),
                            });
                        } else {
                            self.diagnostics.error(
                                field.span.clone(),
                                format!(
                                    "field '{}' on class '{}' is not public",
                                    field.value, bound.trait_fqn.symbol,
                                ),
                            );
                            return Some(TypedExpr {
                                kind: TypedExprKind::UnitLiteral,
                                ty: Type::Error,
                                span: span.clone(),
                            });
                        }
                    }
                }
                current_fqn = class_sig.parent_class;
            }
        }
        None
    }

    /// Try to resolve a property/field through the concrete type pipeline:
    /// record field → module prop → generic module prop → extension prop → method ref.
    /// Returns `None` if nothing matched.
    fn resolve_concrete_type_property(
        &mut self,
        typed_object: &TypedExpr,
        field: &Spanned<String>,
        field_type_params: &[TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        // 1. Record field
        if let Some(result) = self.try_resolve_record_field(typed_object, field, span) {
            return Some(result);
        }
        // 2. Module-for-type instance property
        if let Some(result) = self.try_resolve_module_instance_property(typed_object, field, span) {
            return Some(result);
        }
        // 3. Generic module-for-type instance property
        if let Some(result) = self.try_resolve_generic_module_instance_property(
            typed_object,
            field,
            field_type_params,
            span,
        ) {
            return Some(result);
        }
        // 4. Extension property (trait impl → named extension → generic extension)
        if let Some(result) = self.try_resolve_extension_property(typed_object, field, span) {
            return Some(result);
        }
        // 5. Bound method reference
        if let Some(result) = self.try_resolve_method_ref(typed_object, field, span) {
            return Some(result);
        }
        None
    }

    /// Try to resolve a field access as a module-for-type instance property.
    /// Returns `Some(TypedExpr)` if a matching instance property was found, `None` otherwise.
    fn try_resolve_module_instance_property(
        &mut self,
        typed_object: &TypedExpr,
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        if typed_object.ty.is_error() {
            return None;
        }
        let type_fqn = typed_object.ty.try_to_fqn()?;
        let module_info = self.registry.lookup_module(&type_fqn)?.clone();

        let member_sym = SymbolName(field.value.clone());
        let overloads = module_info.functions.get(&member_sym)?;
        let property = overloads.iter().find(|sig| {
            sig.is_property
                && !sig.params.is_empty()
                && sig.params[0].0 == "self"
                && self.is_member_visible(
                    sig.visibility,
                    &module_info.fqn.package,
                    &sig.source_file,
                )
        })?;

        if property.is_intrinsic
            && let Some(intrinsic) =
                resolve_intrinsic_kind(&type_fqn, &member_sym, &property.return_type)
        {
            return Some(TypedExpr {
                ty: property.return_type.clone(),
                kind: TypedExprKind::IntrinsicCall {
                    intrinsic,
                    args: vec![typed_object.clone()],
                },
                span: span.clone(),
            });
        }

        Some(TypedExpr {
            ty: property.return_type.clone(),
            kind: TypedExprKind::FunctionCall {
                name: property.mangled_name.clone(),
                args: vec![typed_object.clone()],
                type_params: vec![],
            },
            span: span.clone(),
        })
    }

    /// Try to resolve a field access as a generic module-for-type instance property.
    /// `type_args` are explicit property-level type args (e.g., `<Int32>` in `obj.prop<Int32>`).
    fn try_resolve_generic_module_instance_property(
        &mut self,
        typed_object: &TypedExpr,
        field: &Spanned<String>,
        type_args: &[TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        let prop_name = SymbolName(field.value.clone());
        let generic_result =
            self.resolve_generic_module_instance_property(&typed_object.ty, &prop_name, type_args);
        if generic_result.is_empty() {
            return None;
        }

        let resolved = &generic_result[0];
        match resolved {
            super::ResolvedFunction::Regular {
                mangled_name,
                return_type,
                type_args,
            } => Some(TypedExpr {
                ty: return_type.clone(),
                kind: TypedExprKind::FunctionCall {
                    name: mangled_name.clone(),
                    args: vec![typed_object.clone()],
                    type_params: type_args.clone(),
                },
                span: span.clone(),
            }),
            super::ResolvedFunction::Intrinsic {
                intrinsic,
                return_type,
            } => Some(TypedExpr {
                ty: return_type.clone(),
                kind: TypedExprKind::IntrinsicCall {
                    intrinsic: intrinsic.clone(),
                    args: vec![typed_object.clone()],
                },
                span: span.clone(),
            }),
            super::ResolvedFunction::ImplMethod {
                resolved,
                return_type,
            } => Some(TypedExpr {
                ty: return_type.clone(),
                kind: TypedExprKind::ImplFunctionCall {
                    trait_fqn: resolved.trait_fqn.clone(),
                    trait_type_params: resolved.trait_type_params.clone(),
                    for_type: resolved.for_type.clone(),
                    method_name: resolved.method_name.clone(),
                    args: vec![typed_object.clone()],
                    method_type_params: resolved.method_type_params.clone(),
                },
                span: span.clone(),
            }),
            super::ResolvedFunction::ExtMethod {
                ext_fqn,
                for_type,
                method_name,
                type_args,
                return_type,
            } => Some(TypedExpr {
                ty: return_type.clone(),
                kind: TypedExprKind::ExtFunctionCall {
                    ext_fqn: ext_fqn.clone(),
                    for_type: for_type.clone(),
                    method_name: method_name.clone(),
                    args: vec![typed_object.clone()],
                    type_params: type_args.clone(),
                },
                span: span.clone(),
            }),
        }
    }

    /// Try to resolve a field access as an extension property.
    /// Returns `Some(TypedExpr)` if a property was found, `None` otherwise.
    fn try_resolve_extension_property(
        &mut self,
        typed_object: &TypedExpr,
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        if typed_object.ty.is_error() {
            return None;
        }
        let type_fqn = typed_object.ty.try_to_fqn()?;
        let prop_name = SymbolName(field.value.clone());

        // 1. Try named extension properties (extensions take priority over
        // trait impls, appendix §2.1)
        let ext_overloads = self.lookup_named_extension_methods(&type_fqn, &prop_name);
        let ext_props: Vec<_> = ext_overloads
            .into_iter()
            .filter(|(_, m)| m.is_property)
            .collect();

        {
            // Match EVERY candidate against the receiver — sibling extension
            // blocks on different instantiations share a base FQN, and two
            // matching candidates from distinct extensions are ambiguous
            // (appendix §2.2).
            let self_arg_types: Vec<&Type> = vec![&typed_object.ty];
            let matching_ext: Vec<_> = ext_props
                .iter()
                .filter(|(_, m)| {
                    FunctionSignature::params_match_args(&m.params, &self_arg_types, |p, a| {
                        self.is_assignable(p, a)
                    })
                })
                .collect();
            let distinct_exts: Vec<&Fqn> = {
                let mut seen: Vec<&Fqn> = Vec::new();
                for (b, _) in &matching_ext {
                    if !seen.contains(&&b.ext_fqn) {
                        seen.push(&b.ext_fqn);
                    }
                }
                seen
            };
            if distinct_exts.len() > 1 {
                let names: Vec<String> = distinct_exts
                    .iter()
                    .map(|f| format!("'{}'", f.symbol))
                    .collect();
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "ambiguous property '{}': provided by extension {}; use '{}.{}(...)' to choose one",
                        prop_name.0, names.join(" and extension "), distinct_exts[0].symbol, prop_name.0,
                    ),
                );
                return Some(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }
            if let Some((block, method)) = matching_ext.first() {
                if method.is_intrinsic
                    && let Some(intrinsic) =
                        resolve_intrinsic_kind(&type_fqn, &prop_name, &method.return_type)
                {
                    return Some(TypedExpr {
                        ty: method.return_type.clone(),
                        kind: TypedExprKind::IntrinsicCall {
                            intrinsic,
                            args: vec![typed_object.clone()],
                        },
                        span: span.clone(),
                    });
                }
                return Some(TypedExpr {
                    ty: method.return_type.clone(),
                    kind: TypedExprKind::ExtFunctionCall {
                        ext_fqn: block.ext_fqn.clone(),
                        for_type: typed_object.ty.clone(),
                        method_name: prop_name.clone(),
                        args: vec![typed_object.clone()],
                        type_params: vec![],
                    },
                    span: span.clone(),
                });
            }
        }

        // 2. Try generic extension properties
        let generic_result = self.resolve_generic_extension_property(&typed_object.ty, &prop_name);
        if !generic_result.is_empty() {
            // Two DISTINCT extensions both matching is ambiguous (§2.2), just
            // like the non-generic arm above.
            let distinct_generic_exts: Vec<&Fqn> = {
                let mut seen: Vec<&Fqn> = Vec::new();
                for r in &generic_result {
                    if let super::ResolvedFunction::ExtMethod { ext_fqn, .. } = r
                        && !seen.contains(&ext_fqn)
                    {
                        seen.push(ext_fqn);
                    }
                }
                seen
            };
            if distinct_generic_exts.len() > 1 {
                let names: Vec<String> = distinct_generic_exts
                    .iter()
                    .map(|f| format!("'{}'", f.symbol))
                    .collect();
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "ambiguous property '{}': provided by extension {}; use '{}.{}(...)' to choose one",
                        prop_name.0, names.join(" and extension "), distinct_generic_exts[0].symbol, prop_name.0,
                    ),
                );
                return Some(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }
            let resolved = &generic_result[0];
            match resolved {
                super::ResolvedFunction::Intrinsic {
                    intrinsic,
                    return_type,
                } => {
                    return Some(TypedExpr {
                        ty: return_type.clone(),
                        kind: TypedExprKind::IntrinsicCall {
                            intrinsic: intrinsic.clone(),
                            args: vec![typed_object.clone()],
                        },
                        span: span.clone(),
                    });
                }
                super::ResolvedFunction::Regular {
                    mangled_name,
                    return_type,
                    ..
                } => {
                    return Some(TypedExpr {
                        ty: return_type.clone(),
                        kind: TypedExprKind::FunctionCall {
                            name: mangled_name.clone(),
                            args: vec![typed_object.clone()],
                            type_params: vec![],
                        },
                        span: span.clone(),
                    });
                }
                super::ResolvedFunction::ImplMethod {
                    resolved,
                    return_type,
                } => {
                    return Some(TypedExpr {
                        ty: return_type.clone(),
                        kind: TypedExprKind::ImplFunctionCall {
                            trait_fqn: resolved.trait_fqn.clone(),
                            trait_type_params: resolved.trait_type_params.clone(),
                            for_type: resolved.for_type.clone(),
                            method_name: resolved.method_name.clone(),
                            args: vec![typed_object.clone()],
                            method_type_params: resolved.method_type_params.clone(),
                        },
                        span: span.clone(),
                    });
                }
                super::ResolvedFunction::ExtMethod {
                    ext_fqn,
                    for_type,
                    method_name,
                    type_args,
                    return_type,
                } => {
                    return Some(TypedExpr {
                        ty: return_type.clone(),
                        kind: TypedExprKind::ExtFunctionCall {
                            ext_fqn: ext_fqn.clone(),
                            for_type: for_type.clone(),
                            method_name: method_name.clone(),
                            args: vec![typed_object.clone()],
                            type_params: type_args.clone(),
                        },
                        span: span.clone(),
                    });
                }
            }
        }

        // 3. Try trait impl properties — after extensions per the priority
        // rule. Sibling-instantiation-aware: only blocks whose self param
        // accepts the receiver are candidates; two candidates from DISTINCT
        // traits are ambiguous (appendix §3).
        let trait_impl_props: Vec<_> = self
            .registry
            .find_impl_method(&type_fqn, &prop_name)
            .into_iter()
            .filter(|(b, m)| {
                b.type_params.is_empty()
                    && m.method_type_params.is_empty()
                    && m.is_property
                    && (m.visibility != Visibility::Private || m.span.file == self.current_file)
            })
            .map(|(b, m)| (b.clone(), m.clone()))
            .collect();
        let matching: Vec<_> = trait_impl_props
            .iter()
            .filter(|(_, m)| {
                m.params.len() == 1
                    && m.params[0].0 == "self"
                    && self.is_assignable(&m.params[0].1, &typed_object.ty)
            })
            .collect();
        // Distinct APPLICATIONS of one trait (`Conv<Int32>` and `Conv<Bool>`
        // both for Rec) are as ambiguous as two different traits — key the
        // dedup on (fqn, trait args), mirroring the method paths.
        let mut distinct_traits: Vec<(&Fqn, &Vec<Type>)> = {
            let mut seen: Vec<(&Fqn, &Vec<Type>)> = Vec::new();
            for (b, _) in &matching {
                let key = (&b.trait_fqn, &b.trait_type_args);
                if !seen.contains(&key) {
                    seen.push(key);
                }
            }
            seen
        };
        // One inherited declaration reached through a trait and a sub-trait
        // that extends it is not an ambiguity — the origin's direct impl
        // wins, as on the method, bound and explicit paths.
        let mut origin_only: Option<(&Fqn, &Vec<Type>)> = None;
        if distinct_traits.len() > 1 {
            let applications: Vec<(Fqn, Vec<Type>)> = distinct_traits
                .iter()
                .map(|(f, a)| ((*f).clone(), (*a).clone()))
                .collect();
            if let Some(kept) = self.dedup_traits_by_member_origin(
                &applications,
                |sig, member| {
                    sig.properties
                        .iter()
                        .find(|p| p.name == member)
                        .and_then(|p| p.origin.clone())
                },
                &prop_name.0,
            ) {
                origin_only = Some(distinct_traits[kept]);
                distinct_traits = vec![distinct_traits[kept]];
            }
        }
        if distinct_traits.len() > 1 {
            let names: Vec<String> = distinct_traits
                .iter()
                .map(|(f, a)| format!("'{}'", Self::trait_application_display(f, a)))
                .collect();
            let suggestion =
                Self::trait_application_display(distinct_traits[0].0, distinct_traits[0].1);
            self.diagnostics.error(
                span.clone(),
                format!(
                    "ambiguous property '{}': implemented by trait {}; use '{}.{}(...)' to choose one",
                    prop_name.0, names.join(" and trait "), suggestion, prop_name.0,
                ),
            );
            return Some(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }
        let matching: Vec<_> = match origin_only {
            Some((f, a)) => matching
                .into_iter()
                .filter(|(b, _)| b.trait_fqn == *f && b.trait_type_args == *a)
                .collect(),
            None => matching,
        };
        if let Some((block, m)) = matching.first() {
            if m.is_intrinsic
                && let Some(intrinsic) =
                    resolve_intrinsic_kind(&type_fqn, &prop_name, &m.return_type)
            {
                return Some(TypedExpr {
                    ty: m.return_type.clone(),
                    kind: TypedExprKind::IntrinsicCall {
                        intrinsic,
                        args: vec![typed_object.clone()],
                    },
                    span: span.clone(),
                });
            }
            return Some(TypedExpr {
                ty: m.return_type.clone(),
                kind: TypedExprKind::ImplFunctionCall {
                    trait_fqn: block.trait_fqn.clone(),
                    trait_type_params: block.trait_type_args.clone(),
                    for_type: typed_object.ty.clone(),
                    method_name: prop_name.clone(),
                    args: vec![typed_object.clone()],
                    method_type_params: vec![],
                },
                span: span.clone(),
            });
        }

        let generic = self.resolve_generic_trait_impl_member(
            &typed_object.ty,
            &prop_name,
            &[],
            &[],
            None,
            Some(true),
        );
        if !generic.is_empty() {
            return Some(self.resolve_overload(
                &format!("{}.{}", typed_object.ty, field.value),
                generic,
                vec![typed_object.clone()],
                span,
            ));
        }

        None
    }

    /// Materialize a capturing closure so generic and dynamic calls use the
    /// same dispatch and coercion paths as an ordinary method invocation.
    fn try_resolve_dispatched_method_ref(
        &mut self,
        object: &TypedExpr,
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        let method_name = SymbolName(field.value.clone());
        let mut signatures: Vec<Vec<Type>> = Vec::new();
        let mut interface_component = None;
        if let Type::InterfaceObject { traits, .. } = &object.ty {
            let applications: Vec<_> = traits
                .iter()
                .filter_map(|component| {
                    let sig = self
                        .registry
                        .lookup_trait(&component.trait_fqn, &self.package_path)?;
                    sig.methods
                        .iter()
                        .any(|m| m.name == field.value)
                        .then_some((
                            component.trait_fqn.clone(),
                            component.trait_type_args.clone(),
                        ))
                })
                .collect();
            if applications.is_empty() {
                return None;
            }
            let selected = if applications.len() == 1 {
                Some(0)
            } else {
                self.dedup_traits_by_member_origin(
                    &applications,
                    |sig, name| {
                        sig.methods
                            .iter()
                            .find(|m| m.name == name)
                            .and_then(|m| m.origin.clone())
                    },
                    &field.value,
                )
            };
            let Some(index) = selected else {
                self.diagnostics.error(span.clone(), format!("ambiguous reference to '{}': use an interface-qualified call to disambiguate", field.value));
                return Some(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            };
            let (fqn, args) = &applications[index];
            let sig = self.registry.lookup_trait(fqn, &self.package_path)?;
            let method = sig.methods.iter().find(|m| m.name == field.value)?;
            let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                &sig.type_params,
                args,
            )
            .with_self_type(Type::interface_object(fqn.clone(), args.clone()));
            signatures.push(
                method
                    .params
                    .iter()
                    .skip(1)
                    .map(|(_, ty)| apply_substitution(&sub, ty))
                    .collect(),
            );
            interface_component = Some((fqn.clone(), args.clone()));
        } else {
            signatures = self.generic_method_ref_parameters(object, &method_name);
        }
        if let Some(Type::Function(expected, _)) = &self.expected_type {
            signatures.retain(|params| {
                params.len() == expected.len()
                    && params
                        .iter()
                        .zip(expected)
                        .all(|(p, e)| self.is_assignable(p, e))
            });
        }
        let params = signatures.first()?.clone();
        if signatures.len() > 1 {
            self.diagnostics.error(span.clone(), format!("ambiguous reference to '{}': annotate the expected function type or call the member directly", field.value));
            return Some(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }
        let receiver_name = VarName(format!("$bound_receiver_{}_{}", span.line, span.column));
        let receiver = TypedExpr {
            kind: TypedExprKind::VarRef {
                name: receiver_name.clone(),
                boxed: false,
            },
            ty: object.ty.clone(),
            span: span.clone(),
        };
        let closure_params: Vec<_> = params
            .iter()
            .enumerate()
            .map(|(i, ty)| crate::typechecker::types::TypedClosureParam {
                name: VarName(format!("$bound_arg_{i}")),
                ty: ty.clone(),
                span: span.clone(),
            })
            .collect();
        let args: Vec<_> = closure_params
            .iter()
            .map(|p| TypedExpr {
                kind: TypedExprKind::VarRef {
                    name: p.name.clone(),
                    boxed: false,
                },
                ty: p.ty.clone(),
                span: span.clone(),
            })
            .collect();
        let saved_expected = self.expected_type.take();
        self.expected_type = match &saved_expected {
            Some(Type::Function(_, ret)) => Some((**ret).clone()),
            _ => None,
        };
        let body = if let Some((fqn, type_args)) = interface_component {
            self.resolve_interface_object_method_call(
                receiver,
                &field.value,
                &fqn,
                &type_args,
                args,
                span,
            )
        } else {
            self.resolve_concrete_type_instance_method(&receiver, field, &args, &[], span)
        };
        self.expected_type = saved_expected;
        let body = body?;
        let closure_type = Type::Function(params, Box::new(body.ty.clone()));
        let binding = TypedExpr {
            kind: TypedExprKind::Let {
                name: receiver_name,
                mutable: false,
                boxed: false,
                var_ty: object.ty.clone(),
                value: Box::new(object.clone()),
            },
            ty: Type::Unit,
            span: span.clone(),
        };
        let closure = TypedExpr {
            kind: TypedExprKind::Closure {
                params: closure_params,
                body: Box::new(body),
                captures: Vec::new(),
            },
            ty: closure_type.clone(),
            span: span.clone(),
        };
        Some(TypedExpr {
            kind: TypedExprKind::Block(vec![binding, closure]),
            ty: closure_type,
            span: span.clone(),
        })
    }

    fn generic_method_ref_parameters(
        &self,
        object: &TypedExpr,
        method_name: &SymbolName,
    ) -> Vec<Vec<Type>> {
        let Some(fqn) = object.ty.try_to_fqn() else {
            return Vec::new();
        };
        let extensions: Vec<_> = self
            .lookup_all_generic_extension_methods(&object.ty, method_name)
            .into_iter()
            .map(|(block, method)| {
                let mut bounds = block.trait_bounds;
                bounds.merge(&method.trait_bounds);
                (
                    block.for_type,
                    block.type_params,
                    method.method_type_params,
                    bounds,
                    method.params,
                    method.return_type,
                    method.is_property,
                    None,
                )
            })
            .collect();
        let implementations: Vec<_> = self
            .registry
            .find_impl_method(&fqn, method_name)
            .into_iter()
            .filter(|(_, method)| {
                method.visibility != Visibility::Private || method.span.file == self.current_file
            })
            .map(|(block, method)| {
                let mut bounds = block.trait_bounds.clone();
                bounds.merge(&method.trait_bounds);
                (
                    block.for_type.clone(),
                    block.type_params.clone(),
                    method.method_type_params.clone(),
                    bounds,
                    method.params.clone(),
                    method.return_type.clone(),
                    method.is_property,
                    Some((block.trait_fqn.clone(), block.trait_type_args.clone())),
                )
            })
            .collect();
        for methods in [extensions, implementations] {
            let mut applicable = Vec::new();
            for (receiver, block_params, method_params, bounds, params, ret, property, provider) in
                methods
            {
                if property || params.first().is_none_or(|(name, _)| name != "self") {
                    continue;
                }
                let mut sub = super::type_param_substitution::TypeParamSubstitution::new();
                if !sub.unify(&receiver, &object.ty) {
                    continue;
                }
                if let Some(Type::Function(expected_params, expected_ret)) = &self.expected_type {
                    if expected_params.len() + 1 != params.len() {
                        continue;
                    }
                    for ((_, param), expected) in params.iter().skip(1).zip(expected_params) {
                        sub.unify(param, expected);
                    }
                    sub.unify(&ret, expected_ret);
                }
                self.infer_associated_bound_types(&bounds, &mut sub);
                let all_params: Vec<_> = block_params.into_iter().chain(method_params).collect();
                let Some(args) = sub.resolve_type_params(&all_params) else {
                    continue;
                };
                if !self
                    .unsatisfied_trait_bounds(&bounds, &all_params, &args)
                    .is_empty()
                {
                    continue;
                }
                let resolved: Vec<_> = params
                    .iter()
                    .skip(1)
                    .map(|(_, ty)| apply_substitution(&sub, ty))
                    .collect();
                if let Some(Type::Function(expected, _)) = &self.expected_type
                    && !resolved
                        .iter()
                        .zip(expected)
                        .all(|(p, e)| self.is_assignable(p, e))
                {
                    continue;
                }
                let provider = provider.map(|(fqn, args)| {
                    (
                        fqn,
                        args.iter()
                            .map(|arg| apply_substitution(&sub, arg))
                            .collect(),
                    )
                });
                applicable.push((resolved, provider));
            }
            if applicable.is_empty() {
                continue;
            }
            let providers: Vec<_> = applicable
                .iter()
                .filter_map(|(_, provider)| provider.clone())
                .collect();
            if providers.len() == applicable.len()
                && providers.len() > 1
                && let Some(kept) = self.dedup_traits_by_member_origin(
                    &providers,
                    |sig, name| {
                        sig.methods
                            .iter()
                            .find(|m| m.name == name)
                            .and_then(|m| m.origin.clone())
                    },
                    &method_name.0,
                )
            {
                return vec![applicable.swap_remove(kept).0];
            }
            return applicable.into_iter().map(|(params, _)| params).collect();
        }
        Vec::new()
    }

    /// Try to resolve `obj.method` as a bound method reference (first-class value).
    /// The receiver is captured; the result type is `(non-self params) => return_type`.
    fn try_resolve_method_ref(
        &mut self,
        typed_object: &TypedExpr,
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        if typed_object.ty.is_error()
            || matches!(
                typed_object.ty,
                Type::TypeVariable(_, _) | Type::GenericParam(_, _, _)
            )
        {
            return None;
        }

        if matches!(typed_object.ty, Type::InterfaceObject { .. }) {
            return self.try_resolve_dispatched_method_ref(typed_object, field, span);
        }

        let type_fqn = typed_object.ty.try_to_fqn()?;
        let method_name = SymbolName(field.value.clone());

        let mut candidates: Vec<crate::typechecker::registry::FunctionSignature> = Vec::new();

        // 1. Class hierarchy methods
        if let Type::Class(ref class_fqn, _) = typed_object.ty {
            let mut current_fqn = class_fqn.clone();
            loop {
                if let Some(class_sig) = self
                    .registry
                    .lookup_class_type(&current_fqn, &self.package_path)
                    .cloned()
                {
                    if let Some(overloads) = class_sig.instance_methods.get(&method_name) {
                        for sig in overloads {
                            if !sig.is_property
                                && !sig.is_intrinsic
                                && !sig.params.is_empty()
                                && sig.params[0].0 == "self"
                                && self.is_member_visible(
                                    sig.visibility,
                                    &current_fqn.package,
                                    &sig.source_file,
                                )
                            {
                                candidates.push(sig.clone());
                            }
                        }
                    }
                    if candidates.is_empty()
                        && let Some(parent) = class_sig.parent_class
                    {
                        current_fqn = parent;
                        continue;
                    }
                }
                break;
            }
        }

        // 2. Module-for-type instance methods
        if candidates.is_empty()
            && let Some(module_info) = self.registry.lookup_module(&type_fqn).cloned()
            && let Some(overloads) = module_info.functions.get(&method_name)
        {
            for sig in overloads {
                if !sig.is_property
                    && !sig.is_intrinsic
                    && !sig.params.is_empty()
                    && sig.params[0].0 == "self"
                    && self.is_member_visible(
                        sig.visibility,
                        &module_info.fqn.package,
                        &sig.source_file,
                    )
                {
                    candidates.push(sig.clone());
                }
            }
        }

        // 3. Named extension instance methods (extensions take priority over
        // trait impls, appendix §2.1)
        let mut from_extension = false;
        let mut ext_block_methods: Vec<(ExtensionBlockSignature, ExtMethodSignature)> = Vec::new();
        if candidates.is_empty() {
            let ext_overloads = self.lookup_named_extension_methods(&type_fqn, &method_name);
            for (block, method) in ext_overloads {
                if !method.is_property
                    && !method.is_intrinsic
                    && !method.params.is_empty()
                    && method.params[0].0 == "self"
                    // Sibling blocks on different instantiations share a base
                    // FQN — only candidates whose self param accepts the
                    // receiver participate.
                    && self.is_assignable(&method.params[0].1, &typed_object.ty)
                {
                    ext_block_methods.push((block, method));
                    from_extension = true;
                }
            }
        }

        if candidates.is_empty()
            && ext_block_methods.is_empty()
            && let Some(reference) =
                self.try_resolve_dispatched_method_ref(typed_object, field, span)
        {
            return Some(reference);
        }

        // 4. Trait impl instance methods. Sibling-instantiation-aware: only
        // blocks whose self param accepts the receiver contribute candidates
        // (`Tr for List<Int32>` vs `Tr for List<String>` share a base FQN).
        let mut from_trait_impl = false;
        if candidates.is_empty() && ext_block_methods.is_empty() {
            let impl_results: Vec<_> = self
                .registry
                .find_impl_method(&type_fqn, &method_name)
                .into_iter()
                .map(|(b, m)| (b.clone(), m.clone()))
                .collect();
            // One inherited declaration reached through a trait and a
            // sub-trait that extends it is ONE candidate — the origin's
            // direct impl wins, as on the call paths. (Narrow among the
            // blocks that actually accept this receiver.)
            let applicable: Vec<_> = impl_results
                .iter()
                .filter(|(b, m)| {
                    b.type_params.is_empty()
                        && m.method_type_params.is_empty()
                        && !m.is_property
                        && !m.params.is_empty()
                        && m.params[0].0 == "self"
                        && self.is_assignable(&m.params[0].1, &typed_object.ty)
                })
                .map(|(b, _)| (b.trait_fqn.clone(), b.trait_type_args.clone()))
                .fold(Vec::new(), |mut acc, key| {
                    if !acc.contains(&key) {
                        acc.push(key);
                    }
                    acc
                });
            let origin_only: Option<(Fqn, Vec<Type>)> = if applicable.len() > 1 {
                self.dedup_traits_by_member_origin(
                    &applicable,
                    |sig, member| {
                        sig.methods
                            .iter()
                            .find(|mm| mm.name == member)
                            .and_then(|mm| mm.origin.clone())
                    },
                    &method_name.0,
                )
                .map(|i| applicable[i].clone())
            } else {
                None
            };
            for (b, m) in impl_results {
                if let Some((keep_fqn, keep_args)) = &origin_only
                    && (b.trait_fqn != *keep_fqn || b.trait_type_args != *keep_args)
                {
                    continue;
                }
                if b.type_params.is_empty()
                    && m.method_type_params.is_empty()
                    && !m.is_property
                    && !m.is_intrinsic
                    && !m.params.is_empty()
                    && m.params[0].0 == "self"
                    && self.is_assignable(&m.params[0].1, &typed_object.ty)
                    && (m.visibility != Visibility::Private || m.span.file == self.current_file)
                {
                    candidates.push(FunctionSignature {
                        visibility: m.visibility,
                        mangled_name: crate::typechecker::types::impl_member_mangled_name(
                            &b.trait_fqn,
                            &b.for_type,
                            &b.type_params,
                            &m.dispatch_name,
                            &b.trait_type_args,
                        ),
                        params: m.params.clone(),
                        return_type: m.return_type.clone(),
                        source_file: b.source_file.clone(),
                        is_intrinsic: m.is_intrinsic,
                        is_property: m.is_property,
                        is_final_method: false,
                        is_abstract_method: false,
                    });
                    from_trait_impl = true;
                }
            }
        }

        if candidates.is_empty() && ext_block_methods.is_empty() {
            return None;
        }

        // A multi-candidate reference with no expected function type is a real
        // ambiguity — report it as such rather than letting the caller's
        // "no field" fallthrough deny the member exists.
        let ambiguous_ref = |this: &mut Self, count: usize| -> Option<TypedExpr> {
            this.diagnostics.error(
                span.clone(),
                format!(
                    "ambiguous reference to '{}': {} candidates match; annotate the expected function type or call the member directly",
                    field.value, count,
                ),
            );
            Some(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            })
        };

        if from_trait_impl {
            let count = candidates.len();
            return self
                .try_resolve_impl_method_ref(
                    &candidates,
                    typed_object,
                    &type_fqn,
                    &method_name,
                    span,
                )
                .or_else(|| {
                    if count > 1 {
                        ambiguous_ref(self, count)
                    } else {
                        None
                    }
                });
        }

        if from_extension {
            let count = ext_block_methods.len();
            return self
                .try_resolve_ext_bound_method_from_candidates(
                    &ext_block_methods,
                    typed_object,
                    &method_name,
                    &format!("{}.{}", typed_object.ty, field.value),
                    span,
                )
                .or_else(|| {
                    if count > 1 {
                        ambiguous_ref(self, count)
                    } else {
                        None
                    }
                });
        }

        let count = candidates.len();
        self.try_resolve_bound_method_from_candidates(
            &candidates,
            typed_object,
            &format!("{}.{}", typed_object.ty, field.value),
            span,
        )
        .or_else(|| {
            if count > 1 {
                ambiguous_ref(self, count)
            } else {
                None
            }
        })
    }

    /// Given a list of instance method candidates, try to resolve a bound method reference.
    /// Strips self from the function type. Uses expected type or single-overload inference.
    fn try_resolve_bound_method_from_candidates(
        &self,
        candidates: &[crate::typechecker::registry::FunctionSignature],
        typed_object: &TypedExpr,
        display_name: &str,
        span: &Span,
    ) -> Option<TypedExpr> {
        // Try to disambiguate using expected type (compare against non-self params)
        if let Some(Type::Function(expected_params, _)) = &self.expected_type {
            let matching: Vec<_> = candidates
                .iter()
                .filter(|sig| {
                    let non_self_params: Vec<_> =
                        sig.params.iter().skip(1).map(|(_, ty)| ty).collect();
                    non_self_params.len() == expected_params.len()
                        && non_self_params.iter().zip(expected_params.iter()).all(
                            |(param_ty, exp_ty)| {
                                self.is_assignable(param_ty, exp_ty)
                                    || self.is_assignable(exp_ty, param_ty)
                            },
                        )
                })
                .collect();

            if matching.len() == 1 {
                let sig = matching[0];
                let non_self_params: Vec<Type> = sig
                    .params
                    .iter()
                    .skip(1)
                    .map(|(_, ty)| ty.clone())
                    .collect();
                let func_ty = Type::Function(non_self_params, Box::new(sig.return_type.clone()));
                return Some(TypedExpr {
                    kind: TypedExprKind::MethodRef {
                        object: Box::new(typed_object.clone()),
                        method_name: sig.mangled_name.clone(),
                        type_params: vec![],
                    },
                    ty: func_ty,
                    span: span.clone(),
                });
            }
        }

        // Single candidate — infer without expected type
        if candidates.len() == 1 {
            let sig = &candidates[0];
            let non_self_params: Vec<Type> = sig
                .params
                .iter()
                .skip(1)
                .map(|(_, ty)| ty.clone())
                .collect();
            let func_ty = Type::Function(non_self_params, Box::new(sig.return_type.clone()));
            return Some(TypedExpr {
                kind: TypedExprKind::MethodRef {
                    object: Box::new(typed_object.clone()),
                    method_name: sig.mangled_name.clone(),
                    type_params: vec![],
                },
                ty: func_ty,
                span: span.clone(),
            });
        }

        // Multiple overloads, no match — report ambiguity
        let _ = display_name;
        None
    }

    /// Like `try_resolve_bound_method_from_candidates`, but produces `ExtFunctionRef` for extension methods.
    fn try_resolve_ext_bound_method_from_candidates(
        &self,
        candidates: &[(ExtensionBlockSignature, ExtMethodSignature)],
        typed_object: &TypedExpr,
        method_name: &SymbolName,
        display_name: &str,
        span: &Span,
    ) -> Option<TypedExpr> {
        let make_ext_method_ref =
            |block: &ExtensionBlockSignature, method: &ExtMethodSignature| -> TypedExpr {
                let non_self_params: Vec<Type> = method
                    .params
                    .iter()
                    .skip(1)
                    .map(|(_, ty)| ty.clone())
                    .collect();
                let func_ty = Type::Function(non_self_params, Box::new(method.return_type.clone()));
                // A BOUND reference must capture the receiver — emit a MethodRef
                // to the extension method's concrete mangled function (the
                // receiver-less ExtFunctionRef node is for unbound `Type.method`
                // references only).
                let param_types: Vec<&Type> = method.params.iter().map(|(_, ty)| ty).collect();
                let mangled = crate::common::types::MangledName::for_named_extension_method(
                    &block.ext_fqn.package,
                    &block.ext_fqn.symbol,
                    method_name,
                    &block.for_type,
                    &param_types,
                );
                TypedExpr {
                    kind: TypedExprKind::MethodRef {
                        object: Box::new(typed_object.clone()),
                        method_name: mangled,
                        type_params: vec![],
                    },
                    ty: func_ty,
                    span: span.clone(),
                }
            };

        // Try to disambiguate using expected type (compare against non-self params)
        if let Some(Type::Function(expected_params, _)) = &self.expected_type {
            let matching: Vec<_> = candidates
                .iter()
                .filter(|(_, method)| {
                    let non_self_params: Vec<_> =
                        method.params.iter().skip(1).map(|(_, ty)| ty).collect();
                    non_self_params.len() == expected_params.len()
                        && non_self_params.iter().zip(expected_params.iter()).all(
                            |(param_ty, exp_ty)| {
                                self.is_assignable(param_ty, exp_ty)
                                    || self.is_assignable(exp_ty, param_ty)
                            },
                        )
                })
                .collect();

            if matching.len() == 1 {
                return Some(make_ext_method_ref(&matching[0].0, &matching[0].1));
            }
        }

        // Single candidate — infer without expected type
        if candidates.len() == 1 {
            return Some(make_ext_method_ref(&candidates[0].0, &candidates[0].1));
        }

        let _ = display_name;
        None
    }

    fn try_resolve_impl_method_ref(
        &self,
        candidates: &[crate::typechecker::registry::FunctionSignature],
        typed_object: &TypedExpr,
        type_fqn: &Fqn,
        method_name: &SymbolName,
        span: &Span,
    ) -> Option<TypedExpr> {
        let sig = if candidates.len() == 1 {
            &candidates[0]
        } else if let Some(Type::Function(expected_params, _)) = &self.expected_type {
            let matching: Vec<_> = candidates
                .iter()
                .filter(|sig| {
                    let non_self_params: Vec<_> =
                        sig.params.iter().skip(1).map(|(_, ty)| ty).collect();
                    non_self_params.len() == expected_params.len()
                        && non_self_params.iter().zip(expected_params.iter()).all(
                            |(param_ty, exp_ty)| {
                                self.is_assignable(param_ty, exp_ty)
                                    || self.is_assignable(exp_ty, param_ty)
                            },
                        )
                })
                .collect();
            if matching.len() == 1 {
                matching[0]
            } else {
                return None;
            }
        } else {
            return None;
        };

        let non_self_params: Vec<Type> = sig
            .params
            .iter()
            .skip(1)
            .map(|(_, ty)| ty.clone())
            .collect();
        let func_ty = Type::Function(non_self_params, Box::new(sig.return_type.clone()));

        // The selected candidate already carries its block's for_type-aware
        // mangled name — re-deriving from `find_impl_method(...).first()`
        // would pick an arbitrary sibling block.
        let _ = (type_fqn, method_name);
        Some(TypedExpr {
            kind: TypedExprKind::MethodRef {
                object: Box::new(typed_object.clone()),
                method_name: sig.mangled_name.clone(),
                type_params: vec![],
            },
            ty: func_ty,
            span: span.clone(),
        })
    }

    /// Try to resolve a property access on a interface object type.
    pub(super) fn try_resolve_interface_object_property(
        &mut self,
        typed_object: &TypedExpr,
        trait_fqn: &Fqn,
        trait_type_args: &[Type],
        field: &crate::common::span::Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        let trait_sig = self
            .registry
            .lookup_trait(trait_fqn, &self.package_path)?
            .clone();

        // Check properties
        if let Some(prop_sig) = trait_sig.properties.iter().find(|p| p.name == field.value) {
            // `Self` resolves to the declaring COMPONENT's interface type (see
            // resolve_interface_object_method_call) — the ORIGIN trait for
            // inherited members.
            let self_type = match &prop_sig.origin {
                None => Type::interface_object(trait_fqn.clone(), trait_type_args.to_vec()),
                Some((origin_fqn, origin_args)) => {
                    let owner_subst: std::collections::BTreeMap<
                        crate::common::types::TypeParamName,
                        Type,
                    > = trait_sig
                        .type_params
                        .iter()
                        .cloned()
                        .zip(trait_type_args.iter().cloned())
                        .collect();
                    let substituted: Vec<Type> = origin_args
                        .iter()
                        .map(|t| {
                            crate::typechecker::collect::substitute_trait_type_params(
                                t,
                                &owner_subst,
                            )
                        })
                        .collect();
                    Type::interface_object(origin_fqn.clone(), substituted)
                }
            };
            let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                &trait_sig.type_params,
                trait_type_args,
            )
            .with_self_type(self_type);
            let return_type = apply_substitution(&sub, &prop_sig.return_type);
            // Properties have no non-self params
            let member_name = InterfaceMemberName::new(&field.value, &[]);
            // The node's key is the declaring COMPONENT's per-trait key (equal to
            // the set key for a single interface) — the ORIGIN trait's key for
            // inherited properties (their slot lives in the nested super vtable).
            let interface_mangled_name = match &prop_sig.origin {
                Some((origin_fqn, _)) => {
                    MangledName::for_interface_object_per_interface(origin_fqn)
                }
                None => MangledName::for_interface_object_per_interface(trait_fqn),
            };
            return Some(TypedExpr {
                kind: TypedExprKind::InterfaceObjectMethodCall {
                    interface_mangled_name,
                    method_name: field.value.clone(),
                    member_name,
                    receiver: Box::new(typed_object.clone()),
                    args: vec![],
                },
                ty: return_type,
                span: span.clone(),
            });
        }

        None
    }

    /// When inside a module, try resolving a bare identifier as a static module property.
    /// Properties are registered as functions (0-param, `is_property: true`), not globals,
    /// so they aren't found by `lookup_global`. This handles `version` → `Math.version` call.
    fn try_resolve_bare_module_property(&self, name: &str, span: &Span) -> Option<TypedExpr> {
        let module_name = self.container_name.as_ref()?;
        let qualified = SymbolName(format!("{}.{}", module_name, name));
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: qualified,
        };
        let overloads =
            self.registry
                .lookup_function(&fqn, &self.package_path, &self.current_file)?;
        let prop = overloads
            .iter()
            .find(|sig| sig.is_property && sig.params.is_empty())?;
        Some(TypedExpr {
            ty: prop.return_type.clone(),
            kind: TypedExprKind::FunctionCall {
                name: prop.mangled_name.clone(),
                args: vec![],
                type_params: vec![],
            },
            span: span.clone(),
        })
    }

    /// Try to resolve a bare identifier as a function reference (first-class named function).
    /// Returns `Some(TypedExpr)` if the name resolves to a non-generic, non-property function.
    /// Uses expected_type to disambiguate overloads and infer generic type args.
    fn try_resolve_function_ref(&mut self, name: &str, span: &Span) -> Option<TypedExpr> {
        let fqn = self.resolve_fqn(name, super::types::SymbolKind::Function)?;

        // Collect non-generic candidates (filter out properties)
        let non_generic: Vec<_> = self
            .registry
            .lookup_function(&fqn, &self.package_path, &self.current_file)
            .unwrap_or_default()
            .into_iter()
            .filter(|sig| !sig.is_property)
            .collect();

        // Check for generic overloads
        let has_generics = self
            .registry
            .lookup_generic_function(&fqn, &self.package_path)
            .is_some_and(|defs| !defs.is_empty());

        if non_generic.is_empty() && !has_generics {
            return None;
        }

        // Try to disambiguate using expected type
        if let Some(Type::Function(expected_params, expected_ret)) = &self.expected_type {
            // 1. Try non-generic overloads
            let matching: Vec<_> = non_generic
                .iter()
                .filter(|sig| {
                    sig.params.len() == expected_params.len()
                        && sig.params.iter().zip(expected_params.iter()).all(
                            |((_, param_ty), exp_ty)| {
                                self.is_assignable(param_ty, exp_ty)
                                    || self.is_assignable(exp_ty, param_ty)
                            },
                        )
                })
                .collect();

            if matching.len() == 1 {
                let sig = matching[0];
                let param_types: Vec<Type> = sig.params.iter().map(|(_, ty)| ty.clone()).collect();
                let func_ty = Type::Function(param_types, Box::new(sig.return_type.clone()));
                return Some(TypedExpr {
                    kind: TypedExprKind::FunctionRef {
                        name: sig.mangled_name.clone(),
                        type_params: vec![],
                    },
                    ty: func_ty,
                    span: span.clone(),
                });
            } else if matching.len() > 1 {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "ambiguous function reference '{}'; multiple overloads match the expected type",
                        name
                    ),
                );
                return Some(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }

            // 2. Try generic overloads — infer type args from expected function type
            if has_generics {
                let expected_params = expected_params.clone();
                let expected_ret = expected_ret.clone();
                if let Some(result) = self.try_resolve_generic_function_ref(
                    &fqn,
                    &expected_params,
                    &expected_ret,
                    span,
                ) {
                    return Some(result);
                }
            }
        }

        // No expected type or no match — use single non-generic overload
        if non_generic.len() == 1 && !has_generics {
            let sig = &non_generic[0];
            let param_types: Vec<Type> = sig.params.iter().map(|(_, ty)| ty.clone()).collect();
            let func_ty = Type::Function(param_types, Box::new(sig.return_type.clone()));
            return Some(TypedExpr {
                kind: TypedExprKind::FunctionRef {
                    name: sig.mangled_name.clone(),
                    type_params: vec![],
                },
                ty: func_ty,
                span: span.clone(),
            });
        }

        if non_generic.is_empty() && has_generics {
            // Only generic overloads exist but no expected type to infer from
            self.diagnostics.error(
                span.clone(),
                format!(
                    "cannot infer type arguments for generic function '{}'; provide a type annotation",
                    name
                ),
            );
        } else {
            // Multiple overloads and no expected type — ambiguous
            self.diagnostics.error(
                span.clone(),
                format!(
                    "ambiguous function reference '{}'; provide a type annotation to disambiguate",
                    name
                ),
            );
        }
        Some(TypedExpr {
            kind: TypedExprKind::UnitLiteral,
            ty: Type::Error,
            span: span.clone(),
        })
    }

    /// Try to instantiate a generic function as a function reference using expected type.
    /// Unifies the generic def's param/return types against the expected function type
    /// to infer type arguments, then instantiates.
    fn try_resolve_generic_function_ref(
        &mut self,
        fqn: &Fqn,
        expected_params: &[Type],
        expected_ret: &Type,
        span: &Span,
    ) -> Option<TypedExpr> {
        let generic_defs = self
            .registry
            .lookup_generic_function(fqn, &self.package_path)?
            .to_vec();
        let mut matched = Vec::new();

        for def in &generic_defs {
            if def.params.len() != expected_params.len() {
                continue;
            }
            let mut substitution = super::type_param_substitution::TypeParamSubstitution::new();
            let mut ok = true;
            for ((_, param_ty), exp_ty) in def.params.iter().zip(expected_params.iter()) {
                if !substitution.unify(param_ty, exp_ty) {
                    ok = false;
                    break;
                }
            }
            if ok && !substitution.unify(&def.return_type, expected_ret) {
                ok = false;
            }
            if !ok {
                continue;
            }
            if let Some(type_args) = substitution.resolve_type_params(&def.type_params) {
                matched.push((def.clone(), type_args));
            }
        }

        if matched.len() == 1 {
            let (def, type_args) = &matched[0];
            if !self.check_trait_bounds(&def.trait_bounds, &def.type_params, type_args, span) {
                return None;
            }
            let (mangled, concrete_ret) = self.resolve_generic_function_template(
                fqn,
                def,
                type_args,
                super::generic_functions::MethodKind::FreeFunction,
            );
            let substitution = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                &def.type_params,
                type_args,
            );
            let concrete_params: Vec<Type> = def
                .params
                .iter()
                .map(|(_, ty)| apply_substitution(&substitution, ty))
                .collect();
            let func_ty = Type::Function(concrete_params, Box::new(concrete_ret));
            return Some(TypedExpr {
                kind: TypedExprKind::FunctionRef {
                    name: mangled,
                    type_params: type_args.clone(),
                },
                ty: func_ty,
                span: span.clone(),
            });
        }

        None
    }

    /// Resolve generic module functions as values using explicit arguments or
    /// the expected function signature. Calls and adapters share bound checks.
    fn try_resolve_generic_module_function_ref(
        &mut self,
        module: &crate::typechecker::registry::ModuleInfo,
        member: &SymbolName,
        module_args: &[TypeExpr],
        method_args: &[TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        use super::type_param_substitution::TypeParamSubstitution;
        let defs: Vec<_> = module
            .generic_members
            .lookup_visible(member, &self.package_path, &self.current_file)
            .into_iter()
            .cloned()
            .collect();
        let mut matches = Vec::new();
        let mut failures = std::collections::BTreeSet::new();
        for def in defs {
            if def.is_property || def.params.first().is_some_and(|p| p.0 == "self") {
                continue;
            }
            let params: Vec<_> = def
                .type_params
                .iter()
                .chain(&def.method_type_params)
                .cloned()
                .collect();
            let mut sub = TypeParamSubstitution::new();
            let mut valid = true;
            for (names, args) in [
                (&def.type_params, module_args),
                (&def.method_type_params, method_args),
            ] {
                if args.is_empty() {
                    continue;
                }
                if names.len() != args.len() {
                    valid = false;
                    break;
                }
                let Some(types) = self.resolve_type_args(args) else {
                    valid = false;
                    break;
                };
                for (name, ty) in names.iter().zip(types) {
                    sub.insert(name.clone(), ty);
                }
            }
            if !valid {
                continue;
            }
            if sub.resolve_type_params(&params).is_none()
                && let Some(Type::Function(expected_params, expected_ret)) = &self.expected_type
            {
                if expected_params.len() != def.params.len() {
                    continue;
                }
                for ((_, ty), expected) in def.params.iter().zip(expected_params) {
                    valid &= sub.unify(ty, expected);
                }
                valid &= sub.unify(&def.return_type, expected_ret);
            }
            if !valid {
                continue;
            }
            let Some(types) = sub.resolve_type_params(&params) else {
                continue;
            };
            let function_type = Type::Function(
                def.params
                    .iter()
                    .map(|(_, ty)| apply_substitution(&sub, ty))
                    .collect(),
                Box::new(apply_substitution(&sub, &def.return_type)),
            );
            if let Some(expected @ Type::Function(..)) = &self.expected_type
                && !self.is_assignable(expected, &function_type)
            {
                continue;
            }
            let mut bounds = module.trait_bounds.clone();
            bounds.merge(&def.trait_bounds);
            let unsatisfied = self.unsatisfied_trait_bounds(&bounds, &params, &types);
            if !unsatisfied.is_empty() {
                failures.extend(unsatisfied);
                continue;
            }
            if def.is_intrinsic
                && Self::class_identity_intrinsic_kind(&module.fqn, &member.0).is_none()
            {
                failures.insert(format!(
                    "intrinsic '{}.{}' cannot be used as a function value; wrap the call in a lambda",
                    module.fqn, member
                ));
                continue;
            }
            let fqn = Fqn {
                package: def.package.clone(),
                symbol: SymbolName(format!("{}.{}", module.fqn.symbol, member)),
            };
            let signature: Vec<_> = def.params.iter().map(|(_, ty)| ty).collect();
            matches.push(TypedExpr {
                kind: TypedExprKind::FunctionRef {
                    name: MangledName::for_function(&fqn, &signature),
                    type_params: types,
                },
                ty: function_type,
                span: span.clone(),
            });
        }
        if matches.is_empty() {
            for message in failures {
                self.diagnostics.error(span.clone(), message);
            }
        }
        if matches.len() == 1 {
            matches.pop()
        } else {
            None
        }
    }

    /// Shared helper: given a set of function overloads (non-property, static),
    /// try to resolve them as a function reference using expected type or single-overload inference.
    fn try_resolve_overloaded_function_ref(
        &self,
        overloads: &[crate::typechecker::registry::FunctionSignature],
        display_name: &str,
        span: &Span,
    ) -> Option<TypedExpr> {
        let candidates: Vec<_> = overloads.iter().filter(|sig| !sig.is_property).collect();

        if candidates.is_empty() {
            return None;
        }

        // Try to disambiguate using expected type
        if let Some(Type::Function(expected_params, _)) = &self.expected_type {
            let matching: Vec<_> = candidates
                .iter()
                .filter(|sig| {
                    sig.params.len() == expected_params.len()
                        && sig.params.iter().zip(expected_params.iter()).all(
                            |((_, param_ty), exp_ty)| {
                                self.is_assignable(param_ty, exp_ty)
                                    || self.is_assignable(exp_ty, param_ty)
                            },
                        )
                })
                .collect();

            if matching.len() == 1 {
                let sig = matching[0];
                let param_types: Vec<Type> = sig.params.iter().map(|(_, ty)| ty.clone()).collect();
                let func_ty = Type::Function(param_types, Box::new(sig.return_type.clone()));
                return Some(TypedExpr {
                    kind: TypedExprKind::FunctionRef {
                        name: sig.mangled_name.clone(),
                        type_params: vec![],
                    },
                    ty: func_ty,
                    span: span.clone(),
                });
            }
        }

        // Single candidate — infer without expected type
        if candidates.len() == 1 {
            let sig = candidates[0];
            let param_types: Vec<Type> = sig.params.iter().map(|(_, ty)| ty.clone()).collect();
            let func_ty = Type::Function(param_types, Box::new(sig.return_type.clone()));
            return Some(TypedExpr {
                kind: TypedExprKind::FunctionRef {
                    name: sig.mangled_name.clone(),
                    type_params: vec![],
                },
                ty: func_ty,
                span: span.clone(),
            });
        }

        // Multiple overloads, no match — don't emit error here, let caller fall through
        // (the member name may resolve via other dispatch paths)
        let _ = display_name;
        None
    }

    /// Like `try_resolve_overloaded_function_ref`, but produces `ExtFunctionRef` for extension methods.
    fn try_resolve_ext_function_ref(
        &self,
        overloads: &[(ExtensionBlockSignature, ExtMethodSignature)],
        for_type: &Type,
        method_name: &SymbolName,
        display_name: &str,
        span: &Span,
    ) -> Option<TypedExpr> {
        let candidates: Vec<_> = overloads
            .iter()
            .filter(|(_, m)| {
                !m.is_property || m.params.first().is_some_and(|(name, _)| name == "self")
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        let make_ext_ref = |block: &ExtensionBlockSignature,
                            method: &ExtMethodSignature|
         -> TypedExpr {
            let param_types: Vec<Type> = method.params.iter().map(|(_, ty)| ty.clone()).collect();
            let func_ty = Type::Function(param_types, Box::new(method.return_type.clone()));
            TypedExpr {
                kind: TypedExprKind::ExtFunctionRef {
                    ext_fqn: block.ext_fqn.clone(),
                    for_type: for_type.clone(),
                    method_name: method_name.clone(),
                    type_params: vec![],
                },
                ty: func_ty,
                span: span.clone(),
            }
        };

        // Try to disambiguate using expected type
        if let Some(Type::Function(expected_params, _)) = &self.expected_type {
            let matching: Vec<_> = candidates
                .iter()
                .filter(|(_, method)| {
                    method.params.len() == expected_params.len()
                        && method.params.iter().zip(expected_params.iter()).all(
                            |((_, param_ty), exp_ty)| {
                                self.is_assignable(param_ty, exp_ty)
                                    || self.is_assignable(exp_ty, param_ty)
                            },
                        )
                })
                .collect();

            if matching.len() == 1 {
                return Some(make_ext_ref(&matching[0].0, &matching[0].1));
            }
        }

        // Single candidate — infer without expected type
        if candidates.len() == 1 {
            return Some(make_ext_ref(&candidates[0].0, &candidates[0].1));
        }

        let _ = display_name;
        None
    }

    /// Try to resolve `None` as `Option.None`.
    /// Only `None` is a bare no-payload prelude variant; `Some`, `Ok`, `Error` require call syntax.
    fn try_resolve_bare_variant_identifier(
        &mut self,
        name: &str,
        span: &Span,
    ) -> Option<TypedExpr> {
        if name != "None" {
            return None;
        }
        let enum_name = "Option";
        let enum_sig = self.resolve_enum_type(enum_name)?;
        self.check_private_type_access(
            &enum_sig.fqn,
            enum_sig.construction_private,
            "enum",
            span,
            "construct",
        );

        let type_args: Vec<Type> = match &self.expected_type {
            Some(Type::GenericEnum {
                fqn: exp_fqn,
                type_args,
                ..
            }) if *exp_fqn == enum_sig.fqn => type_args.iter().map(|(_, t)| t.clone()).collect(),
            _ => {
                let sub = super::type_param_substitution::TypeParamSubstitution::new();
                match sub.resolve_with_variance_defaults(
                    &enum_sig.type_params,
                    &enum_sig.type_param_variances,
                ) {
                    Some(args) => args,
                    None => {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "cannot infer type arguments for '{}'; add a type annotation",
                                name,
                            ),
                        );
                        return Some(TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        });
                    }
                }
            }
        };
        let fqn = enum_sig.fqn.clone();
        let enum_ty = self.resolve_generic_enum_type(&fqn, &enum_sig, &type_args);
        Some(TypedExpr {
            ty: enum_ty,
            kind: TypedExprKind::EnumCreate {
                fqn,
                variant_name: name.to_string(),
                args: vec![],
                type_params: type_args.clone(),
            },
            span: span.clone(),
        })
    }

    /// Try to resolve a static property: `Type.property` where the property has 0 params.
    fn try_resolve_static_property(
        &mut self,
        type_name: &str,
        field: &Spanned<String>,
        explicit_type_args: &[crate::parser::ast::TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        // A GLOBAL with this name and no competing type: the access is a
        // field read on the global's value — bail before `resolve_type_name`
        // diagnoses the name as a type (e.g. a plain trait) while probing.
        if self
            .resolve_fqn(type_name, super::types::SymbolKind::Global)
            .is_some()
            && self
                .resolve_fqn(type_name, super::types::SymbolKind::Record)
                .is_none()
            && self
                .resolve_fqn(type_name, super::types::SymbolKind::Enum)
                .is_none()
            && self
                .resolve_fqn(type_name, super::types::SymbolKind::Class)
                .is_none()
            && self
                .resolve_fqn(type_name, super::types::SymbolKind::Newtype)
                .is_none()
        {
            return None;
        }

        // `TraitName.prop` — a static property reached through the trait's
        // name: select the unique non-generic implementing block (mirrors the
        // explicit `TraitName.staticFn(...)` call form). Checked before
        // `resolve_type_name` so a plain trait name isn't diagnosed as
        // "cannot be used as a type" while merely being probed.
        if self.lookup_variable(type_name).is_none()
            && self
                .resolve_fqn(type_name, super::types::SymbolKind::Global)
                .is_none()
            && self
                .resolve_fqn(type_name, super::types::SymbolKind::Record)
                .is_none()
            && self
                .resolve_fqn(type_name, super::types::SymbolKind::Enum)
                .is_none()
            && self
                .resolve_fqn(type_name, super::types::SymbolKind::Class)
                .is_none()
            && self
                .registry
                .lookup_module(&crate::common::types::Fqn {
                    package: self.package_path.clone(),
                    symbol: crate::common::types::SymbolName(type_name.to_string()),
                })
                .is_none()
            && let Some(trait_fqn) = self.resolve_trait_fqn(type_name)
        {
            let is_interface = self
                .registry
                .lookup_trait(&trait_fqn, &self.package_path)
                .is_some_and(|sig| sig.is_interface);
            let declares_static_prop = self
                .registry
                .lookup_trait(&trait_fqn, &self.package_path)
                .is_some_and(|sig| {
                    sig.properties.iter().any(|p| {
                        p.name == field.value && !p.params.iter().any(|(n, _)| n == "self")
                    })
                });
            if declares_static_prop {
                let collect_from = |blocks: Vec<
                        &crate::typechecker::registry::ImplBlockSignature,
                    >|
                     -> Vec<(
                        crate::common::types::Fqn,
                        Type,
                        Vec<Type>,
                        Type,
                    )> {
                        blocks
                            .iter()
                            .flat_map(|b| {
                                b.properties
                                    .iter()
                                    .filter(|m| m.name.0 == field.value && m.params.is_empty())
                                    .map(|m| {
                                        (
                                            b.trait_fqn.clone(),
                                            b.for_type.clone(),
                                            b.trait_type_args.clone(),
                                            m.return_type.clone(),
                                        )
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .collect()
                    };
                // `Conv<Int32>.prop` restricts selection to blocks of
                // that trait application, both direct and provided —
                // mirroring the explicit static-call form.
                let required_trait_args: Vec<Type> = explicit_type_args
                    .iter()
                    .map(|te| self.resolve_type_expr(te))
                    .collect();
                let direct: Vec<_> = self
                    .registry
                    .all_implement_blocks()
                    .iter()
                    .filter(|b| {
                        b.trait_fqn == trait_fqn
                            && b.type_params.is_empty()
                            && (required_trait_args.is_empty()
                                || b.trait_type_args == required_trait_args)
                    })
                    .collect();
                let mut matching = collect_from(direct);
                if matching.is_empty() {
                    // "B satisfies A everywhere": a sub-trait provider
                    // block carries the static property inline.
                    let providers: Vec<_> = self
                        .registry
                        .all_implement_blocks()
                        .iter()
                        .filter(|b| {
                            b.trait_fqn != trait_fqn
                                && b.type_params.is_empty()
                                && self
                                    .registry
                                    .super_closure_args(
                                        &b.trait_fqn,
                                        &b.trait_type_args,
                                        &trait_fqn,
                                    )
                                    .is_some_and(|args| {
                                        required_trait_args.is_empty()
                                            || args == required_trait_args
                                    })
                        })
                        .collect();
                    matching = collect_from(providers);
                }
                match matching.len() {
                    1 => {
                        // Target the selected block's own trait — a
                        // provider block keeps its identity so sibling
                        // providers don't collapse at monomorphize.
                        let (block_trait_fqn, for_type, trait_args, return_type) =
                            matching.into_iter().next().unwrap();
                        return Some(TypedExpr {
                            ty: return_type,
                            kind: TypedExprKind::ImplFunctionCall {
                                trait_fqn: block_trait_fqn,
                                trait_type_params: trait_args,
                                for_type,
                                method_name: crate::common::types::SymbolName(field.value.clone()),
                                args: vec![],
                                method_type_params: vec![],
                            },
                            span: span.clone(),
                        });
                    }
                    0 => {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "no implementation of trait '{}' provides a static property '{}'",
                                type_name, field.value,
                            ),
                        );
                        return Some(TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        });
                    }
                    n => {
                        self.diagnostics.error(
                                span.clone(),
                                format!(
                                    "ambiguous access to '{}.{}': implemented for {} types; access it on a concrete type instead",
                                    type_name, field.value, n,
                                ),
                            );
                        return Some(TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        });
                    }
                }
            } else {
                self.diagnostics.error(
                    span.clone(),
                    if is_interface {
                        format!(
                            "interface '{}' member '{}' requires a receiver",
                            type_name, field.value
                        )
                    } else {
                        format!(
                            "trait '{}' has no static property '{}'",
                            type_name, field.value
                        )
                    },
                );
                return Some(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }
            // Interface without such a static property: fall through — the
            // interface-object type path may still resolve the member.
        }

        // Try class static property first
        if let Some(result) =
            self.try_resolve_class_static_property(type_name, field, explicit_type_args, span)
        {
            return Some(result);
        }

        // Resolve the type. `resolve_type_name` handles all cases —
        // primitives, type parameters, Array<T>, generic records/enums/
        // classes/newtypes — based on `explicit_type_args`.
        let resolved_type = self.resolve_type_name(type_name, explicit_type_args, span)?;
        if resolved_type.is_error() {
            return None;
        }

        // Handle type parameter: look up static properties from trait bounds.
        // Produces ImplFunctionCall so monomorphize can resolve after substitution.
        if let Type::TypeVariable(_tp_name, bounds) | Type::GenericParam(_tp_name, bounds, _) =
            &resolved_type
        {
            let bounds = bounds.clone();
            // Every bound declaring the static property competes: a trait and
            // a sub-trait extending it are ONE declaration (origin wins), and
            // otherwise bound ORDER must not silently decide the result.
            let matching: Vec<(usize, Type)> = bounds
                .iter()
                .enumerate()
                .filter_map(|(i, bound)| {
                    let bound = bound.named()?;
                    let trait_sig = self
                        .registry
                        .lookup_trait(&bound.trait_fqn, &self.package_path)
                        .cloned()?;
                    let prop = trait_sig
                        .properties
                        .iter()
                        .find(|p| p.name == field.value && p.params.is_empty())?;
                    let return_type = if prop.return_type == Type::SelfType {
                        resolved_type.clone()
                    } else {
                        prop.return_type.clone()
                    };
                    Some((i, return_type))
                })
                .collect();
            let matching = if matching.len() > 1 {
                match self.dedup_bound_matches_by_origin(
                    &matching,
                    &bounds,
                    |sig, member| {
                        sig.properties
                            .iter()
                            .find(|p| p.name == member)
                            .and_then(|p| p.origin.clone())
                    },
                    &field.value,
                ) {
                    Some(kept) => vec![matching[kept].clone()],
                    None => matching,
                }
            } else {
                matching
            };
            if matching.len() > 1 {
                let names: Vec<String> = matching
                    .iter()
                    .map(|(i, _)| format!("'{}'", Self::bound_display(&bounds[*i])))
                    .collect();
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "ambiguous static property '{}': declared by trait {}",
                        field.value,
                        names.join(" and trait "),
                    ),
                );
                return Some(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }
            if let Some((i, return_type)) = matching.into_iter().next() {
                let bound = bounds[i].named().expect("matched named bound");
                return Some(TypedExpr {
                    ty: return_type,
                    kind: TypedExprKind::ImplFunctionCall {
                        trait_fqn: bound.trait_fqn.clone(),
                        trait_type_params: bound.type_args.clone(),
                        for_type: resolved_type.clone(),
                        method_name: SymbolName(field.value.clone()),
                        args: vec![],
                        method_type_params: vec![],
                    },
                    span: span.clone(),
                });
            }
            return None;
        }

        let type_fqn = resolved_type.try_to_fqn()?;
        let prop_name = SymbolName(field.value.clone());

        // 1. Try named extension static properties (extensions take priority
        // over trait impls, appendix §2.1)
        let ext_overloads = self.lookup_named_extension_methods(&type_fqn, &prop_name);
        let ext_props: Vec<_> = ext_overloads
            .iter()
            .filter(|(_, m)| m.is_property && m.params.is_empty())
            .collect();

        if !ext_props.is_empty() {
            let distinct_exts: Vec<&Fqn> = {
                let mut seen: Vec<&Fqn> = Vec::new();
                for (b, _) in &ext_props {
                    if !seen.contains(&&b.ext_fqn) {
                        seen.push(&b.ext_fqn);
                    }
                }
                seen
            };
            if distinct_exts.len() > 1 {
                let names: Vec<String> = distinct_exts
                    .iter()
                    .map(|f| format!("'{}'", f.symbol))
                    .collect();
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "ambiguous static property '{}': provided by extension {}",
                        field.value,
                        names.join(" and extension "),
                    ),
                );
                return Some(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }
            let (block, method) = &ext_props[0];
            if method.is_intrinsic
                && let Some(intrinsic) =
                    resolve_intrinsic_kind(&type_fqn, &prop_name, &method.return_type)
            {
                return Some(TypedExpr {
                    ty: method.return_type.clone(),
                    kind: TypedExprKind::IntrinsicCall {
                        intrinsic,
                        args: vec![],
                    },
                    span: span.clone(),
                });
            }
            return Some(TypedExpr {
                ty: method.return_type.clone(),
                kind: TypedExprKind::ExtFunctionCall {
                    ext_fqn: block.ext_fqn.clone(),
                    for_type: resolved_type.clone(),
                    method_name: prop_name.clone(),
                    args: vec![],
                    type_params: vec![],
                },
                span: span.clone(),
            });
        }

        // 1b. Try named extension static functions as function references
        let ext_funcs: Vec<_> = ext_overloads
            .into_iter()
            .filter(|(_, m)| !m.is_property || !m.params.is_empty())
            .collect();
        if !ext_funcs.is_empty() {
            let display = format!("{}.{}", type_name, field.value);
            if let Some(result) = self.try_resolve_ext_function_ref(
                &ext_funcs,
                &resolved_type,
                &prop_name,
                &display,
                span,
            ) {
                return Some(result);
            }
        }

        // 2. Try trait impl static properties
        let impl_results = self.registry.find_impl_method(&type_fqn, &prop_name);
        let concrete_impls: Vec<_> = impl_results
            .into_iter()
            .filter(|(b, m)| {
                b.type_params.is_empty()
                    && m.method_type_params.is_empty()
                    && (m.visibility != Visibility::Private || m.span.file == self.current_file)
            })
            .collect();

        let all_trait_impl_props: Vec<_> = concrete_impls
            .iter()
            .filter(|(_, m)| m.is_property && m.params.is_empty())
            .collect();
        // Prefer blocks whose for_type IS the named instantiation (sibling
        // blocks share a base FQN); keep the base set as a fallback for
        // bare-name spellings (mirror of the 2b static-method path).
        let exact_trait_impl_props: Vec<_> = all_trait_impl_props
            .iter()
            .filter(|(b, _)| b.for_type == resolved_type)
            .cloned()
            .collect();
        let trait_impl_props: Vec<_> = if exact_trait_impl_props.is_empty() {
            all_trait_impl_props
        } else {
            exact_trait_impl_props
        };
        // One inherited declaration reached through a trait and a sub-trait
        // that extends it is not an ambiguity — the origin's direct impl
        // wins, as on every other member path.
        let mut trait_impl_props = trait_impl_props;
        {
            let distinct: Vec<(Fqn, Vec<Type>)> = {
                let mut seen: Vec<(Fqn, Vec<Type>)> = Vec::new();
                for (b, _) in &trait_impl_props {
                    let key = (b.trait_fqn.clone(), b.trait_type_args.clone());
                    if !seen.contains(&key) {
                        seen.push(key);
                    }
                }
                seen
            };
            if distinct.len() > 1
                && let Some(kept) = self.dedup_traits_by_member_origin(
                    &distinct,
                    |sig, member| {
                        sig.properties
                            .iter()
                            .find(|p| p.name == member)
                            .and_then(|p| p.origin.clone())
                    },
                    &field.value,
                )
            {
                let (keep_fqn, keep_args) = distinct[kept].clone();
                trait_impl_props
                    .retain(|(b, _)| b.trait_fqn == keep_fqn && b.trait_type_args == keep_args);
            }
        }
        // Two distinct (trait, application) providers of the same static
        // property are ambiguous, like the static-method path.
        {
            let mut seen: Vec<(&Fqn, &Vec<Type>)> = Vec::new();
            for (b, _) in &trait_impl_props {
                let key = (&b.trait_fqn, &b.trait_type_args);
                if !seen.contains(&key) {
                    seen.push(key);
                }
            }
            if seen.len() > 1 {
                let names: Vec<String> = seen
                    .iter()
                    .map(|(f, a)| format!("'{}'", Self::trait_application_display(f, a)))
                    .collect();
                let suggestion = Self::trait_application_display(seen[0].0, seen[0].1);
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "ambiguous static property '{}': implemented by trait {}; use '{}.{}' to choose one",
                        field.value,
                        names.join(" and trait "),
                        suggestion,
                        field.value,
                    ),
                );
                return Some(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }
        }

        if let Some((block, m)) = trait_impl_props.first() {
            if m.is_intrinsic
                && let Some(intrinsic) =
                    resolve_intrinsic_kind(&type_fqn, &prop_name, &m.return_type)
            {
                return Some(TypedExpr {
                    ty: m.return_type.clone(),
                    kind: TypedExprKind::IntrinsicCall {
                        intrinsic,
                        args: vec![],
                    },
                    span: span.clone(),
                });
            }
            return Some(TypedExpr {
                ty: m.return_type.clone(),
                kind: TypedExprKind::ImplFunctionCall {
                    trait_fqn: block.trait_fqn.clone(),
                    trait_type_params: block.trait_type_args.clone(),
                    for_type: resolved_type.clone(),
                    method_name: prop_name.clone(),
                    args: vec![],
                    method_type_params: vec![],
                },
                span: span.clone(),
            });
        }

        // 2b. Try trait impl static functions as function references.
        // When multiple impls share the same method name (e.g.
        // `From<NetError> for E` and `From<HttpError> for E` both expose
        // `from`), disambiguate by `expected_type` the same way
        // `try_resolve_overloaded_function_ref` does — otherwise we'd
        // arbitrarily pick the first impl and silently produce a wrong
        // typing.
        let mut trait_impl_funcs: Vec<_> = concrete_impls
            .iter()
            .filter(|(_, m)| !m.is_property && (m.params.is_empty() || m.params[0].0 != "self"))
            .collect();
        // A trait and a sub-trait extending it contribute ONE inherited
        // declaration — narrow to the origin's block so a reference like
        // `Rec.mk` isn't left ambiguous (its candidates are identical, so no
        // expected-type annotation could ever separate them).
        if trait_impl_funcs.len() > 1 {
            let distinct: Vec<(Fqn, Vec<Type>)> =
                trait_impl_funcs.iter().fold(Vec::new(), |mut acc, (b, _)| {
                    let key = (b.trait_fqn.clone(), b.trait_type_args.clone());
                    if !acc.contains(&key) {
                        acc.push(key);
                    }
                    acc
                });
            if distinct.len() > 1
                && let Some(kept) = self.dedup_traits_by_member_origin(
                    &distinct,
                    |sig, member| {
                        sig.methods
                            .iter()
                            .find(|m| m.name == member)
                            .and_then(|m| m.origin.clone())
                    },
                    &prop_name.0,
                )
            {
                let (keep_fqn, keep_args) = distinct[kept].clone();
                trait_impl_funcs
                    .retain(|(b, _)| b.trait_fqn == keep_fqn && b.trait_type_args == keep_args);
            }
        }
        if !trait_impl_funcs.is_empty() {
            let pick = if trait_impl_funcs.len() == 1 {
                Some(trait_impl_funcs[0])
            } else if let Some(Type::Function(expected_params, _)) = &self.expected_type {
                let matching: Vec<_> = trait_impl_funcs
                    .iter()
                    .filter(|(_, m)| {
                        m.params.len() == expected_params.len()
                            && m.params.iter().zip(expected_params.iter()).all(
                                |((_, param_ty), exp_ty)| {
                                    self.is_assignable(param_ty, exp_ty)
                                        || self.is_assignable(exp_ty, param_ty)
                                },
                            )
                    })
                    .copied()
                    .collect();
                if matching.len() == 1 {
                    Some(matching[0])
                } else {
                    None
                }
            } else {
                None
            };
            if let Some((block, m)) = pick {
                let func_ty = Type::Function(
                    m.params.iter().map(|(_, ty)| ty.clone()).collect(),
                    Box::new(m.return_type.clone()),
                );
                let mangled = crate::typechecker::types::impl_member_mangled_name(
                    &block.trait_fqn,
                    &block.for_type,
                    &block.type_params,
                    &prop_name,
                    &block.trait_type_args,
                );
                return Some(TypedExpr {
                    kind: TypedExprKind::FunctionRef {
                        name: mangled,
                        type_params: vec![],
                    },
                    ty: func_ty,
                    span: span.clone(),
                });
            }
            self.diagnostics.error(span.clone(), format!(
                "ambiguous reference to '{}.{}': {} trait implementations match; annotate the expected function type or use a trait-qualified call",
                type_name, field.value, trait_impl_funcs.len(),
            ));
            return Some(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }

        // 2c. Try generic trait impl static properties (e.g. Array<Int32>.empty())
        {
            let defs = self.registry.find_impl_method(&type_fqn, &prop_name);
            // A trait and a sub-trait extending it provide ONE inherited
            // declaration — the origin's block wins, instead of whichever was
            // registered first (declaration-order-dependent otherwise).
            let eligible: Vec<(Fqn, Vec<Type>)> = defs
                .iter()
                .filter(|(block, im)| {
                    !(block.type_params.is_empty() && im.method_type_params.is_empty())
                        && im.params.is_empty()
                        && im.method_type_params.is_empty()
                        && {
                            let mut sub =
                                super::type_param_substitution::TypeParamSubstitution::new();
                            sub.unify(&block.for_type, &resolved_type)
                        }
                })
                .map(|(block, _)| (block.trait_fqn.clone(), block.trait_type_args.clone()))
                .fold(Vec::new(), |mut acc, key| {
                    if !acc.contains(&key) {
                        acc.push(key);
                    }
                    acc
                });
            let origin_only: Option<(Fqn, Vec<Type>)> = if eligible.len() > 1 {
                self.dedup_traits_by_member_origin(
                    &eligible,
                    |sig, member| {
                        sig.properties
                            .iter()
                            .find(|p| p.name == member)
                            .and_then(|p| p.origin.clone())
                    },
                    &prop_name.0,
                )
                .map(|i| eligible[i].clone())
            } else {
                None
            };
            for (block, im) in &defs {
                if let Some((keep_fqn, keep_args)) = &origin_only
                    && (block.trait_fqn != *keep_fqn || block.trait_type_args != *keep_args)
                {
                    continue;
                }
                if block.type_params.is_empty() && im.method_type_params.is_empty() {
                    continue;
                }
                if !im.params.is_empty() {
                    continue;
                }
                // Only handle block-level generics here; method-level params need arg/expected type to infer
                if !im.method_type_params.is_empty() {
                    continue;
                }
                let mut substitution = super::type_param_substitution::TypeParamSubstitution::new();
                if !substitution.unify(&block.for_type, &resolved_type) {
                    continue;
                }
                let type_args = match substitution.resolve_type_params(&block.type_params) {
                    Some(args) => args,
                    None => continue,
                };
                let mut combined_bounds = block.trait_bounds.clone();
                combined_bounds.merge(&im.trait_bounds);
                let bound_span = Span::point(self.current_file.clone(), 1, 1);
                if !self.check_trait_bounds(
                    &combined_bounds,
                    &block.type_params,
                    &type_args,
                    &bound_span,
                ) {
                    continue;
                }
                let block_sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                    &block.type_params,
                    &type_args,
                );
                let concrete_trait_type_args: Vec<Type> = block
                    .trait_type_args
                    .iter()
                    .map(|a| super::generics::apply_substitution(&block_sub, a))
                    .collect();
                // Substitute return type to get concrete type
                let return_type = if im.return_type == Type::SelfType {
                    resolved_type.clone()
                } else {
                    super::generics::apply_substitution(&block_sub, &im.return_type)
                };
                // Emit ImplFunctionCall — monomorphize's resolve_impl_calls will
                // find the generic impl block, unify type params, and create the
                // concrete function.
                return Some(TypedExpr {
                    ty: return_type,
                    kind: TypedExprKind::ImplFunctionCall {
                        trait_fqn: block.trait_fqn.clone(),
                        trait_type_params: concrete_trait_type_args,
                        for_type: resolved_type.clone(),
                        method_name: prop_name.clone(),
                        args: vec![],
                        method_type_params: vec![],
                    },
                    span: span.clone(),
                });
            }
        }

        // 3. Try unbound method references: Type.method where method has self.
        // The function type INCLUDES self as first param.
        let display = format!("{}.{}", type_name, field.value);

        // 3a. Module-for-type instance methods as unbound function references
        if let Some(module_info) = self.registry.lookup_module(&type_fqn).cloned()
            && let Some(overloads) = module_info.functions.get(&prop_name)
        {
            let instance_methods: Vec<_> = overloads
                .iter()
                .filter(|sig| {
                    !sig.is_property
                        && !sig.is_intrinsic
                        && !sig.params.is_empty()
                        && sig.params[0].0 == "self"
                        && self.is_member_visible(
                            sig.visibility,
                            &module_info.fqn.package,
                            &sig.source_file,
                        )
                })
                .cloned()
                .collect();
            if !instance_methods.is_empty()
                && let Some(result) =
                    self.try_resolve_overloaded_function_ref(&instance_methods, &display, span)
            {
                return Some(result);
            }
        }

        // 3b. Named extension instance methods as unbound function references
        // (extensions take priority over trait impls, appendix §2.1)
        let ext_instance: Vec<_> = self
            .lookup_named_extension_methods(&type_fqn, &prop_name)
            .into_iter()
            .filter(|(_, m)| {
                !m.is_property && !m.is_intrinsic && !m.params.is_empty() && m.params[0].0 == "self"
            })
            .collect();
        if !ext_instance.is_empty()
            && let Some(result) = self.try_resolve_ext_function_ref(
                &ext_instance,
                &resolved_type,
                &prop_name,
                &display,
                span,
            )
        {
            return Some(result);
        }
        // 3c. Trait impl instance methods as unbound function references
        let instance_blocks: Vec<_> = self
            .registry
            .find_impl_method(&type_fqn, &prop_name)
            .into_iter()
            .filter(|(b, m)| {
                b.type_params.is_empty()
                    && m.method_type_params.is_empty()
                    && !m.is_property
                    && (!m.is_intrinsic
                        || crate::typechecker::types::primitive_binary_operator(
                            &type_fqn, &m.name.0,
                        )
                        .is_some())
                    && !m.params.is_empty()
                    && m.params[0].0 == "self"
                    && (m.visibility != Visibility::Private || m.span.file == self.current_file)
            })
            .map(|(b, m)| (b.clone(), m.clone()))
            .collect();
        // Origin dedup (same as the receiver form `r.foo`): a trait and a
        // sub-trait extending it are ONE declaration, and their identical
        // signatures mean no expected-type annotation could separate them.
        let instance_blocks = {
            let distinct: Vec<(Fqn, Vec<Type>)> =
                instance_blocks.iter().fold(Vec::new(), |mut acc, (b, _)| {
                    let key = (b.trait_fqn.clone(), b.trait_type_args.clone());
                    if !acc.contains(&key) {
                        acc.push(key);
                    }
                    acc
                });
            let keep = if distinct.len() > 1 {
                self.dedup_traits_by_member_origin(
                    &distinct,
                    |sig, member| {
                        sig.methods
                            .iter()
                            .find(|m| m.name == member)
                            .and_then(|m| m.origin.clone())
                    },
                    &prop_name.0,
                )
                .map(|i| distinct[i].clone())
            } else {
                None
            };
            match keep {
                Some((keep_fqn, keep_args)) => instance_blocks
                    .into_iter()
                    .filter(|(b, _)| b.trait_fqn == keep_fqn && b.trait_type_args == keep_args)
                    .collect(),
                None => instance_blocks,
            }
        };
        let trait_impl_instance: Vec<FunctionSignature> = instance_blocks
            .into_iter()
            .map(|(b, m)| FunctionSignature {
                visibility: m.visibility,
                mangled_name: crate::typechecker::types::impl_member_mangled_name(
                    &b.trait_fqn,
                    &b.for_type,
                    &b.type_params,
                    &m.dispatch_name,
                    &b.trait_type_args,
                ),
                params: m.params.clone(),
                return_type: m.return_type.clone(),
                source_file: b.source_file.clone(),
                is_intrinsic: m.is_intrinsic,
                is_property: m.is_property,
                is_final_method: false,
                is_abstract_method: false,
            })
            .collect();
        if !trait_impl_instance.is_empty()
            && let Some(result) =
                self.try_resolve_overloaded_function_ref(&trait_impl_instance, &display, span)
        {
            return Some(result);
        }

        None
    }

    /// Infer an enum variant record-style construction: `Shape.Point { x = 1, y = 2 }`.
    pub(super) fn infer_enum_variant_record_create(
        &mut self,
        type_name: &Spanned<String>,
        variant_name: &Spanned<String>,
        fields: &[crate::parser::ast::FieldInit],
        span: &Span,
    ) -> TypedExpr {
        let enum_sig = match self.resolve_enum_type(&type_name.value) {
            Some(sig) => sig,
            None => {
                self.diagnostics.error(
                    type_name.span.clone(),
                    format!("unknown enum type: '{}'", type_name.value),
                );
                return TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                };
            }
        };

        self.check_private_type_access(
            &enum_sig.fqn,
            enum_sig.construction_private,
            "enum",
            span,
            "construct",
        );

        let payload = match enum_sig
            .variants
            .iter()
            .find(|(v, _)| v == &variant_name.value)
        {
            Some((_, p)) => p,
            None => {
                self.diagnostics.error(
                    variant_name.span.clone(),
                    format!(
                        "no variant '{}' in enum '{}'",
                        variant_name.value, type_name.value
                    ),
                );
                return TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                };
            }
        };

        let expected_fields = match payload {
            VariantPayload::Record(f) => f.clone(),
            VariantPayload::Tuple(payload) if payload.len() == 1 => {
                return self.infer_record_payload_create(
                    &enum_sig,
                    variant_name,
                    &payload[0],
                    fields,
                    span,
                );
            }
            VariantPayload::None | VariantPayload::Tuple(_) => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "variant '{}.{}' does not have record-style payload",
                        type_name.value, variant_name.value
                    ),
                );
                return TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                };
            }
        };

        // First pass: infer field values without expected type constraints
        // to enable type parameter unification for generic enums
        let mut typed_args: Vec<TypedExpr> = vec![
            TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            };
            expected_fields.len()
        ];
        let mut supplied = vec![false; expected_fields.len()];

        for field_init in fields {
            let field_name = &field_init.name.value;
            if let Some(idx) = expected_fields.iter().position(|(n, _)| n == field_name) {
                if supplied[idx] {
                    self.diagnostics.error(
                        field_init.name.span.clone(),
                        format!("duplicate field '{}' in variant construction", field_name),
                    );
                    continue;
                }
                supplied[idx] = true;
                let typed_value = self.infer_expr(&field_init.value);
                typed_args[idx] = typed_value;
            } else {
                self.diagnostics.error(
                    field_init.name.span.clone(),
                    format!(
                        "no field '{}' in variant '{}.{}'",
                        field_name, type_name.value, variant_name.value
                    ),
                );
            }
        }

        // Check for missing fields
        for (i, (field_name, _)) in expected_fields.iter().enumerate() {
            if !supplied[i] {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "missing field '{}' in variant '{}.{}'",
                        field_name, type_name.value, variant_name.value
                    ),
                );
            }
        }

        // Determine type args for generic enums
        let mut create_type_args: Vec<Type> = vec![];
        let (enum_ty, substitution) = if !enum_sig.type_params.is_empty() {
            // Try unification from typed arg values against field types
            let mut unification = super::type_param_substitution::TypeParamSubstitution::new();
            for (i, (_, field_ty)) in expected_fields.iter().enumerate() {
                if supplied[i] && !typed_args[i].ty.is_error() {
                    unification.unify(field_ty, &typed_args[i].ty);
                }
            }
            let resolved = unification.resolve_type_params(&enum_sig.type_params);
            let type_args = match resolved {
                Some(args)
                    if !args
                        .iter()
                        .any(|t| matches!(t, Type::TypeVariable(..) | Type::GenericParam(..))) =>
                {
                    args
                }
                _ => {
                    // Fallback: try expected_type
                    match &self.expected_type {
                        Some(Type::GenericEnum {
                            fqn: exp_fqn,
                            type_args,
                            ..
                        }) if *exp_fqn == enum_sig.fqn => {
                            type_args.iter().map(|(_, t)| t.clone()).collect()
                        }
                        _ => {
                            self.diagnostics.error(
                                span.clone(),
                                format!(
                                    "cannot infer type arguments for generic enum '{}'",
                                    type_name.value
                                ),
                            );
                            return TypedExpr {
                                kind: TypedExprKind::UnitLiteral,
                                ty: Type::Error,
                                span: span.clone(),
                            };
                        }
                    }
                }
            };
            let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                &enum_sig.type_params,
                &type_args,
            );
            let enum_ty = self.resolve_generic_enum_type(&enum_sig.fqn, &enum_sig, &type_args);
            create_type_args = type_args;
            (enum_ty, Some(sub))
        } else {
            let mangled_name = MangledName::for_type(&enum_sig.fqn);
            (Type::Enum(enum_sig.fqn.clone(), mangled_name), None)
        };

        // Check assignability of typed values against (substituted) field types
        for (i, (_, field_ty)) in expected_fields.iter().enumerate() {
            if supplied[i] && !typed_args[i].ty.is_error() {
                let mut expected_ty = field_ty.clone();
                if let Some(ref sub) = substitution {
                    expected_ty = apply_substitution(sub, &expected_ty);
                }
                self.check_assignable(typed_args[i].span.clone(), &expected_ty, &typed_args[i].ty);
            }
        }

        TypedExpr {
            ty: enum_ty,
            kind: TypedExprKind::EnumVariantRecordCreate {
                fqn: enum_sig.fqn.clone(),
                variant_name: variant_name.value.clone(),
                args: typed_args,
                type_params: create_type_args,
            },
            span: span.clone(),
        }
    }

    fn error_try_expr(&self, operand: TypedExpr, span: &Span) -> TypedExpr {
        let dummy_resolved = ResolvedImplMethod {
            trait_fqn: Fqn {
                package: PackagePath(vec![]),
                symbol: SymbolName(String::new()),
            },
            trait_type_params: vec![],
            for_type: Type::Error,
            method_name: SymbolName(String::new()),
            method_type_params: vec![],
        };
        TypedExpr {
            kind: TypedExprKind::Try {
                operand: Box::new(operand),
                unwrap_method: dummy_resolved,
                unwrap_return_type: Type::Error,
                return_type: Type::Error,
                from_method: None,
            },
            ty: Type::Error,
            span: span.clone(),
        }
    }

    fn infer_try_expr(&mut self, operand: &Expr, span: &Span) -> TypedExpr {
        // 1. Infer operand (clear expected_type so it doesn't leak)
        let saved_expected = self.expected_type.take();
        let typed_operand = self.infer_expr(operand);
        self.expected_type = saved_expected;

        if typed_operand.ty.is_error() {
            return self.error_try_expr(typed_operand, span);
        }

        // 2. Resolve EarlyReturn impl — finds the impl (concrete or generic),
        //    checks where-clause bounds, and extracts T, OnFailure, and unwrap method.
        let early_return_fqn = Fqn {
            package: PackagePath(vec!["standard".into(), "prelude".into()]),
            symbol: SymbolName("EarlyReturn".to_string()),
        };

        let resolved = self.resolve_early_return_impl(&typed_operand.ty, &early_return_fqn);

        let Some((success_type, on_failure_type, unwrap_resolved, unwrap_return_type)) = resolved
        else {
            self.diagnostics.error(
                span.clone(),
                format!("type '{}' does not implement EarlyReturn", typed_operand.ty),
            );
            return self.error_try_expr(typed_operand, span);
        };

        // 3. Check function return type compatibility
        let fn_return_type = match &self.function_return_type {
            Some(ty) => ty.clone(),
            None => {
                self.diagnostics.error(
                    span.clone(),
                    "try/orReturn can only be used inside a function body".to_string(),
                );
                return TypedExpr {
                    kind: TypedExprKind::Try {
                        operand: Box::new(typed_operand),
                        unwrap_method: unwrap_resolved.clone(),
                        unwrap_return_type: unwrap_return_type.clone(),
                        return_type: Type::Error,
                        from_method: None,
                    },
                    ty: Type::Error,
                    span: span.clone(),
                };
            }
        };

        let mut from_method = None;

        if !self.is_assignable(&fn_return_type, &on_failure_type) {
            if self.async_return_type.is_some() {
                // Try From<OnFailure> for fn_return_type
                let from_fqn = Fqn {
                    package: PackagePath(vec!["standard".into(), "prelude".into()]),
                    symbol: SymbolName("From".to_string()),
                };
                match self.resolve_trait_impl_method_for_type(
                    &fn_return_type,
                    &from_fqn,
                    "from",
                    &[&on_failure_type],
                ) {
                    Some((resolved_from, _)) => {
                        from_method = Some(resolved_from);
                    }
                    None => {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "try/orReturn failure type '{}' is not assignable to async return type '{}' \
                                 and no From<{}> implementation exists for '{}'",
                                on_failure_type, fn_return_type, on_failure_type, fn_return_type
                            ),
                        );
                    }
                }
            } else {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "try/orReturn failure type '{}' is not assignable to function return type '{}'",
                        on_failure_type, fn_return_type
                    ),
                );
            }
        }

        TypedExpr {
            kind: TypedExprKind::Try {
                operand: Box::new(typed_operand),
                unwrap_method: unwrap_resolved,
                unwrap_return_type,
                return_type: fn_return_type,
                from_method,
            },
            ty: success_type,
            span: span.clone(),
        }
    }

    /// Rebind the enclosing context to this operand's success type, then widen
    /// only when assignability permits it. Awaitable defines the wrapper shape;
    /// the compiler does not assume where success or error parameters occur.
    fn widen_awaitable_context(&mut self, operand: &Type, target: &Type) -> Type {
        let Some(success) = self.resolve_awaitable_value_type(operand) else {
            return operand.clone();
        };
        match self.rebind_awaitable(target, &success) {
            Some(widened) if self.is_assignable(&widened, operand) => widened,
            _ => operand.clone(),
        }
    }

    fn infer_await_expr(&mut self, operand: &Expr, span: &Span) -> TypedExpr {
        // 1. Check we're inside an async function
        let fn_return_type = match &self.async_return_type {
            Some(rt) => rt.clone(),
            None => {
                let saved_expected = self.expected_type.take();
                let typed_operand = self.infer_expr(operand);
                self.expected_type = saved_expected;
                self.diagnostics.error(
                    span.clone(),
                    "await can only be used inside an async function".to_string(),
                );
                return self.error_await_expr(typed_operand, span);
            }
        };

        // 2. Infer operand (clear expected_type so it doesn't leak)
        let saved_expected = self.expected_type.take();
        let typed_operand = self.infer_expr(operand);
        self.expected_type = saved_expected;

        if typed_operand.ty.is_error() {
            return self.error_await_expr(typed_operand, span);
        }

        // 3. Check operand implements Awaitable, extract T
        let inner_type = match self.resolve_awaitable_value_type_detailed(&typed_operand.ty) {
            super::traits::SugarTraitResolution::Found(ty) => ty,
            super::traits::SugarTraitResolution::Ambiguous(providers) => {
                let names: Vec<String> = providers
                    .iter()
                    .map(|f| format!("'{}'", f.symbol.0))
                    .collect();
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "ambiguous implementations of trait 'Awaitable' for type '{}': provided by both {}; implement 'Awaitable' directly to disambiguate",
                        typed_operand.ty,
                        names.join(" and "),
                    ),
                );
                return self.error_await_expr(typed_operand, span);
            }
            super::traits::SugarTraitResolution::NotFound => {
                self.diagnostics.error(
                    span.clone(),
                    format!("type '{}' does not implement Awaitable", typed_operand.ty),
                );
                return self.error_await_expr(typed_operand, span);
            }
        };

        if let Some(operands) = &mut self.async_discovery {
            operands.push(typed_operand.ty.clone());
            return TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: inner_type,
                span: span.clone(),
            };
        }

        // 3b. If fn_return_type has unresolved type variables (dry-run pass for async
        // closures inferring method type params), just return the inner type. The full
        // Await node with andThen/map methods will be built in the second (real) pass
        // once the concrete return type is known.
        if fn_return_type.contains_type_variable() {
            return TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: inner_type,
                span: span.clone(),
            };
        }

        // 4. Extract FnT (inner success type) from the function's return type
        let fn_inner_type = match self.resolve_awaitable_value_type(&fn_return_type) {
            Some(ty) => ty,
            None => {
                // Should not happen if validate_async_constraints passed
                return self.error_await_expr(typed_operand, span);
            }
        };

        // 5. Resolve andThen: closure type (T) => Async<FnT, E>, plus SourceLocation arg
        let awaitable_fqn = Fqn::from_dotted("standard.prelude.Awaitable").unwrap();
        let source_location_fqn = Fqn::from_dotted("standard.prelude.SourceLocation").unwrap();
        let source_location_mn = MangledName::for_type(&source_location_fqn);
        let source_location_type = Type::Record(source_location_fqn, source_location_mn.clone());

        // Widen the operand's error type up to the function's error type when
        // the variance allows it. The `andThen` impl for `Async<T, opE>` returns
        // `Async<U, opE>`, but we need to pass a closure returning `Async<U, fnE>`.
        // Since `Async<_, _>` is covariant in E, when `opE <: fnE` we can resolve
        // against `Async<T, fnE>` (the widened type) and the closure type lines up.
        // Dovetail's typechecker doesn't perform this variance-widening through
        // the trait-impl lookup automatically, so we do it here. Common case:
        // `opE = Never` (from `Async.thunk`, `Async.succeed`, etc.), but any
        // subtype relation works (`From<opE> for fnE`-style coercions live at
        // `use` sites, not here).
        let resolved_operand_ty = self.widen_awaitable_context(&typed_operand.ty, &fn_return_type);

        let and_then_closure =
            Type::Function(vec![inner_type.clone()], Box::new(fn_return_type.clone()));
        let and_then_result = self.resolve_trait_impl_method_for_type(
            &resolved_operand_ty,
            &awaitable_fqn,
            "andThen",
            &[&and_then_closure, &source_location_type],
        );

        // 6. Resolve map: closure type (T) => FnT, plus SourceLocation arg
        let map_closure = Type::Function(vec![inner_type.clone()], Box::new(fn_inner_type));
        let map_result = self.resolve_trait_impl_method_for_type(
            &resolved_operand_ty,
            &awaitable_fqn,
            "map",
            &[&map_closure, &source_location_type],
        );

        let (and_then_resolved, map_resolved) = match (and_then_result, map_result) {
            (Some((at_r, _)), Some((m_r, _))) => (at_r, m_r),
            _ => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "cannot await type '{}' in async function returning '{}'",
                        typed_operand.ty, fn_return_type
                    ),
                );
                return self.error_await_expr(typed_operand, span);
            }
        };

        TypedExpr {
            kind: TypedExprKind::Await {
                operand: Box::new(typed_operand),
                return_type: fn_return_type,
                and_then_method: and_then_resolved,
                map_method: map_resolved,
                source_location_mn,
            },
            ty: inner_type,
            span: span.clone(),
        }
    }

    fn infer_use_expr(&mut self, operand: &Expr, span: &Span) -> TypedExpr {
        // Infer operand with no expected type (Usable resolution comes from the operand's type).
        let saved_expected = self.expected_type.take();
        let typed_operand = self.infer_expr(operand);
        self.expected_type = saved_expected;

        let make_error_use = |op: TypedExpr, span: &Span| TypedExpr {
            kind: TypedExprKind::Use {
                operand: Box::new(op),
                inner_type: Type::Error,
                source_error: Type::Error,
                target_error: Type::Error,
                from_method: None,
            },
            ty: Type::Error,
            span: span.clone(),
        };

        if typed_operand.ty.is_error() {
            return make_error_use(typed_operand, span);
        }

        // 1. Resolve `Usable<T, E>` for the operand's type — extract T and E.
        let (inner_type, source_error) = match self.resolve_usable_impl_detailed(&typed_operand.ty)
        {
            super::traits::SugarTraitResolution::Found(pair) => pair,
            super::traits::SugarTraitResolution::Ambiguous(providers) => {
                let names: Vec<String> = providers
                    .iter()
                    .map(|f| format!("'{}'", f.symbol.0))
                    .collect();
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "ambiguous implementations of trait 'Usable' for type '{}': provided by both {}; implement 'Usable' directly to disambiguate",
                        typed_operand.ty,
                        names.join(" and "),
                    ),
                );
                return make_error_use(typed_operand, span);
            }
            super::traits::SugarTraitResolution::NotFound => {
                self.diagnostics.error(
                    span.clone(),
                    format!("type '{}' does not implement Usable", typed_operand.ty),
                );
                return make_error_use(typed_operand, span);
            }
        };

        // 2. Determine the enclosing context's expected error type E2. In priority order:
        //    a. The enclosing block's expected wrapped error (from a let annotation
        //       like `let p: Async<_, E_fn> = …`). Tracked in `block_wrapped_error`.
        //    b. The enclosing async function's return error.
        //    c. Default: `source_error` (no conversion needed).
        let target_error = self
            .block_wrapped_error
            .clone()
            .or_else(|| {
                self.async_return_type
                    .as_ref()
                    .and_then(|rt| self.resolve_awaitable_error_type(rt))
            })
            .unwrap_or_else(|| source_error.clone());

        // 3. If E != E2 and E != Never, look up From<source> for target.
        let mut from_method = None;
        let needs_conversion = source_error != target_error && !matches!(source_error, Type::Never);
        if needs_conversion {
            let from_fqn = Fqn::from_dotted("standard.prelude.From").unwrap();
            match self.resolve_trait_impl_method_for_type(
                &target_error,
                &from_fqn,
                "from",
                &[&source_error],
            ) {
                Some((resolved, _)) => from_method = Some(resolved),
                None => {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "use of resource with error type '{}' in context expecting '{}' \
                             requires a 'From<{}> for {}' impl, but none was found",
                            source_error, target_error, source_error, target_error
                        ),
                    );
                    return make_error_use(typed_operand, span);
                }
            }
        }

        TypedExpr {
            kind: TypedExprKind::Use {
                operand: Box::new(typed_operand),
                inner_type: inner_type.clone(),
                source_error,
                target_error,
                from_method,
            },
            ty: inner_type,
            span: span.clone(),
        }
    }

    /// Extract the error type E from a wrapper type whose second type
    /// parameter conventionally represents an error. Currently recognises
    /// `Usable.Wrapped<U, E>` projections and the concrete stdlib wrappers
    /// `standard.io.Async<U, E>` and `standard.prelude.Result<U, E>`.
    /// These are the targets of the async and sync `Usable` impls. Without the FQN guard, any generic type with
    /// two parameters (e.g. `Response<T, E>`, an arbitrary user record)
    /// would also surface its second arg, causing `use` sites inside
    /// blocks whose expected type happens to have two type parameters to
    /// pick the wrong `From<...>` target.
    fn resolve_awaitable_error_type(&self, ty: &Type) -> Option<Type> {
        let async_fqn = Fqn::from_dotted("standard.io.Async");
        let result_fqn = Fqn::from_dotted("standard.prelude.Result");
        match ty {
            Type::AssociatedProjection(projection)
                if projection.trait_fqn == Fqn::from_dotted("standard.prelude.Usable").unwrap()
                    && projection.member == "Wrapped"
                    && projection.parameters.len() == 2 =>
            {
                Some(projection.parameters[1].clone())
            }

            Type::GenericClass { fqn, type_args, .. }
            | Type::GenericEnum { fqn, type_args, .. }
            | Type::GenericRecord { fqn, type_args, .. } => {
                let is_recognized = async_fqn.as_ref().is_some_and(|f| *fqn == *f)
                    || result_fqn.as_ref().is_some_and(|f| *fqn == *f);
                if is_recognized && type_args.len() >= 2 {
                    Some(type_args[1].1.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn error_await_expr(&self, typed_operand: TypedExpr, span: &Span) -> TypedExpr {
        let error_resolved = crate::typechecker::types::ResolvedImplMethod {
            trait_fqn: Fqn::from_dotted("standard.prelude.Awaitable").unwrap(),
            trait_type_params: vec![],
            for_type: Type::Error,
            method_name: SymbolName("$error".to_string()),
            method_type_params: vec![],
        };
        let source_location_fqn = Fqn::from_dotted("standard.prelude.SourceLocation").unwrap();
        let source_location_mn = MangledName::for_type(&source_location_fqn);
        TypedExpr {
            kind: TypedExprKind::Await {
                operand: Box::new(typed_operand),
                return_type: Type::Error,
                and_then_method: error_resolved.clone(),
                map_method: error_resolved,
                source_location_mn,
            },
            ty: Type::Error,
            span: span.clone(),
        }
    }

    fn infer_for_expr(
        &mut self,
        pattern: &crate::parser::ast::Pattern,
        iterable: &Expr,
        body: &Expr,
        span: &Span,
    ) -> TypedExpr {
        // 1. Infer iterable expression
        let prev_expected = self.expected_type.take();
        let typed_iterable = self.infer_expr(iterable);
        let iterable_ty = typed_iterable.ty.clone();

        if iterable_ty.is_error() {
            // Error recovery: still infer body to collect more errors
            self.push_scope();
            self.expected_type = Some(Type::Unit);
            self.loop_depth += 1;
            let typed_body = self.infer_expr(body);
            self.loop_depth -= 1;
            self.pop_scope();
            self.expected_type = prev_expected;
            let error_iter_resolved = crate::typechecker::types::ResolvedImplMethod {
                trait_fqn: Fqn {
                    package: PackagePath(vec!["standard".into(), "prelude".into()]),
                    symbol: SymbolName("Iterable".to_string()),
                },
                trait_type_params: vec![],
                for_type: Type::Error,
                method_name: SymbolName("iterator".to_string()),
                method_type_params: vec![],
            };
            return TypedExpr {
                kind: TypedExprKind::ForLoop {
                    pattern: self.infer_for_pattern(pattern, &Type::Error),
                    iterable: Box::new(typed_iterable),
                    iterator_method: error_iter_resolved,
                    iterator_type: Type::Error,
                    element_type: Type::Error,
                    body: Box::new(typed_body),
                },
                ty: Type::Unit,
                span: span.clone(),
            };
        }

        let iterable_fqn = Fqn {
            package: PackagePath(vec!["standard".into(), "prelude".into()]),
            symbol: SymbolName("Iterable".to_string()),
        };

        let mut already_diagnosed = false;
        let resolved = match self.resolve_trait_impl_method_for_type_detailed(
            &iterable_ty,
            &iterable_fqn,
            "iterator",
            &[],
            &[],
        ) {
            super::traits::ImplMethodResolution::Found {
                resolved,
                return_type,
                ..
            } => Some((resolved, return_type)),
            super::traits::ImplMethodResolution::Ambiguous => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "type '{}' implements 'Iterable' more than once; a for-loop needs a unique element type",
                        iterable_ty
                    ),
                );
                already_diagnosed = true;
                None
            }
            super::traits::ImplMethodResolution::NotFound => None,
        };

        let Some((iterator_method, iterator_return_type)) = resolved else {
            if !already_diagnosed {
                self.diagnostics.error(
                    span.clone(),
                    format!("type '{}' does not implement Iterable", iterable_ty),
                );
            }
            let error_iter_resolved = crate::typechecker::types::ResolvedImplMethod {
                trait_fqn: iterable_fqn,
                trait_type_params: vec![],
                for_type: Type::Error,
                method_name: SymbolName("iterator".to_string()),
                method_type_params: vec![],
            };
            self.push_scope();
            self.expected_type = Some(Type::Unit);
            self.loop_depth += 1;
            let typed_body = self.infer_expr(body);
            self.loop_depth -= 1;
            self.pop_scope();
            self.expected_type = prev_expected;
            return TypedExpr {
                kind: TypedExprKind::ForLoop {
                    pattern: self.infer_for_pattern(pattern, &Type::Error),
                    iterable: Box::new(typed_iterable),
                    iterator_method: error_iter_resolved,
                    iterator_type: Type::Error,
                    element_type: Type::Error,
                    body: Box::new(typed_body),
                },
                ty: Type::Unit,
                span: span.clone(),
            };
        };

        // 3. Extract element type T from the Iterator<T> interface object return type
        let element_type = match &iterator_return_type {
            Type::InterfaceObject { traits, .. }
                if traits.len() == 1 && !traits[0].trait_type_args.is_empty() =>
            {
                traits[0].trait_type_args[0].clone()
            }
            _ => {
                self.diagnostics.error(
                    span.clone(),
                    "could not determine element type from iterator".to_string(),
                );
                Type::Error
            }
        };

        // 4. Infer pattern against element type, push scope for body
        self.push_scope();
        let typed_pattern = self.infer_for_pattern(pattern, &element_type);

        // 5. Infer body
        self.expected_type = Some(Type::Unit);
        self.loop_depth += 1;
        let typed_body = self.infer_expr(body);
        self.loop_depth -= 1;
        self.check_async_loop(None, &typed_body);
        self.expected_type = prev_expected;

        // Body must be Unit (same as while loop)
        if !typed_body.ty.is_error() && !typed_body.ty.is_never() {
            self.check_assignable(typed_body.span.clone(), &Type::Unit, &typed_body.ty);
        }

        self.pop_scope();

        TypedExpr {
            kind: TypedExprKind::ForLoop {
                pattern: typed_pattern,
                iterable: Box::new(typed_iterable),
                iterator_method,
                iterator_type: iterator_return_type,
                element_type,
                body: Box::new(typed_body),
            },
            ty: Type::Unit,
            span: span.clone(),
        }
    }

    /// Infer a for-loop pattern against the element type.
    /// Supports Variable, Wildcard, and Tuple patterns.
    fn infer_for_pattern(
        &mut self,
        pattern: &crate::parser::ast::Pattern,
        element_type: &Type,
    ) -> TypedPattern {
        use crate::parser::ast::Pattern;

        match pattern {
            Pattern::Wildcard(_) => TypedPattern::Wildcard,
            Pattern::Variable(name, _) => {
                let var_name = VarName(name.clone());
                self.define_variable(var_name.clone(), element_type.clone(), false);
                TypedPattern::Variable(var_name, element_type.clone())
            }
            Pattern::Tuple(sub_patterns, pat_span) => {
                if let Type::Tuple(elem_types, _) = element_type {
                    if sub_patterns.len() != elem_types.len() {
                        self.diagnostics.error(
                            pat_span.clone(),
                            format!(
                                "tuple pattern has {} elements but element type has {}",
                                sub_patterns.len(),
                                elem_types.len()
                            ),
                        );
                        return TypedPattern::Wildcard;
                    }
                    let typed_elements: Vec<TypedPattern> = sub_patterns
                        .iter()
                        .zip(elem_types.iter())
                        .map(|(p, t)| self.infer_for_pattern(p, t))
                        .collect();
                    TypedPattern::Tuple {
                        element_patterns: typed_elements,
                        tuple_type: element_type.clone(),
                    }
                } else {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!(
                            "cannot destructure non-tuple type '{}' with tuple pattern",
                            element_type
                        ),
                    );
                    TypedPattern::Wildcard
                }
            }
            _ => {
                self.diagnostics.error(
                    pattern.span(),
                    "unsupported pattern in for loop; use a variable, wildcard, or tuple pattern"
                        .to_string(),
                );
                TypedPattern::Wildcard
            }
        }
    }
}
