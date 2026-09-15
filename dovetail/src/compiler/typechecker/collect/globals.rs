use crate::common::span::FilePath;
use crate::common::types::{Fqn, MangledName, SymbolName};
use crate::parser::ast::{Expr, GlobalVarDecl};

use crate::typechecker::infer::{
    generics::apply_substitution, type_param_substitution::TypeParamSubstitution,
};
use crate::typechecker::registry::GlobalSignature;
use crate::typechecker::types::{TraitBound, Type};

use super::Collector;

/// An unresolved global whose type needs to be inferred via fixpoint.
pub(super) struct UnresolvedGlobal<'a> {
    pub fqn: Fqn,
    pub mangled_name: MangledName,
    pub visibility: crate::common::types::Visibility,
    pub mutable: bool,
    pub expr: &'a Expr,
    pub span: crate::common::span::Span,
    pub source_file: FilePath,
}

impl Collector<'_> {
    /// Collect a single global variable declaration.
    /// If explicitly typed, register immediately.
    /// If untyped, return as unresolved for fixpoint inference.
    pub(super) fn collect_global<'a>(
        &mut self,
        global: &'a GlobalVarDecl,
    ) -> Option<UnresolvedGlobal<'a>> {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(global.name.value.clone()),
        };

        if let Some(doc) = &global.doc_comment {
            self.package_registry
                .register_doc_comment(fqn.clone(), doc.clone());
        }

        let mangled_name = MangledName::for_global(&fqn);
        let source_file = global.name.span.file.clone();

        // Check for name collision with zero-param functions (same mangled name format)
        if self
            .package_registry
            .lookup_function_in_package(
                &self.package_path,
                &global.name.value,
                &self.package_path,
                &source_file,
            )
            .is_some()
        {
            self.diagnostics.error(
                global.name.span.clone(),
                format!(
                    "global '{}' conflicts with a function of the same name",
                    global.name.value
                ),
            );
            return None;
        }

        if let Some(type_expr) = &global.type_annotation {
            // Explicitly typed — register immediately
            let ty = self.resolve_type_expr(type_expr);
            let registered = self.package_registry.register_global(
                fqn,
                GlobalSignature {
                    visibility: global.visibility,
                    mangled_name,
                    ty,
                    mutable: global.mutable,
                    source_file,
                },
            );
            if !registered {
                self.diagnostics.error(
                    global.name.span.clone(),
                    format!("duplicate global: '{}'", global.name.value),
                );
            }
            None
        } else {
            // Untyped — defer to fixpoint
            Some(UnresolvedGlobal {
                fqn,
                mangled_name,
                visibility: global.visibility,
                mutable: global.mutable,
                expr: &global.value,
                span: global.name.span.clone(),
                source_file,
            })
        }
    }

    /// Run fixpoint inference on unresolved globals.
    /// Each round, try to infer the type of each unresolved global.
    /// If progress is made (at least one resolved), continue.
    /// If no progress, emit errors for remaining unresolved globals.
    pub(super) fn resolve_untyped_globals(&mut self, mut unresolved: Vec<UnresolvedGlobal<'_>>) {
        loop {
            if unresolved.is_empty() {
                break;
            }

            let mut progress = false;
            let mut remaining = Vec::new();

            for global in unresolved {
                if let Some(ty) = self.try_infer_expr_type(global.expr, &global.source_file) {
                    let registered = self.package_registry.register_global(
                        global.fqn,
                        GlobalSignature {
                            visibility: global.visibility,
                            mangled_name: global.mangled_name,
                            ty,
                            mutable: global.mutable,
                            source_file: global.source_file,
                        },
                    );
                    if !registered {
                        self.diagnostics
                            .error(global.span, "duplicate global".to_string());
                    }
                    progress = true;
                } else {
                    remaining.push(global);
                }
            }

            unresolved = remaining;

            if !progress {
                // No progress — emit errors for all remaining
                for global in &unresolved {
                    self.diagnostics.error(
                        global.span.clone(),
                        format!(
                            "global '{}' requires a type annotation; \
                             complex initializers cannot be inferred at the top level",
                            global.fqn.symbol
                        ),
                    );
                }
                break;
            }
        }
    }

    /// Select an operator's declared output using its operands and applicable bounds.
    fn try_infer_operator_output(
        &self,
        name: &str,
        left_ty: &Type,
        right_ty: &Type,
    ) -> Option<Type> {
        let trait_fqn = Fqn::from_dotted(&format!("standard.prelude.{name}")).unwrap();
        if let Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) = left_ty {
            return infer_bound_operator_output(&trait_fqn, bounds, left_ty, right_ty);
        }
        let type_fqn = left_ty.try_to_fqn()?;
        let mut outputs = Vec::new();
        for registry in [&self.package_registry, self.dependency_registry] {
            for info in registry.find_impl_blocks(&trait_fqn, &type_fqn) {
                let mut sub = TypeParamSubstitution::new();
                if sub.unify(&info.for_type, left_ty)
                    && info.trait_type_args.len() == 1
                    && sub.unify(&info.trait_type_args[0], right_ty)
                {
                    if info.trait_bounds.iter().next().is_some() {
                        let merged = self.dependency_registry.merge(&self.package_registry);
                        let Some(completed) = crate::typechecker::infer::complete_impl_substitution(
                            &merged, info, sub,
                        ) else {
                            continue;
                        };
                        sub = completed;
                    }
                    if let Some((params, output)) = info.associated_type_defs.get("Output")
                        && params.is_empty()
                    {
                        outputs.push(apply_substitution(&sub, output));
                    }
                }
            }
        }
        if outputs.len() == 1 {
            outputs.pop()
        } else {
            None
        }
    }

    /// Try to infer the type of an expression without full type-checking.
    /// Returns `Some(ty)` if the type can be determined, `None` if not yet.
    pub(super) fn try_infer_expr_type(&self, expr: &Expr, caller_file: &FilePath) -> Option<Type> {
        self.try_infer_expr_type_with_locals(expr, caller_file, &[])
    }

    /// Try to infer the type of an expression with additional local variable bindings.
    /// `locals` provides name→type pairs (e.g. constructor params for class let bindings).
    pub(super) fn try_infer_expr_type_with_locals(
        &self,
        expr: &Expr,
        caller_file: &FilePath,
        locals: &[(&str, &Type)],
    ) -> Option<Type> {
        match expr {
            // Literals
            Expr::UnitLiteral(_) => Some(Type::Unit),
            Expr::BoolLiteral(_, _) => Some(Type::Bool),
            Expr::StringLiteral(_, _) => Some(Type::String),
            Expr::Int8Literal(_, _) => Some(Type::Int8),
            Expr::Int16Literal(_, _) => Some(Type::Int16),
            Expr::Int32Literal(_, _) => Some(Type::Int32),
            Expr::Int64Literal(_, _) => Some(Type::Int64),
            Expr::Uint8Literal(_, _) => Some(Type::Uint8),
            Expr::Uint16Literal(_, _) => Some(Type::Uint16),
            Expr::Uint32Literal(_, _) => Some(Type::Uint32),
            Expr::Uint64Literal(_, _) => Some(Type::Uint64),
            Expr::Float32Literal(_, _) => Some(Type::Float32),
            Expr::Float64Literal(_, _) => Some(Type::Float64),
            Expr::ExactNumberLiteral(text, _) => {
                let name = if text.ends_with("dec") {
                    "Decimal"
                } else {
                    "BigInt"
                };
                let fqn = Fqn::from_dotted(&format!("standard.prelude.{name}")).unwrap();
                Some(Type::Record(fqn.clone(), MangledName::for_type(&fqn)))
            }

            // Identifier — check locals first, then same-package globals.
            Expr::Identifier(name, _) => {
                // Check locals (constructor params, prior let bindings, etc.)
                if let Some((_, ty)) = locals.iter().find(|(n, _)| *n == name.as_str()) {
                    return Some((*ty).clone());
                }
                self.package_registry
                    .lookup_global_in_package(
                        &self.package_path,
                        name,
                        &self.package_path,
                        caller_file,
                    )
                    .map(|sig| sig.ty.clone())
            }

            // Binary op — if both sides inferrable and same type, compute result
            Expr::BinaryOp {
                op, left, right, ..
            } => {
                let left_ty = self.try_infer_expr_type_with_locals(left, caller_file, locals)?;
                let right_ty = self.try_infer_expr_type_with_locals(right, caller_file, locals)?;
                use crate::parser::ast::BinOp;
                let operator_trait = match op {
                    BinOp::Add => Some("Add"),
                    BinOp::Sub => Some("Sub"),
                    BinOp::Mul => Some("Mul"),
                    BinOp::Div => Some("Div"),
                    BinOp::Concat => Some("Concat"),
                    _ => None,
                };
                if let Some(name) = operator_trait {
                    return self.try_infer_operator_output(name, &left_ty, &right_ty);
                }
                if *op == BinOp::TupleExtend {
                    return Some(Type::tuple_extend(left_ty, right_ty));
                }
                if left_ty != right_ty {
                    return None;
                }
                match op {
                    BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                        Some(Type::Bool)
                    }
                    BinOp::TupleExtend => unreachable!("extension handled above"),
                    BinOp::Add
                    | BinOp::Concat
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::Div
                    | BinOp::Rem
                    | BinOp::BitAnd
                    | BinOp::BitOr
                    | BinOp::BitXor
                    | BinOp::Shl
                    | BinOp::Shr => Some(left_ty),
                    BinOp::LogicalAnd | BinOp::LogicalOr => Some(Type::Bool),
                }
            }

            // Unary op
            Expr::UnaryOp { operand, .. } => {
                self.try_infer_expr_type_with_locals(operand, caller_file, locals)
            }

            // If-else — if both branches inferrable and compatible
            Expr::If {
                then_branch,
                else_branch: Some(else_branch),
                ..
            } => {
                let then_ty =
                    self.try_infer_expr_type_with_locals(then_branch, caller_file, locals)?;
                let else_ty =
                    self.try_infer_expr_type_with_locals(else_branch, caller_file, locals)?;
                if then_ty == else_ty {
                    Some(then_ty)
                } else {
                    None
                }
            }

            // Block — type of last expression
            Expr::Block(block) => {
                if let Some(last) = block.expressions.last() {
                    self.try_infer_expr_type_with_locals(last, caller_file, locals)
                } else {
                    Some(Type::Unit)
                }
            }

            _ => None,
        }
    }
}

fn infer_bound_operator_output(
    trait_fqn: &Fqn,
    bounds: &[TraitBound],
    left_ty: &Type,
    right_ty: &Type,
) -> Option<Type> {
    let substitution = operand_type_substitution(left_ty, right_ty);
    let mut outputs = bounds.iter().filter_map(|bound| {
        let named = bound.named()?;
        if named.trait_fqn != *trait_fqn
            || named.type_args.len() != 1
            || apply_substitution(&substitution, &named.type_args[0]) != *right_ty
        {
            return None;
        }
        named
            .associated_types
            .get("Output")
            .map(|output| apply_substitution(&substitution, output))
    });
    let output = outputs.next()?;
    outputs.next().is_none().then_some(output)
}

fn operand_type_substitution(left_ty: &Type, right_ty: &Type) -> TypeParamSubstitution {
    let mut substitution = TypeParamSubstitution::new();
    let mut operand_types = vec![left_ty, right_ty];
    while let Some(ty) = operand_types.pop() {
        match ty {
            Type::TypeVariable(param, _) | Type::GenericParam(param, _, _) => {
                substitution.insert(param.clone(), ty.clone());
            }
            Type::Array(element) => operand_types.push(element),
            Type::GenericRecord { type_args, .. }
            | Type::GenericEnum { type_args, .. }
            | Type::GenericClass { type_args, .. }
            | Type::GenericNewtype { type_args, .. } => {
                operand_types.extend(type_args.iter().map(|(_, ty)| ty));
            }
            Type::InterfaceObject { traits, .. } => {
                operand_types.extend(traits.iter().flat_map(|bound| &bound.trait_type_args));
            }
            Type::Tuple(elements, _) => operand_types.extend(elements),
            Type::Function(params, result) => {
                operand_types.extend(params);
                operand_types.push(result);
            }
            _ => {}
        }
    }
    substitution
}
