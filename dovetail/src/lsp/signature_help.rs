use tower_lsp::lsp_types::{
    Documentation, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel, SignatureHelp,
    SignatureInformation,
};

use crate::common::span::{FilePath, Span};
use crate::common::types::Fqn;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind, TypedModule, TypedPattern};

use super::position::span_contains;

/// Produce signature help for a call expression at the cursor position.
///
/// Walks all function/global/test bodies in the file to find the innermost
/// call expression containing the cursor, then builds a `SignatureHelp`
/// response with parameter labels and doc comments.
pub fn signature_help_at_position(
    typed_module: &TypedModule,
    registry: &Registry,
    file: &FilePath,
    line: u32,
    column: u32,
) -> Option<SignatureHelp> {
    let mut best: Option<CallInfo> = None;

    for func in super::source_functions::values(typed_module) {
        if func.span.file != *file {
            continue;
        }
        find_enclosing_call(&func.body, file, line, column, &mut best);
    }

    for global in typed_module.globals.values() {
        if global.span.file != *file {
            continue;
        }
        find_enclosing_call(&global.initializer, file, line, column, &mut best);
    }

    for test in &typed_module.tests {
        if test.span.file != *file {
            continue;
        }
        find_enclosing_call(&test.body, file, line, column, &mut best);
    }

    let call = best?;
    build_signature_help(&call, typed_module, registry)
}

/// Information about a call expression found at the cursor position.
struct CallInfo {
    kind: CallKind,
    active_param: u32,
    /// Span area for finding the innermost call.
    area: u64,
}

enum CallKind {
    FunctionCall {
        name: crate::common::types::MangledName,
    },
    ClassNew {
        mangled_name: crate::common::types::MangledName,
    },
    ClassVirtualCall {
        args: Vec<(String, Type)>,
    },
    ClassSuperCall {
        method_mangled: crate::common::types::MangledName,
    },
    InterfaceObjectMethodCall {
        trait_fqn: Fqn,
        method_name: String,
    },
    ClosureCall {
        callee_type: Type,
    },
}

/// Recursively walk the typed AST to find the innermost call expression
/// containing the cursor, and determine the active parameter index.
fn find_enclosing_call(
    expr: &TypedExpr,
    file: &FilePath,
    line: u32,
    column: u32,
    best: &mut Option<CallInfo>,
) {
    if expr.span.file != *file || !span_contains(&expr.span, line, column) {
        return;
    }

    // Check if this expression is a call expression
    let call_kind = match &expr.kind {
        TypedExprKind::FunctionCall {
            name,
            args,
            type_params: _,
        } => {
            let active = compute_active_param(args, &expr.span, line, column);
            Some((CallKind::FunctionCall { name: name.clone() }, active))
        }
        TypedExprKind::ClassNew {
            mangled_name, args, ..
        } => {
            let active = compute_active_param(args, &expr.span, line, column);
            Some((
                CallKind::ClassNew {
                    mangled_name: mangled_name.clone(),
                },
                active,
            ))
        }
        TypedExprKind::ClassVirtualCall { args, .. } => {
            // args includes self as first arg in the typed AST, so skip it for display
            let user_args = if args.len() > 1 { &args[1..] } else { &[] };
            let active = compute_active_param(user_args, &expr.span, line, column);
            // Build param info from the virtual call args (excluding self)
            let param_info: Vec<(String, Type)> = user_args
                .iter()
                .enumerate()
                .map(|(i, a)| (format!("arg{i}"), a.ty.clone()))
                .collect();
            Some((CallKind::ClassVirtualCall { args: param_info }, active))
        }
        TypedExprKind::ClassSuperCall {
            method_mangled,
            args,
        } => {
            // args includes self as first arg
            let user_args = if args.len() > 1 { &args[1..] } else { &[] };
            let active = compute_active_param(user_args, &expr.span, line, column);
            Some((
                CallKind::ClassSuperCall {
                    method_mangled: method_mangled.clone(),
                },
                active,
            ))
        }
        TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name,
            method_name,
            receiver,
            args,
            ..
        } => {
            let active = compute_active_param(args, &expr.span, line, column);
            // The node's key is the declaring COMPONENT's per-trait key — find that
            // component in the receiver's set (single component: trivially first).
            let trait_fqn = match &receiver.ty {
                Type::InterfaceObject { traits, .. } => traits
                    .iter()
                    .find(|c| {
                        crate::common::types::MangledName::for_interface_object_per_interface(
                            &c.trait_fqn,
                        ) == *interface_mangled_name
                    })
                    .map(|c| c.trait_fqn.clone())
                    .unwrap_or_else(|| traits[0].trait_fqn.clone()),
                _ => return,
            };
            Some((
                CallKind::InterfaceObjectMethodCall {
                    trait_fqn,
                    method_name: method_name.clone(),
                },
                active,
            ))
        }
        TypedExprKind::ClosureCall { callee, args } => {
            let active = compute_active_param(args, &expr.span, line, column);
            Some((
                CallKind::ClosureCall {
                    callee_type: callee.ty.clone(),
                },
                active,
            ))
        }
        _ => None,
    };

    if let Some((kind, active_param)) = call_kind {
        let area = span_area(&expr.span);
        let is_better = match best {
            Some(b) => area <= b.area,
            None => true,
        };
        if is_better {
            *best = Some(CallInfo {
                kind,
                active_param,
                area,
            });
        }
    }

    // Recurse into children
    walk_children_for_calls(expr, file, line, column, best);
}

/// Compute which parameter is "active" based on cursor position relative to argument spans.
fn compute_active_param(args: &[TypedExpr], call_span: &Span, line: u32, column: u32) -> u32 {
    if args.is_empty() {
        return 0;
    }

    // Check each argument span
    for (i, arg) in args.iter().enumerate() {
        if span_contains(&arg.span, line, column) {
            return i as u32;
        }
    }

    // Cursor is after the last arg (typing next arg) or between args
    // Find the first arg whose span starts after the cursor
    for (i, arg) in args.iter().enumerate() {
        if arg.span.line > line || (arg.span.line == line && arg.span.column > column) {
            // Cursor is before this arg
            return i as u32;
        }
    }

    // Cursor is after all args — check if still within call span
    if span_contains(call_span, line, column) {
        return args.len() as u32;
    }

    args.len().saturating_sub(1) as u32
}

fn span_area(span: &Span) -> u64 {
    let lines = span.end_line.saturating_sub(span.line) as u64;
    let cols = span.end_column.saturating_sub(span.column) as u64;
    lines * 1000 + cols
}

/// Walk into children of a typed expression looking for call expressions.
fn walk_children_for_calls(
    expr: &TypedExpr,
    file: &FilePath,
    line: u32,
    column: u32,
    best: &mut Option<CallInfo>,
) {
    match &expr.kind {
        TypedExprKind::BinaryOp { left, right, .. } => {
            find_enclosing_call(left, file, line, column, best);
            find_enclosing_call(right, file, line, column, best);
        }
        TypedExprKind::UnaryOp { operand, .. } => {
            find_enclosing_call(operand, file, line, column, best);
        }
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                find_enclosing_call(e, file, line, column, best);
            }
        }
        TypedExprKind::Panic { message } => {
            find_enclosing_call(message, file, line, column, best);
        }
        TypedExprKind::Assert { condition, message } => {
            find_enclosing_call(condition, file, line, column, best);
            if let Some(msg) = message {
                find_enclosing_call(msg, file, line, column, best);
            }
        }
        TypedExprKind::Let { value, .. } => {
            find_enclosing_call(value, file, line, column, best);
        }
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            find_enclosing_call(value, file, line, column, best);
        }
        TypedExprKind::FunctionCall { args, .. } => {
            for arg in args {
                find_enclosing_call(arg, file, line, column, best);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            find_enclosing_call(condition, file, line, column, best);
            find_enclosing_call(then_branch, file, line, column, best);
            if let Some(eb) = else_branch {
                find_enclosing_call(eb, file, line, column, best);
            }
        }
        TypedExprKind::While { condition, body } => {
            find_enclosing_call(condition, file, line, column, best);
            find_enclosing_call(body, file, line, column, best);
        }
        TypedExprKind::Match { subject, arms } => {
            find_enclosing_call(subject, file, line, column, best);
            for arm in arms {
                find_enclosing_call(&arm.body, file, line, column, best);
                walk_pattern_for_calls(&arm.pattern, file, line, column, best);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for element in elements {
                find_enclosing_call(element, file, line, column, best);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, field_expr) in fields {
                find_enclosing_call(field_expr, file, line, column, best);
            }
        }
        TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                find_enclosing_call(arg, file, line, column, best);
            }
        }
        TypedExprKind::FieldAccess { object, .. } | TypedExprKind::FieldAssign { object, .. } => {
            find_enclosing_call(object, file, line, column, best);
            if let TypedExprKind::FieldAssign { value, .. } = &expr.kind {
                find_enclosing_call(value, file, line, column, best);
            }
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            find_enclosing_call(object, file, line, column, best);
            for (_, _, val) in overrides {
                find_enclosing_call(val, file, line, column, best);
            }
        }
        TypedExprKind::ArrayLiteral { elements } => {
            for e in elements {
                find_enclosing_call(e, file, line, column, best);
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for arg in args {
                find_enclosing_call(arg, file, line, column, best);
            }
        }
        TypedExprKind::BoxToAny { inner } => {
            find_enclosing_call(inner, file, line, column, best);
        }
        TypedExprKind::TypeTest { value, .. } | TypedExprKind::TypeCast { value, .. } => {
            find_enclosing_call(value, file, line, column, best);
        }
        TypedExprKind::LetDestructure { value, .. } => {
            find_enclosing_call(value, file, line, column, best);
        }
        TypedExprKind::ClassNew { args, .. } => {
            for arg in args {
                find_enclosing_call(arg, file, line, column, best);
            }
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for f in fields {
                find_enclosing_call(f, file, line, column, best);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            find_enclosing_call(object, file, line, column, best);
            for arg in args {
                find_enclosing_call(arg, file, line, column, best);
            }
        }
        TypedExprKind::ClassSuperCall { args, .. } => {
            for arg in args {
                find_enclosing_call(arg, file, line, column, best);
            }
        }
        TypedExprKind::NewtypeCreate { value } | TypedExprKind::NewtypeValue { value } => {
            find_enclosing_call(value, file, line, column, best);
        }
        TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner } => {
            find_enclosing_call(inner, file, line, column, best);
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            find_enclosing_call(receiver, file, line, column, best);
            for arg in args {
                find_enclosing_call(arg, file, line, column, best);
            }
        }
        TypedExprKind::Await { operand, .. } | TypedExprKind::Use { operand, .. } => {
            find_enclosing_call(operand, file, line, column, best);
        }
        TypedExprKind::ForLoop { iterable, body, .. } => {
            find_enclosing_call(iterable, file, line, column, best);
            find_enclosing_call(body, file, line, column, best);
        }
        TypedExprKind::AsyncBlock { body, .. } => {
            find_enclosing_call(body, file, line, column, best);
        }
        TypedExprKind::Try { operand, .. } => {
            find_enclosing_call(operand, file, line, column, best);
        }
        TypedExprKind::Return { value, .. } => {
            find_enclosing_call(value, file, line, column, best);
        }
        TypedExprKind::Closure { body, .. } => {
            find_enclosing_call(body, file, line, column, best);
        }
        TypedExprKind::ClosureCall { callee, args } => {
            find_enclosing_call(callee, file, line, column, best);
            for arg in args {
                find_enclosing_call(arg, file, line, column, best);
            }
        }
        TypedExprKind::MethodRef { object, .. } => {
            find_enclosing_call(object, file, line, column, best);
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                find_enclosing_call(arg, file, line, column, best);
            }
        }
        TypedExprKind::ImplFunctionRef { .. } | TypedExprKind::ExtFunctionRef { .. } => {}
        TypedExprKind::FunctionRef { .. }
        | TypedExprKind::VarRef { .. }
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::UnitLiteral
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
        | TypedExprKind::Break
        | TypedExprKind::Continue => {}
    }
}

fn walk_pattern_for_calls(
    pattern: &TypedPattern,
    file: &FilePath,
    line: u32,
    column: u32,
    best: &mut Option<CallInfo>,
) {
    match pattern {
        TypedPattern::Literal(expr) => find_enclosing_call(expr, file, line, column, best),
        TypedPattern::Record { fields, .. } => {
            for fp in fields {
                walk_pattern_for_calls(&fp.pattern, file, line, column, best);
            }
        }
        TypedPattern::EnumVariant {
            payload_patterns, ..
        } => {
            for p in payload_patterns {
                walk_pattern_for_calls(p, file, line, column, best);
            }
        }
        TypedPattern::EnumVariantRecord { field_patterns, .. } => {
            for fp in field_patterns {
                walk_pattern_for_calls(&fp.pattern, file, line, column, best);
            }
        }
        TypedPattern::Tuple {
            element_patterns, ..
        } => {
            for p in element_patterns {
                walk_pattern_for_calls(p, file, line, column, best);
            }
        }
        TypedPattern::Newtype { inner_pattern, .. } => {
            walk_pattern_for_calls(inner_pattern, file, line, column, best);
        }
        TypedPattern::Wildcard
        | TypedPattern::Variable(_, _)
        | TypedPattern::TypeAnnotated { .. } => {}
    }
}

/// Build a `SignatureHelp` response from a resolved call.
fn build_signature_help(
    call: &CallInfo,
    typed_module: &TypedModule,
    registry: &Registry,
) -> Option<SignatureHelp> {
    let (label, params, doc) = match &call.kind {
        CallKind::FunctionCall { name } => {
            let func = super::source_functions::get(typed_module, name)?;
            let sig_label = format_func_sig(func);
            let params = func
                .params
                .iter()
                .map(|p| ParameterInformation {
                    label: ParameterLabel::Simple(format!("{}: {}", p.name, p.ty)),
                    documentation: None,
                })
                .collect();
            let fqn_str = func.name.0.split('$').next().unwrap_or(&func.name.0);
            let doc = Fqn::from_dotted(fqn_str)
                .and_then(|fqn| registry.lookup_doc_comment(&fqn).map(str::to_string));
            (sig_label, params, doc)
        }
        CallKind::ClassNew { mangled_name } => {
            if let Some(crate::typechecker::types::TypeDef::Class(class_def)) =
                typed_module.types.get(mangled_name)
            {
                let params_str: Vec<String> = class_def
                    .constructor_params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.ty))
                    .collect();
                let label = format!("new {}({})", class_def.fqn.symbol, params_str.join(", "));
                let params = class_def
                    .constructor_params
                    .iter()
                    .map(|p| ParameterInformation {
                        label: ParameterLabel::Simple(format!("{}: {}", p.name, p.ty)),
                        documentation: None,
                    })
                    .collect();
                let doc = registry
                    .lookup_doc_comment(&class_def.fqn)
                    .map(str::to_string);
                (label, params, doc)
            } else {
                return None;
            }
        }
        CallKind::ClassVirtualCall { args, .. } => {
            let params_str: Vec<String> = args.iter().map(|(n, t)| format!("{n}: {t}")).collect();
            let label = format!("({})", params_str.join(", "));
            let params = args
                .iter()
                .map(|(n, t)| ParameterInformation {
                    label: ParameterLabel::Simple(format!("{n}: {t}")),
                    documentation: None,
                })
                .collect();
            (label, params, None)
        }
        CallKind::ClassSuperCall { method_mangled } => {
            let func = super::source_functions::get(typed_module, method_mangled)?;
            // Skip self param (first) for display
            let user_params: Vec<_> = func.params.iter().skip(1).collect();
            let params_str: Vec<String> = user_params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty))
                .collect();
            let display = func.name.0.split('$').next().unwrap_or(&func.name.0);
            let name = display.rsplit('.').next().unwrap_or(display);
            let label = format!("super.{}({})", name, params_str.join(", "));
            let params = user_params
                .iter()
                .map(|p| ParameterInformation {
                    label: ParameterLabel::Simple(format!("{}: {}", p.name, p.ty)),
                    documentation: None,
                })
                .collect();
            (label, params, None)
        }
        CallKind::InterfaceObjectMethodCall {
            trait_fqn,
            method_name,
        } => {
            let trait_sig = registry.get_trait(trait_fqn)?;
            let method = trait_sig.methods.iter().find(|m| m.name == *method_name)?;
            let params_str: Vec<String> = method
                .params
                .iter()
                .map(|(n, t)| format!("{n}: {t}"))
                .collect();
            let label = format!(
                "{}({}): {}",
                method_name,
                params_str.join(", "),
                method.return_type
            );
            let params = method
                .params
                .iter()
                .map(|(n, t)| ParameterInformation {
                    label: ParameterLabel::Simple(format!("{n}: {t}")),
                    documentation: None,
                })
                .collect();
            (label, params, None)
        }
        CallKind::ClosureCall { callee_type } => {
            if let Type::Function(param_types, return_type) = callee_type {
                let params_str: Vec<String> = param_types
                    .iter()
                    .enumerate()
                    .map(|(i, t)| format!("arg{i}: {t}"))
                    .collect();
                let label = format!("({}): {}", params_str.join(", "), return_type);
                let params = param_types
                    .iter()
                    .enumerate()
                    .map(|(i, t)| ParameterInformation {
                        label: ParameterLabel::Simple(format!("arg{i}: {t}")),
                        documentation: None,
                    })
                    .collect();
                (label, params, None)
            } else {
                return None;
            }
        }
    };

    let documentation = doc.map(|d| {
        Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: d,
        })
    });

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label,
            documentation,
            parameters: Some(params),
            active_parameter: Some(call.active_param),
        }],
        active_signature: Some(0),
        active_parameter: Some(call.active_param),
    })
}

/// Format a function signature label for signature help.
fn format_func_sig(func: &crate::typechecker::types::TypedFunction) -> String {
    let params: Vec<String> = func
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty))
        .collect();
    let async_prefix = if func.is_async { "async " } else { "" };
    let display = if func.display_name.is_empty() {
        func.name.0.split('$').next().unwrap_or(&func.name.0)
    } else {
        &func.display_name
    };
    let name = display.rsplit('.').next().unwrap_or(display);
    let type_params_str = if func.type_params.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = func.type_params.iter().map(|tp| tp.0.as_str()).collect();
        format!("<{}>", names.join(", "))
    };
    format!(
        "{}{}{}({}): {}",
        async_prefix,
        name,
        type_params_str,
        params.join(", "),
        func.return_type,
    )
}
