use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::common::span::Span;
use crate::common::types::VarName;

use crate::typechecker::types::{
    CapturedVar, Type, TypeDef, TypedExpr, TypedExprKind, TypedMatchArm, TypedModule, TypedPattern,
};

/// Analyze closures in the typed module and populate their `captures` lists.
/// Also sets `boxed: true` on `Let`, `VarRef`, and `Assign` nodes for mutable
/// captured variables.
///
/// Runs after desugar_try and before the coerce pass.
pub fn analyze_captures(module: &mut TypedModule) {
    for func in module.functions.values_mut().chain(module.function_templates.values_mut()) {
        let mut initial_scope = Scope::new();
        // Function params are immutable variables at depth 0
        for param in &func.params {
            initial_scope.define(
                VarName(param.name.clone()),
                VarInfo {
                    ty: param.ty.clone(),
                    mutable: false,
                },
            );
        }
        func.body = analyze_expr(
            std::mem::replace(&mut func.body, dummy_expr()),
            &mut vec![initial_scope],
            0,
        );
        // Propagate boxed flags to the outer function scope:
        // closures inside the body may have mutable captures that need boxing
        // at the definition site (Let/VarRef/Assign in the outer function).
        let mut all_boxed = Vec::new();
        collect_all_boxed_vars(&func.body, &mut all_boxed);
        if !all_boxed.is_empty() {
            func.body = set_boxed_flags(
                std::mem::replace(&mut func.body, dummy_expr()),
                &all_boxed,
            );
        }
    }
    for global in module.globals.values_mut() {
        global.initializer = analyze_expr(
            std::mem::replace(&mut global.initializer, dummy_expr()),
            &mut vec![Scope::new()],
            0,
        );
    }
    for test in module.tests.iter_mut() {
        let initial_scope = Scope::new();
        test.body = analyze_expr(
            std::mem::replace(&mut test.body, dummy_expr()),
            &mut vec![initial_scope],
            0,
        );
        let mut all_boxed = Vec::new();
        collect_all_boxed_vars(&test.body, &mut all_boxed);
        if !all_boxed.is_empty() {
            test.body = set_boxed_flags(
                std::mem::replace(&mut test.body, dummy_expr()),
                &all_boxed,
            );
        }
    }
    for block in &mut module.implement_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            let mut initial_scope = Scope::new();
            for param in &method.params {
                initial_scope.define(
                    VarName(param.name.clone()),
                    VarInfo {
                        ty: param.ty.clone(),
                        mutable: false,
                    },
                );
            }
            method.body = analyze_expr(
                std::mem::replace(&mut method.body, dummy_expr()),
                &mut vec![initial_scope],
                0,
            );
            let mut all_boxed = Vec::new();
            collect_all_boxed_vars(&method.body, &mut all_boxed);
            if !all_boxed.is_empty() {
                method.body = set_boxed_flags(
                    std::mem::replace(&mut method.body, dummy_expr()),
                    &all_boxed,
                );
            }
        }
    }
    for block in &mut module.extension_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            let mut initial_scope = Scope::new();
            for param in &method.params {
                initial_scope.define(
                    VarName(param.name.clone()),
                    VarInfo {
                        ty: param.ty.clone(),
                        mutable: false,
                    },
                );
            }
            method.body = analyze_expr(
                std::mem::replace(&mut method.body, dummy_expr()),
                &mut vec![initial_scope],
                0,
            );
            let mut all_boxed = Vec::new();
            collect_all_boxed_vars(&method.body, &mut all_boxed);
            if !all_boxed.is_empty() {
                method.body = set_boxed_flags(
                    std::mem::replace(&mut method.body, dummy_expr()),
                    &all_boxed,
                );
            }
        }
    }
    // Class initializers and extends-args are expression bodies too: they are
    // inlined at each `ClassNew` site with the constructor params bound as
    // locals, so a closure in one captures those params exactly like a closure
    // in a function body captures its params. Without this pass their
    // `captures` stayed empty and codegen hit "undefined local: <param>".
    for type_def in module.types.values_mut() {
        let TypeDef::Class(cls) = type_def else { continue };
        if cls.initializer.is_empty() && cls.extends_args.is_none() {
            continue;
        }
        let mut initial_scope = Scope::new();
        for param in &cls.constructor_params {
            initial_scope.define(
                VarName(param.name.clone()),
                VarInfo {
                    ty: param.ty.clone(),
                    mutable: false,
                },
            );
        }
        let mut scopes = vec![initial_scope];
        // Extends-args are evaluated in this class's own scope, before the
        // initializer statements — same order the emitter uses.
        if let Some(extends_args) = &mut cls.extends_args {
            for arg in extends_args.iter_mut() {
                *arg = analyze_expr(std::mem::replace(arg, dummy_expr()), &mut scopes, 0);
            }
        }
        // The statements share one scope stack: a `let` in one is visible to the
        // next (and to the field pushes that follow).
        for stmt in cls.initializer.iter_mut() {
            *stmt = analyze_expr(std::mem::replace(stmt, dummy_expr()), &mut scopes, 0);
        }
        let mut all_boxed = Vec::new();
        if let Some(extends_args) = &cls.extends_args {
            for arg in extends_args {
                collect_all_boxed_vars(arg, &mut all_boxed);
            }
        }
        for stmt in &cls.initializer {
            collect_all_boxed_vars(stmt, &mut all_boxed);
        }
        if !all_boxed.is_empty() {
            if let Some(extends_args) = &mut cls.extends_args {
                for arg in extends_args.iter_mut() {
                    *arg = set_boxed_flags(std::mem::replace(arg, dummy_expr()), &all_boxed);
                }
            }
            for stmt in cls.initializer.iter_mut() {
                *stmt = set_boxed_flags(std::mem::replace(stmt, dummy_expr()), &all_boxed);
            }
        }
    }
}

/// Collect mutable captures of closures directly enclosed by this body.
/// Each closure analyzes its own body separately: names of its locals must not
/// escape into the enclosing body or an unrelated sibling closure.
fn collect_all_boxed_vars(expr: &TypedExpr, names: &mut Vec<VarName>) {
    match &expr.kind {
        TypedExprKind::Closure { captures, .. } => {
            for cap in captures {
                if cap.mutable && !names.contains(&cap.name) {
                    names.push(cap.name.clone());
                }
            }
        }
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                collect_all_boxed_vars(e, names);
            }
        }
        TypedExprKind::Let { value, .. } | TypedExprKind::Assign { value, .. }
        | TypedExprKind::Panic { message: value } | TypedExprKind::BoxToAny { inner: value }
        | TypedExprKind::NewtypeCreate { value } | TypedExprKind::NewtypeValue { value }
        | TypedExprKind::GlobalAssign { value, .. }
        | TypedExprKind::Return { value, .. }
        | TypedExprKind::UnaryOp { operand: value, .. }
        | TypedExprKind::FieldAccess { object: value, .. }
        | TypedExprKind::MethodRef { object: value, .. }
        | TypedExprKind::TypeTest { value, .. }
        | TypedExprKind::TypeCast { value, .. }
        | TypedExprKind::InterfaceObjectCoerce { inner: value, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner: value, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner: value } => {
            collect_all_boxed_vars(value, names);
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            collect_all_boxed_vars(object, names);
            collect_all_boxed_vars(value, names);
        }
        TypedExprKind::BinaryOp { left, right, .. } => {
            collect_all_boxed_vars(left, names);
            collect_all_boxed_vars(right, names);
        }
        TypedExprKind::If { condition, then_branch, else_branch } => {
            collect_all_boxed_vars(condition, names);
            collect_all_boxed_vars(then_branch, names);
            if let Some(e) = else_branch { collect_all_boxed_vars(e, names); }
        }
        TypedExprKind::While { condition, body } => {
            collect_all_boxed_vars(condition, names);
            collect_all_boxed_vars(body, names);
        }
        TypedExprKind::Assert { condition, message } => {
            collect_all_boxed_vars(condition, names);
            if let Some(m) = message { collect_all_boxed_vars(m, names); }
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::ArrayLiteral { elements: args }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. } => {
            for a in args { collect_all_boxed_vars(a, names); }
        }
        TypedExprKind::Match { subject, arms } => {
            collect_all_boxed_vars(subject, names);
            for arm in arms {
                collect_all_boxed_vars(&arm.body, names);
                if let Some(g) = &arm.guard { collect_all_boxed_vars(g, names); }
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, e) in fields { collect_all_boxed_vars(e, names); }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for e in elements { collect_all_boxed_vars(e, names); }
        }
        TypedExprKind::RecordWith { object, overrides, .. } => {
            collect_all_boxed_vars(object, names);
            for (_, _, e) in overrides { collect_all_boxed_vars(e, names); }
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. }
        | TypedExprKind::ClassVirtualCall { object: receiver, args, .. } => {
            collect_all_boxed_vars(receiver, names);
            for a in args { collect_all_boxed_vars(a, names); }
        }
        TypedExprKind::ClosureCall { callee, args } => {
            collect_all_boxed_vars(callee, names);
            for a in args { collect_all_boxed_vars(a, names); }
        }
        TypedExprKind::LetDestructure { value, .. } => {
            collect_all_boxed_vars(value, names);
        }
        TypedExprKind::Try { operand, .. } => {
            collect_all_boxed_vars(operand, names);
        }
        TypedExprKind::Await { operand, .. } => {
            collect_all_boxed_vars(operand, names);
        }
        TypedExprKind::Use { operand, .. } => {
            collect_all_boxed_vars(operand, names);
        }
        TypedExprKind::AsyncBlock { body, .. } => {
            collect_all_boxed_vars(body, names);
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                collect_all_boxed_vars(arg, names);
            }
        }
        TypedExprKind::ImplFunctionRef { .. }
        | TypedExprKind::ExtFunctionRef { .. } => {}
        _ => {}
    }
}

fn dummy_expr() -> TypedExpr {
    TypedExpr {
        kind: TypedExprKind::UnitLiteral,
        ty: Type::Unit,
        span: Span::point(Arc::from(""), 0, 0),
    }
}

/// Information about a variable in scope.
#[derive(Clone)]
struct VarInfo {
    ty: Type,
    mutable: bool,
}

/// A lexical scope level.
struct Scope {
    vars: HashMap<VarName, VarInfo>,
}

impl Scope {
    fn new() -> Self {
        Self {
            vars: HashMap::new(),
        }
    }

    fn define(&mut self, name: VarName, info: VarInfo) {
        self.vars.insert(name, info);
    }
}

/// Look up a variable in the scope stack.
fn lookup_var(scopes: &[Scope], name: &VarName) -> Option<VarInfo> {
    for scope in scopes.iter().rev() {
        if let Some(info) = scope.vars.get(name) {
            return Some(info.clone());
        }
    }
    None
}

/// Two-pass approach: first collect what each closure captures, then rewrite.
/// We do this in a single recursive walk by processing closures specially.
fn analyze_expr(
    expr: TypedExpr,
    scopes: &mut Vec<Scope>,
    closure_depth: usize,
) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty;

    let kind = match expr.kind {
        TypedExprKind::Closure {
            params,
            body,
            captures: _,
        } => {
            let new_closure_depth = closure_depth + 1;

            // Push a new scope for the closure's parameters
            let mut closure_scope = Scope::new();
            for param in &params {
                closure_scope.define(
                    param.name.clone(),
                    VarInfo {
                        ty: param.ty.clone(),
                        mutable: false,
                    },
                );
            }
            scopes.push(closure_scope);

            // Recursively analyze the body
            let analyzed_body = analyze_expr(*body, scopes, new_closure_depth);

            scopes.pop();

            // Now collect captures: walk the analyzed body to find VarRefs/Assigns
            // that reference variables from an outer closure depth
            // BTreeMap, not HashMap: `capture_map.into_values()` below fixes the closure
            // environment's field order, so a randomly-seeded hash order made the emitted
            // WASM differ byte-for-byte between otherwise identical builds.
            let mut capture_map: BTreeMap<VarName, CapturedVar> = BTreeMap::new();
            collect_captures_in_expr(&analyzed_body, scopes, &mut capture_map);

            // Determine which vars need boxing (mutable captures)
            let mut boxed_vars: Vec<VarName> = capture_map
                .values()
                .filter(|c| c.mutable)
                .map(|c| c.name.clone())
                .collect();
            // Locals captured by nested closures need cells in this body too.
            // Process them here, before returning across the closure boundary.
            collect_all_boxed_vars(&analyzed_body, &mut boxed_vars);

            // If there are mutable captures, we need to mark their Let/VarRef/Assign
            // nodes as boxed. Re-walk the body to set boxed flags.
            let final_body = if boxed_vars.is_empty() {
                analyzed_body
            } else {
                set_boxed_flags(analyzed_body, &boxed_vars)
            };

            let captures: Vec<CapturedVar> = capture_map.into_values().collect();

            TypedExprKind::Closure {
                params,
                body: Box::new(final_body),
                captures,
            }
        }

        TypedExprKind::Let {
            name,
            mutable,
            boxed,
            var_ty,
            value,
        } => {
            let value = analyze_expr(*value, scopes, closure_depth);

            // Define this variable in the current scope
            if let Some(scope) = scopes.last_mut() {
                scope.define(
                    name.clone(),
                    VarInfo {
                        ty: var_ty.clone(),
                        mutable,
                    },
                );
            }

            TypedExprKind::Let {
                name,
                mutable,
                boxed,
                var_ty,
                value: Box::new(value),
            }
        }

        TypedExprKind::Block(exprs) => {
            scopes.push(Scope::new());
            let exprs = exprs
                .into_iter()
                .map(|e| analyze_expr(e, scopes, closure_depth))
                .collect();
            scopes.pop();
            TypedExprKind::Block(exprs)
        }

        // === Recursive cases (same pattern as desugar_try.rs) ===

        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: Box::new(analyze_expr(*message, scopes, closure_depth)),
        },

        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: Box::new(analyze_expr(*condition, scopes, closure_depth)),
            message: message.map(|m| Box::new(analyze_expr(*m, scopes, closure_depth))),
        },

        TypedExprKind::Assign {
            name,
            target_ty,
            boxed,
            value,
        } => TypedExprKind::Assign {
            name,
            target_ty,
            boxed,
            value: Box::new(analyze_expr(*value, scopes, closure_depth)),
        },

        TypedExprKind::GlobalAssign { name, type_params, value } => TypedExprKind::GlobalAssign {
            name,
            type_params,
            value: Box::new(analyze_expr(*value, scopes, closure_depth)),
        },

        TypedExprKind::FunctionCall { name, args, type_params } => TypedExprKind::FunctionCall {
            name,
            args: args
                .into_iter()
                .map(|a| analyze_expr(a, scopes, closure_depth))
                .collect(),
            type_params,
        },

        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: Box::new(analyze_expr(*left, scopes, closure_depth)),
            right: Box::new(analyze_expr(*right, scopes, closure_depth)),
        },

        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: Box::new(analyze_expr(*operand, scopes, closure_depth)),
        },

        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => TypedExprKind::If {
            condition: Box::new(analyze_expr(*condition, scopes, closure_depth)),
            then_branch: Box::new(analyze_expr(*then_branch, scopes, closure_depth)),
            else_branch: else_branch
                .map(|e| Box::new(analyze_expr(*e, scopes, closure_depth))),
        },

        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: Box::new(analyze_expr(*condition, scopes, closure_depth)),
            body: Box::new(analyze_expr(*body, scopes, closure_depth)),
        },

        TypedExprKind::Match { subject, arms } => TypedExprKind::Match {
            subject: Box::new(analyze_expr(*subject, scopes, closure_depth)),
            arms: arms
                .into_iter()
                .map(|arm| {
                    // Define pattern variables in a new scope
                    scopes.push(Scope::new());
                    define_pattern_vars(&arm.pattern, scopes);
                    let guard = arm
                        .guard
                        .map(|g| Box::new(analyze_expr(*g, scopes, closure_depth)));
                    let body = Box::new(analyze_expr(*arm.body, scopes, closure_depth));
                    scopes.pop();
                    TypedMatchArm {
                        pattern: arm.pattern,
                        guard,
                        body,
                        span: arm.span,
                    }
                })
                .collect(),
        },

        TypedExprKind::RecordCreate { fqn, fields, type_params } => TypedExprKind::RecordCreate {
            fqn,
            type_params,
            fields: fields
                .into_iter()
                .map(|(name, expr)| (name, analyze_expr(expr, scopes, closure_depth)))
                .collect(),
        },

        TypedExprKind::TupleLiteral { elements } => TypedExprKind::TupleLiteral {
            elements: elements.into_iter().map(|e| analyze_expr(e, scopes, closure_depth)).collect(),
        },

        TypedExprKind::EnumCreate {
            fqn,
            variant_name,
            args,
            type_params,
        } => TypedExprKind::EnumCreate {
            fqn,
            variant_name,
            type_params,
            args: args
                .into_iter()
                .map(|a| analyze_expr(a, scopes, closure_depth))
                .collect(),
        },

        TypedExprKind::EnumVariantRecordCreate {
            fqn,
            variant_name,
            args,
            type_params,
        } => TypedExprKind::EnumVariantRecordCreate {
            fqn,
            variant_name,
            type_params,
            args: args
                .into_iter()
                .map(|a| analyze_expr(a, scopes, closure_depth))
                .collect(),
        },

        TypedExprKind::FieldAccess {
            object,
            field_name,
            field_index,
            boxed,
        } => TypedExprKind::FieldAccess {
            object: Box::new(analyze_expr(*object, scopes, closure_depth)),
            field_name,
            field_index,
            boxed,
        },

        TypedExprKind::FieldAssign {
            object,
            field_name,
            field_index,
            value,
            boxed,
        } => TypedExprKind::FieldAssign {
            object: Box::new(analyze_expr(*object, scopes, closure_depth)),
            field_name,
            field_index,
            value: Box::new(analyze_expr(*value, scopes, closure_depth)),
            boxed,
        },

        TypedExprKind::RecordWith {
            object,
            fqn,
            overrides,
            type_params,
        } => TypedExprKind::RecordWith {
            object: Box::new(analyze_expr(*object, scopes, closure_depth)),
            fqn,
            type_params,
            overrides: overrides
                .into_iter()
                .map(|(name, idx, expr)| {
                    (name, idx, analyze_expr(expr, scopes, closure_depth))
                })
                .collect(),
        },

        TypedExprKind::ArrayLiteral { elements } => TypedExprKind::ArrayLiteral {
            elements: elements
                .into_iter()
                .map(|e| analyze_expr(e, scopes, closure_depth))
                .collect(),
        },

        TypedExprKind::IntrinsicCall { intrinsic, args } => TypedExprKind::IntrinsicCall {
            intrinsic,
            args: args
                .into_iter()
                .map(|a| analyze_expr(a, scopes, closure_depth))
                .collect(),
        },

        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: Box::new(analyze_expr(*value, scopes, closure_depth)),
            target_type,
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: Box::new(analyze_expr(*value, scopes, closure_depth)),
            target_type,
        },
        TypedExprKind::LetDestructure {
            pattern,
            var_ty,
            value,
        } => {
            let value = analyze_expr(*value, scopes, closure_depth);
            define_pattern_vars(&pattern, scopes);
            TypedExprKind::LetDestructure {
                pattern,
                var_ty,
                value: Box::new(value),
            }
        }
        TypedExprKind::NewtypeCreate { value } => TypedExprKind::NewtypeCreate {
            value: Box::new(analyze_expr(*value, scopes, closure_depth)),
        },
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: Box::new(analyze_expr(*value, scopes, closure_depth)),
        },
        TypedExprKind::InterfaceObjectCoerce {
            inner,
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        } => TypedExprKind::InterfaceObjectCoerce {
            inner: Box::new(analyze_expr(*inner, scopes, closure_depth)),
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        },
        TypedExprKind::TemplateInterfaceObjectCoerce {
            inner,
            traits,
            concrete_type,
        } => TypedExprKind::TemplateInterfaceObjectCoerce {
            inner: Box::new(analyze_expr(*inner, scopes, closure_depth)),
            traits,
            concrete_type,
        },
        TypedExprKind::InterfaceObjectUpcast { inner } => TypedExprKind::InterfaceObjectUpcast {
            inner: Box::new(analyze_expr(*inner, scopes, closure_depth)),
        },
        TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name,
            method_name,
            member_name,
            receiver,
            args,
        } => TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name,
            method_name,
            member_name,
            receiver: Box::new(analyze_expr(*receiver, scopes, closure_depth)),
            args: args
                .into_iter()
                .map(|a| analyze_expr(a, scopes, closure_depth))
                .collect(),
        },
        TypedExprKind::ClassNew { mangled_name, args, type_params } => TypedExprKind::ClassNew {
            mangled_name,
            type_params,
            args: args
                .into_iter()
                .map(|a| analyze_expr(a, scopes, closure_depth))
                .collect(),
        },
        TypedExprKind::ClassStructCreate { target_mangled_name, fields, type_params } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields
                .into_iter()
                .map(|f| analyze_expr(f, scopes, closure_depth))
                .collect(),
        },
        TypedExprKind::ClassVirtualCall {
            object,
            vtable_slot,
            args,
        } => TypedExprKind::ClassVirtualCall {
            object: Box::new(analyze_expr(*object, scopes, closure_depth)),
            vtable_slot,
            args: args
                .into_iter()
                .map(|a| analyze_expr(a, scopes, closure_depth))
                .collect(),
        },
        TypedExprKind::ClassSuperCall {
            method_mangled,
            args,
        } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args
                .into_iter()
                .map(|a| analyze_expr(a, scopes, closure_depth))
                .collect(),
        },
        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: Box::new(analyze_expr(*value, scopes, closure_depth)),
            return_type,
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: Box::new(analyze_expr(*callee, scopes, closure_depth)),
            args: args
                .into_iter()
                .map(|a| analyze_expr(a, scopes, closure_depth))
                .collect(),
        },
        TypedExprKind::BoxToAny { inner } => TypedExprKind::BoxToAny {
            inner: Box::new(analyze_expr(*inner, scopes, closure_depth)),
        },
        TypedExprKind::MethodRef { object, method_name, type_params } => TypedExprKind::MethodRef {
            object: Box::new(analyze_expr(*object, scopes, closure_depth)),
            method_name,
            type_params,
        },

        // Leaf nodes — no sub-expressions
        kind @ (TypedExprKind::UnitLiteral
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
        | TypedExprKind::Continue) => kind,

        // Try should already be desugared, but pass through just in case
        TypedExprKind::Try {
            operand,
            unwrap_method,
            unwrap_return_type,
            return_type,
            from_method,
        } => TypedExprKind::Try {
            operand: Box::new(analyze_expr(*operand, scopes, closure_depth)),
            unwrap_method,
            unwrap_return_type,
            return_type,
            from_method,
        },

        TypedExprKind::Await { operand, return_type, and_then_method, map_method, source_location_mn } => TypedExprKind::Await {
            operand: Box::new(analyze_expr(*operand, scopes, closure_depth)),
            return_type,
            and_then_method,
            map_method,
            source_location_mn,
        },

        TypedExprKind::Use { .. } => {
            unreachable!("Use nodes should be desugared before capture analysis")
        }

        TypedExprKind::ForLoop { .. } => {
            unreachable!("ForLoop nodes should be desugared before capture analysis")
        }

        TypedExprKind::AsyncBlock { body, succeed_method } => TypedExprKind::AsyncBlock {
            body: Box::new(analyze_expr(*body, scopes, closure_depth)),
            succeed_method,
        },

        TypedExprKind::ImplFunctionCall { trait_fqn, trait_type_params, for_type, method_name, args, method_type_params } => TypedExprKind::ImplFunctionCall {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            args: args.into_iter().map(|a| analyze_expr(a, scopes, closure_depth)).collect(),
            method_type_params,
        },
        kind @ TypedExprKind::ImplFunctionRef { .. } => kind,
        TypedExprKind::ExtFunctionCall { ext_fqn, for_type, method_name, args, type_params } => TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type,
            method_name,
            args: args.into_iter().map(|a| analyze_expr(a, scopes, closure_depth)).collect(),
            type_params,
        },
        kind @ TypedExprKind::ExtFunctionRef { .. } => kind,
    };

    TypedExpr { kind, ty, span }
}

/// Define variables introduced by a pattern into the current scope.
fn define_pattern_vars(
    pattern: &TypedPattern,
    scopes: &mut [Scope],
) {
    match pattern {
        TypedPattern::Variable(name, ty) => {
            if let Some(scope) = scopes.last_mut() {
                scope.define(
                    name.clone(),
                    VarInfo {
                        ty: ty.clone(),
                        mutable: false,
                    },
                );
            }
        }
        TypedPattern::TypeAnnotated { binding, ty } => {
            if let Some(scope) = scopes.last_mut() {
                scope.define(
                    binding.clone(),
                    VarInfo {
                        ty: ty.clone(),
                        mutable: false,
                    },
                );
            }
        }
        TypedPattern::Record { fields, .. } => {
            for field in fields {
                define_pattern_vars(&field.pattern, scopes);
            }
        }
        TypedPattern::EnumVariant {
            payload_patterns, ..
        } => {
            for sub_pat in payload_patterns {
                define_pattern_vars(sub_pat, scopes);
            }
        }
        TypedPattern::EnumVariantRecord {
            field_patterns, ..
        } => {
            for field in field_patterns {
                define_pattern_vars(&field.pattern, scopes);
            }
        }
        TypedPattern::Tuple {
            element_patterns, ..
        } => {
            for sub_pat in element_patterns {
                define_pattern_vars(sub_pat, scopes);
            }
        }
        TypedPattern::Newtype { inner_pattern, .. } => {
            define_pattern_vars(inner_pattern, scopes);
        }
        TypedPattern::Wildcard | TypedPattern::Literal(_) => {}
    }
}

/// Walk an analyzed expression tree to find variables referenced inside a closure
/// that were defined at `parent_closure_depth` or shallower.
fn collect_captures_in_expr(
    expr: &TypedExpr,
    scopes: &[Scope],
    captures: &mut BTreeMap<VarName, CapturedVar>,
) {
    match &expr.kind {
        TypedExprKind::VarRef { name, .. } => {
            // Check if this variable is defined at an outer closure depth
            if let Some(info) = lookup_var(scopes, name) {
                // Variable was defined before the closure boundary
                captures.entry(name.clone()).or_insert(CapturedVar {
                    name: name.clone(),
                    ty: info.ty,
                    mutable: info.mutable,
                });
            }
        }
        TypedExprKind::Assign { name, value, .. } => {
            // The assigned variable might be a capture
            if let Some(info) = lookup_var(scopes, name) {
                captures.entry(name.clone()).or_insert(CapturedVar {
                    name: name.clone(),
                    ty: info.ty,
                    mutable: info.mutable,
                });
            }
            collect_captures_in_expr(value, scopes, captures);
        }
        // Nested closures: their captures that reference our parent scope
        // need to be propagated up
        TypedExprKind::Closure {
            captures: inner_captures,
            body,
            ..
        } => {
            // Any capture of the inner closure that refers to a var from
            // parent_closure_depth or shallower also needs to be captured by us
            for cap in inner_captures {
                if let Some(info) = lookup_var(scopes, &cap.name) {
                    captures.entry(cap.name.clone()).or_insert(CapturedVar {
                        name: cap.name.clone(),
                        ty: info.ty,
                        mutable: info.mutable,
                    });
                }
            }
            // Also walk the body for any direct references we might have missed
            // (though inner closure captures should cover it)
            let _ = body;
        }
        // Recursive walk for all other node types
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                collect_captures_in_expr(e, scopes, captures);
            }
        }
        TypedExprKind::Let { value, .. } => {
            collect_captures_in_expr(value, scopes, captures);
        }
        TypedExprKind::BinaryOp { left, right, .. } => {
            collect_captures_in_expr(left, scopes, captures);
            collect_captures_in_expr(right, scopes, captures);
        }
        TypedExprKind::UnaryOp { operand, .. } => {
            collect_captures_in_expr(operand, scopes, captures);
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_captures_in_expr(condition, scopes, captures);
            collect_captures_in_expr(then_branch, scopes, captures);
            if let Some(e) = else_branch {
                collect_captures_in_expr(e, scopes, captures);
            }
        }
        TypedExprKind::While { condition, body } => {
            collect_captures_in_expr(condition, scopes, captures);
            collect_captures_in_expr(body, scopes, captures);
        }
        TypedExprKind::Match { subject, arms } => {
            collect_captures_in_expr(subject, scopes, captures);
            for arm in arms {
                if let Some(guard) = &arm.guard {
                    collect_captures_in_expr(guard, scopes, captures);
                }
                collect_captures_in_expr(&arm.body, scopes, captures);
            }
        }
        TypedExprKind::Panic { message } => {
            collect_captures_in_expr(message, scopes, captures);
        }
        TypedExprKind::Assert { condition, message } => {
            collect_captures_in_expr(condition, scopes, captures);
            if let Some(m) = message {
                collect_captures_in_expr(m, scopes, captures);
            }
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. } => {
            for a in args {
                collect_captures_in_expr(a, scopes, captures);
            }
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for f in fields {
                collect_captures_in_expr(f, scopes, captures);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, e) in fields {
                collect_captures_in_expr(e, scopes, captures);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for e in elements {
                collect_captures_in_expr(e, scopes, captures);
            }
        }
        TypedExprKind::FieldAccess { object, .. }
        | TypedExprKind::MethodRef { object, .. }
        | TypedExprKind::NewtypeCreate { value: object }
        | TypedExprKind::NewtypeValue { value: object }
        | TypedExprKind::BoxToAny { inner: object }
        | TypedExprKind::TypeTest { value: object, .. }
        | TypedExprKind::TypeCast { value: object, .. }
        | TypedExprKind::GlobalAssign { value: object, .. }
        | TypedExprKind::Return { value: object, .. }
        | TypedExprKind::Try { operand: object, .. }
        | TypedExprKind::Await { operand: object, .. }
        | TypedExprKind::Use { operand: object, .. }
        | TypedExprKind::AsyncBlock { body: object, .. } => {
            collect_captures_in_expr(object, scopes, captures);
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            collect_captures_in_expr(object, scopes, captures);
            collect_captures_in_expr(value, scopes, captures);
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            collect_captures_in_expr(object, scopes, captures);
            for (_, _, e) in overrides {
                collect_captures_in_expr(e, scopes, captures);
            }
        }
        TypedExprKind::ArrayLiteral { elements } => {
            for e in elements {
                collect_captures_in_expr(e, scopes, captures);
            }
        }
        TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner } => {
            collect_captures_in_expr(inner, scopes, captures);
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. }
        | TypedExprKind::ClassVirtualCall {
            object: receiver,
            args,
            ..
        } => {
            collect_captures_in_expr(receiver, scopes, captures);
            for a in args {
                collect_captures_in_expr(a, scopes, captures);
            }
        }
        TypedExprKind::ClosureCall { callee, args } => {
            collect_captures_in_expr(callee, scopes, captures);
            for a in args {
                collect_captures_in_expr(a, scopes, captures);
            }
        }
        TypedExprKind::LetDestructure { value, .. } => {
            collect_captures_in_expr(value, scopes, captures);
        }
        TypedExprKind::ForLoop { .. } => {
            unreachable!("ForLoop nodes should be desugared before capture analysis")
        }
        // Leaf nodes
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
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::FunctionRef { .. }
        | TypedExprKind::Break
        | TypedExprKind::Continue => {}
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                collect_captures_in_expr(arg, scopes, captures);
            }
        }
        TypedExprKind::ImplFunctionRef { .. }
        | TypedExprKind::ExtFunctionRef { .. } => {}
    }
}

/// Walk an expression tree and set `boxed: true` on Let/VarRef/Assign nodes
/// for variables in the `boxed_vars` set.
fn set_boxed_flags(expr: TypedExpr, boxed_vars: &[VarName]) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty;

    let kind = match expr.kind {
        TypedExprKind::Let {
            name,
            mutable,
            boxed,
            var_ty,
            value,
        } => {
            let value = set_boxed_flags(*value, boxed_vars);
            let boxed = boxed || boxed_vars.contains(&name);
            TypedExprKind::Let {
                name,
                mutable,
                boxed,
                var_ty,
                value: Box::new(value),
            }
        }
        TypedExprKind::VarRef { name, boxed } => {
            let boxed = boxed || boxed_vars.contains(&name);
            TypedExprKind::VarRef { name, boxed }
        }
        TypedExprKind::Assign {
            name,
            target_ty,
            boxed,
            value,
        } => {
            let value = set_boxed_flags(*value, boxed_vars);
            let boxed = boxed || boxed_vars.contains(&name);
            TypedExprKind::Assign {
                name,
                target_ty,
                boxed,
                value: Box::new(value),
            }
        }

        // Recursive cases
        TypedExprKind::Block(exprs) => {
            TypedExprKind::Block(exprs.into_iter().map(|e| set_boxed_flags(e, boxed_vars)).collect())
        }
        // Its captures and locals were already boxed when analyzing the closure.
        // Reapplying this body's names can mistake an immutable parameter in a
        // sibling closure for a mutable local with the same spelling.
        kind @ TypedExprKind::Closure { .. } => kind,
        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: Box::new(set_boxed_flags(*left, boxed_vars)),
            right: Box::new(set_boxed_flags(*right, boxed_vars)),
        },
        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: Box::new(set_boxed_flags(*operand, boxed_vars)),
        },
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => TypedExprKind::If {
            condition: Box::new(set_boxed_flags(*condition, boxed_vars)),
            then_branch: Box::new(set_boxed_flags(*then_branch, boxed_vars)),
            else_branch: else_branch.map(|e| Box::new(set_boxed_flags(*e, boxed_vars))),
        },
        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: Box::new(set_boxed_flags(*condition, boxed_vars)),
            body: Box::new(set_boxed_flags(*body, boxed_vars)),
        },
        TypedExprKind::Match { subject, arms } => TypedExprKind::Match {
            subject: Box::new(set_boxed_flags(*subject, boxed_vars)),
            arms: arms
                .into_iter()
                .map(|arm| TypedMatchArm {
                    body: Box::new(set_boxed_flags(*arm.body, boxed_vars)),
                    guard: arm.guard.map(|g| Box::new(set_boxed_flags(*g, boxed_vars))),
                    ..arm
                })
                .collect(),
        },
        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: Box::new(set_boxed_flags(*message, boxed_vars)),
        },
        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: Box::new(set_boxed_flags(*condition, boxed_vars)),
            message: message.map(|m| Box::new(set_boxed_flags(*m, boxed_vars))),
        },
        TypedExprKind::FunctionCall { name, args, type_params } => TypedExprKind::FunctionCall {
            name,
            args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
            type_params,
        },
        TypedExprKind::GlobalAssign { name, type_params, value } => TypedExprKind::GlobalAssign {
            name,
            type_params,
            value: Box::new(set_boxed_flags(*value, boxed_vars)),
        },
        TypedExprKind::RecordCreate { fqn, fields, type_params } => TypedExprKind::RecordCreate {
            fqn,
            type_params,
            fields: fields
                .into_iter()
                .map(|(n, e)| (n, set_boxed_flags(e, boxed_vars)))
                .collect(),
        },
        TypedExprKind::TupleLiteral { elements } => TypedExprKind::TupleLiteral {
            elements: elements.into_iter().map(|e| set_boxed_flags(e, boxed_vars)).collect(),
        },
        TypedExprKind::EnumCreate { fqn, variant_name, args, type_params } => TypedExprKind::EnumCreate {
            fqn,
            variant_name,
            type_params,
            args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
        },
        TypedExprKind::EnumVariantRecordCreate { fqn, variant_name, args, type_params } => {
            TypedExprKind::EnumVariantRecordCreate {
                fqn,
                variant_name,
                type_params,
                args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
            }
        }
        TypedExprKind::FieldAccess { object, field_name, field_index, boxed } => {
            TypedExprKind::FieldAccess {
                object: Box::new(set_boxed_flags(*object, boxed_vars)),
                field_name,
                field_index,
                boxed,
            }
        }
        TypedExprKind::FieldAssign { object, field_name, field_index, value, boxed } => {
            TypedExprKind::FieldAssign {
                object: Box::new(set_boxed_flags(*object, boxed_vars)),
                field_name,
                field_index,
                value: Box::new(set_boxed_flags(*value, boxed_vars)),
                boxed,
            }
        }
        TypedExprKind::RecordWith { object, fqn, overrides, type_params } => TypedExprKind::RecordWith {
            object: Box::new(set_boxed_flags(*object, boxed_vars)),
            fqn,
            type_params,
            overrides: overrides
                .into_iter()
                .map(|(n, i, e)| (n, i, set_boxed_flags(e, boxed_vars)))
                .collect(),
        },
        TypedExprKind::ArrayLiteral { elements } => TypedExprKind::ArrayLiteral {
            elements: elements.into_iter().map(|e| set_boxed_flags(e, boxed_vars)).collect(),
        },
        TypedExprKind::IntrinsicCall { intrinsic, args } => TypedExprKind::IntrinsicCall {
            intrinsic,
            args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
        },
        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: Box::new(set_boxed_flags(*value, boxed_vars)),
            target_type,
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: Box::new(set_boxed_flags(*value, boxed_vars)),
            target_type,
        },
        TypedExprKind::LetDestructure { pattern, var_ty, value } => {
            TypedExprKind::LetDestructure {
                pattern,
                var_ty,
                value: Box::new(set_boxed_flags(*value, boxed_vars)),
            }
        }
        TypedExprKind::NewtypeCreate { value } => TypedExprKind::NewtypeCreate {
            value: Box::new(set_boxed_flags(*value, boxed_vars)),
        },
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: Box::new(set_boxed_flags(*value, boxed_vars)),
        },
        TypedExprKind::BoxToAny { inner } => TypedExprKind::BoxToAny {
            inner: Box::new(set_boxed_flags(*inner, boxed_vars)),
        },
        TypedExprKind::InterfaceObjectCoerce { inner, interface_mangled_name, concrete_type, vtable_methods } => {
            TypedExprKind::InterfaceObjectCoerce {
                inner: Box::new(set_boxed_flags(*inner, boxed_vars)),
                interface_mangled_name,
                concrete_type,
                vtable_methods,
            }
        }
        TypedExprKind::TemplateInterfaceObjectCoerce { inner, traits, concrete_type } => {
            TypedExprKind::TemplateInterfaceObjectCoerce {
                inner: Box::new(set_boxed_flags(*inner, boxed_vars)),
                traits,
                concrete_type,
            }
        }
        TypedExprKind::InterfaceObjectUpcast { inner } => TypedExprKind::InterfaceObjectUpcast {
            inner: Box::new(set_boxed_flags(*inner, boxed_vars)),
        },
        TypedExprKind::InterfaceObjectMethodCall { interface_mangled_name, method_name, member_name, receiver, args } => {
            TypedExprKind::InterfaceObjectMethodCall {
                interface_mangled_name,
                method_name,
                member_name,
                receiver: Box::new(set_boxed_flags(*receiver, boxed_vars)),
                args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
            }
        }
        TypedExprKind::ClassNew { mangled_name, args, type_params } => TypedExprKind::ClassNew {
            mangled_name,
            type_params,
            args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
        },
        TypedExprKind::ClassStructCreate { target_mangled_name, fields, type_params } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields.into_iter().map(|f| set_boxed_flags(f, boxed_vars)).collect(),
        },
        TypedExprKind::ClassVirtualCall { object, vtable_slot, args } => {
            TypedExprKind::ClassVirtualCall {
                object: Box::new(set_boxed_flags(*object, boxed_vars)),
                vtable_slot,
                args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
            }
        }
        TypedExprKind::ClassSuperCall { method_mangled, args } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
        },
        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: Box::new(set_boxed_flags(*value, boxed_vars)),
            return_type,
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: Box::new(set_boxed_flags(*callee, boxed_vars)),
            args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
        },
        TypedExprKind::MethodRef { object, method_name, type_params } => TypedExprKind::MethodRef {
            object: Box::new(set_boxed_flags(*object, boxed_vars)),
            method_name,
            type_params,
        },
        TypedExprKind::Try { operand, unwrap_method, unwrap_return_type, return_type, from_method } => {
            TypedExprKind::Try {
                operand: Box::new(set_boxed_flags(*operand, boxed_vars)),
                unwrap_method,
                unwrap_return_type,
                return_type,
                from_method,
            }
        }
        TypedExprKind::Await { operand, return_type, and_then_method, map_method, source_location_mn } => {
            TypedExprKind::Await {
                operand: Box::new(set_boxed_flags(*operand, boxed_vars)),
                return_type,
                and_then_method,
                map_method,
                source_location_mn,
            }
        }
        TypedExprKind::Use { .. } => {
            unreachable!("Use nodes should be desugared before capture analysis")
        }
        TypedExprKind::ForLoop { .. } => {
            unreachable!("ForLoop nodes should be desugared before capture analysis")
        }
        TypedExprKind::AsyncBlock { body, succeed_method } => {
            TypedExprKind::AsyncBlock {
                body: Box::new(set_boxed_flags(*body, boxed_vars)),
                succeed_method,
            }
        }

        // Leaf nodes — unchanged
        kind @ (TypedExprKind::UnitLiteral
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
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::FunctionRef { .. }
        | TypedExprKind::Break
        | TypedExprKind::Continue) => kind,

        TypedExprKind::ImplFunctionCall { trait_fqn, trait_type_params, for_type, method_name, args, method_type_params } => TypedExprKind::ImplFunctionCall {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
            method_type_params,
        },
        kind @ TypedExprKind::ImplFunctionRef { .. } => kind,
        TypedExprKind::ExtFunctionCall { ext_fqn, for_type, method_name, args, type_params } => TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type,
            method_name,
            args: args.into_iter().map(|a| set_boxed_flags(a, boxed_vars)).collect(),
            type_params,
        },
        kind @ TypedExprKind::ExtFunctionRef { .. } => kind,
    };

    TypedExpr { kind, ty, span }
}
