use std::collections::{BTreeMap, BTreeSet};

use wasm_encoder::GlobalType;

use super::Codegen;
use super::function_emitter::{self, ExprContext};
use crate::common::types::MangledName;
use crate::typechecker::types::{TypeDef, TypedExpr, TypedExprKind, TypedFunction, TypedPattern};

impl Codegen<'_> {
    /// Emit user globals into the global section (after the bump allocator at index 0).
    /// All are declared mutable so `initialize()` can set them.
    pub(super) fn emit_user_globals(&self, globals: &mut wasm_encoder::GlobalSection) {
        for typed_global in self.typed_module.globals.values() {
            let val_type = self.single_val_type(&typed_global.ty);
            let (val_type, init_expr) = match val_type {
                wasm_encoder::ValType::I32 => (val_type, wasm_encoder::ConstExpr::i32_const(0)),
                wasm_encoder::ValType::I64 => (val_type, wasm_encoder::ConstExpr::i64_const(0)),
                wasm_encoder::ValType::F32 => {
                    (val_type, wasm_encoder::ConstExpr::f32_const(0.0f32.into()))
                }
                wasm_encoder::ValType::F64 => {
                    (val_type, wasm_encoder::ConstExpr::f64_const(0.0f64.into()))
                }
                wasm_encoder::ValType::Ref(ref_type) => {
                    // Globals for ref types must be nullable (initialized with ref.null).
                    // When reading, we emit ref.as_non_null to convert back.
                    let nullable_type = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                        nullable: true,
                        heap_type: ref_type.heap_type,
                    });
                    let init = wasm_encoder::ConstExpr::ref_null(ref_type.heap_type);
                    (nullable_type, init)
                }
                _ => unreachable!("unsupported global val_type"),
            };
            globals.global(
                GlobalType {
                    val_type,
                    mutable: true,
                    shared: false,
                },
                &init_expr,
            );
        }
    }

    /// Emit global initializers in dependency order into the given FunctionEmitter.
    pub(super) fn emit_global_initializers(
        &self,
        emitter: &mut function_emitter::FunctionEmitter<'_>,
    ) {
        for name in self.global_initializer_order() {
            let global = &self.typed_module.globals[name];
            let global_wasm_index = self.global_indices[name];
            emitter.emit_expr(&global.initializer, ExprContext::Value);
            if self.is_tuple(&global.ty) {
                emitter.emit_rebox_tuple(&global.ty);
            } else if self.is_uint128(&global.ty) {
                emitter.emit_rebox_uint128();
            }
            emitter.instruction(wasm_encoder::Instruction::GlobalSet(global_wasm_index));
        }
    }

    /// The order global initializers run in: a global must be initialized before
    /// any global whose initializer CAN READ it — including reads reached
    /// through function calls, not only `GlobalRef`s syntactically present in
    /// the initializer expression. `Console`'s stream globals reach the
    /// `mutexIds` counter only through `Mutex.makeProcessWide()` →
    /// `freshState()` → `freshId()`; with call-blind edges their correct order
    /// was an accident of key order, and ref-typed globals default to
    /// `ref.null`, so the first mis-sort traps every program at component
    /// start.
    ///
    /// The order is fully deterministic: among globals whose dependencies are
    /// satisfied, the one earliest in key order goes first. If the dependency
    /// graph has a cycle — which the conservative call-following analysis can
    /// also introduce for programs that are actually fine, e.g. two globals
    /// whose stored closures mention each other — only the cycle is broken, at
    /// its earliest member, and every constraint outside the cycle still
    /// holds. A genuine initialization cycle is a program bug this layer
    /// cannot prove (the analysis over-approximates), so it is not rejected
    /// here; breaking it deterministically at least keeps the failure stable.
    ///
    /// Shared with `prescan_closures`, which has to walk initializers in exactly
    /// this order — closures are numbered by a running emit counter, so scanning
    /// them in declaration order while emitting them topologically hands every
    /// closure the wrong env type.
    pub(super) fn global_initializer_order(&self) -> Vec<&MangledName> {
        if self.typed_module.globals.is_empty() {
            return Vec::new();
        }

        // Build dependency graph: for each global, every global it can read,
        // through calls included.
        let global_names: Vec<&MangledName> = self.typed_module.globals.keys().collect();
        let name_to_idx: BTreeMap<&MangledName, usize> = global_names
            .iter()
            .enumerate()
            .map(|(i, n)| (*n, i))
            .collect();

        let mut in_degree = vec![0usize; global_names.len()];
        let mut dependents: Vec<Vec<usize>> = vec![Vec::new(); global_names.len()];

        for (i, name) in global_names.iter().enumerate() {
            let global = &self.typed_module.globals[*name];
            let deps = collect_global_refs(
                &global.initializer,
                &self.typed_module.functions,
                &self.typed_module.types,
            );
            for dep in &deps {
                if let Some(&dep_idx) = name_to_idx.get(dep)
                    && dep_idx != i
                {
                    dependents[dep_idx].push(i);
                    in_degree[i] += 1;
                }
            }
        }

        // Kahn's algorithm, smallest ready index first so independent globals
        // keep their key order rather than whatever a work-stack happens to
        // reverse.
        let mut ready: BTreeSet<usize> = (0..global_names.len())
            .filter(|&i| in_degree[i] == 0)
            .collect();
        let mut emitted = vec![false; global_names.len()];
        let mut order = Vec::with_capacity(global_names.len());

        while order.len() < global_names.len() {
            let idx = match ready.iter().next().copied() {
                Some(i) => {
                    ready.remove(&i);
                    i
                }
                // No ready node and globals remain: a cycle. Break it at its
                // earliest not-yet-emitted member and carry on — everything
                // outside the cycle keeps its constraints.
                None => (0..global_names.len())
                    .find(|&i| !emitted[i])
                    .expect("unemitted global must exist while order is short"),
            };
            if emitted[idx] {
                continue;
            }
            emitted[idx] = true;
            order.push(idx);
            for &dep_idx in &dependents[idx] {
                if !emitted[dep_idx] {
                    in_degree[dep_idx] -= 1;
                    if in_degree[dep_idx] == 0 {
                        ready.insert(dep_idx);
                    }
                }
            }
        }

        order.into_iter().map(|idx| global_names[idx]).collect()
    }
}

/// Collect every global an expression can read when it is evaluated —
/// `GlobalRef`s in the expression itself, plus everything reachable through
/// the statically-resolvable calls it makes (`FunctionCall`, `ClassSuperCall`),
/// the class bodies a `ClassNew` inlines (extends-args, parent chain, and
/// class-body field initializers — everything `emit_class_hierarchy` emits at
/// the construction site), and the function values it constructs
/// (`FunctionRef`, `MethodRef`, `Closure` bodies). Following constructed-but-maybe-never-called code
/// over-approximates on purpose: a spurious edge only constrains the order,
/// and a spurious cycle is broken deterministically by the caller — while a
/// MISSING edge initializes a global from another's `ref.null`.
///
/// Dynamic dispatch (`ClassVirtualCall`, `InterfaceObjectMethodCall`,
/// `ClosureCall`) cannot be resolved to a body here; their receivers and
/// arguments are still walked, and the closures/refs those were built from
/// were followed where they were constructed.
fn collect_global_refs(
    expr: &TypedExpr,
    functions: &BTreeMap<MangledName, TypedFunction>,
    types: &BTreeMap<MangledName, TypeDef>,
) -> Vec<MangledName> {
    let mut refs = Vec::new();
    let mut visited_functions = BTreeSet::new();
    let mut visited_classes = BTreeSet::new();
    walk_for_global_refs(
        expr,
        &mut refs,
        functions,
        types,
        &mut visited_functions,
        &mut visited_classes,
    );
    refs
}

/// Walk into a called (or referenced) function's body, once per function per
/// top-level collection — the visited set both memoizes and terminates
/// recursion through call cycles.
fn walk_called_function(
    name: &MangledName,
    refs: &mut Vec<MangledName>,
    functions: &BTreeMap<MangledName, TypedFunction>,
    types: &BTreeMap<MangledName, TypeDef>,
    visited_functions: &mut BTreeSet<MangledName>,
    visited_classes: &mut BTreeSet<MangledName>,
) {
    if !visited_functions.insert(name.clone()) {
        return;
    }
    if let Some(function) = functions.get(name) {
        walk_for_global_refs(
            &function.body,
            refs,
            functions,
            types,
            visited_functions,
            visited_classes,
        );
    }
}

/// Walk everything a `ClassNew` of this class inlines at the construction
/// site: the extends-args, then the parent chain, then the class's own
/// initializer statements — the exact traversal `emit_class_hierarchy`
/// performs (and that `scan_class_hierarchy_closures` mirrors for closures).
/// Step 3 of emission (pushing `initializer_fields`) only reloads locals the
/// initializer statements already bound, so it can read no globals and needs
/// no walking. Unlike the closure scan, which must re-run per construction
/// site, a class visited once has already contributed its full read set, so
/// `visited_classes` both memoizes and terminates any class-reference cycles.
fn walk_class_hierarchy(
    mangled_name: &MangledName,
    refs: &mut Vec<MangledName>,
    functions: &BTreeMap<MangledName, TypedFunction>,
    types: &BTreeMap<MangledName, TypeDef>,
    visited_functions: &mut BTreeSet<MangledName>,
    visited_classes: &mut BTreeSet<MangledName>,
) {
    if !visited_classes.insert(mangled_name.clone()) {
        return;
    }
    let Some(TypeDef::Class(cls)) = types.get(mangled_name) else {
        return;
    };
    if let (Some(extends_args), Some(parent)) = (&cls.extends_args, &cls.parent_mangled_name) {
        for arg in extends_args {
            walk_for_global_refs(
                arg,
                refs,
                functions,
                types,
                visited_functions,
                visited_classes,
            );
        }
        walk_class_hierarchy(
            parent,
            refs,
            functions,
            types,
            visited_functions,
            visited_classes,
        );
    }
    for stmt in &cls.initializer {
        walk_for_global_refs(
            stmt,
            refs,
            functions,
            types,
            visited_functions,
            visited_classes,
        );
    }
}

fn walk_for_global_refs(
    expr: &TypedExpr,
    refs: &mut Vec<MangledName>,
    functions: &BTreeMap<MangledName, TypedFunction>,
    types: &BTreeMap<MangledName, TypeDef>,
    visited_functions: &mut BTreeSet<MangledName>,
    visited_classes: &mut BTreeSet<MangledName>,
) {
    let walk = |e: &TypedExpr,
                refs: &mut Vec<MangledName>,
                visited_functions: &mut BTreeSet<MangledName>,
                visited_classes: &mut BTreeSet<MangledName>| {
        walk_for_global_refs(
            e,
            refs,
            functions,
            types,
            visited_functions,
            visited_classes,
        );
    };
    match &expr.kind {
        TypedExprKind::GlobalRef { name, .. } => refs.push(name.clone()),
        TypedExprKind::BinaryOp { left, right, .. } => {
            walk(left, refs, visited_functions, visited_classes);
            walk(right, refs, visited_functions, visited_classes);
        }
        TypedExprKind::UnaryOp { operand, .. } => {
            walk(operand, refs, visited_functions, visited_classes);
        }
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                walk(e, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            walk(condition, refs, visited_functions, visited_classes);
            walk(then_branch, refs, visited_functions, visited_classes);
            if let Some(else_br) = else_branch {
                walk(else_br, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::FunctionCall { name, args, .. } => {
            for arg in args {
                walk(arg, refs, visited_functions, visited_classes);
            }
            walk_called_function(
                name,
                refs,
                functions,
                types,
                visited_functions,
                visited_classes,
            );
        }
        TypedExprKind::ClassSuperCall {
            method_mangled,
            args,
        } => {
            for arg in args {
                walk(arg, refs, visited_functions, visited_classes);
            }
            walk_called_function(
                method_mangled,
                refs,
                functions,
                types,
                visited_functions,
                visited_classes,
            );
        }
        TypedExprKind::FunctionRef { name, .. } => {
            walk_called_function(
                name,
                refs,
                functions,
                types,
                visited_functions,
                visited_classes,
            );
        }
        TypedExprKind::MethodRef {
            object,
            method_name,
            ..
        } => {
            walk(object, refs, visited_functions, visited_classes);
            walk_called_function(
                method_name,
                refs,
                functions,
                types,
                visited_functions,
                visited_classes,
            );
        }
        TypedExprKind::Closure { body, .. } => {
            walk(body, refs, visited_functions, visited_classes);
        }
        TypedExprKind::ClosureCall { callee, args } => {
            walk(callee, refs, visited_functions, visited_classes);
            for arg in args {
                walk(arg, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            walk(object, refs, visited_functions, visited_classes);
            for arg in args {
                walk(arg, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            walk(receiver, refs, visited_functions, visited_classes);
            for arg in args {
                walk(arg, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner } => {
            walk(inner, refs, visited_functions, visited_classes);
        }
        TypedExprKind::BoxToAny { inner } => {
            walk(inner, refs, visited_functions, visited_classes);
        }
        TypedExprKind::TypeTest { value, .. } | TypedExprKind::TypeCast { value, .. } => {
            walk(value, refs, visited_functions, visited_classes);
        }
        TypedExprKind::Let { value, .. } => walk(value, refs, visited_functions, visited_classes),
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            walk(value, refs, visited_functions, visited_classes)
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            walk(object, refs, visited_functions, visited_classes);
            walk(value, refs, visited_functions, visited_classes);
        }
        TypedExprKind::NamedCall { call: message, .. } | TypedExprKind::Panic { message } => {
            walk(message, refs, visited_functions, visited_classes)
        }
        TypedExprKind::Assert {
            condition, message, ..
        } => {
            walk(condition, refs, visited_functions, visited_classes);
            if let Some(msg) = message {
                walk(msg, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::While { condition, body } => {
            walk(condition, refs, visited_functions, visited_classes);
            walk(body, refs, visited_functions, visited_classes);
        }
        TypedExprKind::Match { subject, arms } => {
            walk(subject, refs, visited_functions, visited_classes);
            for arm in arms {
                walk_pattern_for_global_refs(
                    &arm.pattern,
                    refs,
                    functions,
                    types,
                    visited_functions,
                    visited_classes,
                );
                if let Some(guard) = &arm.guard {
                    walk(guard, refs, visited_functions, visited_classes);
                }
                walk(&arm.body, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, value) in fields {
                walk(value, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for element in elements {
                walk(element, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                walk(arg, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::FieldAccess { object, .. } => {
            walk(object, refs, visited_functions, visited_classes);
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            walk(object, refs, visited_functions, visited_classes);
            for (_, _, value) in overrides {
                walk(value, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::ArrayLiteral { elements, .. } => {
            for e in elements {
                walk(e, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for arg in args {
                walk(arg, refs, visited_functions, visited_classes);
            }
        }
        TypedExprKind::LetDestructure { value, .. } => {
            walk(value, refs, visited_functions, visited_classes);
        }
        TypedExprKind::NewtypeCreate { value, .. } => {
            walk(value, refs, visited_functions, visited_classes);
        }
        TypedExprKind::NewtypeValue { value, .. } => {
            walk(value, refs, visited_functions, visited_classes);
        }
        TypedExprKind::ClassNew {
            mangled_name, args, ..
        } => {
            for arg in args {
                walk(arg, refs, visited_functions, visited_classes);
            }
            // ClassNew EMISSION inlines the whole class hierarchy at the
            // construction site (`emit_class_hierarchy`): the extends-args,
            // the parent chain, and every class-body `let` field initializer
            // all execute right here — so their global reads are this
            // expression's reads too.
            walk_class_hierarchy(
                mangled_name,
                refs,
                functions,
                types,
                visited_functions,
                visited_classes,
            );
        }
        // ClassStructCreate emission is field-values-only: it pushes the
        // vtable and the given field expressions, then `struct.new` — it does
        // NOT run extends-args or class-body initializers (see the
        // `ClassStructCreate` arm in function_emitter/expressions.rs). Walking
        // the field expressions is therefore the complete set of reads.
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for field in fields {
                walk(field, refs, visited_functions, visited_classes);
            }
        }
        // Sugar forms are desugared before codegen, but a missed edge costs a
        // trap at component start while walking them costs nothing — so their
        // subexpressions are covered rather than assumed away.
        TypedExprKind::Return { value, .. } => {
            walk(value, refs, visited_functions, visited_classes);
        }
        TypedExprKind::Await { operand, .. } | TypedExprKind::Try { operand, .. } => {
            walk(operand, refs, visited_functions, visited_classes);
        }
        TypedExprKind::Use { operand, .. } => {
            walk(operand, refs, visited_functions, visited_classes);
        }
        TypedExprKind::AsyncBlock { body, .. } => {
            walk(body, refs, visited_functions, visited_classes);
        }
        TypedExprKind::ForLoop { iterable, body, .. } => {
            walk(iterable, refs, visited_functions, visited_classes);
            walk(body, refs, visited_functions, visited_classes);
        }
        // Leaf nodes (literals, VarRef, Break, Continue) and the impl/ext
        // calls monomorphize resolves before codegen. Nothing to follow.
        _ => {}
    }
}

fn walk_pattern_for_global_refs(
    pattern: &TypedPattern,
    refs: &mut Vec<MangledName>,
    functions: &BTreeMap<MangledName, TypedFunction>,
    types: &BTreeMap<MangledName, TypeDef>,
    visited_functions: &mut BTreeSet<MangledName>,
    visited_classes: &mut BTreeSet<MangledName>,
) {
    match pattern {
        TypedPattern::Literal(expr) => walk_for_global_refs(
            expr,
            refs,
            functions,
            types,
            visited_functions,
            visited_classes,
        ),
        TypedPattern::Record { fields, .. } => {
            for field in fields {
                walk_pattern_for_global_refs(
                    &field.pattern,
                    refs,
                    functions,
                    types,
                    visited_functions,
                    visited_classes,
                );
            }
        }
        TypedPattern::EnumVariant {
            payload_patterns, ..
        } => {
            for sub_pat in payload_patterns {
                walk_pattern_for_global_refs(
                    sub_pat,
                    refs,
                    functions,
                    types,
                    visited_functions,
                    visited_classes,
                );
            }
        }
        TypedPattern::EnumVariantRecord { field_patterns, .. } => {
            for field in field_patterns {
                walk_pattern_for_global_refs(
                    &field.pattern,
                    refs,
                    functions,
                    types,
                    visited_functions,
                    visited_classes,
                );
            }
        }
        TypedPattern::Tuple {
            element_patterns, ..
        } => {
            for sub_pat in element_patterns {
                walk_pattern_for_global_refs(
                    sub_pat,
                    refs,
                    functions,
                    types,
                    visited_functions,
                    visited_classes,
                );
            }
        }
        TypedPattern::Newtype { inner_pattern, .. } => {
            walk_pattern_for_global_refs(
                inner_pattern,
                refs,
                functions,
                types,
                visited_functions,
                visited_classes,
            );
        }
        TypedPattern::TypeAnnotated { .. }
        | TypedPattern::Wildcard
        | TypedPattern::Variable(_, _) => {}
    }
}
