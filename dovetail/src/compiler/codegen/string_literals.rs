use wasm_encoder::{DataCountSection, DataSection};

use super::Codegen;
use crate::typechecker::types::{TypeDef, TypedExpr, TypedExprKind, TypedPattern};

impl Codegen<'_> {
    /// Walk all function bodies, global initializers, and class initializers,
    /// collecting string literals. Deduplicates by content and assigns sequential
    /// data segment indices.
    pub(super) fn collect_string_literals(&mut self) {
        let mut strings = Vec::new();

        // Collect from global initializers
        for global in self.typed_module.globals.values() {
            collect_strings_from_expr(&global.initializer, &mut strings);
        }

        // Collect from function bodies
        for func in self.typed_module.functions.values() {
            collect_strings_from_expr(&func.body, &mut strings);
        }

        // Collect from class initializers and extends_args
        for type_def in self.typed_module.types.values() {
            if let TypeDef::Class(cls) = type_def {
                for stmt in &cls.initializer {
                    collect_strings_from_expr(stmt, &mut strings);
                }
                if let Some(extends_args) = &cls.extends_args {
                    for arg in extends_args {
                        collect_strings_from_expr(arg, &mut strings);
                    }
                }
            }
        }

        // Deduplicate and assign indices
        for s in strings {
            if !self.string_data_indices.contains_key(&s) {
                let index = self.string_data_payloads.len() as u32;
                self.string_data_payloads.push(s.as_bytes().to_vec());
                self.string_data_indices.insert(s, index);
            }
        }

        // Embedded resources: each `Dovetail.toml`-declared blob is appended
        // after the string segments. Indices are tracked per
        // `(declaring_root, resource_name)` so `Resource.bytes(...)` calls
        // can emit `array.new_data` against the right segment.
        let resources: Vec<((crate::common::types::PackagePath, String), Vec<u8>)> = self
            .typed_module
            .resources
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (key, bytes) in resources {
            let index = self.string_data_payloads.len() as u32;
            let byte_len = bytes.len();
            self.string_data_payloads.push(bytes);
            self.resource_data_segment_indices
                .insert(key.clone(), index);
            self.resource_byte_lengths.insert(key, byte_len);
        }
    }

    /// Emit the DataCountSection (required before code section when using array.new_data).
    pub(super) fn emit_data_count_section(&mut self) {
        if self.string_data_payloads.is_empty() {
            return;
        }
        let section = DataCountSection {
            count: self.string_data_payloads.len() as u32,
        };
        self.module.section(&section);
    }

    /// Emit passive data segments for string literals.
    pub(super) fn emit_data_section(&mut self) {
        if self.string_data_payloads.is_empty() {
            return;
        }
        let mut data = DataSection::new();
        for payload in &self.string_data_payloads {
            data.passive(payload.clone());
        }
        self.module.section(&data);
    }
}

/// Recursively collect string literals from a typed expression tree.
fn collect_strings_from_expr(expr: &TypedExpr, strings: &mut Vec<String>) {
    match &expr.kind {
        TypedExprKind::StringLiteral(s) => strings.push(s.clone()),
        TypedExprKind::BinaryOp { left, right, .. } => {
            collect_strings_from_expr(left, strings);
            collect_strings_from_expr(right, strings);
        }
        TypedExprKind::UnaryOp { operand, .. } => {
            collect_strings_from_expr(operand, strings);
        }
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                collect_strings_from_expr(e, strings);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_strings_from_expr(condition, strings);
            collect_strings_from_expr(then_branch, strings);
            if let Some(else_br) = else_branch {
                collect_strings_from_expr(else_br, strings);
            }
        }
        TypedExprKind::Let { value, .. } => collect_strings_from_expr(value, strings),
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            collect_strings_from_expr(value, strings);
        }
        TypedExprKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_strings_from_expr(arg, strings);
            }
        }
        TypedExprKind::Panic { message } => collect_strings_from_expr(message, strings),
        TypedExprKind::Assert {
            condition, message, ..
        } => {
            collect_strings_from_expr(condition, strings);
            match message {
                Some(msg) => collect_strings_from_expr(msg, strings),
                None => {
                    let auto_msg = format!(
                        "Assertion failed at {}:{}:{}",
                        expr.span.file, expr.span.line, expr.span.column
                    );
                    strings.push(auto_msg);
                }
            }
        }
        TypedExprKind::While { condition, body } => {
            collect_strings_from_expr(condition, strings);
            collect_strings_from_expr(body, strings);
        }
        TypedExprKind::Match { subject, arms } => {
            collect_strings_from_expr(subject, strings);
            for arm in arms {
                collect_strings_from_pattern(&arm.pattern, strings);
                if let Some(guard) = &arm.guard {
                    collect_strings_from_expr(guard, strings);
                }
                collect_strings_from_expr(&arm.body, strings);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, value) in fields {
                collect_strings_from_expr(value, strings);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for element in elements {
                collect_strings_from_expr(element, strings);
            }
        }
        TypedExprKind::EnumCreate { args, .. } => {
            for arg in args {
                collect_strings_from_expr(arg, strings);
            }
        }
        TypedExprKind::FieldAccess { object, .. } => {
            collect_strings_from_expr(object, strings);
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            collect_strings_from_expr(object, strings);
            collect_strings_from_expr(value, strings);
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            collect_strings_from_expr(object, strings);
            for (_, _, value) in overrides {
                collect_strings_from_expr(value, strings);
            }
        }
        TypedExprKind::ArrayLiteral { elements, .. } => {
            for e in elements {
                collect_strings_from_expr(e, strings);
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for arg in args {
                collect_strings_from_expr(arg, strings);
            }
        }
        TypedExprKind::LetDestructure { value, .. } => {
            collect_strings_from_expr(value, strings);
        }
        TypedExprKind::NewtypeCreate { value, .. } => {
            collect_strings_from_expr(value, strings);
        }
        TypedExprKind::NewtypeValue { value, .. } => {
            collect_strings_from_expr(value, strings);
        }
        TypedExprKind::Return { value, .. } => {
            collect_strings_from_expr(value, strings);
        }
        TypedExprKind::ClassNew { args, .. } => {
            for arg in args {
                collect_strings_from_expr(arg, strings);
            }
        }
        TypedExprKind::Closure { body, .. } => {
            collect_strings_from_expr(body, strings);
        }
        TypedExprKind::ClosureCall { callee, args } => {
            collect_strings_from_expr(callee, strings);
            for arg in args {
                collect_strings_from_expr(arg, strings);
            }
        }
        TypedExprKind::EnumVariantRecordCreate { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. } => {
            for arg in args {
                collect_strings_from_expr(arg, strings);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            collect_strings_from_expr(object, strings);
            for arg in args {
                collect_strings_from_expr(arg, strings);
            }
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            collect_strings_from_expr(receiver, strings);
            for arg in args {
                collect_strings_from_expr(arg, strings);
            }
        }
        TypedExprKind::BoxToAny { inner, .. } => {
            collect_strings_from_expr(inner, strings);
        }
        TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner } => {
            collect_strings_from_expr(inner, strings);
        }
        TypedExprKind::TypeTest { value, .. } | TypedExprKind::TypeCast { value, .. } => {
            collect_strings_from_expr(value, strings);
        }
        TypedExprKind::MethodRef { object, .. } => {
            collect_strings_from_expr(object, strings);
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for field in fields {
                collect_strings_from_expr(field, strings);
            }
        }
        _ => {} // Leaf nodes: literals, VarRef, GlobalRef, FunctionRef, Break, Continue, UnitLiteral, etc.
    }
}

/// Recursively collect string literals from a typed pattern.
fn collect_strings_from_pattern(pattern: &TypedPattern, strings: &mut Vec<String>) {
    match pattern {
        TypedPattern::Literal(expr) => collect_strings_from_expr(expr, strings),
        TypedPattern::Record { fields, .. } => {
            for field in fields {
                collect_strings_from_pattern(&field.pattern, strings);
            }
        }
        TypedPattern::EnumVariant {
            payload_patterns, ..
        } => {
            for sub_pat in payload_patterns {
                collect_strings_from_pattern(sub_pat, strings);
            }
        }
        TypedPattern::EnumVariantRecord {
            field_patterns, ..
        } => {
            for field in field_patterns {
                collect_strings_from_pattern(&field.pattern, strings);
            }
        }
        TypedPattern::Tuple {
            element_patterns, ..
        } => {
            for sub_pat in element_patterns {
                collect_strings_from_pattern(sub_pat, strings);
            }
        }
        TypedPattern::Newtype { inner_pattern, .. } => {
            collect_strings_from_pattern(inner_pattern, strings);
        }
        TypedPattern::TypeAnnotated { .. }
        | TypedPattern::Wildcard
        | TypedPattern::Variable(_, _) => {}
    }
}
