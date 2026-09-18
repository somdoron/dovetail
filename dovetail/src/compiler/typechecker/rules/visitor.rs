use crate::common::diagnostics::Diagnostics;
use crate::parser::ast::{Declaration, Expr, FieldPattern, Pattern, SourceFile};
use crate::typechecker::types::{TypedExpr, TypedExprKind, TypedModule, TypedPattern};

/// Visitor trait for walking typed expression trees.
///
/// Each `visit_X` method defaults to calling the corresponding `walk_X` free
/// function, which recurses into sub-expressions. Override a `visit_X` method
/// to inject custom logic before/after walking children.
pub trait TypedExprVisitor {
    fn visit_expr(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_expr(self, expr, diagnostics);
    }

    fn visit_block(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_block(self, expr, diagnostics);
    }

    fn visit_binary_op(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_binary_op(self, expr, diagnostics);
    }

    fn visit_unary_op(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_unary_op(self, expr, diagnostics);
    }

    fn visit_if(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_if(self, expr, diagnostics);
    }

    fn visit_while(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_while(self, expr, diagnostics);
    }

    fn visit_match(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_match(self, expr, diagnostics);
    }

    fn visit_let(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_let(self, expr, diagnostics);
    }

    fn visit_assign(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_assign(self, expr, diagnostics);
    }

    fn visit_function_call(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_function_call(self, expr, diagnostics);
    }

    fn visit_panic(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_panic(self, expr, diagnostics);
    }

    fn visit_assert(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_assert(self, expr, diagnostics);
    }

    fn visit_record_create(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_record_create(self, expr, diagnostics);
    }

    fn visit_tuple_literal(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_tuple_literal(self, expr, diagnostics);
    }

    fn visit_enum_create(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_enum_create(self, expr, diagnostics);
    }

    fn visit_field_access(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_field_access(self, expr, diagnostics);
    }

    fn visit_record_with(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_record_with(self, expr, diagnostics);
    }

    fn visit_newtype_create(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_newtype_create(self, expr, diagnostics);
    }

    fn visit_newtype_value(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        walk_newtype_value(self, expr, diagnostics);
    }
}

/// Dispatch to the appropriate `visit_X` method based on the expression kind.
pub fn walk_expr<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    match &expr.kind {
        TypedExprKind::NamedCall { call, .. } => visitor.visit_expr(call, diagnostics),
        TypedExprKind::Block(_) => visitor.visit_block(expr, diagnostics),
        TypedExprKind::BinaryOp { .. } => visitor.visit_binary_op(expr, diagnostics),
        TypedExprKind::UnaryOp { .. } => visitor.visit_unary_op(expr, diagnostics),
        TypedExprKind::If { .. } => visitor.visit_if(expr, diagnostics),
        TypedExprKind::While { .. } => visitor.visit_while(expr, diagnostics),
        TypedExprKind::Match { .. } => visitor.visit_match(expr, diagnostics),
        TypedExprKind::Let { .. } => visitor.visit_let(expr, diagnostics),
        TypedExprKind::Assign { .. } => visitor.visit_assign(expr, diagnostics),
        TypedExprKind::FunctionCall { .. } => visitor.visit_function_call(expr, diagnostics),
        TypedExprKind::Panic { .. } => visitor.visit_panic(expr, diagnostics),
        TypedExprKind::Assert { .. } => visitor.visit_assert(expr, diagnostics),
        TypedExprKind::RecordCreate { .. } => visitor.visit_record_create(expr, diagnostics),
        TypedExprKind::TupleLiteral { .. } => visitor.visit_tuple_literal(expr, diagnostics),
        TypedExprKind::EnumCreate { .. } => visitor.visit_enum_create(expr, diagnostics),
        TypedExprKind::FieldAccess { .. } => visitor.visit_field_access(expr, diagnostics),
        TypedExprKind::FieldAssign { object, value, .. } => {
            visitor.visit_expr(object, diagnostics);
            visitor.visit_expr(value, diagnostics);
        }
        TypedExprKind::RecordWith { .. } => visitor.visit_record_with(expr, diagnostics),
        TypedExprKind::NewtypeCreate { .. } => visitor.visit_newtype_create(expr, diagnostics),
        TypedExprKind::NewtypeValue { .. } => visitor.visit_newtype_value(expr, diagnostics),
        // Leaf nodes — no sub-expressions
        TypedExprKind::UnitLiteral
        | TypedExprKind::BoolLiteral(_)
        | TypedExprKind::StringLiteral(_)
        | TypedExprKind::CharLiteral(_)
        | TypedExprKind::Int8Literal(_)
        | TypedExprKind::Int16Literal(_)
        | TypedExprKind::Int32Literal(_)
        | TypedExprKind::Int64Literal(_)
        | TypedExprKind::Uint8Literal(_)
        | TypedExprKind::Uint16Literal(_)
        | TypedExprKind::Uint32Literal(_)
        | TypedExprKind::Uint64Literal(_)
        | TypedExprKind::Uint128Literal(_)
        | TypedExprKind::Float32Literal(_)
        | TypedExprKind::Float64Literal(_)
        | TypedExprKind::VarRef { .. }
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::FunctionRef { .. }
        | TypedExprKind::Break
        | TypedExprKind::Continue => {}
        TypedExprKind::GlobalAssign { value, .. } => visitor.visit_expr(value, diagnostics),
        TypedExprKind::ArrayLiteral { elements, .. } => {
            for e in elements {
                visitor.visit_expr(e, diagnostics);
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for arg in args {
                visitor.visit_expr(arg, diagnostics);
            }
        }
        TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                visitor.visit_expr(arg, diagnostics);
            }
        }
        TypedExprKind::ClassNew { args, .. } => {
            for arg in args {
                visitor.visit_expr(arg, diagnostics);
            }
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for field in fields {
                visitor.visit_expr(field, diagnostics);
            }
        }
        TypedExprKind::BoxToAny { inner, .. } => visitor.visit_expr(inner, diagnostics),
        TypedExprKind::TypeTest { value, .. } => visitor.visit_expr(value, diagnostics),
        TypedExprKind::TypeCast { value, .. } => visitor.visit_expr(value, diagnostics),
        TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner } => visitor.visit_expr(inner, diagnostics),
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            visitor.visit_expr(receiver, diagnostics);
            for arg in args {
                visitor.visit_expr(arg, diagnostics);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            visitor.visit_expr(object, diagnostics);
            for arg in args {
                visitor.visit_expr(arg, diagnostics);
            }
        }
        TypedExprKind::ClassSuperCall { args, .. } => {
            for arg in args {
                visitor.visit_expr(arg, diagnostics);
            }
        }
        TypedExprKind::LetDestructure { pattern, value, .. } => {
            walk_pattern(visitor, pattern, diagnostics);
            visitor.visit_expr(value, diagnostics);
        }
        TypedExprKind::ForLoop { iterable, body, .. } => {
            visitor.visit_expr(iterable, diagnostics);
            visitor.visit_expr(body, diagnostics);
        }
        TypedExprKind::Try { operand, .. } => visitor.visit_expr(operand, diagnostics),
        TypedExprKind::Await { operand, .. } => visitor.visit_expr(operand, diagnostics),
        TypedExprKind::Use { operand, .. } => visitor.visit_expr(operand, diagnostics),
        TypedExprKind::AsyncBlock { body, .. } => visitor.visit_expr(body, diagnostics),
        TypedExprKind::Return { value, .. } => visitor.visit_expr(value, diagnostics),
        TypedExprKind::Closure { body, .. } => visitor.visit_expr(body, diagnostics),
        TypedExprKind::MethodRef { object, .. } => visitor.visit_expr(object, diagnostics),
        TypedExprKind::ClosureCall { callee, args, .. } => {
            visitor.visit_expr(callee, diagnostics);
            for arg in args {
                visitor.visit_expr(arg, diagnostics);
            }
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                visitor.visit_expr(arg, diagnostics);
            }
        }
        TypedExprKind::ImplFunctionRef { .. } | TypedExprKind::ExtFunctionRef { .. } => {}
    }
}

pub fn walk_block<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::Block(exprs) = &expr.kind {
        for e in exprs {
            visitor.visit_expr(e, diagnostics);
        }
    }
}

pub fn walk_binary_op<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::BinaryOp { left, right, .. } = &expr.kind {
        visitor.visit_expr(left, diagnostics);
        visitor.visit_expr(right, diagnostics);
    }
}

pub fn walk_unary_op<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::UnaryOp { operand, .. } = &expr.kind {
        visitor.visit_expr(operand, diagnostics);
    }
}

pub fn walk_if<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::If {
        condition,
        then_branch,
        else_branch,
    } = &expr.kind
    {
        visitor.visit_expr(condition, diagnostics);
        visitor.visit_expr(then_branch, diagnostics);
        if let Some(else_br) = else_branch {
            visitor.visit_expr(else_br, diagnostics);
        }
    }
}

pub fn walk_while<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::While { condition, body } = &expr.kind {
        visitor.visit_expr(condition, diagnostics);
        visitor.visit_expr(body, diagnostics);
    }
}

pub fn walk_match<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::Match { subject, arms } = &expr.kind {
        visitor.visit_expr(subject, diagnostics);
        for arm in arms {
            walk_pattern(visitor, &arm.pattern, diagnostics);
            if let Some(guard) = &arm.guard {
                visitor.visit_expr(guard, diagnostics);
            }
            visitor.visit_expr(&arm.body, diagnostics);
        }
    }
}

fn walk_pattern<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    pattern: &TypedPattern,
    diagnostics: &mut Diagnostics,
) {
    match pattern {
        TypedPattern::Literal(expr) => visitor.visit_expr(expr, diagnostics),
        TypedPattern::Record { fields, .. } => {
            for field in fields {
                walk_pattern(visitor, &field.pattern, diagnostics);
            }
        }
        TypedPattern::EnumVariant {
            payload_patterns, ..
        } => {
            for sub_pat in payload_patterns {
                walk_pattern(visitor, sub_pat, diagnostics);
            }
        }
        TypedPattern::EnumVariantRecord { field_patterns, .. } => {
            for field in field_patterns {
                walk_pattern(visitor, &field.pattern, diagnostics);
            }
        }
        TypedPattern::Tuple {
            element_patterns, ..
        } => {
            for sub_pat in element_patterns {
                walk_pattern(visitor, sub_pat, diagnostics);
            }
        }
        TypedPattern::Newtype { inner_pattern, .. } => {
            walk_pattern(visitor, inner_pattern, diagnostics);
        }
        TypedPattern::TypeAnnotated { .. }
        | TypedPattern::Wildcard
        | TypedPattern::Variable(_, _) => {}
    }
}

pub fn walk_let<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::Let { value, .. } = &expr.kind {
        visitor.visit_expr(value, diagnostics);
    }
}

pub fn walk_assign<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::Assign { value, .. } = &expr.kind {
        visitor.visit_expr(value, diagnostics);
    }
}

pub fn walk_function_call<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    let args = match &expr.kind {
        TypedExprKind::FunctionCall { args, .. } => args,
        _ => return,
    };
    for arg in args {
        visitor.visit_expr(arg, diagnostics);
    }
}

pub fn walk_panic<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::Panic { message } = &expr.kind {
        visitor.visit_expr(message, diagnostics);
    }
}

pub fn walk_assert<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::Assert {
        condition, message, ..
    } = &expr.kind
    {
        visitor.visit_expr(condition, diagnostics);
        if let Some(msg) = message {
            visitor.visit_expr(msg, diagnostics);
        }
    }
}

pub fn walk_record_create<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    let fields = match &expr.kind {
        TypedExprKind::RecordCreate { fields, .. } => fields,
        _ => return,
    };
    for (_, field_expr) in fields {
        visitor.visit_expr(field_expr, diagnostics);
    }
}

pub fn walk_tuple_literal<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::TupleLiteral { elements } = &expr.kind {
        for element in elements {
            visitor.visit_expr(element, diagnostics);
        }
    }
}

pub fn walk_enum_create<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    let args = match &expr.kind {
        TypedExprKind::EnumCreate { args, .. } => args,
        _ => return,
    };
    for arg in args {
        visitor.visit_expr(arg, diagnostics);
    }
}

pub fn walk_field_access<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    let object = match &expr.kind {
        TypedExprKind::FieldAccess { object, .. } => object,
        _ => return,
    };
    visitor.visit_expr(object, diagnostics);
}

pub fn walk_record_with<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    let (object, overrides) = match &expr.kind {
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => (object.as_ref(), overrides.as_slice()),
        _ => return,
    };
    visitor.visit_expr(object, diagnostics);
    for (_, _, value) in overrides {
        visitor.visit_expr(value, diagnostics);
    }
}

pub fn walk_newtype_create<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::NewtypeCreate { value, .. } = &expr.kind {
        visitor.visit_expr(value, diagnostics);
    }
}

pub fn walk_newtype_value<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &TypedExpr,
    diagnostics: &mut Diagnostics,
) {
    if let TypedExprKind::NewtypeValue { value, .. } = &expr.kind {
        visitor.visit_expr(value, diagnostics);
    }
}

/// Visit all function bodies in a typed module.
pub fn visit_all_functions<V: TypedExprVisitor + ?Sized>(
    visitor: &mut V,
    typed_module: &TypedModule,
    diagnostics: &mut Diagnostics,
) {
    for func in typed_module.functions.values() {
        visitor.visit_expr(&func.body, diagnostics);
    }
    for block in &typed_module.implement_blocks {
        for method in block.methods.iter().chain(block.properties.iter()) {
            visitor.visit_expr(&method.body, diagnostics);
        }
    }
    // Trait default bodies live outside `functions` (they survive the
    // monomorphize template strip there) but are real user code checked in
    // this package — rules must see them too.
    for func in typed_module.default_templates.values() {
        visitor.visit_expr(&func.body, diagnostics);
    }
}

// ---------------------------------------------------------------------------
// Untyped AST visitor (for parser::ast::Expr)
// ---------------------------------------------------------------------------

/// Visitor trait for walking untyped (parser) expression trees.
///
/// Each `visit_X` method defaults to calling the corresponding `walk_untyped_X`
/// free function, which recurses into sub-expressions. Override a method to
/// inject custom logic before/after walking children.
pub trait ExprVisitor {
    fn visit_expr(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_expr(self, expr, diagnostics);
    }

    fn visit_let(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_let(self, expr, diagnostics);
    }

    fn visit_match(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_match(self, expr, diagnostics);
    }

    fn visit_block(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_block(self, expr, diagnostics);
    }

    fn visit_binary_op(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_binary_op(self, expr, diagnostics);
    }

    fn visit_unary_op(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_unary_op(self, expr, diagnostics);
    }

    fn visit_if(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_if(self, expr, diagnostics);
    }

    fn visit_while(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_while(self, expr, diagnostics);
    }

    fn visit_function_call(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_function_call(self, expr, diagnostics);
    }

    fn visit_method_call(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_method_call(self, expr, diagnostics);
    }

    fn visit_field_access(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_field_access(self, expr, diagnostics);
    }

    fn visit_assignment(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_assignment(self, expr, diagnostics);
    }

    fn visit_panic(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_panic(self, expr, diagnostics);
    }

    fn visit_assert(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_assert(self, expr, diagnostics);
    }

    fn visit_record_create(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_record_create(self, expr, diagnostics);
    }

    fn visit_record_with(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_record_with(self, expr, diagnostics);
    }

    fn visit_array_literal(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_array_literal(self, expr, diagnostics);
    }

    fn visit_list_literal(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_list_literal(self, expr, diagnostics);
    }

    fn visit_slice_index(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        if let Expr::SliceIndex {
            object, start, end, ..
        } = expr
        {
            self.visit_expr(object, diagnostics);
            for bound in start.iter().chain(end.iter()) {
                self.visit_expr(bound, diagnostics);
            }
        }
    }

    fn visit_index(&mut self, expr: &Expr, diagnostics: &mut Diagnostics) {
        walk_untyped_index(self, expr, diagnostics);
    }

    fn visit_pattern(&mut self, pattern: &Pattern, diagnostics: &mut Diagnostics) {
        walk_untyped_pattern(self, pattern, diagnostics);
    }
}

/// Dispatch to the appropriate `visit_X` method based on the expression variant.
pub fn walk_untyped_expr<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    match expr {
        Expr::NamedArgument { value, .. } => visitor.visit_expr(value, diagnostics),
        Expr::Let { .. } | Expr::LetDestructure { .. } => visitor.visit_let(expr, diagnostics),
        Expr::Match { .. } => visitor.visit_match(expr, diagnostics),
        Expr::Block(_) => visitor.visit_block(expr, diagnostics),
        Expr::BinaryOp { .. } => visitor.visit_binary_op(expr, diagnostics),
        Expr::UnaryOp { .. } => visitor.visit_unary_op(expr, diagnostics),
        Expr::If { .. } => visitor.visit_if(expr, diagnostics),
        Expr::While { .. } => visitor.visit_while(expr, diagnostics),
        Expr::FunctionCall { .. } => visitor.visit_function_call(expr, diagnostics),
        Expr::MethodCall { .. } => visitor.visit_method_call(expr, diagnostics),
        Expr::FieldAccess { .. } => visitor.visit_field_access(expr, diagnostics),
        Expr::Assignment { .. } => visitor.visit_assignment(expr, diagnostics),
        Expr::Panic { .. } => visitor.visit_panic(expr, diagnostics),
        Expr::Assert { .. } => visitor.visit_assert(expr, diagnostics),
        Expr::RecordCreate { .. } => visitor.visit_record_create(expr, diagnostics),
        Expr::RecordWith { .. } => visitor.visit_record_with(expr, diagnostics),
        Expr::ArrayLiteral { .. } => visitor.visit_array_literal(expr, diagnostics),
        Expr::ListLiteral { .. } => visitor.visit_list_literal(expr, diagnostics),
        Expr::Index { .. } => visitor.visit_index(expr, diagnostics),
        Expr::SliceIndex { .. } => visitor.visit_slice_index(expr, diagnostics),
        // Leaf nodes — no sub-expressions
        Expr::UnitLiteral(_)
        | Expr::BoolLiteral(_, _)
        | Expr::StringLiteral(_, _)
        | Expr::CharLiteral(_, _)
        | Expr::Int8Literal(_, _)
        | Expr::Int16Literal(_, _)
        | Expr::Int32Literal(_, _)
        | Expr::Int64Literal(_, _)
        | Expr::Uint8Literal(_, _)
        | Expr::Uint16Literal(_, _)
        | Expr::Uint32Literal(_, _)
        | Expr::Uint64Literal(_, _)
        | Expr::Uint128Literal(_, _)
        | Expr::ExactNumberLiteral(_, _)
        | Expr::Float32Literal(_, _)
        | Expr::Float64Literal(_, _)
        | Expr::Identifier(_, _)
        | Expr::Break(_)
        | Expr::Continue(_)
        | Expr::Intrinsic(_)
        | Expr::ResolvedTypeRef(_, _) => {}
        Expr::TupleLiteral { elements, .. } => {
            for elem in elements {
                visitor.visit_expr(elem, diagnostics);
            }
        }
        Expr::EnumVariantRecordCreate { fields, .. } => {
            for field in fields {
                visitor.visit_expr(&field.value, diagnostics);
            }
        }
        Expr::TypeTest { expr, .. } => visitor.visit_expr(expr, diagnostics),
        Expr::TypeCast { expr, .. } => visitor.visit_expr(expr, diagnostics),
        Expr::Try { operand, .. }
        | Expr::OrReturn { operand, .. }
        | Expr::Await { operand, .. }
        | Expr::Use { operand, .. } => {
            visitor.visit_expr(operand, diagnostics);
        }
        Expr::For {
            iterable,
            body,
            pattern,
            ..
        } => {
            visitor.visit_pattern(pattern, diagnostics);
            visitor.visit_expr(iterable, diagnostics);
            visitor.visit_expr(body, diagnostics);
        }
        Expr::Closure { body, .. } | Expr::AsyncDo { body, .. } => {
            visitor.visit_expr(body, diagnostics);
        }
        Expr::PrefixedLiteral { parts, .. } => {
            // Rules run after inference has already replaced the literal with
            // its builder chain, so this arm is defensive — but if one ever
            // survives, its interpolated expressions still deserve visiting.
            for part in parts {
                match part {
                    crate::parser::ast::LiteralPart::Text(_, _) => {}
                    crate::parser::ast::LiteralPart::Value(expr, _)
                    | crate::parser::ast::LiteralPart::Spread(expr, _) => {
                        visitor.visit_expr(expr, diagnostics);
                    }
                }
            }
        }
    }
}

pub fn walk_untyped_let<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    match expr {
        Expr::Let { value, .. } => visitor.visit_expr(value, diagnostics),
        Expr::LetDestructure { pattern, value, .. } => {
            visitor.visit_pattern(pattern, diagnostics);
            visitor.visit_expr(value, diagnostics);
        }
        _ => {}
    }
}

pub fn walk_untyped_match<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::Match { subject, arms, .. } = expr {
        visitor.visit_expr(subject, diagnostics);
        for arm in arms {
            visitor.visit_pattern(&arm.pattern, diagnostics);
            if let Some(guard) = &arm.guard {
                visitor.visit_expr(guard, diagnostics);
            }
            visitor.visit_expr(&arm.body, diagnostics);
        }
    }
}

pub fn walk_untyped_block<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::Block(block) = expr {
        for e in &block.expressions {
            visitor.visit_expr(e, diagnostics);
        }
    }
}

pub fn walk_untyped_binary_op<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::BinaryOp { left, right, .. } = expr {
        visitor.visit_expr(left, diagnostics);
        visitor.visit_expr(right, diagnostics);
    }
}

pub fn walk_untyped_unary_op<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::UnaryOp { operand, .. } = expr {
        visitor.visit_expr(operand, diagnostics);
    }
}

pub fn walk_untyped_if<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::If {
        condition,
        then_branch,
        else_branch,
        ..
    } = expr
    {
        visitor.visit_expr(condition, diagnostics);
        visitor.visit_expr(then_branch, diagnostics);
        if let Some(else_br) = else_branch {
            visitor.visit_expr(else_br, diagnostics);
        }
    }
}

pub fn walk_untyped_while<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::While {
        condition, body, ..
    } = expr
    {
        visitor.visit_expr(condition, diagnostics);
        visitor.visit_expr(body, diagnostics);
    }
}

pub fn walk_untyped_function_call<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::FunctionCall { args, .. } = expr {
        for arg in args {
            visitor.visit_expr(arg, diagnostics);
        }
    }
}

pub fn walk_untyped_method_call<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::MethodCall { receiver, args, .. } = expr {
        visitor.visit_expr(receiver, diagnostics);
        for arg in args {
            visitor.visit_expr(arg, diagnostics);
        }
    }
}

pub fn walk_untyped_field_access<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::FieldAccess { object, .. } = expr {
        visitor.visit_expr(object, diagnostics);
    }
}

pub fn walk_untyped_assignment<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::Assignment { target, value, .. } = expr {
        visitor.visit_expr(target, diagnostics);
        visitor.visit_expr(value, diagnostics);
    }
}

pub fn walk_untyped_panic<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::Panic { message, .. } = expr {
        visitor.visit_expr(message, diagnostics);
    }
}

pub fn walk_untyped_assert<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::Assert {
        condition, message, ..
    } = expr
    {
        visitor.visit_expr(condition, diagnostics);
        if let Some(msg) = message {
            visitor.visit_expr(msg, diagnostics);
        }
    }
}

pub fn walk_untyped_record_create<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::RecordCreate { fields, .. } = expr {
        for field in fields {
            visitor.visit_expr(&field.value, diagnostics);
        }
    }
}

pub fn walk_untyped_record_with<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::RecordWith { object, fields, .. } = expr {
        visitor.visit_expr(object, diagnostics);
        for field in fields {
            visitor.visit_expr(&field.value, diagnostics);
        }
    }
}

pub fn walk_untyped_array_literal<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::ArrayLiteral { elements, .. } = expr {
        for e in elements {
            visitor.visit_expr(e, diagnostics);
        }
    }
}

pub fn walk_untyped_list_literal<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::ListLiteral { elements, tail, .. } = expr {
        for e in elements {
            visitor.visit_expr(e, diagnostics);
        }
        if let Some(tail) = tail {
            visitor.visit_expr(tail, diagnostics);
        }
    }
}

pub fn walk_untyped_index<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    expr: &Expr,
    diagnostics: &mut Diagnostics,
) {
    if let Expr::Index { object, index, .. } = expr {
        visitor.visit_expr(object, diagnostics);
        visitor.visit_expr(index, diagnostics);
    }
}

/// Walk an untyped pattern, recursing into sub-patterns.
pub fn walk_untyped_pattern<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    pattern: &Pattern,
    diagnostics: &mut Diagnostics,
) {
    match pattern {
        Pattern::Record { fields, .. } => {
            for field in fields {
                walk_untyped_field_pattern(visitor, field, diagnostics);
            }
        }
        Pattern::EnumVariant { .. } => {}
        Pattern::EnumVariantTuple {
            payload_patterns, ..
        } => {
            for sub_pat in payload_patterns {
                visitor.visit_pattern(sub_pat, diagnostics);
            }
        }
        Pattern::EnumVariantRecord { fields, .. } => {
            for field in fields {
                walk_untyped_field_pattern(visitor, field, diagnostics);
            }
        }
        Pattern::Tuple(sub_pats, _) => {
            for sub_pat in sub_pats {
                visitor.visit_pattern(sub_pat, diagnostics);
            }
        }
        Pattern::Variable(_, _)
        | Pattern::TypeAnnotated { .. }
        | Pattern::Wildcard(_)
        | Pattern::Literal(_, _) => {}
    }
}

fn walk_untyped_field_pattern<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    field: &FieldPattern,
    diagnostics: &mut Diagnostics,
) {
    if let Some(pat) = &field.pattern {
        visitor.visit_pattern(pat, diagnostics);
    }
}

/// Visit all function bodies across source files (untyped AST).
/// Calls `visitor.visit_expr` on each function body (top-level and module functions).
pub fn visit_all_declarations<V: ExprVisitor + ?Sized>(
    visitor: &mut V,
    source_files: &[&SourceFile],
    diagnostics: &mut Diagnostics,
) {
    for file in source_files {
        for decl in &file.declarations {
            match decl {
                Declaration::Function(func) => {
                    visitor.visit_expr(&func.body, diagnostics);
                }
                Declaration::Module(module) => {
                    for func in &module.functions {
                        visitor.visit_expr(&func.body, diagnostics);
                    }
                }
                Declaration::Class(class) => {
                    for member in &class.body {
                        match member {
                            crate::parser::ast::ClassMember::LetBinding(lb) => {
                                visitor.visit_expr(&lb.value, diagnostics);
                            }
                            crate::parser::ast::ClassMember::Method(func) => {
                                visitor.visit_expr(&func.body, diagnostics);
                            }
                            crate::parser::ast::ClassMember::Property(prop) => {
                                if let Some(ref body) = prop.body {
                                    visitor.visit_expr(body, diagnostics);
                                }
                            }
                            crate::parser::ast::ClassMember::Expression(expr) => {
                                visitor.visit_expr(expr, diagnostics);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
