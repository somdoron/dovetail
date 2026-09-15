use wasm_encoder::{BlockType, Instruction};

use crate::codegen::Codegen;
use crate::common::types::VarName;
use crate::parser::ast::{BinOp, UnaryOp};
use crate::typechecker::types::{CapturedVar, Type, TypedExpr, TypedExprKind, TypedPattern};

use super::{ExprContext, FunctionEmitter};

/// The identity of a closure's environment: one entry per capture, in env-field
/// order. Used to assert that the closure the emitter is at is the one the
/// prescan assigned this id to — see the `Closure` emit arm.
fn capture_signature(captures: &[CapturedVar]) -> Vec<(String, String, bool)> {
    captures
        .iter()
        .map(|c| (c.name.0.clone(), format!("{:?}", c.ty), c.mutable))
        .collect()
}

impl FunctionEmitter<'_> {
    /// Emit instructions for a typed expression.
    ///
    /// When `ctx` is `Statement`, the expression should not leave a value on the
    /// WASM stack. Expressions that are naturally side-effect-only (let, assign,
    /// assert, while) skip pushing their Unit result. Expressions that always
    /// produce a value (literals, operators, var refs, calls) emit a `Drop`.
    pub fn emit_expr(&mut self, expr: &TypedExpr, ctx: ExprContext) {
        // Record source mapping at the instruction index where this expression starts
        let inst_idx = self.instructions.len();
        self.source_mappings.push((inst_idx, expr.span.clone()));

        match &expr.kind {
            // Literals are effectless — skip entirely in statement context
            TypedExprKind::UnitLiteral => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0));
                }
            }
            TypedExprKind::BoolLiteral(val) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(if *val { 1 } else { 0 }));
                }
            }
            TypedExprKind::CharLiteral(c) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(*c as i32));
                }
            }
            TypedExprKind::StringLiteral(s) => {
                if ctx == ExprContext::Value {
                    let data_index = self.codegen.string_data_indices[s.as_str()];
                    let byte_len = s.len() as i32;
                    // 1. Create backing array from data segment
                    self.instruction(Instruction::I32Const(0));
                    self.instruction(Instruction::I32Const(byte_len));
                    self.instruction(Instruction::ArrayNewData {
                        array_type_index: super::super::U8_BACKING_TYPE_INDEX,
                        array_data_index: data_index,
                    });
                    // 2. Push precomputed UTF-8 metadata field
                    let utf8_char_count = s.chars().count() as i32;
                    let is_ascii = s.is_ascii();
                    let utf8_field = utf8_char_count | if is_ascii { i32::MIN } else { 0 };
                    self.instruction(Instruction::I32Const(utf8_field));
                    // 3. Wrap in string struct
                    self.instruction(Instruction::StructNew(
                        super::super::STRING_STRUCT_TYPE_INDEX,
                    ));
                }
            }
            TypedExprKind::Int8Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(*v as i32));
                }
            }
            TypedExprKind::Int16Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(*v as i32));
                }
            }
            TypedExprKind::Int32Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(*v));
                }
            }
            TypedExprKind::Int64Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I64Const(*v));
                }
            }
            TypedExprKind::Uint8Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(*v as i32));
                }
            }
            TypedExprKind::Uint16Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(*v as i32));
                }
            }
            TypedExprKind::Uint32Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(*v as i32));
                }
            }
            TypedExprKind::Uint64Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I64Const(*v as i64));
                }
            }
            TypedExprKind::Uint128Literal(v) => {
                if ctx == ExprContext::Value {
                    // Flattened `[i64, i64]` (lo, hi): the low 64 bits then the high 64 bits.
                    self.instruction(Instruction::I64Const(*v as u64 as i64));
                    self.instruction(Instruction::I64Const((*v >> 64) as u64 as i64));
                }
            }
            TypedExprKind::Float32Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::F32Const((*v).into()));
                }
            }
            TypedExprKind::Float64Literal(v) => {
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::F64Const((*v).into()));
                }
            }

            // Binary operator
            TypedExprKind::BinaryOp { op, left, right } => match op {
                BinOp::LogicalAnd => {
                    // Short-circuit: if left is false, result is false; otherwise result is right
                    self.emit_expr(left, ExprContext::Value);
                    self.emit_if_block(BlockType::Result(wasm_encoder::ValType::I32));
                    self.emit_expr(right, ExprContext::Value);
                    self.instruction(Instruction::Else);
                    self.instruction(Instruction::I32Const(0));
                    self.emit_end_block();
                    self.drop_if_statement(ctx, &expr.ty);
                }
                BinOp::LogicalOr => {
                    // Short-circuit: if left is true, result is true; otherwise result is right
                    self.emit_expr(left, ExprContext::Value);
                    self.emit_if_block(BlockType::Result(wasm_encoder::ValType::I32));
                    self.instruction(Instruction::I32Const(1));
                    self.instruction(Instruction::Else);
                    self.emit_expr(right, ExprContext::Value);
                    self.emit_end_block();
                    self.drop_if_statement(ctx, &expr.ty);
                }
                // Widening-multiply peephole: `x.toUint128() * y.toUint128()` with both `x`, `y`
                // provably ≤64-bit unsigned widenings collapses to a single `i64.mul_wide_u`,
                // skipping the composed 128×128 multiply.
                BinOp::Mul
                    if matches!(expr.ty, Type::Uint128)
                        && Self::uint128_widen_source(left).is_some()
                        && Self::uint128_widen_source(right).is_some() =>
                {
                    let lx = Self::uint128_widen_source(left).unwrap();
                    let rx = Self::uint128_widen_source(right).unwrap();
                    self.emit_widened_to_i64(lx);
                    self.emit_widened_to_i64(rx);
                    self.instruction(Instruction::I64MulWideU);
                    self.drop_if_statement(ctx, &expr.ty);
                }
                _ => {
                    self.emit_expr(left, ExprContext::Value);
                    self.emit_expr(right, ExprContext::Value);
                    let operand_ty = &left.ty;
                    self.emit_binary_instruction(*op, operand_ty);
                    self.drop_if_statement(ctx, &expr.ty);
                }
            },

            // Unary operator
            TypedExprKind::UnaryOp { op, operand } => {
                self.emit_unary_op(*op, operand);
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::Block(exprs) => {
                self.push_scope();
                for (i, e) in exprs.iter().enumerate() {
                    if i < exprs.len() - 1 {
                        self.emit_expr(e, ExprContext::Statement);
                    } else {
                        self.emit_expr(e, ctx);
                    }
                }
                self.pop_scope();
            }
            TypedExprKind::Panic { message } => {
                self.emit_expr(message, ExprContext::Value);
                self.instruction(Instruction::Call(self.codegen.func_panic_with_message()));
                // panic_with_message never returns (traps internally), but the WASM
                // validator sees its return type as void. Emit Unreachable to enter
                // the polymorphic stack state so the validator accepts any follow-up.
                self.instruction(Instruction::Unreachable);
            }
            TypedExprKind::Assert { condition, message } => {
                self.emit_expr(condition, ExprContext::Value);
                self.instruction(Instruction::I32Eqz);
                self.emit_if_block(BlockType::Empty);
                match message {
                    Some(msg) => self.emit_expr(msg, ExprContext::Value),
                    None => {
                        let auto_msg = format!(
                            "Assertion failed at {}:{}:{}",
                            expr.span.file, expr.span.line, expr.span.column
                        );
                        self.emit_string_from_data(&auto_msg);
                    }
                }
                self.instruction(Instruction::Call(self.codegen.func_panic_with_message()));
                self.emit_end_block();
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0));
                }
            }

            TypedExprKind::Let {
                name,
                var_ty,
                value,
                boxed,
                ..
            } => {
                if *boxed {
                    // Mutable capture: store the value in a heap mut-box shared with the closure.
                    self.emit_expr(value, ExprContext::Value);
                    self.emit_mut_box_new(var_ty);
                    let index = self.define_local(name.clone(), self.codegen.mut_box_valtype(var_ty));
                    self.instruction(Instruction::LocalSet(index));
                } else {
                    let valtypes = self.codegen.type_to_valtypes(var_ty);
                    // Emit value BEFORE defining the local so that VarRefs in the
                    // value expression resolve to the previous binding of the same
                    // name (standard non-recursive let semantics). A tuple occupies a flattened value.
                    self.emit_expr(value, ExprContext::Value);
                    let base = self.define_value_locals(name.clone(), &valtypes);
                    self.store_value(base, &valtypes);
                }
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0));
                }
            }
            TypedExprKind::VarRef { name, boxed } => {
                if *boxed {
                    self.emit_mut_box_load(self.lookup_local(name), &expr.ty);
                } else {
                    let base = self.lookup_local(name);
                    self.load_value(base, &self.codegen.type_to_valtypes(&expr.ty));
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            TypedExprKind::Assign { name, value, boxed, target_ty, .. } => {
                if *boxed {
                    // Reassign a mutably-captured variable: write the value into its shared mut-box.
                    let index = self.lookup_local(name);
                    self.emit_expr(value, ExprContext::Value);
                    self.emit_mut_box_store(index, target_ty);
                } else {
                    let base = self.lookup_local(name);
                    self.emit_expr(value, ExprContext::Value);
                    self.store_value(base, &self.codegen.type_to_valtypes(target_ty));
                }
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0));
                }
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.emit_expr(condition, ExprContext::Value);
                if let Some(else_br) = else_branch {
                    if ctx == ExprContext::Value
                        && (self.codegen.is_tuple(&expr.ty) || self.codegen.is_uint128(&expr.ty))
                    {
                        // Multi-value result (tuple or Uint128): WASM `if` blocks can't carry a
                        // multi-value result inline, so each branch spills its values into temp
                        // locals and we load them after.
                        let valtypes = self.codegen.type_to_valtypes(&expr.ty);
                        let base = self.add_result_temp(&valtypes);
                        self.emit_if_block(BlockType::Empty);
                        self.emit_expr(then_branch, ExprContext::Value);
                        self.store_value(base, &valtypes);
                        self.instruction(Instruction::Else);
                        self.emit_expr(else_br, ExprContext::Value);
                        self.store_value(base, &valtypes);
                        self.emit_end_block();
                        self.load_result_temp(base, &valtypes);
                    } else if ctx == ExprContext::Value {
                        let block_ty = BlockType::Result(self.codegen.single_val_type(&expr.ty));
                        self.emit_if_block(block_ty);
                        self.emit_expr(then_branch, ExprContext::Value);
                        self.instruction(Instruction::Else);
                        self.emit_expr(else_br, ExprContext::Value);
                        self.emit_end_block();
                    } else {
                        self.emit_if_block(BlockType::Empty);
                        self.emit_expr(then_branch, ExprContext::Statement);
                        self.instruction(Instruction::Else);
                        self.emit_expr(else_br, ExprContext::Statement);
                        self.emit_end_block();
                    }
                } else {
                    // If without else: then-branch is always a statement
                    self.emit_if_block(BlockType::Empty);
                    self.emit_expr(then_branch, ExprContext::Statement);
                    self.emit_end_block();
                    if ctx == ExprContext::Value {
                        self.instruction(Instruction::I32Const(0));
                    }
                }
            }

            TypedExprKind::While { condition, body } => {
                self.emit_while_block();

                self.emit_expr(condition, ExprContext::Value);
                self.instruction(Instruction::I32Eqz);
                self.instruction(Instruction::BrIf(self.break_label()));

                self.emit_expr(body, ExprContext::Statement);

                self.instruction(Instruction::Br(self.continue_label()));

                self.emit_end_while_block();
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0));
                }
            }

            TypedExprKind::Break => {
                self.instruction(Instruction::Br(self.break_label()));
            }

            TypedExprKind::Continue => {
                self.instruction(Instruction::Br(self.continue_label()));
            }

            TypedExprKind::Match { subject, arms } => {
                self.emit_match_expr(expr, subject, arms, ctx);
            }

            TypedExprKind::FunctionCall { name, args, type_params: _ } => {
                // If this is a direct call into a monomorphized class method whose WASM signature
                // was erased to match the vtable slot, we must box/cast at type-param positions
                // on the calling side too (mirroring ClassVirtualCall). The slot's signature
                // comes from the canonical class's `VtableSlot` (via `virtual_method_slot_sigs`).
                let template_sig: Option<(Vec<Type>, Type)> =
                    self.codegen.virtual_method_slot_sigs.get(name).cloned();

                for (i, arg) in args.iter().enumerate() {
                    self.emit_expr(arg, ExprContext::Value);
                    // A vtable method uses the erased slot signature: coerce each arg's values to its
                    // slot param. A direct (non-vtable) callee already takes the flattened values.
                    if let Some((tparams, _)) = &template_sig {
                        let slot = tparams.get(i).cloned().unwrap_or_else(|| arg.ty.clone());
                        self.coerce_value(&arg.ty, &slot);
                    }
                }
                let func_index = *self.codegen.function_indices.get(name).unwrap_or_else(|| {
                    if name.0 == "<error>" {
                        // Codegen runs only when the typechecker reported zero errors, so
                        // an error placeholder here means some inference path built one
                        // without reporting the failure it stands in for. Say that, rather
                        // than blaming a lookup that was never going to succeed.
                        panic!(
                            "internal compiler error: an unreported type error at {:?} reached codegen",
                            expr.span
                        );
                    }
                    panic!("missing function index for: {} at {:?}", name, expr.span)
                });
                self.instruction(Instruction::Call(func_index));

                // Coerce the callee's (possibly erased) return values back to the concrete static type
                // — a no-op for a concrete return, a cast-back for an erased one.
                if let Some((_, tret)) = template_sig {
                    self.coerce_value(&tret, &expr.ty);
                }
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::GlobalRef { name, .. } => {
                if ctx == ExprContext::Value {
                    let index = *self.codegen.global_indices.get(name).unwrap_or_else(|| panic!("no global_index for GlobalRef name: {}", name.0));
                    self.instruction(Instruction::GlobalGet(index));
                    // Globals for ref types are declared nullable (initialized with ref.null),
                    // so convert to non-null when reading.
                    if matches!(
                        self.codegen.single_val_type(&expr.ty),
                        wasm_encoder::ValType::Ref(_)
                    ) {
                        self.instruction(Instruction::RefAsNonNull);
                    }
                    // A tuple global holds a single boxed `(ref $Tuple)`; explode it into its flattened values.
                    if self.codegen.is_tuple(&expr.ty) {
                        self.emit_unbox_tuple(&expr.ty);
                    } else if self.codegen.is_uint128(&expr.ty) {
                        // A Uint128 global holds a single boxed `(ref $Uint128)`; explode to `[lo, hi]`.
                        self.emit_unbox_uint128();
                    }
                }
                // In statement context, GlobalGet is effectless — emit nothing.
            }

            TypedExprKind::FunctionRef { name, type_params: _ } => {
                if ctx == ExprContext::Value {
                    let arity = match &expr.ty {
                        Type::Function(pts, _) => pts.len() as u32,
                        _ => unreachable!("FunctionRef must have Function type"),
                    };
                    let (_, closure_struct_idx) = self.codegen.closure_arity_indices[&arity];
                    let key = format!("$ref_func${}", name.0);
                    let tramp_func_idx = self.codegen.ref_trampoline_indices[&key];
                    // null env (FunctionRef has no captured self)
                    self.instruction(Instruction::RefNull(wasm_encoder::HeapType::Abstract {
                        shared: false,
                        ty: wasm_encoder::AbstractHeapType::None,
                    }));
                    self.instruction(Instruction::RefFunc(tramp_func_idx));
                    self.instruction(Instruction::StructNew(closure_struct_idx));
                }
                // In statement context: FunctionRef is pure, emit nothing
            }

            TypedExprKind::MethodRef { object, method_name, type_params: _ } => {
                if ctx == ExprContext::Value {
                    let arity = match &expr.ty {
                        Type::Function(pts, _) => pts.len() as u32,
                        _ => unreachable!("MethodRef must have Function type"),
                    };
                    let (_, closure_struct_idx) = self.codegen.closure_arity_indices[&arity];
                    let key = format!("$ref_method${}", method_name.0);
                    let tramp_func_idx = self.codegen.ref_trampoline_indices[&key];
                    // Emit object as env — box primitives so the env slot (anyref) accepts it.
                    self.emit_expr(object, ExprContext::Value);
                    if !object.ty.is_reference_type() {
                        let box_idx = self.codegen.box_type_index_for(&object.ty);
                        self.instruction(Instruction::StructNew(box_idx));
                    }
                    self.instruction(Instruction::RefFunc(tramp_func_idx));
                    self.instruction(Instruction::StructNew(closure_struct_idx));
                } else {
                    // In statement context: emit object for side effects only
                    self.emit_expr(object, ExprContext::Statement);
                }
            }

            TypedExprKind::GlobalAssign { name, value, .. } => {
                let index = *self.codegen.global_indices.get(name).unwrap_or_else(|| panic!("no global_index for GlobalAssign name: {}", name.0));
                self.emit_expr(value, ExprContext::Value);
                // A tuple global holds a single boxed `(ref $Tuple)`; rebox the values. A Uint128
                // global holds a single boxed `(ref $Uint128)`.
                if self.codegen.is_tuple(&value.ty) {
                    self.emit_rebox_tuple(&value.ty);
                } else if self.codegen.is_uint128(&value.ty) {
                    self.emit_rebox_uint128();
                }
                self.instruction(Instruction::GlobalSet(index));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            TypedExprKind::TupleLiteral { elements } => {
                // A tuple literal leaves the concatenated element values on the stack — the
                // flattened tuple *is* those values, no heap struct. An element whose slot is an
                // *erased* (`Any`/type-param) position is boxed to a single anyref; a concrete
                // (possibly nested-tuple) element stays flattened in place.
                let elems: Vec<Type> = match &expr.ty {
                    Type::Tuple(elems, _) => elems.clone(),
                    _ => unreachable!("TupleLiteral must have a tuple type"),
                };
                for (i, element) in elements.iter().enumerate() {
                    self.emit_expr(element, ExprContext::Value);
                    if let Some(slot) = elems.get(i) {
                        if matches!(slot, Type::Any) {
                            self.emit_box_to_any(&element.ty);
                        } else if Codegen::is_erased_slot(slot) {
                            self.box_if_erased_slot(&element.ty, slot);
                        }
                    }
                }
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::RecordCreate { fields, .. } => {
                // Record: a heap struct. Box/rebox each field value into its (single) slot.
                let type_idx = match &expr.ty {
                    Type::Record(_, mn) | Type::GenericRecord { mangled_name: mn, .. } => self.codegen.type_indices[mn],
                    _ => unreachable!("RecordCreate must have Record type"),
                };
                let slot_types: Vec<Type> = match &expr.ty {
                    Type::Record(_, mn) | Type::GenericRecord { mangled_name: mn, .. } => {
                        match &self.codegen.typed_module.types[mn] {
                            crate::typechecker::types::TypeDef::Record(r) => {
                                r.fields.iter().map(|(_, t)| t.clone()).collect()
                            }
                            _ => unreachable!("RecordCreate target is not a RecordTypeDef"),
                        }
                    }
                    _ => unreachable!(),
                };
                for (i, (_, field_expr)) in fields.iter().enumerate() {
                    self.emit_expr(field_expr, ExprContext::Value);
                    self.box_into_field(&field_expr.ty, &slot_types[i]);
                }
                self.emit_nominal_struct_new(&self.codegen.construction_type(expr), type_idx);
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::EnumCreate {
                variant_name, args, ..
            } => {
                let enum_mn = match &expr.ty {
                    Type::Enum(_, mn)
                    | Type::GenericEnum {
                        mangled_name: mn, ..
                    } => mn.clone(),
                    _ => unreachable!("EnumCreate must have Enum type"),
                };
                let variant_idx =
                    self.codegen.variant_type_indices[&(enum_mn.clone(), variant_name.clone())];
                let payload_types: Vec<Type> = match &self.codegen.typed_module.types[&enum_mn] {
                    crate::typechecker::types::TypeDef::Enum(e) => e
                        .variants
                        .iter()
                        .find(|v| &v.name == variant_name)
                        .map(|v| v.payload_types.clone())
                        .unwrap_or_default(),
                    _ => unreachable!("EnumCreate target is not an EnumTypeDef"),
                };
                for (i, arg) in args.iter().enumerate() {
                    self.emit_expr(arg, ExprContext::Value);
                    if let Some(slot_ty) = payload_types.get(i) {
                        // Payloads splice: a tuple's values flows into its N fields; an erased/`Any`
                        // payload boxes into one anyref slot.
                        self.box_into_field(&arg.ty, slot_ty);
                    }
                }
                self.emit_nominal_struct_new(&self.codegen.construction_type(expr), variant_idx);
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::RecordWith {
                object, overrides, ..
            } => {
                self.emit_record_with(expr, object, overrides, ctx);
            }

            TypedExprKind::ArrayLiteral { elements } => {
                let elem = match &expr.ty {
                    Type::Array(elem) => elem.as_ref(),
                    _ => unreachable!("ArrayLiteral must have Array type"),
                };
                let array_type_index = self.codegen.array_type_index(elem);
                let elem = elem.clone();
                for element in elements {
                    self.emit_expr(element, ExprContext::Value);
                    // Array element slots are boxed; a tuple element reboxes its values, a Uint128 its `[lo, hi]`.
                    if self.codegen.is_tuple(&elem) {
                        self.emit_rebox_tuple(&elem);
                    } else if self.codegen.is_uint128(&elem) {
                        self.emit_rebox_uint128();
                    }
                }
                self.instruction(Instruction::ArrayNewFixed {
                    array_type_index,
                    array_size: elements.len() as u32,
                });
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::IntrinsicCall { intrinsic, args } => {
                self.emit_intrinsic_call(intrinsic, args, expr, ctx);
            }

            TypedExprKind::FieldAccess {
                object,
                field_index,
                boxed,
                ..
            } => {
                // Look up the slot's declared type (for erased-slot cast-back).
                let slot_ty: Option<Type> = match &object.ty {
                    Type::Record(_, mn) | Type::GenericRecord { mangled_name: mn, .. } => {
                        match &self.codegen.typed_module.types[mn] {
                            crate::typechecker::types::TypeDef::Record(r) => r
                                .fields
                                .get(*field_index as usize)
                                .map(|(_, t)| t.clone()),
                            _ => None,
                        }
                    }
                    Type::Class(_, mn) | Type::GenericClass { mangled_name: mn, .. } => {
                        match &self.codegen.typed_module.types[mn] {
                            crate::typechecker::types::TypeDef::Class(c) => c
                                .fields
                                .get(*field_index as usize)
                                .map(|f| f.ty.clone()),
                            _ => None,
                        }
                    }
                    _ => None,
                };

                // A tuple object is itself a flattened values; pick the element's sub-range without
                // touching the heap. (Handled before emitting the object as a single ref.)
                if let Type::Tuple(elems, _) = &object.ty {
                    let elems = elems.clone();
                    self.emit_expr(object, ExprContext::Value);
                    let obj_valtypes = self.codegen.type_to_valtypes(&object.ty);
                    let temp_base = self.add_value_locals(&obj_valtypes);
                    self.store_value(temp_base, &obj_valtypes);
                    let (start, width) = self.codegen.tuple_elem_offset(&elems, *field_index as usize);
                    self.load_value(temp_base + start, &obj_valtypes[start as usize..(start + width) as usize]);
                    self.drop_if_statement(ctx, &expr.ty);
                    return;
                }

                self.emit_expr(object, ExprContext::Value);
                match &object.ty {
                    Type::Record(_, mn)
                    | Type::GenericRecord { mangled_name: mn, .. } => {
                        // Records are spliced: a concrete tuple field occupies a range of WASM
                        // fields. Read the whole range as the field's values.
                        let struct_idx = self.codegen.type_indices[mn];
                        let (start, width) = self.codegen.struct_field_range(mn, *field_index as usize);
                        if width == 1 {
                            self.instruction(Instruction::StructGet {
                                struct_type_index: struct_idx,
                                field_index: start,
                            });
                            if let Some(slot_ty) = slot_ty.as_ref() {
                                // Width-1 field can only be erased (anyref) → cast back.
                                self.unbox_from_slot(slot_ty, &expr.ty);
                            }
                        } else {
                            // Tuple field: spill the object ref, then one struct.get per WASM field.
                            let obj_vt = self.codegen.single_val_type(&object.ty);
                            let obj_local = self.add_local(obj_vt);
                            self.instruction(Instruction::LocalSet(obj_local));
                            for k in 0..width {
                                self.instruction(Instruction::LocalGet(obj_local));
                                self.instruction(Instruction::StructGet {
                                    struct_type_index: struct_idx,
                                    field_index: start + k,
                                });
                            }
                            if let Some(slot_ty) = slot_ty.as_ref() {
                                self.coerce_value(slot_ty, &expr.ty);
                            }
                        }
                    }
                    Type::Class(_, mn) | Type::GenericClass { mangled_name: mn, .. } => {
                        // Field 0 is the vtable ref; data fields follow. An immutable tuple field is
                        // spliced into a WASM-field range (read as a flattened value); other fields are width-1
                        // at their mapped index.
                        let struct_idx = self.codegen.type_indices[mn];
                        let (start, width) = self.codegen.struct_field_range(mn, *field_index as usize);
                        if width > 1 {
                            let obj_vt = self.codegen.single_val_type(&object.ty);
                            let obj_local = self.add_local(obj_vt);
                            self.instruction(Instruction::LocalSet(obj_local));
                            for k in 0..width {
                                self.instruction(Instruction::LocalGet(obj_local));
                                self.instruction(Instruction::StructGet {
                                    struct_type_index: struct_idx,
                                    field_index: start + k,
                                });
                            }
                            if let Some(slot_ty) = slot_ty.as_ref() {
                                self.coerce_value(slot_ty, &expr.ty);
                            }
                        } else {
                            self.instruction(Instruction::StructGet {
                                struct_type_index: struct_idx,
                                field_index: start,
                            });
                            if *boxed {
                                // Boxed field: the struct field holds a ref to a mutable box.
                                // Read through the box to get the actual value.
                                let box_idx = self.codegen.mut_box_type_index_for(&expr.ty);
                                self.instruction(Instruction::StructGet {
                                    struct_type_index: box_idx,
                                    field_index: 0,
                                });
                                // MUT_BOX_REF stores anyref — need ref.cast to concrete type
                                if box_idx == super::super::MUT_BOX_REF_TYPE_INDEX {
                                    let concrete_valtype = self.codegen.single_val_type(&expr.ty);
                                    if let wasm_encoder::ValType::Ref(rt) = concrete_valtype {
                                        self.instruction(Instruction::RefCastNonNull(rt.heap_type));
                                    }
                                }
                            } else if let Some(slot_ty) = slot_ty.as_ref() {
                                self.unbox_from_slot(slot_ty, &expr.ty);
                            }
                        }
                    }
                    _ => unreachable!("FieldAccess object must have Record type"),
                }
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::FieldAssign {
                object,
                field_index,
                value,
                boxed,
                ..
            } => {
                let mn = match &object.ty {
                    Type::Class(_, mn) | Type::GenericClass { mangled_name: mn, .. } => mn.clone(),
                    _ => unreachable!("FieldAssign must be on a Class type"),
                };
                let struct_idx = self.codegen.type_indices[&mn];
                let (start, width) = self.codegen.struct_field_range(&mn, *field_index as usize);
                if width > 1 {
                    // Spliced (mutable) tuple field: store the value's flattened values leaf-by-leaf
                    // into the field's WASM range. Spill the object ref and the values, then set each.
                    self.emit_expr(object, ExprContext::Value);
                    let obj_vt = self.codegen.single_val_type(&object.ty);
                    let obj_local = self.add_local(obj_vt);
                    self.instruction(Instruction::LocalSet(obj_local));
                    self.emit_expr(value, ExprContext::Value);
                    let run_vts = self.codegen.type_to_valtypes(&value.ty);
                    let base = self.add_value_locals(&run_vts);
                    self.store_value(base, &run_vts);
                    for k in 0..width {
                        self.instruction(Instruction::LocalGet(obj_local));
                        self.instruction(Instruction::LocalGet(base + k));
                        self.instruction(Instruction::StructSet {
                            struct_type_index: struct_idx,
                            field_index: start + k,
                        });
                    }
                } else if *boxed {
                    // Boxed (variance mutable) field: get box ref, emit value, set through the box.
                    self.emit_expr(object, ExprContext::Value);
                    self.instruction(Instruction::StructGet {
                        struct_type_index: struct_idx,
                        field_index: start,
                    });
                    self.emit_expr(value, ExprContext::Value);
                    let box_idx = self.codegen.mut_box_type_index_for(&value.ty);
                    self.instruction(Instruction::StructSet {
                        struct_type_index: box_idx,
                        field_index: 0,
                    });
                } else {
                    let slot_ty: Option<Type> = match &self.codegen.typed_module.types[&mn] {
                        crate::typechecker::types::TypeDef::Class(c) => {
                            c.fields.get(*field_index as usize).map(|f| f.ty.clone())
                        }
                        _ => None,
                    };
                    self.emit_expr(object, ExprContext::Value);
                    self.emit_expr(value, ExprContext::Value);
                    // Width-1 erased single slot: box the value into the anyref slot if needed.
                    if let Some(slot_ty) = slot_ty.as_ref() {
                        self.box_into_slot(&value.ty, slot_ty);
                    }
                    self.instruction(Instruction::StructSet {
                        struct_type_index: struct_idx,
                        field_index: start,
                    });
                }
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            // Record-style variant construction — args already reordered to declaration order
            TypedExprKind::EnumVariantRecordCreate {
                variant_name, args, ..
            } => {
                let enum_mn = match &expr.ty {
                    Type::Enum(_, mn)
                    | Type::GenericEnum {
                        mangled_name: mn, ..
                    } => mn.clone(),
                    _ => unreachable!("EnumVariantRecordCreate must have Enum type"),
                };
                let variant_idx =
                    self.codegen.variant_type_indices[&(enum_mn.clone(), variant_name.clone())];
                let payload_types: Vec<Type> = match &self.codegen.typed_module.types[&enum_mn] {
                    crate::typechecker::types::TypeDef::Enum(e) => e
                        .variants
                        .iter()
                        .find(|v| &v.name == variant_name)
                        .map(|v| v.payload_types.clone())
                        .unwrap_or_default(),
                    _ => unreachable!("EnumVariantRecordCreate target is not an EnumTypeDef"),
                };
                for (i, arg) in args.iter().enumerate() {
                    self.emit_expr(arg, ExprContext::Value);
                    if let Some(slot_ty) = payload_types.get(i) {
                        // Payloads splice: a tuple's values flows into its N fields; an erased/`Any`
                        // payload boxes into one anyref slot.
                        self.box_into_field(&arg.ty, slot_ty);
                    }
                }
                self.emit_nominal_struct_new(&self.codegen.construction_type(expr), variant_idx);
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::BoxToAny { inner } => {
                self.emit_expr(inner, ExprContext::Value);
                self.emit_box_to_any(&inner.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::TypeTest { value, target_type } => {
                self.emit_expr(value, ExprContext::Value);
                self.emit_runtime_type_test(target_type);
                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::TypeCast { value, target_type } => {
                if matches!(target_type, Type::Never) {
                    // Never is uninhabited — cast to Never is unreachable
                    self.emit_expr(value, ExprContext::Statement);
                    self.instruction(Instruction::Unreachable);
                } else if matches!(target_type, Type::Any) {
                    // Cast to Any widens to anyref. A tuple value is a flattened value; rebox it into its
                    // `(ref $Tuple)` (an anyref subtype) first.
                    self.emit_expr(value, ExprContext::Value);
                    if self.codegen.is_tuple(&value.ty) {
                        self.emit_rebox_tuple(&value.ty);
                    } else if self.codegen.is_uint128(&value.ty) {
                        self.emit_rebox_uint128();
                    }
                    self.drop_if_statement(ctx, &expr.ty);
                    return;
                } else {
                    self.emit_expr(value, ExprContext::Value);
                    self.enforce_reified_cast(target_type);
                    let type_idx = self.codegen.wasm_type_index_for_any_cast(target_type);
                    self.instruction(Instruction::RefCastNonNull(
                        wasm_encoder::HeapType::Concrete(type_idx),
                    ));
                    if self.codegen.is_tuple(target_type) {
                        // Downcast to a concrete tuple: explode the `(ref $Tuple)` into its values.
                        self.emit_unbox_tuple(target_type);
                    } else if self.codegen.is_uint128(target_type) {
                        // Downcast to Uint128: explode the `(ref $Uint128)` into `[lo, hi]`.
                        self.emit_unbox_uint128();
                    } else if !target_type.is_reference_type() {
                        // Unbox: extract field 0 from the boxing struct
                        self.instruction(Instruction::StructGet {
                            struct_type_index: type_idx,
                            field_index: 0,
                        });
                    }
                    self.drop_if_statement(ctx, &expr.ty);
                }
            }

            TypedExprKind::LetDestructure { pattern, value, .. } => {
                self.emit_let_destructure(pattern, value);
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }
            TypedExprKind::NewtypeCreate { value, .. } => {
                // Transparent — just emit the inner value
                self.emit_expr(value, ctx);
            }
            TypedExprKind::NewtypeValue { value, .. } => {
                // Transparent — just emit the inner value
                self.emit_expr(value, ctx);
            }

            TypedExprKind::TemplateInterfaceObjectCoerce { traits, concrete_type, .. } => {
                unreachable!("TemplateInterfaceObjectCoerce not resolved by monomorphize: traits={:?} concrete={}", traits, concrete_type)
            }

            TypedExprKind::InterfaceObjectCoerce {
                inner,
                interface_mangled_name,
                concrete_type,
                vtable_methods,
            } => {
                // 1. Emit inner expression (concrete value on stack)
                self.emit_expr(inner, ExprContext::Value);

                // 2. Box/upcast to anyref. `emit_box_to_any` covers every
                // shape: reference types upcast for free, primitives box, and
                // flattened tuples rebox into their `(ref $Tuple)` (the
                // wrapper's self-cast unpacks them symmetrically).
                self.emit_box_to_any(concrete_type);

                // 3. Load vtable global (discriminated by tagged group keys —
                // sibling instantiations / via-providers get their own).
                let global_key = crate::compiler::codegen::Codegen::coercion_global_key(
                    interface_mangled_name,
                    vtable_methods,
                );
                let vtable_global = self.codegen.vtable_global_indices
                    [&(super::super::instance_key(concrete_type), global_key)];
                self.instruction(Instruction::GlobalGet(vtable_global));

                // 4. Create interface object struct (data, vtable)
                let traitobj_type = self.codegen.interface_object_type_indices[interface_mangled_name];
                self.instruction(Instruction::StructNew(traitobj_type));

                self.drop_if_statement(ctx, &expr.ty);
            }

            TypedExprKind::InterfaceObjectMethodCall {
                interface_mangled_name,
                receiver,
                member_name,
                args,
                ..
            } => {
                // The vtable slot uses the *erased* ABI (the trait's generic params lowered to
                // `anyref`). The dispatch is the caller of that ABI, so — symmetric to the wrapper —
                // it `coerce_value`s each arg into the slot layout and the return back to concrete.
                // Fetch the slot's raw param/return types (cloned to avoid borrowing `self` across).
                let (slot_param_tys, slot_return_ty): (Vec<Type>, Option<Type>) =
                    match self.codegen.typed_module.types.get(interface_mangled_name) {
                        Some(crate::typechecker::types::TypeDef::InterfaceObject(to)) => to
                            .vtable_members
                            .iter()
                            .find(|(mn, _, _)| mn == member_name)
                            .map(|(_, ps, r)| (ps.clone(), Some(r.clone())))
                            .unwrap_or_default(),
                        _ => (Vec::new(), None),
                    };

                // The receiver's SET key ($IfaceObj$A / $IfaceObj$A&B) names the fat
                // pointer struct; the node's `interface_mangled_name` is the declaring
                // COMPONENT's key (equal to the set key for single interfaces). For an
                // intersection receiver, dispatch goes through one extra struct.get:
                // set vtable → component vtable.
                // With `extends`, the declaring component (the node key) may be a
                // SUPER of the receiver's component: dispatch then navigates the
                // nested super-vtable refs (`nav_path`).
                let (set_mn, component_idx, source_component) = match &receiver.ty {
                    Type::InterfaceObject { traits, mangled_name } if traits.len() > 1 => {
                        // Prefer the EXACT declaring component: when both a
                        // super and a sub that extends it are components, the
                        // typechecker's dedup chose one deliberately — a
                        // first-reachable scan would re-route through
                        // whichever sorts first (the sub's inline impl vs the
                        // super's direct one differ observably).
                        let found = traits
                            .iter()
                            .enumerate()
                            .find_map(|(i, c)| {
                                let key = crate::common::types::MangledName::for_interface_object_per_interface(&c.trait_fqn);
                                (key == *interface_mangled_name).then_some((i as u32, key))
                            })
                            .or_else(|| {
                                traits.iter().enumerate().find_map(|(i, c)| {
                                    let key = crate::common::types::MangledName::for_interface_object_per_interface(&c.trait_fqn);
                                    self.codegen
                                        .super_vtable_path(&key, interface_mangled_name)
                                        .map(|_| (i as u32, key))
                                })
                            })
                            .unwrap_or_else(|| panic!(
                                "component {} not in intersection receiver {}", interface_mangled_name, mangled_name
                            ));
                        (mangled_name.clone(), Some(found.0), found.1)
                    }
                    Type::InterfaceObject { traits, .. } if traits.len() == 1 => {
                        let key = crate::common::types::MangledName::for_interface_object_per_interface(&traits[0].trait_fqn);
                        (key.clone(), None, key)
                    }
                    _ => (interface_mangled_name.clone(), None, interface_mangled_name.clone()),
                };
                let nav_path = self
                    .codegen
                    .super_vtable_path(&source_component, interface_mangled_name)
                    .unwrap_or_else(|| panic!(
                        "declaring component {} unreachable from receiver component {}",
                        interface_mangled_name, source_component
                    ));

                // 1. Emit receiver (interface object on stack)
                self.emit_expr(receiver, ExprContext::Value);

                // 2. Save receiver to local (need it twice: for data and vtable)
                let recv_valtype = self.codegen.single_val_type(&receiver.ty);
                let recv_local = self.add_local(recv_valtype);
                self.instruction(Instruction::LocalSet(recv_local));

                // 3. Extract data (field 0) — first arg to wrapper
                let traitobj_type = *self.codegen.interface_object_type_indices.get(&set_mn)
                    .unwrap_or_else(|| panic!("missing interface_object_type_index for: {}", set_mn));
                self.instruction(Instruction::LocalGet(recv_local));
                self.instruction(Instruction::StructGet {
                    struct_type_index: traitobj_type,
                    field_index: 0,
                });

                // 4. Emit remaining args, coercing each from its concrete layout into the slot's
                // (possibly erased) layout — symmetric to the wrapper. `coerce_value` is a no-op for
                // a concrete param, boxes a fully-erased one, and handles a partially-erased tuple
                // param `(Int32, T)` (a width-changing per-element coercion).
                for (i, arg) in args.iter().enumerate() {
                    self.emit_expr(arg, ExprContext::Value);
                    if let Some(slot_ty) = slot_param_tys.get(i) {
                        self.coerce_value(&arg.ty, slot_ty);
                    }
                }

                // 5. Extract vtable (field 1) — for an intersection receiver, then
                // extract the component's vtable from the set vtable — then get the
                // method funcref.
                self.instruction(Instruction::LocalGet(recv_local));
                self.instruction(Instruction::StructGet {
                    struct_type_index: traitobj_type,
                    field_index: 1,
                });
                if let Some(idx) = component_idx {
                    let set_vtable_type = self.codegen.vtable_type_indices[&set_mn];
                    self.instruction(Instruction::StructGet {
                        struct_type_index: set_vtable_type,
                        field_index: idx,
                    });
                }
                for (owner_mn, field_idx) in &nav_path {
                    let owner_vtable_type = self.codegen.vtable_type_indices[owner_mn];
                    self.instruction(Instruction::StructGet {
                        struct_type_index: owner_vtable_type,
                        field_index: *field_idx,
                    });
                }
                let vtable_type = self.codegen.vtable_type_indices[interface_mangled_name];
                let method_index = self.codegen.trait_method_vtable_indices
                    [&(interface_mangled_name.clone(), member_name.clone())];
                self.instruction(Instruction::StructGet {
                    struct_type_index: vtable_type,
                    field_index: method_index,
                });

                // 6. call_ref through the funcref
                let func_type_idx =
                    self.codegen.wrapper_func_type_indices[&(interface_mangled_name.clone(), method_index)];
                self.instruction(Instruction::CallRef(func_type_idx));

                // Coerce the wrapper's slot-layout return back into the call's concrete static type
                // (a no-op for a concrete return; casts/unboxes an erased one).
                if let Some(slot_ret) = &slot_return_ty {
                    self.coerce_value(slot_ret, &expr.ty);
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            TypedExprKind::InterfaceObjectUpcast { inner } => {
                // Static upcast (A and B) → subset: extract the data field plus the
                // needed component-vtable refs and rebuild the target fat pointer.
                let (source_traits, source_mn) = match &inner.ty {
                    Type::InterfaceObject { traits, mangled_name } => (traits.clone(), mangled_name.clone()),
                    other => panic!("InterfaceObjectUpcast source is not an interface object: {other}"),
                };
                let (target_traits, target_mn) = match &expr.ty {
                    Type::InterfaceObject { traits, mangled_name } => (traits.clone(), mangled_name.clone()),
                    other => panic!("InterfaceObjectUpcast target is not an interface object: {other}"),
                };

                let source_obj_type = self.codegen.interface_object_type_indices[&source_mn];
                let source_vtable_type = self.codegen.vtable_type_indices[&source_mn];

                // Spill the source object to a local.
                self.emit_expr(inner, ExprContext::Value);
                let src_valtype = self.codegen.single_val_type(&inner.ty);
                let src_local = self.add_local(src_valtype);
                self.instruction(Instruction::LocalSet(src_local));

                // Data field.
                self.instruction(Instruction::LocalGet(src_local));
                self.instruction(Instruction::StructGet {
                    struct_type_index: source_obj_type,
                    field_index: 0,
                });

                // Per target component (already sorted): source vtable → component
                // ref — either the component itself, or (extends) a nested
                // super-vtable ref reached through `super_vtable_path`.
                for component in &target_traits {
                    let target_key = crate::common::types::MangledName::for_interface_object_per_interface(&component.trait_fqn);
                    // Exact source component first (see the dispatch scan
                    // above) — only fall back to a nested super-vtable ref
                    // when the target is not itself a source component.
                    let (src_idx, nav_path) = source_traits
                        .iter()
                        .enumerate()
                        .find_map(|(i, c)| {
                            let src_key = crate::common::types::MangledName::for_interface_object_per_interface(&c.trait_fqn);
                            (src_key == target_key).then(|| (i as u32, Vec::new()))
                        })
                        .or_else(|| {
                            source_traits.iter().enumerate().find_map(|(i, c)| {
                                let src_key = crate::common::types::MangledName::for_interface_object_per_interface(&c.trait_fqn);
                                self.codegen
                                    .super_vtable_path(&src_key, &target_key)
                                    .map(|p| (i as u32, p))
                            })
                        })
                        .unwrap_or_else(|| panic!(
                            "upcast target component '{}' not in source set {}",
                            component.trait_fqn, source_mn
                        ));
                    self.instruction(Instruction::LocalGet(src_local));
                    self.instruction(Instruction::StructGet {
                        struct_type_index: source_obj_type,
                        field_index: 1,
                    });
                    if source_traits.len() > 1 {
                        self.instruction(Instruction::StructGet {
                            struct_type_index: source_vtable_type,
                            field_index: src_idx,
                        });
                    }
                    for (owner_mn, field_idx) in &nav_path {
                        let owner_vtable_type = self.codegen.vtable_type_indices[owner_mn];
                        self.instruction(Instruction::StructGet {
                            struct_type_index: owner_vtable_type,
                            field_index: *field_idx,
                        });
                    }
                }

                // Pack a multi-component target's vtable, then the target object.
                if target_traits.len() > 1 {
                    let target_vtable_type = self.codegen.vtable_type_indices[&target_mn];
                    self.instruction(Instruction::StructNew(target_vtable_type));
                }
                let target_obj_type = self.codegen.interface_object_type_indices[&target_mn];
                self.instruction(Instruction::StructNew(target_obj_type));

                self.drop_if_statement(ctx, &expr.ty);
            }
            TypedExprKind::ClassNew { mangled_name, args, type_params } => {
                let type_idx = self.codegen.type_indices[mangled_name];

                // The vtable follows the optional type ID. For generic classes, each (class,
                // type_args) instantiation has its own vtable global instance.
                let type_args = type_params.clone();
                self.emit_type_id(&self.codegen.construction_type(expr));
                let vtable_global_idx = self.codegen.class_vtable_global_indices
                    [&(mangled_name.clone(), type_args)];
                self.instruction(Instruction::GlobalGet(vtable_global_idx));
                self.instruction(Instruction::I32Const(0));

                // Look up the ClassTypeDef
                let cls = match &self.codegen.typed_module.types[mangled_name] {
                    crate::typechecker::types::TypeDef::Class(cls) => cls.clone(),
                    _ => panic!("ClassNew target is not a class type"),
                };

                // Push scope, bind constructor params from call-site args.
                // For canonical generic classes, `param.ty` may be a `TypeVariable` (erased).
                // In that case the local is `anyref`-typed and we must box primitive args
                // before storing — otherwise WASM validation fails (i32 into anyref slot).
                self.push_scope();
                for (param, arg) in cls.constructor_params.iter().zip(args.iter()) {
                    self.emit_expr(arg, ExprContext::Value);
                    self.bind_param_local(VarName(param.name.clone()), &arg.ty, &param.ty);
                }

                // Chain initializers from root to leaf
                self.emit_class_hierarchy(&cls);

                self.pop_scope();
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }
            TypedExprKind::ClassVirtualCall {
                object,
                vtable_slot,
                args,
            } => {
                // Virtual dispatch through class vtable. The slot's func type uses the ERASED
                // signature (anyref at type-param positions). Args at those positions must be
                // boxed before push; the return must be cast back if the call's static type
                // expects a concrete value.
                let (class_mn, class_type_idx) = match &object.ty {
                    Type::Class(_, mn) | Type::GenericClass { mangled_name: mn, .. } => (mn.clone(), self.codegen.type_indices[mn]),
                    _ => unreachable!("ClassVirtualCall object must have Class type"),
                };

                // Read the slot's signature directly — `param_types` (including `self`) and
                // `return_type` carry the canonical/erased shape used by the vtable slot.
                let template_sig: Option<(Vec<Type>, Type)> = {
                    if let crate::typechecker::types::TypeDef::Class(cls) =
                        &self.codegen.typed_module.types[&class_mn]
                    {
                        cls.vtable_methods.get(*vtable_slot as usize)
                            .map(|slot| (slot.param_types.clone(), slot.return_type.clone()))
                    } else {
                        None
                    }
                };

                // Emit object, tee to temp local (leaves value on stack as self arg)
                self.emit_expr(object, ExprContext::Value);
                let obj_valtype = self.codegen.single_val_type(&object.ty);
                let obj_local = self.add_local(obj_valtype);
                self.instruction(Instruction::LocalTee(obj_local));

                // Push remaining args (skip args[0] which is the object/self) in the slot's
                // flattened layout: coerce each arg's values to the (possibly erased) slot param.
                for (i, arg) in args.iter().enumerate().skip(1) {
                    self.emit_expr(arg, ExprContext::Value);
                    let slot = template_sig
                        .as_ref()
                        .and_then(|(tp, _)| tp.get(i))
                        .cloned()
                        .unwrap_or_else(|| arg.ty.clone());
                    self.coerce_value(&arg.ty, &slot);
                }

                // Get funcref from vtable
                // struct_get 0 on object → (ref $vtable_type) [no cast needed]
                let vtable_type_idx = self.codegen.class_vtable_type_indices[&class_mn];
                self.instruction(Instruction::LocalGet(obj_local));
                self.instruction(Instruction::StructGet {
                    struct_type_index: class_type_idx,
                    field_index: self.codegen.class_vtable_field(&class_mn),
                });

                // struct_get the method slot from the vtable
                self.instruction(Instruction::StructGet {
                    struct_type_index: vtable_type_idx,
                    field_index: *vtable_slot,
                });

                // call_ref with the func type for this slot
                let func_type_idx = self.codegen.class_vtable_slot_func_types
                    [&(class_mn, *vtable_slot)];
                self.instruction(Instruction::CallRef(func_type_idx));

                // Coerce the slot's (possibly erased) return values back to the call's concrete static
                // type — a no-op for a concrete return, a cast-back for an erased one.
                if let Some((_, tret)) = template_sig {
                    self.coerce_value(&tret, &expr.ty);
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            TypedExprKind::ClassSuperCall {
                method_mangled,
                args,
            } => {
                // Direct call to a parent class method. A vtable method uses the erased slot
                // signature, so coerce each arg's values to its slot param; otherwise pass directly.
                let slot_params = self
                    .codegen
                    .virtual_method_slot_sigs
                    .get(method_mangled)
                    .map(|(params, _)| params.clone());
                for (i, arg) in args.iter().enumerate() {
                    self.emit_expr(arg, ExprContext::Value);
                    // Skip `self` (arg 0): a subclass ref flows into the base-typed self param by
                    // implicit WASM-GC upcast. Coerce the remaining args to their slot params.
                    if i == 0 {
                        continue;
                    }
                    if let Some(slot) = slot_params.as_ref().and_then(|p| p.get(i)) {
                        let slot = slot.clone();
                        self.coerce_value(&arg.ty, &slot);
                    }
                }
                let func_index = self.codegen.function_indices[method_mangled];
                self.instruction(Instruction::Call(func_index));
                // A concrete-tuple return arrives as its flattened values (multi-value); flows on.
                self.drop_if_statement(ctx, &expr.ty);
            }
            TypedExprKind::Closure { captures, .. } => {
                // Get closure ID from the emit counter (matches wave-order pre-scan)
                let closure_id = self.codegen.closure_emit_counter.get();
                self.codegen.closure_emit_counter.set(closure_id + 1);

                // All closures share Closure_N by arity under always-erased closures.
                let arity = match &expr.ty {
                    Type::Function(pts, _) => pts.len() as u32,
                    _ => unreachable!("Closure expression must have Function type"),
                };
                let (_, closure_struct_idx) = self.codegen.closure_arity_indices[&arity];
                // Closure ids come from a counter shared by `prescan_closures` and
                // this emit walk, so the two must visit closure sites in exactly
                // the same order. Nothing enforces that structurally, and when it
                // slips every closure silently gets a later one's env struct.
                // Compare the full capture signature (name, type, mutability) —
                // two swapped closures very often agree on capture *count*, which
                // is how such a desync used to pass this check and corrupt the
                // WASM instead of failing here.
                assert_eq!(
                    capture_signature(&self.codegen.closure_infos[closure_id].captures),
                    capture_signature(captures),
                    "closure #{closure_id} emitted with captures {:?} but was pre-scanned with {:?} — \
                     the closure prescan is walking bodies in a different order than the code section \
                     emits them",
                    capture_signature(captures),
                    capture_signature(&self.codegen.closure_infos[closure_id].captures),
                );
                let lifted_func_index = self.codegen.closure_func_indices[closure_id];

                // Build env
                if captures.is_empty() {
                    // No captures: push null env
                    self.instruction(Instruction::RefNull(wasm_encoder::HeapType::Abstract {
                        shared: false,
                        ty: wasm_encoder::AbstractHeapType::None,
                    }));
                } else {
                    // Push each capture value into the env struct (single, boxed slots).
                    for cap in captures {
                        let base = self.lookup_local(&cap.name);
                        if cap.mutable {
                            // Mutable: push box ref (already stored as box in local)
                            self.instruction(Instruction::LocalGet(base));
                        } else {
                            // Immutable: push the capture's flattened values. A tuple splices into a
                            // run of env fields (no `(ref $Tuple)` box), so push its values as-is.
                            self.load_value(base, &self.codegen.type_to_valtypes(&cap.ty));
                        }
                    }
                    let env_type_index = self.codegen.closure_env_type_indices[closure_id]
                        .expect("closure with captures must have env type");
                    self.instruction(Instruction::StructNew(env_type_index));
                }

                // Push funcref
                self.instruction(Instruction::RefFunc(lifted_func_index));

                // Create closure struct
                self.instruction(Instruction::StructNew(closure_struct_idx));

                self.drop_if_statement(ctx, &expr.ty);
            }
            TypedExprKind::ClosureCall { callee, args } => {
                // Under always-erased closures: every closure of arity N has WASM type
                // (ref Closure_N) with funcref signature Func_N = (anyref env, anyref ×N) → anyref.
                // We box each arg to anyref before pushing, then cast the anyref result back
                // to the call's static return type.
                let (param_types, return_type) = match &callee.ty {
                    Type::Function(pts, ret) => (pts.clone(), (**ret).clone()),
                    _ => unreachable!("ClosureCall callee must have Function type"),
                };
                let arity = param_types.len() as u32;
                let (call_func_type_index, closure_struct_idx) = self.codegen.closure_arity_indices[&arity];

                // Emit callee → store in temp local
                let closure_valtype = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(closure_struct_idx),
                });
                self.emit_expr(callee, ExprContext::Value);
                let temp = self.add_local(closure_valtype);
                self.instruction(Instruction::LocalSet(temp));

                // Push env (field 0) as first WASM arg.
                self.instruction(Instruction::LocalGet(temp));
                self.instruction(Instruction::StructGet {
                    struct_type_index: closure_struct_idx,
                    field_index: 0,
                });

                // Push each arg, boxing primitives so they fit the anyref param slot.
                for arg in args {
                    self.emit_expr(arg, ExprContext::Value);
                    self.emit_box_to_any(&arg.ty);
                }

                // Get funcref (field 1) and call_ref Func_N → anyref result.
                self.instruction(Instruction::LocalGet(temp));
                self.instruction(Instruction::StructGet {
                    struct_type_index: closure_struct_idx,
                    field_index: 1,
                });
                self.instruction(Instruction::CallRef(call_func_type_index));

                // Cast the anyref result back to the call's static return type.
                self.emit_cast_back_from_any(&return_type);

                // When this call appears in statement context, drop the result. Doing it
                // after the cast keeps the bytecode simpler (drop accepts any valtype, but
                // emitting cast then drop is consistent with other contexts that always cast).
                self.drop_if_statement(ctx, &expr.ty);
            }
            TypedExprKind::ForLoop { .. } => {
                unreachable!("ForLoop nodes should be desugared before codegen");
            }
            TypedExprKind::Try { .. } => {
                unreachable!("Try nodes should be desugared before codegen");
            }
            TypedExprKind::Await { .. } => {
                unreachable!("Await nodes should be desugared before codegen");
            }
            TypedExprKind::Use { .. } => {
                unreachable!("Use nodes should be desugared before codegen");
            }
            TypedExprKind::AsyncBlock { .. } => {
                unreachable!("AsyncBlock nodes should be desugared before codegen");
            }
            TypedExprKind::Return { value, .. } => {
                self.emit_expr(value, ExprContext::Value);
                if self.in_closure {
                    // Closure Func_N returns a single anyref; box the value to match the block.
                    self.emit_box_to_any(&value.ty);
                } else if let Some((base, spill_valtypes)) = self.return_spill.clone() {
                    // For a tuple return, spill the value into the return temp locals; the outer
                    // block is empty and reloads them after the `br`. Non-tuple returns ride the
                    // block result.
                    self.store_value(base, &spill_valtypes);
                }
                let depth = self.wasm_block_depth - self.return_block_depth - 1;
                self.instruction(Instruction::Br(depth));
            }
            TypedExprKind::ClassStructCreate { target_mangled_name, fields, type_params } => {
                // Get vtable global for the target (class, type_args) instantiation.
                let type_args = type_params.clone();
                self.emit_type_id(&self.codegen.construction_type(expr));
                let vtable_global_idx = self.codegen.class_vtable_global_indices
                    [&(target_mangled_name.clone(), type_args)];
                self.instruction(Instruction::GlobalGet(vtable_global_idx));
                self.instruction(Instruction::I32Const(0));
                let class_fields: Vec<crate::typechecker::types::ClassFieldDef> =
                    match &self.codegen.typed_module.types[target_mangled_name] {
                        crate::typechecker::types::TypeDef::Class(c) => c.fields.clone(),
                        _ => unreachable!("ClassStructCreate target is not a ClassTypeDef"),
                    };
                // Emit each field expression (already in declaration order). A spliced immutable
                // tuple field's values flows into its N fields; other fields box into one slot.
                for (i, field) in fields.iter().enumerate() {
                    self.emit_expr(field, ExprContext::Value);
                    if let Some(def) = class_fields.get(i) {
                        if self.codegen.class_field_is_spliced(def) {
                            self.box_into_field(&field.ty, &def.ty);
                        } else {
                            self.box_into_slot(&field.ty, &def.ty);
                        }
                    }
                }
                let type_idx = self.codegen.type_indices[target_mangled_name];
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }
            TypedExprKind::ImplFunctionCall { trait_fqn, for_type, method_name, trait_type_params, method_type_params, .. } => {
                unreachable!("ImplFunctionCall not resolved by monomorphize: trait={} for_type={:?} method={} trait_type_params={:?} method_type_params={:?}", trait_fqn, for_type, method_name, trait_type_params, method_type_params)
            }
            TypedExprKind::ImplFunctionRef { .. }
            | TypedExprKind::ExtFunctionCall { .. }
            | TypedExprKind::ExtFunctionRef { .. } => {
                unreachable!("resolved by monomorphize")
            }
        }
    }

    /// Emit instructions to create a string from a data segment (same as StringLiteral emission).
    fn emit_string_from_data(&mut self, s: &str) {
        let data_index = self.codegen.string_data_indices[s];
        let byte_len = s.len() as i32;
        // 1. Create backing array from data segment
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::I32Const(byte_len));
        self.instruction(Instruction::ArrayNewData {
            array_type_index: super::super::U8_BACKING_TYPE_INDEX,
            array_data_index: data_index,
        });
        // 2. Push precomputed UTF-8 metadata field
        let utf8_char_count = s.chars().count() as i32;
        let is_ascii = s.is_ascii();
        let utf8_field = utf8_char_count | if is_ascii { i32::MIN } else { 0 };
        self.instruction(Instruction::I32Const(utf8_field));
        // 3. Wrap in string struct
        self.instruction(Instruction::StructNew(
            super::super::STRING_STRUCT_TYPE_INDEX,
        ));
    }

    /// Drop a value of type `ty` when in statement context. A value occupies `flat_width(ty)` WASM
    /// stack slots (N for a flattened tuple, 1 otherwise), so the matching number of `Drop`s is
    /// emitted — never under- or over-dropping regardless of the value's width.
    pub(super) fn drop_if_statement(&mut self, ctx: ExprContext, ty: &Type) {
        if ctx == ExprContext::Statement {
            for _ in 0..self.codegen.flat_width(ty) {
                self.instruction(Instruction::Drop);
            }
        }
    }

    /// Recursively emit class initializers from root to leaf.
    /// Each class's scope is pushed/popped so name collisions between parent and child
    /// params are handled naturally.
    fn emit_class_hierarchy(&mut self, cls: &crate::typechecker::types::ClassTypeDef) {
        // 1. If has parent: evaluate extends_args, push parent scope, recurse, pop
        if let (Some(extends_args), Some(parent_mn)) = (&cls.extends_args, &cls.parent_mangled_name) {
            let parent_cls = match &self.codegen.typed_module.types[parent_mn] {
                crate::typechecker::types::TypeDef::Class(cls) => cls.clone(),
                _ => panic!("parent is not a class type"),
            };

            // Evaluate ALL extends args in current (child) scope, save to temp locals/flattens.
            // An erased param boxes into a single anyref temp; a concrete tuple stores into a
            // temp locals; the parent param name is then aliased directly to the temp base.
            let mut temp_bases = Vec::new();
            for (i, arg) in extends_args.iter().enumerate() {
                let param_ty = parent_cls.constructor_params[i].ty.clone();
                self.emit_expr(arg, ExprContext::Value);
                if Codegen::is_erased_slot(&param_ty) {
                    self.box_if_erased_slot(&arg.ty, &param_ty);
                    let val_type = self.codegen.single_val_type(&param_ty);
                    let temp = self.add_local(val_type);
                    self.instruction(Instruction::LocalSet(temp));
                    temp_bases.push(temp);
                } else {
                    let valtypes = self.codegen.type_to_valtypes(&param_ty);
                    let base = self.add_value_locals(&valtypes);
                    self.store_value(base, &valtypes);
                    temp_bases.push(base);
                }
            }

            // Push parent scope, alias parent param names to the temp bases (no copy).
            self.push_scope();
            for (i, param) in parent_cls.constructor_params.iter().enumerate() {
                self.bind_name(VarName(param.name.clone()), temp_bases[i]);
            }

            // Recurse for grandparent
            self.emit_class_hierarchy(&parent_cls);

            self.pop_scope();
        }

        // 2. Emit own initializer statements directly (not as Block,
        //    so let-binding locals stay in scope for field pushes)
        for stmt in &cls.initializer {
            self.emit_expr(stmt, ExprContext::Statement);
        }

        // 3. Push own field values to stack — load the binding's values. A spliced immutable tuple
        // field's values splices into its N fields; other fields box into one slot.
        for (field_name, field_ty) in &cls.initializer_fields {
            let base = self.lookup_local(&VarName(field_name.clone()));
            self.load_value(base, &self.codegen.type_to_valtypes(field_ty));
            if let Some(def) = cls.fields.iter().find(|f| f.name == *field_name) {
                if self.codegen.class_field_is_spliced(def) {
                    self.box_into_field(field_ty, &def.ty);
                } else {
                    self.box_into_slot(field_ty, &def.ty);
                }
            }
        }
    }

    /// Emit a tuple destructuring let binding. The RHS is a flattened value; spill it into temp
    /// locals, then bind each element pattern to its sub-range of those locals (no heap, no copies).
    fn emit_let_destructure(&mut self, pattern: &TypedPattern, value: &TypedExpr) {
        self.emit_expr(value, ExprContext::Value);
        let valtypes = self.codegen.type_to_valtypes(&value.ty);
        let base = self.add_value_locals(&valtypes);
        self.store_value(base, &valtypes);
        self.emit_destructure_pattern(pattern, base);
    }

    /// Recursively bind a tuple pattern to sub-ranges of a flattened values starting at `base`.
    /// Each variable aliases its element's slice of the values; nested tuples recurse into their
    /// sub-range; wildcards are skipped. No `struct.get` — the tuple is already its values.
    fn emit_destructure_pattern(&mut self, pattern: &TypedPattern, base: u32) {
        match pattern {
            TypedPattern::Tuple {
                element_patterns,
                tuple_type,
            } => {
                let elems: Vec<Type> = match tuple_type {
                    Type::Tuple(elems, _) => elems.clone(),
                    _ => unreachable!("Tuple pattern must have Tuple type"),
                };
                for (i, sub_pat) in element_patterns.iter().enumerate() {
                    let (start, _width) = self.codegen.tuple_elem_offset(&elems, i);
                    let sub_base = base + start;
                    match sub_pat {
                        TypedPattern::Wildcard => {}
                        TypedPattern::Variable(name, _ty) => {
                            self.bind_name(name.clone(), sub_base);
                        }
                        TypedPattern::Tuple { .. } => {
                            self.emit_destructure_pattern(sub_pat, sub_base);
                        }
                        _ => unreachable!(
                            "unexpected sub-pattern in tuple destructure: {:?}",
                            sub_pat
                        ),
                    }
                }
            }
            _ => unreachable!("emit_destructure_pattern called with non-tuple pattern"),
        }
    }

    /// Emit a binary operator instruction based on the operator and operand type.
    fn emit_binary_instruction(&mut self, op: BinOp, operand_ty: &Type) {
        // Reuse the shared equality instruction emitter for `==`
        if op == BinOp::Eq {
            self.emit_eq_instruction(operand_ty);
            return;
        }

        match operand_ty {
            // i32 signed types (Int8, Int16, Int32)
            Type::Int8 | Type::Int16 | Type::Int32 => match op {
                BinOp::Add => {
                    self.instruction(Instruction::I32Add);
                    self.emit_narrow(operand_ty);
                }
                BinOp::Sub => {
                    self.instruction(Instruction::I32Sub);
                    self.emit_narrow(operand_ty);
                }
                BinOp::Mul => {
                    self.instruction(Instruction::I32Mul);
                    self.emit_narrow(operand_ty);
                }
                BinOp::Div => self.instruction(Instruction::I32DivS),
                BinOp::Rem => self.instruction(Instruction::I32RemS),
                BinOp::Ne => self.instruction(Instruction::I32Ne),
                BinOp::Lt => self.instruction(Instruction::I32LtS),
                BinOp::Gt => self.instruction(Instruction::I32GtS),
                BinOp::Le => self.instruction(Instruction::I32LeS),
                BinOp::Ge => self.instruction(Instruction::I32GeS),
                BinOp::BitAnd => self.instruction(Instruction::I32And),
                BinOp::BitOr => self.instruction(Instruction::I32Or),
                BinOp::BitXor => self.instruction(Instruction::I32Xor),
                BinOp::Shl => {
                    self.instruction(Instruction::I32Shl);
                    self.emit_narrow(operand_ty);
                }
                BinOp::Shr => self.instruction(Instruction::I32ShrS),
                BinOp::TupleExtend | BinOp::Eq | BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::Concat => {
                    unreachable!("handled above")
                }
            },

            // i32 unsigned types (Uint8, Uint16, Uint32) and Char (Unicode code point, unsigned)
            Type::Uint8 | Type::Uint16 | Type::Uint32 | Type::Char => match op {
                BinOp::Add => {
                    self.instruction(Instruction::I32Add);
                    self.emit_narrow(operand_ty);
                }
                BinOp::Sub => {
                    self.instruction(Instruction::I32Sub);
                    self.emit_narrow(operand_ty);
                }
                BinOp::Mul => {
                    self.instruction(Instruction::I32Mul);
                    self.emit_narrow(operand_ty);
                }
                BinOp::Div => self.instruction(Instruction::I32DivU),
                BinOp::Rem => self.instruction(Instruction::I32RemU),
                BinOp::Ne => self.instruction(Instruction::I32Ne),
                BinOp::Lt => self.instruction(Instruction::I32LtU),
                BinOp::Gt => self.instruction(Instruction::I32GtU),
                BinOp::Le => self.instruction(Instruction::I32LeU),
                BinOp::Ge => self.instruction(Instruction::I32GeU),
                BinOp::BitAnd => self.instruction(Instruction::I32And),
                BinOp::BitOr => self.instruction(Instruction::I32Or),
                BinOp::BitXor => self.instruction(Instruction::I32Xor),
                BinOp::Shl => {
                    self.instruction(Instruction::I32Shl);
                    self.emit_narrow(operand_ty);
                }
                BinOp::Shr => self.instruction(Instruction::I32ShrU),
                BinOp::TupleExtend | BinOp::Eq | BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::Concat => {
                    unreachable!("handled above")
                }
            },

            // i64 signed (Int64)
            Type::Int64 => match op {
                BinOp::Add => self.instruction(Instruction::I64Add),
                BinOp::Sub => self.instruction(Instruction::I64Sub),
                BinOp::Mul => self.instruction(Instruction::I64Mul),
                BinOp::Div => self.instruction(Instruction::I64DivS),
                BinOp::Rem => self.instruction(Instruction::I64RemS),
                BinOp::Ne => self.instruction(Instruction::I64Ne),
                BinOp::Lt => self.instruction(Instruction::I64LtS),
                BinOp::Gt => self.instruction(Instruction::I64GtS),
                BinOp::Le => self.instruction(Instruction::I64LeS),
                BinOp::Ge => self.instruction(Instruction::I64GeS),
                BinOp::BitAnd => self.instruction(Instruction::I64And),
                BinOp::BitOr => self.instruction(Instruction::I64Or),
                BinOp::BitXor => self.instruction(Instruction::I64Xor),
                BinOp::Shl => self.instruction(Instruction::I64Shl),
                BinOp::Shr => self.instruction(Instruction::I64ShrS),
                BinOp::TupleExtend | BinOp::Eq | BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::Concat => {
                    unreachable!("handled above")
                }
            },

            // i64 unsigned (Uint64)
            Type::Uint64 => match op {
                BinOp::Add => self.instruction(Instruction::I64Add),
                BinOp::Sub => self.instruction(Instruction::I64Sub),
                BinOp::Mul => self.instruction(Instruction::I64Mul),
                BinOp::Div => self.instruction(Instruction::I64DivU),
                BinOp::Rem => self.instruction(Instruction::I64RemU),
                BinOp::Ne => self.instruction(Instruction::I64Ne),
                BinOp::Lt => self.instruction(Instruction::I64LtU),
                BinOp::Gt => self.instruction(Instruction::I64GtU),
                BinOp::Le => self.instruction(Instruction::I64LeU),
                BinOp::Ge => self.instruction(Instruction::I64GeU),
                BinOp::BitAnd => self.instruction(Instruction::I64And),
                BinOp::BitOr => self.instruction(Instruction::I64Or),
                BinOp::BitXor => self.instruction(Instruction::I64Xor),
                BinOp::Shl => self.instruction(Instruction::I64Shl),
                BinOp::Shr => self.instruction(Instruction::I64ShrU),
                BinOp::TupleExtend | BinOp::Eq | BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::Concat => {
                    unreachable!("handled above")
                }
            },

            // Uint128 — flattened `[a_lo, a_hi, b_lo, b_hi]` on the stack; composed wide-arith.
            Type::Uint128 => self.emit_uint128_binop(op),

            // Float32
            Type::Float32 => match op {
                BinOp::Add => self.instruction(Instruction::F32Add),
                BinOp::Sub => self.instruction(Instruction::F32Sub),
                BinOp::Mul => self.instruction(Instruction::F32Mul),
                BinOp::Div => self.instruction(Instruction::F32Div),
                BinOp::Ne => self.instruction(Instruction::F32Ne),
                BinOp::Lt => self.instruction(Instruction::F32Lt),
                BinOp::Gt => self.instruction(Instruction::F32Gt),
                BinOp::Le => self.instruction(Instruction::F32Le),
                BinOp::Ge => self.instruction(Instruction::F32Ge),
                BinOp::Rem
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr => {
                    unreachable!("operator {} not supported for Float32", op)
                }
                BinOp::TupleExtend | BinOp::Eq | BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::Concat => {
                    unreachable!("handled above")
                }
            },

            // Float64
            Type::Float64 => match op {
                BinOp::Add => self.instruction(Instruction::F64Add),
                BinOp::Sub => self.instruction(Instruction::F64Sub),
                BinOp::Mul => self.instruction(Instruction::F64Mul),
                BinOp::Div => self.instruction(Instruction::F64Div),
                BinOp::Ne => self.instruction(Instruction::F64Ne),
                BinOp::Lt => self.instruction(Instruction::F64Lt),
                BinOp::Gt => self.instruction(Instruction::F64Gt),
                BinOp::Le => self.instruction(Instruction::F64Le),
                BinOp::Ge => self.instruction(Instruction::F64Ge),
                BinOp::Rem
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr => {
                    unreachable!("operator {} not supported for Float64", op)
                }
                BinOp::TupleExtend | BinOp::Eq | BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::Concat => {
                    unreachable!("handled above")
                }
            },

            // Bool (only == and !=)
            Type::Bool => match op {
                BinOp::Ne => self.instruction(Instruction::I32Ne),
                _ => unreachable!("operator {} not supported for Bool", op),
            },

            // String (==, !=, ++, <, >, <=, >=; == handled above)
            Type::String => match op {
                BinOp::Concat => {
                    self.instruction(Instruction::Call(self.codegen.func_string_concat()));
                }
                BinOp::Ne => {
                    self.instruction(Instruction::Call(self.codegen.func_string_eq()));
                    self.instruction(Instruction::I32Eqz);
                }
                BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    self.instruction(Instruction::Call(self.codegen.func_string_cmp()));
                    // cmp returns -1/0/1; compare with 0 using signed i32 ops
                    self.instruction(Instruction::I32Const(0));
                    match op {
                        BinOp::Lt => self.instruction(Instruction::I32LtS),
                        BinOp::Gt => self.instruction(Instruction::I32GtS),
                        BinOp::Le => self.instruction(Instruction::I32LeS),
                        BinOp::Ge => self.instruction(Instruction::I32GeS),
                        _ => unreachable!(),
                    }
                }
                _ => unreachable!("operator {} not supported for String", op),
            },

            // Newtype — delegate to inner type
            Type::Newtype(_, inner) | Type::GenericNewtype { concrete_inner_type: inner, .. } => {
                self.emit_binary_instruction(op, inner);
            }

            _ => unreachable!("binary op on unsupported type: {}", operand_ty),
        }
    }

    /// Emit a `Uint128` binary operator. The two operand runs are on the stack as
    /// `[a_lo, a_hi, b_lo, b_hi]`. `+`/`-` map to the `i64.add128`/`i64.sub128` wide-arith
    /// instructions directly (operands already in order); every other op spills the four words
    /// into locals (`al, ah, bl, bh`) and composes the result. `==` is handled by
    /// `emit_eq_instruction`, so it never reaches here. Division/remainder are rejected by the
    /// typechecker (no wide-arith divide instruction).
    fn emit_uint128_binop(&mut self, op: BinOp) {
        // add128/sub128 consume `[a_lo, a_hi, b_lo, b_hi]` directly — no spill needed.
        match op {
            BinOp::Add => {
                self.instruction(Instruction::I64Add128);
                return;
            }
            BinOp::Sub => {
                self.instruction(Instruction::I64Sub128);
                return;
            }
            _ => {}
        }
        // Spill the four words. Stack top is b_hi, so it pops first.
        let bh = self.add_local(wasm_encoder::ValType::I64);
        self.instruction(Instruction::LocalSet(bh));
        let bl = self.add_local(wasm_encoder::ValType::I64);
        self.instruction(Instruction::LocalSet(bl));
        let ah = self.add_local(wasm_encoder::ValType::I64);
        self.instruction(Instruction::LocalSet(ah));
        let al = self.add_local(wasm_encoder::ValType::I64);
        self.instruction(Instruction::LocalSet(al));

        match op {
            BinOp::Mul => {
                // Low 128 bits of a*b. `(ll_lo, ll_hi) = mul_wide_u(a_lo, b_lo)`, then
                // `result_hi = ll_hi + a_lo*b_hi + a_hi*b_lo` (wrapping); `result_lo = ll_lo`.
                self.instruction(Instruction::LocalGet(al));
                self.instruction(Instruction::LocalGet(bl));
                self.instruction(Instruction::I64MulWideU); // → ll_lo, ll_hi
                self.instruction(Instruction::LocalGet(al));
                self.instruction(Instruction::LocalGet(bh));
                self.instruction(Instruction::I64Mul); // a_lo*b_hi
                self.instruction(Instruction::LocalGet(ah));
                self.instruction(Instruction::LocalGet(bl));
                self.instruction(Instruction::I64Mul); // a_hi*b_lo
                self.instruction(Instruction::I64Add); // cross
                self.instruction(Instruction::I64Add); // ll_hi + cross
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor => {
                let inst = match op {
                    BinOp::BitAnd => Instruction::I64And,
                    BinOp::BitOr => Instruction::I64Or,
                    _ => Instruction::I64Xor,
                };
                self.instruction(Instruction::LocalGet(al));
                self.instruction(Instruction::LocalGet(bl));
                self.instruction(inst.clone());
                self.instruction(Instruction::LocalGet(ah));
                self.instruction(Instruction::LocalGet(bh));
                self.instruction(inst);
            }
            BinOp::Shl => self.emit_uint128_shl(al, ah, bl),
            BinOp::Shr => self.emit_uint128_shr(al, ah, bl),
            BinOp::Ne => {
                self.instruction(Instruction::LocalGet(al));
                self.instruction(Instruction::LocalGet(bl));
                self.instruction(Instruction::I64Ne);
                self.instruction(Instruction::LocalGet(ah));
                self.instruction(Instruction::LocalGet(bh));
                self.instruction(Instruction::I64Ne);
                self.instruction(Instruction::I32Or);
            }
            // Ordered comparisons (unsigned): compare high words; on a tie, compare low words.
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let (hi_strict, lo_cmp) = match op {
                    BinOp::Lt => (Instruction::I64LtU, Instruction::I64LtU),
                    BinOp::Le => (Instruction::I64LtU, Instruction::I64LeU),
                    BinOp::Gt => (Instruction::I64GtU, Instruction::I64GtU),
                    _ => (Instruction::I64GtU, Instruction::I64GeU),
                };
                self.instruction(Instruction::LocalGet(ah));
                self.instruction(Instruction::LocalGet(bh));
                self.instruction(hi_strict); // a_hi </> b_hi
                self.instruction(Instruction::LocalGet(ah));
                self.instruction(Instruction::LocalGet(bh));
                self.instruction(Instruction::I64Eq); // a_hi == b_hi
                self.instruction(Instruction::LocalGet(al));
                self.instruction(Instruction::LocalGet(bl));
                self.instruction(lo_cmp); // a_lo cmp b_lo
                self.instruction(Instruction::I32And);
                self.instruction(Instruction::I32Or);
            }
            BinOp::Add | BinOp::Sub => unreachable!("handled before spill"),
            BinOp::Div | BinOp::Rem => {
                unreachable!("Uint128 division/remainder is rejected by the typechecker")
            }
            BinOp::TupleExtend | BinOp::Eq | BinOp::LogicalAnd | BinOp::LogicalOr | BinOp::Concat => {
                unreachable!("handled by emit_eq_instruction / short-circuit path")
            }
        }
    }

    /// Two-word left shift `(al, ah) << n` where `n = bl & 127`. Leaves `[lo, hi]`.
    fn emit_uint128_shl(&mut self, al: u32, ah: u32, bl: u32) {
        let n = self.add_local(wasm_encoder::ValType::I64);
        self.instruction(Instruction::LocalGet(bl));
        self.instruction(Instruction::I64Const(127));
        self.instruction(Instruction::I64And);
        self.instruction(Instruction::LocalSet(n));
        let lo = self.add_local(wasm_encoder::ValType::I64);
        let hi = self.add_local(wasm_encoder::ValType::I64);

        // if n >= 64: lo = 0; hi = al << (n - 64)
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Const(64));
        self.instruction(Instruction::I64GeU);
        self.emit_if_block(BlockType::Empty);
        self.instruction(Instruction::I64Const(0));
        self.instruction(Instruction::LocalSet(lo));
        self.instruction(Instruction::LocalGet(al));
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Const(64));
        self.instruction(Instruction::I64Sub);
        self.instruction(Instruction::I64Shl);
        self.instruction(Instruction::LocalSet(hi));
        self.instruction(Instruction::Else);
        // elif n == 0: lo = al; hi = ah
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Eqz);
        self.emit_if_block(BlockType::Empty);
        self.instruction(Instruction::LocalGet(al));
        self.instruction(Instruction::LocalSet(lo));
        self.instruction(Instruction::LocalGet(ah));
        self.instruction(Instruction::LocalSet(hi));
        self.instruction(Instruction::Else);
        // else 0 < n < 64: lo = al << n; hi = (ah << n) | (al >>u (64 - n))
        self.instruction(Instruction::LocalGet(al));
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Shl);
        self.instruction(Instruction::LocalSet(lo));
        self.instruction(Instruction::LocalGet(ah));
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Shl);
        self.instruction(Instruction::LocalGet(al));
        self.instruction(Instruction::I64Const(64));
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Sub);
        self.instruction(Instruction::I64ShrU);
        self.instruction(Instruction::I64Or);
        self.instruction(Instruction::LocalSet(hi));
        self.emit_end_block();
        self.emit_end_block();

        self.instruction(Instruction::LocalGet(lo));
        self.instruction(Instruction::LocalGet(hi));
    }

    /// Two-word logical right shift `(al, ah) >> n` where `n = bl & 127`. Leaves `[lo, hi]`.
    fn emit_uint128_shr(&mut self, al: u32, ah: u32, bl: u32) {
        let n = self.add_local(wasm_encoder::ValType::I64);
        self.instruction(Instruction::LocalGet(bl));
        self.instruction(Instruction::I64Const(127));
        self.instruction(Instruction::I64And);
        self.instruction(Instruction::LocalSet(n));
        let lo = self.add_local(wasm_encoder::ValType::I64);
        let hi = self.add_local(wasm_encoder::ValType::I64);

        // if n >= 64: lo = ah >>u (n - 64); hi = 0
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Const(64));
        self.instruction(Instruction::I64GeU);
        self.emit_if_block(BlockType::Empty);
        self.instruction(Instruction::LocalGet(ah));
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Const(64));
        self.instruction(Instruction::I64Sub);
        self.instruction(Instruction::I64ShrU);
        self.instruction(Instruction::LocalSet(lo));
        self.instruction(Instruction::I64Const(0));
        self.instruction(Instruction::LocalSet(hi));
        self.instruction(Instruction::Else);
        // elif n == 0: lo = al; hi = ah
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Eqz);
        self.emit_if_block(BlockType::Empty);
        self.instruction(Instruction::LocalGet(al));
        self.instruction(Instruction::LocalSet(lo));
        self.instruction(Instruction::LocalGet(ah));
        self.instruction(Instruction::LocalSet(hi));
        self.instruction(Instruction::Else);
        // else 0 < n < 64: lo = (al >>u n) | (ah << (64 - n)); hi = ah >>u n
        self.instruction(Instruction::LocalGet(al));
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64ShrU);
        self.instruction(Instruction::LocalGet(ah));
        self.instruction(Instruction::I64Const(64));
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64Sub);
        self.instruction(Instruction::I64Shl);
        self.instruction(Instruction::I64Or);
        self.instruction(Instruction::LocalSet(lo));
        self.instruction(Instruction::LocalGet(ah));
        self.instruction(Instruction::LocalGet(n));
        self.instruction(Instruction::I64ShrU);
        self.instruction(Instruction::LocalSet(hi));
        self.emit_end_block();
        self.emit_end_block();

        self.instruction(Instruction::LocalGet(lo));
        self.instruction(Instruction::LocalGet(hi));
    }

    /// If `e` is `someValue.toUint128()` where `someValue` is an unsigned ≤64-bit integer (so the
    /// widened high word is provably zero), return the inner value. Used to recognize the
    /// widening-multiply peephole.
    fn uint128_widen_source(e: &TypedExpr) -> Option<&TypedExpr> {
        if let TypedExprKind::IntrinsicCall {
            intrinsic: crate::typechecker::types::IntrinsicKind::NumericConvert(target),
            args,
        } = &e.kind
        {
            if matches!(target, Type::Uint128)
                && args.len() == 1
                && matches!(
                    args[0].ty,
                    Type::Uint8 | Type::Uint16 | Type::Uint32 | Type::Uint64
                )
            {
                return Some(&args[0]);
            }
        }
        None
    }

    /// Emit an unsigned ≤64-bit integer expression and leave it as a single `i64` on the stack.
    fn emit_widened_to_i64(&mut self, e: &TypedExpr) {
        self.emit_expr(e, ExprContext::Value);
        match e.ty {
            Type::Uint64 => {}
            Type::Uint8 | Type::Uint16 | Type::Uint32 => {
                self.instruction(Instruction::I64ExtendI32U)
            }
            _ => unreachable!("emit_widened_to_i64 on non-unsigned-≤64 type: {}", e.ty),
        }
    }

    /// Emit narrowing/masking after arithmetic on sub-32-bit types.
    /// Uses sign-extension ops per the WebAssembly sign-extension proposal.
    fn emit_narrow(&mut self, ty: &Type) {
        match ty {
            Type::Int8 => self.instruction(Instruction::I32Extend8S),
            Type::Int16 => self.instruction(Instruction::I32Extend16S),
            Type::Uint8 => {
                self.instruction(Instruction::I32Const(0xFF));
                self.instruction(Instruction::I32And);
            }
            Type::Uint16 => {
                self.instruction(Instruction::I32Const(0xFFFF));
                self.instruction(Instruction::I32And);
            }
            _ => {} // no narrowing needed
        }
    }

    /// Emit WASM instructions for a numeric type conversion.
    pub(super) fn emit_numeric_convert(&mut self, source: &Type, target: &Type) {
        use Type::*;
        match (source, target) {
            // i32 → i32 (sub-width conversions): just narrow/reinterpret
            (Int8 | Int16 | Int32 | Uint8 | Uint16 | Uint32, Int8 | Int16 | Uint8 | Uint16) => {
                self.emit_narrow(target);
            }
            (Int8 | Int16 | Int32 | Uint8 | Uint16 | Uint32, Int32 | Uint32) => {
                // i32 to i32/u32: no-op (already full-width i32)
                // But if source is sub-32 signed converting to Uint32, we need to mask
                // and if source is sub-32 unsigned converting to Int32, it's fine (zero-extended)
                // Actually, the source value is already properly narrowed from prior ops,
                // so Int8(-1) is stored as 0xFFFFFFFF in i32.
                // For signed→Uint32: need to mask to 32 bits (already 32-bit, no-op)
                // For unsigned→Int32: already zero-extended, no-op
                // The WASM i32 representation handles this correctly.
            }

            // i32 (signed) → i64
            (Int8 | Int16 | Int32, Int64) => {
                self.instruction(Instruction::I64ExtendI32S);
            }
            // i32 (unsigned) → i64
            (Uint8 | Uint16 | Uint32, Int64 | Uint64) => {
                self.instruction(Instruction::I64ExtendI32U);
            }
            // i32 (signed) → Uint64
            (Int8 | Int16 | Int32, Uint64) => {
                self.instruction(Instruction::I64ExtendI32S);
            }

            // i64 → i32 types
            (Int64 | Uint64, Int8 | Int16 | Int32 | Uint8 | Uint16 | Uint32) => {
                self.instruction(Instruction::I32WrapI64);
                self.emit_narrow(target);
            }
            // i64 → i64 (Int64↔Uint64): no-op (bit reinterpretation)
            (Int64, Uint64) | (Uint64, Int64) => {}

            // i32 (signed) → f32
            (Int8 | Int16 | Int32, Float32) => {
                self.instruction(Instruction::F32ConvertI32S);
            }
            // i32 (unsigned) → f32
            (Uint8 | Uint16 | Uint32, Float32) => {
                self.instruction(Instruction::F32ConvertI32U);
            }
            // i32 (signed) → f64
            (Int8 | Int16 | Int32, Float64) => {
                self.instruction(Instruction::F64ConvertI32S);
            }
            // i32 (unsigned) → f64
            (Uint8 | Uint16 | Uint32, Float64) => {
                self.instruction(Instruction::F64ConvertI32U);
            }

            // i64 (signed) → f32
            (Int64, Float32) => {
                self.instruction(Instruction::F32ConvertI64S);
            }
            // i64 (unsigned) → f32
            (Uint64, Float32) => {
                self.instruction(Instruction::F32ConvertI64U);
            }
            // i64 (signed) → f64
            (Int64, Float64) => {
                self.instruction(Instruction::F64ConvertI64S);
            }
            // i64 (unsigned) → f64
            (Uint64, Float64) => {
                self.instruction(Instruction::F64ConvertI64U);
            }

            // f32 → i32 (signed target)
            (Float32, Int8 | Int16 | Int32) => {
                self.instruction(Instruction::I32TruncF32S);
                self.emit_narrow(target);
            }
            // f32 → i32 (unsigned target)
            (Float32, Uint8 | Uint16 | Uint32) => {
                self.instruction(Instruction::I32TruncF32U);
                self.emit_narrow(target);
            }
            // f32 → i64 (signed)
            (Float32, Int64) => {
                self.instruction(Instruction::I64TruncF32S);
            }
            // f32 → i64 (unsigned)
            (Float32, Uint64) => {
                self.instruction(Instruction::I64TruncF32U);
            }

            // f64 → i32 (signed target)
            (Float64, Int8 | Int16 | Int32) => {
                self.instruction(Instruction::I32TruncF64S);
                self.emit_narrow(target);
            }
            // f64 → i32 (unsigned target)
            (Float64, Uint8 | Uint16 | Uint32) => {
                self.instruction(Instruction::I32TruncF64U);
                self.emit_narrow(target);
            }
            // f64 → i64 (signed)
            (Float64, Int64) => {
                self.instruction(Instruction::I64TruncF64S);
            }
            // f64 → i64 (unsigned)
            (Float64, Uint64) => {
                self.instruction(Instruction::I64TruncF64U);
            }

            // f32 ↔ f64
            (Float32, Float64) => {
                self.instruction(Instruction::F64PromoteF32);
            }
            (Float64, Float32) => {
                self.instruction(Instruction::F32DemoteF64);
            }

            // Widen into Uint128 → flattened `[lo, hi]`. Unsigned sources zero-extend (hi = 0);
            // signed sources sign-extend (lo sign-extended to i64, hi = the sign bits).
            (Uint8 | Uint16 | Uint32, Uint128) => {
                self.instruction(Instruction::I64ExtendI32U);
                self.instruction(Instruction::I64Const(0));
            }
            (Uint64, Uint128) => {
                // lo already on the stack as i64; push hi = 0.
                self.instruction(Instruction::I64Const(0));
            }
            (Int8 | Int16 | Int32, Uint128) => {
                self.instruction(Instruction::I64ExtendI32S);
                self.emit_uint128_sign_extend_high();
            }
            (Int64, Uint128) => {
                self.emit_uint128_sign_extend_high();
            }

            // Narrow out of Uint128: drop the high word, keep the low 64 bits, then narrow further.
            (Uint128, Int64 | Uint64) => {
                self.instruction(Instruction::Drop);
            }
            (Uint128, Int8 | Int16 | Int32 | Uint8 | Uint16 | Uint32) => {
                self.instruction(Instruction::Drop);
                self.instruction(Instruction::I32WrapI64);
                self.emit_narrow(target);
            }

            _ => unreachable!(
                "unsupported numeric conversion: {:?} → {:?}",
                source, target
            ),
        }
    }

    /// With a sign-extended `i64` low word on top of the stack, leave the flattened `Uint128`
    /// run `[lo, hi]` where `hi` is the sign bits (`lo >>s 63`). Used when widening a *signed*
    /// integer into `Uint128`.
    fn emit_uint128_sign_extend_high(&mut self) {
        let t = self.add_local(wasm_encoder::ValType::I64);
        self.instruction(Instruction::LocalTee(t));
        self.instruction(Instruction::LocalGet(t));
        self.instruction(Instruction::I64Const(63));
        self.instruction(Instruction::I64ShrS);
    }

    /// Emit an equality comparison instruction for the given type.
    pub(super) fn emit_eq_instruction(&mut self, ty: &Type) {
        match ty {
            Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Uint8
            | Type::Uint16
            | Type::Uint32
            | Type::Bool
            | Type::Char => {
                self.instruction(Instruction::I32Eq);
            }
            Type::Int64 | Type::Uint64 => {
                self.instruction(Instruction::I64Eq);
            }
            Type::Uint128 => {
                // Stack: `[a_lo, a_hi, b_lo, b_hi]`. Equal iff both words match.
                let bh = self.add_local(wasm_encoder::ValType::I64);
                self.instruction(Instruction::LocalSet(bh));
                let bl = self.add_local(wasm_encoder::ValType::I64);
                self.instruction(Instruction::LocalSet(bl));
                let ah = self.add_local(wasm_encoder::ValType::I64);
                self.instruction(Instruction::LocalSet(ah));
                let al = self.add_local(wasm_encoder::ValType::I64);
                self.instruction(Instruction::LocalSet(al));
                self.instruction(Instruction::LocalGet(al));
                self.instruction(Instruction::LocalGet(bl));
                self.instruction(Instruction::I64Eq);
                self.instruction(Instruction::LocalGet(ah));
                self.instruction(Instruction::LocalGet(bh));
                self.instruction(Instruction::I64Eq);
                self.instruction(Instruction::I32And);
            }
            Type::Float32 => {
                self.instruction(Instruction::F32Eq);
            }
            Type::Float64 => {
                self.instruction(Instruction::F64Eq);
            }
            Type::String => {
                self.instruction(Instruction::Call(self.codegen.func_string_eq()));
            }
            Type::Newtype(_, inner) | Type::GenericNewtype { concrete_inner_type: inner, .. } => {
                self.emit_eq_instruction(inner);
            }
            _ => {
                // Fallback for unsupported equality types: emit a runtime trap
                // rather than panicking during code generation.
                self.instruction(Instruction::Unreachable);
            }
        }
    }

    /// Coerce an on-stack value into a single WASM slot that holds it *boxed*: an erased anyref
    /// slot, or a concrete `(ref $Tuple)` struct field / enum payload / global. A flattened tuple's
    /// values are reboxed into its `(ref $Tuple)`; an erased scalar is boxed; reference values pass.
    /// Used at boxed single-slot boundaries (enum payloads, globals, and — until struct-field
    /// splicing lands — concrete record/class tuple fields).
    fn box_into_slot(&mut self, value_ty: &Type, slot_ty: &Type) {
        if matches!(slot_ty, Type::Any) {
            // An `Any` slot is a single anyref: box a scalar, rebox a flattened tuple, upcast a ref.
            self.emit_box_to_any(value_ty);
        } else if Codegen::is_erased_slot(slot_ty) {
            self.box_if_erased_slot(value_ty, slot_ty);
        } else if self.codegen.is_tuple(slot_ty) {
            self.emit_rebox_tuple(slot_ty);
        } else if self.codegen.is_uint128(slot_ty) {
            self.emit_rebox_uint128();
        }
    }

    /// Coerce a value into a *spliced* record/class field slot. An `Any`/erased field is a single
    /// anyref (box the value); a concrete tuple field is spliced into N WASM fields, so its values
    /// flows in directly with no boxing; everything else is width-1.
    fn box_into_field(&mut self, value_ty: &Type, slot_ty: &Type) {
        if matches!(slot_ty, Type::Any) || Codegen::is_erased_slot(slot_ty) {
            self.emit_box_to_any(value_ty);
        } else if self.codegen.is_tuple(slot_ty) {
            self.coerce_value(value_ty, slot_ty);
        }
    }

    /// Bind a value currently on top of the stack (a scalar, a ref, or a flattened tuple) to a
    /// named local. An erased-slot param is boxed into a single anyref local; a concrete tuple is
    /// bound as a sequence of locals; everything else is a single local — matching how the binding is
    /// later read by `VarRef`.
    fn bind_param_local(&mut self, name: VarName, value_ty: &Type, declared_ty: &Type) {
        if Codegen::is_erased_slot(declared_ty) {
            self.box_if_erased_slot(value_ty, declared_ty);
            let val_type = self.codegen.single_val_type(declared_ty);
            let idx = self.define_local(name, val_type);
            self.instruction(Instruction::LocalSet(idx));
        } else {
            let valtypes = self.codegen.type_to_valtypes(declared_ty);
            let base = self.define_value_locals(name, &valtypes);
            self.store_value(base, &valtypes);
        }
    }

    /// Inverse of `box_into_slot`: after reading a boxed slot value (via `struct.get`/`global.get`),
    /// explode a concrete `(ref $Tuple)` into its values, or cast an erased value back to `target_ty`.
    fn unbox_from_slot(&mut self, slot_ty: &Type, target_ty: &Type) {
        if Codegen::is_erased_slot(slot_ty) {
            self.cast_back_from_erased(slot_ty, target_ty);
        } else if self.codegen.is_tuple(slot_ty) {
            self.emit_unbox_tuple(slot_ty);
            self.coerce_value(slot_ty, target_ty);
        } else if self.codegen.is_uint128(slot_ty) {
            self.emit_unbox_uint128();
        }
    }

    /// If `slot_ty` is an erased TypeDef slot and the on-stack value is a primitive,
    /// emit `struct.new` of the appropriate primitive box so the value upcasts cleanly to anyref.
    /// Reference-type values upcast to anyref implicitly — no emission needed.
    fn box_if_erased_slot(&mut self, value_ty: &Type, slot_ty: &Type) {
        if !Codegen::is_erased_slot(slot_ty) {
            return;
        }
        // If the value's static type is itself a type parameter, it's already anyref —
        // nothing to box. (Happens in unsubstituted template-context emissions, e.g., when
        // a generic class's initializer is emitted from its canonical TypeDef.)
        if matches!(value_ty, Type::TypeVariable(_, _) | Type::GenericParam(_, _, _)) {
            return;
        }
        // A flattened tuple must be reboxed into its `(ref $Tuple)` to occupy the single
        // erased anyref slot. (A tuple is a reference type, so the branch below would skip it.)
        if self.codegen.is_tuple(value_ty) {
            self.emit_rebox_tuple(value_ty);
            return;
        }
        // A flattened `Uint128` reboxes into its dedicated `(ref $Uint128)` to occupy the slot.
        if self.codegen.is_uint128(value_ty) {
            self.emit_rebox_uint128();
            return;
        }
        // Never is uninhabited at runtime but is lowered as `i32` in WASM. Box it so the
        // erased anyref slot receives a reference. (`is_reference_type(Never)` returns true
        // so the next branch would otherwise skip boxing.)
        if matches!(value_ty, Type::Never) {
            let box_idx = self.codegen.box_type_index_for(value_ty);
            self.instruction(Instruction::StructNew(box_idx));
            return;
        }
        if !value_ty.is_reference_type() {
            let box_idx = self.codegen.box_type_index_for(value_ty);
            self.instruction(Instruction::StructNew(box_idx));
        }
    }

    /// After a `struct.get` from a slot whose TypeDef type is erased, cast the anyref
    /// back to the expression's static concrete type, unboxing primitives.
    /// No-op when the slot isn't erased, or when the target is `Any`/`Never`/`Error`.
    pub(super) fn cast_back_from_erased(&mut self, slot_ty: &Type, target_ty: &Type) {
        if !Codegen::is_erased_slot(slot_ty) {
            return;
        }
        // Uninhabited / error target: emit `unreachable` so downstream local.set passes
        // WASM validation (unreachable is polymorphic and produces any type).
        if matches!(target_ty, Type::Never | Type::Error) {
            self.instruction(Instruction::Unreachable);
            return;
        }
        // Target itself is anyref (type parameter, Any) — no cast needed.
        if matches!(target_ty, Type::Any) || Codegen::is_erased_slot(target_ty) {
            return;
        }
        // A boxed tuple read from an erased slot: cast to `(ref $Tuple)` then explode to a flattened value.
        if self.codegen.is_tuple(target_ty) {
            let target_idx = self.codegen.wasm_type_index_for_any_cast(target_ty);
            self.instruction(Instruction::RefCastNonNull(wasm_encoder::HeapType::Concrete(target_idx)));
            self.emit_unbox_tuple(target_ty);
            return;
        }
        // A boxed `Uint128` read from an erased slot: cast to `(ref $Uint128)` then explode to `[lo, hi]`.
        if self.codegen.is_uint128(target_ty) {
            let target_idx = self.codegen.wasm_type_index_for_any_cast(target_ty);
            self.instruction(Instruction::RefCastNonNull(wasm_encoder::HeapType::Concrete(target_idx)));
            self.emit_unbox_uint128();
            return;
        }
        let target_idx = self.codegen.wasm_type_index_for_any_cast(target_ty);
        self.instruction(Instruction::RefCastNonNull(wasm_encoder::HeapType::Concrete(target_idx)));
        if !target_ty.is_reference_type() {
            self.instruction(Instruction::StructGet { struct_type_index: target_idx, field_index: 0 });
        }
    }

    /// Emit a record `with` expression — copy an existing record, overriding specified fields.
    fn emit_record_with(
        &mut self,
        expr: &TypedExpr,
        object: &TypedExpr,
        overrides: &[(String, u32, TypedExpr)],
        ctx: ExprContext,
    ) {
        let (struct_type_index, mn) = match &expr.ty {
            Type::Record(_, mn)
            | Type::GenericRecord {
                mangled_name: mn, ..
            } => (self.codegen.type_indices[mn], mn.clone()),
            _ => unreachable!("RecordWith must have Record type"),
        };

        // Snapshot the record's declared field types so we can mutate `self` later.
        let slot_types: Vec<Type> = {
            let crate::typechecker::types::TypeDef::Record(record_def) =
                &self.codegen.typed_module.types[&mn]
            else {
                unreachable!("RecordWith must reference a Record TypeDef")
            };
            record_def.fields.iter().map(|(_, t)| t.clone()).collect()
        };
        let num_fields = slot_types.len() as u32;

        // Emit the object expression and save to a temp local
        self.emit_expr(object, ExprContext::Value);
        let ref_type = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(struct_type_index),
        });
        let tmp = self.add_local(ref_type);
        self.instruction(Instruction::LocalSet(tmp));

        // Build a lookup of overridden field indices
        let override_map: std::collections::BTreeMap<u32, usize> = overrides
            .iter()
            .enumerate()
            .map(|(i, (_, field_index, _))| (*field_index, i))
            .collect();
        // For each field in declaration order, emit override or copy from original. A concrete
        // tuple field is spliced into N WASM fields, so both paths push all the field's values.
        for i in 0..num_fields {
            if let Some(&override_idx) = override_map.get(&i) {
                let override_expr = &overrides[override_idx].2;
                self.emit_expr(override_expr, ExprContext::Value);
                if let Some(slot_ty) = slot_types.get(i as usize) {
                    self.box_into_field(&override_expr.ty, slot_ty);
                }
            } else {
                // Copy path: read the field's WASM range from the original (same WASM type, so no
                // boxing/casting needed) — one struct.get per spliced WASM field.
                let (start, width) = self.codegen.struct_field_range(&mn, i as usize);
                for k in 0..width {
                    self.instruction(Instruction::LocalGet(tmp));
                    self.instruction(Instruction::StructGet {
                        struct_type_index,
                        field_index: start + k,
                    });
                }
            }
        }

        self.emit_nominal_struct_new(&self.codegen.construction_type(expr), struct_type_index);
        self.drop_if_statement(ctx, &expr.ty);
    }

    /// Emit clone for any array type (unified for all element types).
    pub(super) fn emit_array_clone(&mut self, elem_type: &Type, source: &TypedExpr) {
        let array_type_index = self.codegen.array_type_index(elem_type);

        // 1. Emit source array, store in local
        self.emit_expr(source, ExprContext::Value);
        let src_ref_type = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(array_type_index),
        });
        let src_local = self.add_local(src_ref_type);
        self.instruction(Instruction::LocalSet(src_local));

        // 2. Get length, store in local
        let len_local = self.add_local(wasm_encoder::ValType::I32);
        self.instruction(Instruction::LocalGet(src_local));
        self.instruction(Instruction::ArrayLen);
        self.instruction(Instruction::LocalSet(len_local));

        // 3. Branch on empty vs non-empty
        let result_ref_type = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(array_type_index),
        });
        self.instruction(Instruction::LocalGet(len_local));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::I32GtS);
        self.emit_if_block(BlockType::Result(result_ref_type));

        // Non-empty path: array.new with source[0] as default, then array.copy
        let dst_local = self.add_local(src_ref_type);
        self.instruction(Instruction::LocalGet(src_local));
        self.instruction(Instruction::I32Const(0));
        let get_instr = match elem_type {
            Type::Int8 | Type::Int16 => Instruction::ArrayGetS(array_type_index),
            Type::Uint8 | Type::Uint16 => Instruction::ArrayGetU(array_type_index),
            _ => Instruction::ArrayGet(array_type_index),
        };
        self.instruction(get_instr);
        self.instruction(Instruction::LocalGet(len_local));
        self.instruction(Instruction::ArrayNew(array_type_index));
        self.instruction(Instruction::LocalSet(dst_local));

        self.instruction(Instruction::LocalGet(dst_local));
        self.instruction(Instruction::I32Const(0)); // dst offset
        self.instruction(Instruction::LocalGet(src_local));
        self.instruction(Instruction::I32Const(0)); // src offset
        self.instruction(Instruction::LocalGet(len_local));
        self.instruction(Instruction::ArrayCopy {
            array_type_index_dst: array_type_index,
            array_type_index_src: array_type_index,
        });

        self.instruction(Instruction::LocalGet(dst_local));

        // Empty path: create zero-length array
        self.instruction(Instruction::Else);
        self.instruction(Instruction::ArrayNewFixed {
            array_type_index,
            array_size: 0,
        });

        self.emit_end_block();
    }

    /// Emit inline array extend: create new array with fill, copy min(src.len, newSize) from src.
    pub(super) fn emit_inline_array_extend(&mut self, elem_type: &Type, args: &[TypedExpr]) {
        let array_type_index = self.codegen.array_type_index(elem_type);

        // Emit and save args
        self.emit_expr(&args[0], ExprContext::Value);
        let src_ref_type = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(array_type_index),
        });
        let src_local = self.add_local(src_ref_type);
        self.instruction(Instruction::LocalSet(src_local));

        self.emit_expr(&args[1], ExprContext::Value);
        // The fill occupies a single boxed array-element slot; rebox a tuple's values / a Uint128.
        if self.codegen.is_tuple(elem_type) {
            self.emit_rebox_tuple(elem_type);
        } else if self.codegen.is_uint128(elem_type) {
            self.emit_rebox_uint128();
        }
        let fill_valtype = self.codegen.single_val_type(elem_type);
        let fill_local = self.add_local(fill_valtype);
        self.instruction(Instruction::LocalSet(fill_local));

        self.emit_expr(&args[2], ExprContext::Value);
        let new_size_local = self.add_local(wasm_encoder::ValType::I32);
        self.instruction(Instruction::LocalSet(new_size_local));

        // Create new array: ArrayNew(fill_value, new_size)
        let dst_local = self.add_local(src_ref_type);
        self.instruction(Instruction::LocalGet(fill_local));
        self.instruction(Instruction::LocalGet(new_size_local));
        self.instruction(Instruction::ArrayNew(array_type_index));
        self.instruction(Instruction::LocalSet(dst_local));

        // src_len = src.length
        let src_len_local = self.add_local(wasm_encoder::ValType::I32);
        self.instruction(Instruction::LocalGet(src_local));
        self.instruction(Instruction::ArrayLen);
        self.instruction(Instruction::LocalSet(src_len_local));

        // copy_len = min(src_len, new_size)
        let copy_len_local = self.add_local(wasm_encoder::ValType::I32);
        self.instruction(Instruction::LocalGet(src_len_local));
        self.instruction(Instruction::LocalGet(new_size_local));
        self.instruction(Instruction::LocalGet(src_len_local));
        self.instruction(Instruction::LocalGet(new_size_local));
        self.instruction(Instruction::I32LeU);
        self.instruction(Instruction::Select);
        self.instruction(Instruction::LocalSet(copy_len_local));

        // ArrayCopy src[0..copy_len] to dst[0..copy_len]
        self.instruction(Instruction::LocalGet(dst_local));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalGet(src_local));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalGet(copy_len_local));
        self.instruction(Instruction::ArrayCopy {
            array_type_index_dst: array_type_index,
            array_type_index_src: array_type_index,
        });

        // Return dst
        self.instruction(Instruction::LocalGet(dst_local));
    }

    /// Emit inline array concat: allocate new array of len_a + len_b, copy both.
    /// Uses first element of either array as fill value (non-nullable arrays).
    pub(super) fn emit_inline_array_concat(&mut self, elem_type: &Type, args: &[TypedExpr]) {
        let array_type_index = self.codegen.array_type_index(elem_type);

        // Emit and save both arrays
        self.emit_expr(&args[0], ExprContext::Value);
        let arr_ref_type = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(array_type_index),
        });
        let arr_a_local = self.add_local(arr_ref_type);
        self.instruction(Instruction::LocalSet(arr_a_local));

        self.emit_expr(&args[1], ExprContext::Value);
        let arr_b_local = self.add_local(arr_ref_type);
        self.instruction(Instruction::LocalSet(arr_b_local));

        // len_a, len_b
        let len_a_local = self.add_local(wasm_encoder::ValType::I32);
        self.instruction(Instruction::LocalGet(arr_a_local));
        self.instruction(Instruction::ArrayLen);
        self.instruction(Instruction::LocalSet(len_a_local));

        let len_b_local = self.add_local(wasm_encoder::ValType::I32);
        self.instruction(Instruction::LocalGet(arr_b_local));
        self.instruction(Instruction::ArrayLen);
        self.instruction(Instruction::LocalSet(len_b_local));

        // total_len = len_a + len_b
        let total_len_local = self.add_local(wasm_encoder::ValType::I32);
        self.instruction(Instruction::LocalGet(len_a_local));
        self.instruction(Instruction::LocalGet(len_b_local));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(total_len_local));

        // Branch: if total_len > 0, create array using first available element as fill
        self.instruction(Instruction::LocalGet(total_len_local));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::I32GtS);
        self.emit_if_block(BlockType::Result(arr_ref_type));

        // Non-empty path: pick fill from arr_a[0] if len_a > 0, else arr_b[0]. The element is read
        // as its single value type (cast back from `(ref any)` for `$Array$ref`) — matching
        // `fill_valtype` — then fed straight back into `array.new` as the fill (stays boxed).
        let fill_valtype = self.codegen.single_val_type(elem_type);

        self.instruction(Instruction::LocalGet(len_a_local));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::I32GtS);
        self.emit_if_block(BlockType::Result(fill_valtype));
        self.instruction(Instruction::LocalGet(arr_a_local));
        self.instruction(Instruction::I32Const(0));
        self.emit_array_element_get(elem_type, array_type_index);
        self.instruction(Instruction::Else);
        self.instruction(Instruction::LocalGet(arr_b_local));
        self.instruction(Instruction::I32Const(0));
        self.emit_array_element_get(elem_type, array_type_index);
        self.emit_end_block();

        // Create array with fill value and total_len
        self.instruction(Instruction::LocalGet(total_len_local));
        self.instruction(Instruction::ArrayNew(array_type_index));
        let dst_local = self.add_local(arr_ref_type);
        self.instruction(Instruction::LocalSet(dst_local));

        // ArrayCopy arr_a[0..len_a] to dst[0..len_a]
        self.instruction(Instruction::LocalGet(dst_local));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalGet(arr_a_local));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalGet(len_a_local));
        self.instruction(Instruction::ArrayCopy {
            array_type_index_dst: array_type_index,
            array_type_index_src: array_type_index,
        });

        // ArrayCopy arr_b[0..len_b] to dst[len_a..len_a+len_b]
        self.instruction(Instruction::LocalGet(dst_local));
        self.instruction(Instruction::LocalGet(len_a_local));
        self.instruction(Instruction::LocalGet(arr_b_local));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalGet(len_b_local));
        self.instruction(Instruction::ArrayCopy {
            array_type_index_dst: array_type_index,
            array_type_index_src: array_type_index,
        });

        self.instruction(Instruction::LocalGet(dst_local));

        // Empty path: return empty array
        self.instruction(Instruction::Else);
        self.instruction(Instruction::ArrayNewFixed {
            array_type_index,
            array_size: 0,
        });

        self.emit_end_block();
    }

    /// Emit reading one array element onto the stack as its single Dovetail value type. For the
    /// shared `$Array$ref` (every reference element) the element comes off as `(ref any)`, so this
    /// `ref.cast`s it back to the concrete element type the receiver names; an `Any` element is
    /// already `(ref any)`, so no cast. Primitive arrays and `$Array$u128` store the concrete type
    /// inline, so it's just the get instruction. Does NOT unbox tuples — a boxed `(ref $Tuple_N)`
    /// stays boxed; callers that want a flattened value call `emit_unbox_tuple` afterward.
    pub(super) fn emit_array_element_get(&mut self, elem: &Type, array_type_index: u32) {
        let elem = super::super::Codegen::array_element_type(elem);
        let get_instr = self.array_get_instruction(elem, array_type_index);
        self.instruction(get_instr);
        if array_type_index == super::super::ARRAY_REF_TYPE_INDEX && !matches!(elem, Type::Any) {
            let target_idx = self.codegen.wasm_type_index_for_any_cast(elem);
            self.instruction(Instruction::RefCastNonNull(wasm_encoder::HeapType::Concrete(
                target_idx,
            )));
        }
    }

    /// Get the appropriate array get instruction for an element type.
    fn array_get_instruction(&self, elem: &Type, array_type_index: u32) -> Instruction<'static> {
        match elem {
            Type::Int8 => Instruction::ArrayGetS(array_type_index),
            Type::Int16 => Instruction::ArrayGetS(array_type_index),
            Type::Uint8 => Instruction::ArrayGetU(array_type_index),
            Type::Uint16 => Instruction::ArrayGetU(array_type_index),
            _ => Instruction::ArrayGet(array_type_index),
        }
    }

    /// Emit a unary operator.
    fn emit_unary_op(&mut self, op: UnaryOp, operand: &TypedExpr) {
        match op {
            UnaryOp::Neg => match &operand.ty {
                Type::Int8 | Type::Int16 | Type::Int32 => {
                    self.instruction(Instruction::I32Const(0));
                    self.emit_expr(operand, ExprContext::Value);
                    self.instruction(Instruction::I32Sub);
                    self.emit_narrow(&operand.ty);
                }
                Type::Int64 => {
                    self.instruction(Instruction::I64Const(0));
                    self.emit_expr(operand, ExprContext::Value);
                    self.instruction(Instruction::I64Sub);
                }
                Type::Float32 => {
                    self.emit_expr(operand, ExprContext::Value);
                    self.instruction(Instruction::F32Neg);
                }
                Type::Float64 => {
                    self.emit_expr(operand, ExprContext::Value);
                    self.instruction(Instruction::F64Neg);
                }
                _ => unreachable!("Neg on unsupported type: {}", operand.ty),
            },
            UnaryOp::Not => {
                // Bool only: !x => x == 0
                self.emit_expr(operand, ExprContext::Value);
                self.instruction(Instruction::I32Eqz);
            }
            UnaryOp::BitNot => match &operand.ty {
                Type::Int8
                | Type::Int16
                | Type::Int32
                | Type::Uint8
                | Type::Uint16
                | Type::Uint32 => {
                    self.emit_expr(operand, ExprContext::Value);
                    self.instruction(Instruction::I32Const(-1));
                    self.instruction(Instruction::I32Xor);
                    self.emit_narrow(&operand.ty);
                }
                Type::Int64 | Type::Uint64 => {
                    self.emit_expr(operand, ExprContext::Value);
                    self.instruction(Instruction::I64Const(-1));
                    self.instruction(Instruction::I64Xor);
                }
                _ => unreachable!("BitNot on unsupported type: {}", operand.ty),
            },
        }
    }
}
