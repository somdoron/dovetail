//! Generic canonical-ABI lift/lower engine for WIT interface imports.
//!
//! Replaces a generated binding function's stub body with: lower Dovetail
//! params to flat core args → call the import → lift the result back into
//! Dovetail GC values. Driven entirely by wit-parser's ABI machinery
//! (`wasm_signature`, `push_flat`, `SizeAlign`) walking the WIT type and the
//! Dovetail type in lockstep — the Dovetail side mirrors `witgen::mapping`.

use wasm_encoder::{BlockType, HeapType, Instruction, MemArg, RefType, ValType};
use wit_parser::abi::{AbiVariant, WasmType};
use wit_parser::{Resolve, SizeAlign, Type as WitType, TypeDefKind};

use crate::common::types::MangledName;
use crate::compiler::witgen::{WitFuncKind, WitFuncRef};
use crate::typechecker::types::{Type, TypeDef, TypedParam};

fn val_type(ty: &WasmType) -> ValType {
    match ty {
        WasmType::I32 | WasmType::Pointer | WasmType::Length => ValType::I32,
        WasmType::I64 | WasmType::PointerOrI64 => ValType::I64,
        WasmType::F32 => ValType::F32,
        WasmType::F64 => ValType::F64,
    }
}

/// Flatten a WIT type to its canonical-ABI core types. wit-parser 0.255 writes
/// into a fixed-capacity `FlatTypes`; a v1 import param/variant never exceeds
/// the 16-slot flat limit (bindgen rejects wider functions), so a 32-slot
/// scratch buffer is always sufficient.
fn flat_types(resolve: &Resolve, ty: &WitType) -> Vec<WasmType> {
    let mut storage = [WasmType::I32; 32];
    let mut flat = wit_parser::abi::FlatTypes::new(&mut storage);
    resolve.push_flat(ty, &mut flat);
    flat.to_vec()
}

fn mem_arg(offset: u32) -> MemArg {
    MemArg {
        offset: offset as u64,
        align: 0,
        memory_index: 0,
    }
}

/// Resolve WIT type aliases to the underlying type.
fn resolve_alias<'r>(resolve: &'r Resolve, ty: &'r WitType) -> &'r WitType {
    if let WitType::Id(id) = ty
        && let TypeDefKind::Type(inner) = &resolve.types[*id].kind
    {
        return resolve_alias(resolve, inner);
    }
    ty
}

/// Discriminant byte width for a tag integer.
fn tag_bytes(tag: wit_parser::Int) -> u32 {
    match tag {
        wit_parser::Int::U8 => 1,
        wit_parser::Int::U16 => 2,
        wit_parser::Int::U32 | wit_parser::Int::U64 => 4,
    }
}

/// The (disc, cases) shape shared by enum/variant/option/result.
struct VariantShape {
    tag: wit_parser::Int,
    /// Per-case optional payload type, in WIT discriminant order.
    cases: Vec<Option<WitType>>,
    /// For option/result, the Dovetail variant name per WIT case (the Dovetail
    /// enum declares `Some` before `None`, so positional mapping is wrong).
    dovetail_names: Option<Vec<&'static str>>,
}

fn variant_shape(resolve: &Resolve, ty: &WitType) -> Option<VariantShape> {
    let WitType::Id(id) = ty else { return None };
    match &resolve.types[*id].kind {
        TypeDefKind::Enum(e) => Some(VariantShape {
            tag: e.tag(),
            cases: e.cases.iter().map(|_| None).collect(),
            dovetail_names: None,
        }),
        TypeDefKind::Variant(v) => Some(VariantShape {
            tag: v.tag(),
            cases: v.cases.iter().map(|c| c.ty).collect(),
            dovetail_names: None,
        }),
        TypeDefKind::Option(t) => Some(VariantShape {
            tag: wit_parser::Int::U8,
            cases: vec![None, Some(*t)],
            dovetail_names: Some(vec!["None", "Some"]),
        }),
        TypeDefKind::Result(r) => Some(VariantShape {
            tag: wit_parser::Int::U8,
            cases: vec![r.ok, r.err],
            dovetail_names: Some(vec!["Ok", "Error"]),
        }),
        _ => None,
    }
}

/// The Dovetail-side enum info for a WIT variant-like type: mangled name plus
/// each case's Dovetail payload type (`None` = no payload; Unit payloads from
/// option/result map to `Some(Type::Unit)`).
struct DovetailEnumShape {
    mangled_name: MangledName,
    /// (variant name in Dovetail enum order, concrete payload type if any,
    /// declared/def payload type if any — erased for generic enums).
    cases: Vec<(String, Option<Type>, Option<Type>)>,
}

impl<'a> super::FunctionEmitter<'a> {
    /// Emit the complete body of a WIT-import binding function. Parameters
    /// occupy locals starting at 0 (flattened per the Dovetail ABI); the body
    /// leaves the return value in `return_type`'s representation.
    pub(in crate::compiler::codegen) fn emit_wit_import_body(
        &mut self,
        wit_ref: &WitFuncRef,
        params: &[TypedParam],
        return_type: &Type,
    ) {
        let universe = self.codegen.wit_imports;

        // Dovetail param base locals (cumulative flattened widths).
        let mut param_bases = Vec::with_capacity(params.len());
        let mut next = 0u32;
        for param in params {
            param_bases.push(next);
            next += self.codegen.type_to_valtypes(&param.ty).len() as u32;
        }

        match &wit_ref.kind {
            WitFuncKind::ResourceDrop(_) => {
                let import_index = self
                    .codegen
                    .wit_registry
                    .func_index(wit_ref)
                    .expect("wit import must be registered");
                // drop(self): push the handle, call, produce Unit.
                self.instruction(Instruction::LocalGet(param_bases[0]));
                self.instruction(Instruction::Call(import_index));
                self.instruction(Instruction::I32Const(0));
            }
            WitFuncKind::Function(func_name) => {
                let import_index = self
                    .codegen
                    .wit_registry
                    .func_index(wit_ref)
                    .expect("wit import must be registered");
                // Scratch discipline: everything a sync body (and the host's
                // realloc during the call) allocates is reclaimed on return.
                let scratch = self.scratch_save();
                let interface = &universe.interfaces[wit_ref.interface_idx];
                let resolve = &universe.resolve;
                let iface = &resolve.interfaces[interface.interface_id];
                let func = &iface.functions[func_name.as_str()];
                let sig = resolve.wasm_signature(AbiVariant::GuestImport, func);
                assert!(
                    !sig.indirect_params,
                    "indirect params should be rejected at bindgen time"
                );
                let mut sizes = SizeAlign::default();
                sizes.fill(resolve);

                // Lower each param into freshly-allocated flat slot locals.
                let mut all_slots: Vec<u32> = Vec::new();
                assert_eq!(func.params.len(), params.len());
                for (wit_param, (param, base)) in
                    func.params.iter().zip(params.iter().zip(&param_bases))
                {
                    let wit_ty = &wit_param.ty;
                    let flat = flat_types(resolve, wit_ty);
                    let slot_types: Vec<ValType> = flat.iter().map(val_type).collect();
                    let slots: Vec<u32> = slot_types.iter().map(|vt| self.add_local(*vt)).collect();
                    let mut pos = 0usize;
                    self.wit_lower(
                        resolve,
                        &sizes,
                        wit_ty,
                        &param.ty,
                        *base,
                        &slots,
                        &slot_types,
                        &mut pos,
                    );
                    debug_assert_eq!(pos, slots.len());
                    all_slots.extend(slots);
                }

                // Return area for indirect results. wit-parser 0.255: a
                // function has at most one (anonymous) result.
                let result_ty = func.result;
                let retptr = if sig.retptr {
                    let size = sizes
                        .size(result_ty.as_ref().expect("retptr implies a result"))
                        .size_wasm32() as u32;
                    Some(self.wasi_bump_alloc(size))
                } else {
                    None
                };

                // Call: flat args, then retptr if any.
                for slot in &all_slots {
                    self.instruction(Instruction::LocalGet(*slot));
                }
                if let Some(retptr) = retptr {
                    self.instruction(Instruction::LocalGet(retptr));
                }
                self.instruction(Instruction::Call(import_index));

                // Lift the result.
                match (result_ty, retptr) {
                    (None, _) => self.instruction(Instruction::I32Const(0)), // Unit
                    (Some(ty), Some(retptr)) => {
                        self.wit_lift_load(resolve, &sizes, &ty, return_type, retptr, 0)
                    }
                    (Some(ty), None) => {
                        // Single flat result on the stack: spill and lift.
                        let vt = val_type(&sig.results[0]);
                        let tmp = self.add_local(vt);
                        self.instruction(Instruction::LocalSet(tmp));
                        self.wit_lift_scalar(resolve, &sizes, &ty, return_type, tmp);
                    }
                }
                self.scratch_restore(scratch);
            }
            WitFuncKind::AsyncStart(func_name) => {
                self.emit_async_start(wit_ref, func_name, params, &param_bases);
            }
            WitFuncKind::AsyncFinish(func_name) => {
                self.emit_async_finish(wit_ref, func_name, return_type, &param_bases);
            }
        }
    }

    /// `start` half of an async import: lower params (into persistent, leaked
    /// buffers — they must outlive the subtask), allocate a persistent result
    /// buffer, call `[async-lower]<fn>` and pack its status + buffer pointer
    /// into an `AsyncCall` (`(status << 32) | ptr`), left on the stack as i64.
    fn emit_async_start(
        &mut self,
        wit_ref: &WitFuncRef,
        func_name: &str,
        params: &[TypedParam],
        param_bases: &[u32],
    ) {
        let import_index = self
            .codegen
            .wit_registry
            .func_index(wit_ref)
            .expect("async-lower import must be registered");
        let universe = self.codegen.wit_imports;
        let interface = &universe.interfaces[wit_ref.interface_idx];
        let resolve = &universe.resolve;
        let iface = &resolve.interfaces[interface.interface_id];
        let func = &iface.functions[func_name];
        let mut sizes = SizeAlign::default();
        sizes.fill(resolve);

        // Two lifetimes here. Param buffers (string/list bytes) are read by the
        // host only during this call — the canonical ABI copies by-value params
        // as part of lowering — so they live in the scratch arena and a window
        // reclaims them the moment the call returns. The RESULT block is
        // different: the host writes it *after* the subtask completes and
        // `finish` reads it, so it must outlive the await. It comes from the
        // pinned heap (which grows) and is freed by `finish` — never the single
        // 64 KiB scratch page, which a per-call leak would exhaust.
        let scratch = self.scratch_save();

        let mut all_slots: Vec<u32> = Vec::new();
        assert_eq!(func.params.len(), params.len());
        for (wit_param, (param, base)) in func.params.iter().zip(params.iter().zip(param_bases)) {
            let wit_ty = &wit_param.ty;
            let flat = flat_types(resolve, wit_ty);
            let slot_types: Vec<ValType> = flat.iter().map(val_type).collect();
            let slots: Vec<u32> = slot_types.iter().map(|vt| self.add_local(*vt)).collect();
            let mut pos = 0usize;
            self.wit_lower(
                resolve,
                &sizes,
                wit_ty,
                &param.ty,
                *base,
                &slots,
                &slot_types,
                &mut pos,
            );
            all_slots.extend(slots);
        }

        let result_ptr = func.result.as_ref().map(|ty| {
            let size = sizes.size(ty).size_wasm32() as u32;
            self.emit_p3_pinned_retptr(size.max(1))
        });

        for slot in &all_slots {
            self.instruction(Instruction::LocalGet(*slot));
        }
        if let Some(rp) = result_ptr {
            self.instruction(Instruction::LocalGet(rp));
        }
        self.instruction(Instruction::Call(import_index)); // -> status i32
        let status = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalSet(status));

        // Params are copied; reclaim their scratch. The pinned result block is
        // not on the scratch list, so it survives this rewind.
        self.scratch_restore(scratch);

        // AsyncCall = (status << 32) | result_ptr.
        self.instruction(Instruction::LocalGet(status));
        self.instruction(Instruction::I64ExtendI32U);
        self.instruction(Instruction::I64Const(32));
        self.instruction(Instruction::I64Shl);
        if let Some(rp) = result_ptr {
            self.instruction(Instruction::LocalGet(rp));
            self.instruction(Instruction::I64ExtendI32U);
            self.instruction(Instruction::I64Or);
        }
    }

    /// `finish` half: recover the result buffer pointer from the `AsyncCall`
    /// (its low 32 bits) and lift the result into `return_type`. A unit result
    /// carries no buffer and lifts to Unit.
    fn emit_async_finish(
        &mut self,
        wit_ref: &WitFuncRef,
        func_name: &str,
        return_type: &Type,
        param_bases: &[u32],
    ) {
        let universe = self.codegen.wit_imports;
        let interface = &universe.interfaces[wit_ref.interface_idx];
        let resolve = &universe.resolve;
        let iface = &resolve.interfaces[interface.interface_id];
        let func = &iface.functions[func_name];
        let mut sizes = SizeAlign::default();
        sizes.fill(resolve);

        let scratch = self.scratch_save();
        let result_ptr = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalGet(param_bases[0])); // AsyncCall (i64)
        self.instruction(Instruction::I32WrapI64);
        self.instruction(Instruction::LocalSet(result_ptr));

        match func.result {
            Some(ty) => {
                self.wit_lift_load(resolve, &sizes, &ty, return_type, result_ptr, 0);
                // Free the pinned result block `start` allocated. The lifted
                // value is already on the stack; freeing the block leaves it.
                self.instruction(Instruction::LocalGet(result_ptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
            }
            None => self.instruction(Instruction::I32Const(0)), // Unit
        }
        self.scratch_restore(scratch);
    }

    // ------------------------------------------------------------------
    // Lowering: Dovetail value (locals at `base`) → flat slot locals
    // ------------------------------------------------------------------

    /// Write the next flat value (currently on the stack, of `native` type)
    /// into slot `pos`, converting to the slot's join type if needed.
    fn write_slot(
        &mut self,
        native: ValType,
        slots: &[u32],
        slot_types: &[ValType],
        pos: &mut usize,
    ) {
        let slot_ty = slot_types[*pos];
        match (native, slot_ty) {
            (a, b) if a == b => {}
            (ValType::I32, ValType::I64) => self.instruction(Instruction::I64ExtendI32U),
            (ValType::F32, ValType::I32) => self.instruction(Instruction::I32ReinterpretF32),
            (ValType::F32, ValType::I64) => {
                self.instruction(Instruction::I32ReinterpretF32);
                self.instruction(Instruction::I64ExtendI32U);
            }
            (ValType::F64, ValType::I64) => self.instruction(Instruction::I64ReinterpretF64),
            (a, b) => panic!("unsupported flat join conversion {a:?} -> {b:?}"),
        }
        self.instruction(Instruction::LocalSet(slots[*pos]));
        *pos += 1;
    }

    /// Native core type a WIT scalar lowers to.
    fn wit_scalar_val_type(ty: &WitType) -> ValType {
        match ty {
            WitType::U64 | WitType::S64 => ValType::I64,
            WitType::F32 => ValType::F32,
            WitType::F64 => ValType::F64,
            _ => ValType::I32,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn wit_lower(
        &mut self,
        resolve: &Resolve,
        sizes: &SizeAlign,
        wit_ty: &WitType,
        dovetail_ty: &Type,
        base: u32,
        slots: &[u32],
        slot_types: &[ValType],
        pos: &mut usize,
    ) {
        let wit_ty = resolve_alias(resolve, wit_ty);
        match wit_ty {
            WitType::Bool
            | WitType::U8
            | WitType::U16
            | WitType::U32
            | WitType::S8
            | WitType::S16
            | WitType::S32
            | WitType::U64
            | WitType::S64
            | WitType::F32
            | WitType::F64
            | WitType::Char => {
                self.instruction(Instruction::LocalGet(base));
                self.write_slot(Self::wit_scalar_val_type(wit_ty), slots, slot_types, pos);
            }
            WitType::String => {
                let (ptr, len) = self.wasi_marshal_string_to_memory(base);
                self.instruction(Instruction::LocalGet(ptr));
                self.write_slot(ValType::I32, slots, slot_types, pos);
                self.instruction(Instruction::LocalGet(len));
                self.write_slot(ValType::I32, slots, slot_types, pos);
            }
            WitType::ErrorContext => {
                unreachable!("error-context is rejected at bindgen time")
            }
            WitType::Id(id) => match &resolve.types[*id].kind {
                TypeDefKind::Resource => unreachable!("bare resource type in signature"),
                TypeDefKind::Handle(_) | TypeDefKind::Flags(_) => {
                    // Handle newtype / flags newtype: an i32/u32 scalar.
                    self.instruction(Instruction::LocalGet(base));
                    self.write_slot(ValType::I32, slots, slot_types, pos);
                }
                TypeDefKind::List(elem) => {
                    let (ptr, len) =
                        self.wit_lower_list_to_memory(resolve, sizes, elem, dovetail_ty, base);
                    self.instruction(Instruction::LocalGet(ptr));
                    self.write_slot(ValType::I32, slots, slot_types, pos);
                    self.instruction(Instruction::LocalGet(len));
                    self.write_slot(ValType::I32, slots, slot_types, pos);
                }
                TypeDefKind::Tuple(t) => {
                    // Dovetail tuple: leaves already flattened in consecutive locals.
                    let Type::Tuple(elems, _) = strip_newtype(dovetail_ty) else {
                        panic!("wit tuple must map to a Dovetail tuple, got {dovetail_ty}")
                    };
                    let mut elem_base = base;
                    for (wit_elem, dovetail_elem) in t.types.iter().zip(elems) {
                        self.wit_lower(
                            resolve,
                            sizes,
                            wit_elem,
                            dovetail_elem,
                            elem_base,
                            slots,
                            slot_types,
                            pos,
                        );
                        elem_base += self.codegen.type_to_valtypes(dovetail_elem).len() as u32;
                    }
                }
                TypeDefKind::Record(record) => {
                    let (struct_idx, field_info) = self.dovetail_record_info(dovetail_ty);
                    assert_eq!(record.fields.len(), field_info.len());
                    for (wit_field, (field_ty, flat_base)) in record.fields.iter().zip(&field_info)
                    {
                        // Materialize the field into fresh locals.
                        let vts = self.codegen.type_to_valtypes(field_ty);
                        let tmp = self.add_value_locals(&vts);
                        for (k, _) in vts.iter().enumerate() {
                            self.instruction(Instruction::LocalGet(base));
                            self.instruction(Instruction::StructGet {
                                struct_type_index: struct_idx,
                                field_index: flat_base + k as u32,
                            });
                            self.instruction(Instruction::LocalSet(tmp + k as u32));
                        }
                        self.wit_lower(
                            resolve,
                            sizes,
                            &wit_field.ty,
                            &field_ty.clone(),
                            tmp,
                            slots,
                            slot_types,
                            pos,
                        );
                    }
                }
                TypeDefKind::Enum(_)
                | TypeDefKind::Variant(_)
                | TypeDefKind::Option(_)
                | TypeDefKind::Result(_) => {
                    let shape = variant_shape(resolve, wit_ty).unwrap();
                    let dovetail_shape = self.dovetail_enum_shape(dovetail_ty, &shape);
                    let saved_src = self.wit_variant_src.replace(base);
                    let start = *pos;
                    let total_width = flat_types(resolve, wit_ty).len();
                    for (i, (case_ty, (variant_name, concrete_payload, def_payload))) in
                        shape.cases.iter().zip(&dovetail_shape.cases).enumerate()
                    {
                        let variant_idx = self.codegen.variant_type_indices
                            [&(dovetail_shape.mangled_name.clone(), variant_name.clone())];
                        self.instruction(Instruction::LocalGet(base));
                        self.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
                            variant_idx,
                        )));
                        self.instruction(Instruction::If(BlockType::Empty));
                        {
                            let mut case_pos = start;
                            self.instruction(Instruction::I32Const(i as i32));
                            self.write_slot(ValType::I32, slots, slot_types, &mut case_pos);
                            if let (Some(case_wit_ty), Some(concrete)) = (case_ty, concrete_payload)
                            {
                                let def = def_payload.as_ref().unwrap();
                                let tmp =
                                    self.materialize_variant_payload(variant_idx, def, concrete);
                                self.wit_lower(
                                    resolve,
                                    sizes,
                                    case_wit_ty,
                                    concrete,
                                    tmp,
                                    slots,
                                    slot_types,
                                    &mut case_pos,
                                );
                            }
                        }
                        self.instruction(Instruction::End);
                    }
                    *pos = start + total_width;
                    self.wit_variant_src = saved_src;
                }
                TypeDefKind::Type(_) => unreachable!("aliases resolved above"),
                other => panic!("unsupported WIT type in lowering: {other:?}"),
            },
        }
    }

    /// Serialize a Dovetail `Array<T>` into linear memory as a canonical list.
    /// Returns `(ptr_local, len_local)`.
    fn wit_lower_list_to_memory(
        &mut self,
        resolve: &Resolve,
        sizes: &SizeAlign,
        elem_wit: &WitType,
        dovetail_ty: &Type,
        base: u32,
    ) -> (u32, u32) {
        let Type::Array(elem_dovetail) = strip_newtype(dovetail_ty) else {
            panic!("wit list must map to a Dovetail Array, got {dovetail_ty}")
        };
        let elem_dovetail = elem_dovetail.as_ref();
        let elem_size = sizes.size(elem_wit).size_wasm32() as u32;
        let elem_align = sizes.align(elem_wit).align_wasm32() as u32;

        // len = array.len
        let len = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalGet(base));
        self.instruction(Instruction::ArrayLen);
        self.instruction(Instruction::LocalSet(len));

        // ptr = aligned bump-alloc of len * elem_size
        let ptr = self.add_local(ValType::I32);
        self.instruction(Instruction::GlobalGet(0));
        self.instruction(Instruction::I32Const(elem_align.max(1) as i32 - 1));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::I32Const(-(elem_align.max(1) as i32)));
        self.instruction(Instruction::I32And);
        self.instruction(Instruction::LocalTee(ptr));
        self.instruction(Instruction::LocalGet(len));
        self.instruction(Instruction::I32Const(elem_size as i32));
        self.instruction(Instruction::I32Mul);
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::GlobalSet(0));

        // Loop: store each element at ptr + i*elem_size.
        let idx = self.add_local(ValType::I32);
        let elem_ptr = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(idx));
        self.instruction(Instruction::Block(BlockType::Empty));
        self.instruction(Instruction::Loop(BlockType::Empty));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::LocalGet(len));
        self.instruction(Instruction::I32GeU);
        self.instruction(Instruction::BrIf(1));

        self.instruction(Instruction::LocalGet(ptr));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Const(elem_size as i32));
        self.instruction(Instruction::I32Mul);
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(elem_ptr));

        // Materialize element into locals, then store into memory.
        let tmp = self.materialize_array_element(base, idx, elem_dovetail);
        self.wit_lower_store(resolve, sizes, elem_wit, elem_dovetail, tmp, elem_ptr, 0);

        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(idx));
        self.instruction(Instruction::Br(0));
        self.instruction(Instruction::End);
        self.instruction(Instruction::End);

        (ptr, len)
    }

    /// Store a Dovetail value (locals at `base`) into linear memory at
    /// `ptr_local + offset` per the canonical ABI layout.
    #[allow(clippy::too_many_arguments)]
    fn wit_lower_store(
        &mut self,
        resolve: &Resolve,
        sizes: &SizeAlign,
        wit_ty: &WitType,
        dovetail_ty: &Type,
        base: u32,
        ptr_local: u32,
        offset: u32,
    ) {
        let wit_ty = resolve_alias(resolve, wit_ty);
        match wit_ty {
            WitType::Bool | WitType::U8 | WitType::S8 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::LocalGet(base));
                self.instruction(Instruction::I32Store8(mem_arg(offset)));
            }
            WitType::U16 | WitType::S16 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::LocalGet(base));
                self.instruction(Instruction::I32Store16(mem_arg(offset)));
            }
            WitType::U32 | WitType::S32 | WitType::Char => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::LocalGet(base));
                self.instruction(Instruction::I32Store(mem_arg(offset)));
            }
            WitType::U64 | WitType::S64 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::LocalGet(base));
                self.instruction(Instruction::I64Store(mem_arg(offset)));
            }
            WitType::F32 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::LocalGet(base));
                self.instruction(Instruction::F32Store(mem_arg(offset)));
            }
            WitType::F64 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::LocalGet(base));
                self.instruction(Instruction::F64Store(mem_arg(offset)));
            }
            WitType::String => {
                let (ptr, len) = self.wasi_marshal_string_to_memory(base);
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::LocalGet(ptr));
                self.instruction(Instruction::I32Store(mem_arg(offset)));
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::LocalGet(len));
                self.instruction(Instruction::I32Store(mem_arg(offset + 4)));
            }
            WitType::ErrorContext => {
                unreachable!("error-context is rejected at bindgen time")
            }
            WitType::Id(id) => match &resolve.types[*id].kind {
                TypeDefKind::Handle(_) | TypeDefKind::Flags(_) => {
                    self.instruction(Instruction::LocalGet(ptr_local));
                    self.instruction(Instruction::LocalGet(base));
                    self.instruction(Instruction::I32Store(mem_arg(offset)));
                }
                TypeDefKind::List(elem) => {
                    let (ptr, len) =
                        self.wit_lower_list_to_memory(resolve, sizes, elem, dovetail_ty, base);
                    self.instruction(Instruction::LocalGet(ptr_local));
                    self.instruction(Instruction::LocalGet(ptr));
                    self.instruction(Instruction::I32Store(mem_arg(offset)));
                    self.instruction(Instruction::LocalGet(ptr_local));
                    self.instruction(Instruction::LocalGet(len));
                    self.instruction(Instruction::I32Store(mem_arg(offset + 4)));
                }
                TypeDefKind::Tuple(t) => {
                    let Type::Tuple(elems, _) = strip_newtype(dovetail_ty) else {
                        panic!("wit tuple must map to a Dovetail tuple")
                    };
                    let offsets = sizes.field_offsets(t.types.iter());
                    let mut elem_base = base;
                    for ((field_offset, wit_elem), dovetail_elem) in offsets.iter().zip(elems) {
                        self.wit_lower_store(
                            resolve,
                            sizes,
                            wit_elem,
                            dovetail_elem,
                            elem_base,
                            ptr_local,
                            offset + field_offset.size_wasm32() as u32,
                        );
                        elem_base += self.codegen.type_to_valtypes(dovetail_elem).len() as u32;
                    }
                }
                TypeDefKind::Record(record) => {
                    let (struct_idx, field_info) = self.dovetail_record_info(dovetail_ty);
                    let offsets = sizes.field_offsets(record.fields.iter().map(|f| &f.ty));
                    for ((field_offset, wit_field_ty), (field_ty, flat_base)) in
                        offsets.iter().zip(&field_info)
                    {
                        let vts = self.codegen.type_to_valtypes(field_ty);
                        let tmp = self.add_value_locals(&vts);
                        for (k, _) in vts.iter().enumerate() {
                            self.instruction(Instruction::LocalGet(base));
                            self.instruction(Instruction::StructGet {
                                struct_type_index: struct_idx,
                                field_index: flat_base + k as u32,
                            });
                            self.instruction(Instruction::LocalSet(tmp + k as u32));
                        }
                        self.wit_lower_store(
                            resolve,
                            sizes,
                            wit_field_ty,
                            &field_ty.clone(),
                            tmp,
                            ptr_local,
                            offset + field_offset.size_wasm32() as u32,
                        );
                    }
                }
                TypeDefKind::Enum(_)
                | TypeDefKind::Variant(_)
                | TypeDefKind::Option(_)
                | TypeDefKind::Result(_) => {
                    let shape = variant_shape(resolve, wit_ty).unwrap();
                    let dovetail_shape = self.dovetail_enum_shape(dovetail_ty, &shape);
                    let saved_src = self.wit_variant_src.replace(base);
                    let payload_offset = sizes
                        .payload_offset(shape.tag, shape.cases.iter().map(|c| c.as_ref()))
                        .size_wasm32() as u32;
                    for (i, (case_ty, (variant_name, concrete_payload, def_payload))) in
                        shape.cases.iter().zip(&dovetail_shape.cases).enumerate()
                    {
                        let variant_idx = self.codegen.variant_type_indices
                            [&(dovetail_shape.mangled_name.clone(), variant_name.clone())];
                        self.instruction(Instruction::LocalGet(base));
                        self.instruction(Instruction::RefTestNonNull(HeapType::Concrete(
                            variant_idx,
                        )));
                        self.instruction(Instruction::If(BlockType::Empty));
                        {
                            // Store discriminant.
                            self.instruction(Instruction::LocalGet(ptr_local));
                            self.instruction(Instruction::I32Const(i as i32));
                            match tag_bytes(shape.tag) {
                                1 => self.instruction(Instruction::I32Store8(mem_arg(offset))),
                                2 => self.instruction(Instruction::I32Store16(mem_arg(offset))),
                                _ => self.instruction(Instruction::I32Store(mem_arg(offset))),
                            }
                            if let (Some(case_wit_ty), Some(concrete)) = (case_ty, concrete_payload)
                            {
                                let def = def_payload.as_ref().unwrap();
                                let tmp =
                                    self.materialize_variant_payload(variant_idx, def, concrete);
                                self.wit_lower_store(
                                    resolve,
                                    sizes,
                                    case_wit_ty,
                                    concrete,
                                    tmp,
                                    ptr_local,
                                    offset + payload_offset,
                                );
                            }
                        }
                        self.instruction(Instruction::End);
                    }
                    self.wit_variant_src = saved_src;
                }
                TypeDefKind::Type(_) => unreachable!("aliases resolved above"),
                other => panic!("unsupported WIT type in store lowering: {other:?}"),
            },
        }
    }

    // ------------------------------------------------------------------
    // Lifting: linear memory / flat result → Dovetail GC value on the stack
    // ------------------------------------------------------------------

    /// Lift a value from `ptr_local + offset`, leaving the Dovetail value on
    /// the stack (flattened for tuples).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn wit_lift_load(
        &mut self,
        resolve: &Resolve,
        sizes: &SizeAlign,
        wit_ty: &WitType,
        dovetail_ty: &Type,
        ptr_local: u32,
        offset: u32,
    ) {
        let wit_ty = resolve_alias(resolve, wit_ty);
        match wit_ty {
            WitType::Bool | WitType::U8 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::I32Load8U(mem_arg(offset)));
            }
            WitType::S8 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::I32Load8S(mem_arg(offset)));
            }
            WitType::U16 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::I32Load16U(mem_arg(offset)));
            }
            WitType::S16 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::I32Load16S(mem_arg(offset)));
            }
            WitType::U32 | WitType::S32 | WitType::Char => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::I32Load(mem_arg(offset)));
            }
            WitType::U64 | WitType::S64 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::I64Load(mem_arg(offset)));
            }
            WitType::F32 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::F32Load(mem_arg(offset)));
            }
            WitType::F64 => {
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::F64Load(mem_arg(offset)));
            }
            WitType::String => {
                let data_ptr = self.add_local(ValType::I32);
                let data_len = self.add_local(ValType::I32);
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::I32Load(mem_arg(offset)));
                self.instruction(Instruction::LocalSet(data_ptr));
                self.instruction(Instruction::LocalGet(ptr_local));
                self.instruction(Instruction::I32Load(mem_arg(offset + 4)));
                self.instruction(Instruction::LocalSet(data_len));
                self.wasi_create_string_from_memory(data_ptr, data_len);
            }
            WitType::ErrorContext => {
                unreachable!("error-context is rejected at bindgen time")
            }
            WitType::Id(id) => match &resolve.types[*id].kind {
                TypeDefKind::Handle(_) | TypeDefKind::Flags(_) => {
                    self.instruction(Instruction::LocalGet(ptr_local));
                    self.instruction(Instruction::I32Load(mem_arg(offset)));
                }
                TypeDefKind::List(elem) => {
                    let data_ptr = self.add_local(ValType::I32);
                    let data_len = self.add_local(ValType::I32);
                    self.instruction(Instruction::LocalGet(ptr_local));
                    self.instruction(Instruction::I32Load(mem_arg(offset)));
                    self.instruction(Instruction::LocalSet(data_ptr));
                    self.instruction(Instruction::LocalGet(ptr_local));
                    self.instruction(Instruction::I32Load(mem_arg(offset + 4)));
                    self.instruction(Instruction::LocalSet(data_len));
                    self.wit_lift_list(resolve, sizes, elem, dovetail_ty, data_ptr, data_len);
                }
                TypeDefKind::Tuple(t) => {
                    let Type::Tuple(elems, _) = strip_newtype(dovetail_ty) else {
                        panic!("wit tuple must map to a Dovetail tuple")
                    };
                    let offsets = sizes.field_offsets(t.types.iter());
                    for ((field_offset, wit_elem), dovetail_elem) in offsets.iter().zip(elems) {
                        self.wit_lift_load(
                            resolve,
                            sizes,
                            wit_elem,
                            dovetail_elem,
                            ptr_local,
                            offset + field_offset.size_wasm32() as u32,
                        );
                    }
                }
                TypeDefKind::Record(record) => {
                    let (struct_idx, field_info) = self.dovetail_record_info(dovetail_ty);
                    let offsets = sizes.field_offsets(record.fields.iter().map(|f| &f.ty));
                    for ((field_offset, wit_field_ty), (field_ty, _)) in
                        offsets.iter().zip(&field_info)
                    {
                        self.wit_lift_load(
                            resolve,
                            sizes,
                            wit_field_ty,
                            &field_ty.clone(),
                            ptr_local,
                            offset + field_offset.size_wasm32() as u32,
                        );
                    }
                    self.emit_nominal_struct_new(strip_newtype(dovetail_ty), struct_idx);
                }
                TypeDefKind::Enum(_)
                | TypeDefKind::Variant(_)
                | TypeDefKind::Option(_)
                | TypeDefKind::Result(_) => {
                    let shape = variant_shape(resolve, wit_ty).unwrap();
                    let payload_offset = sizes
                        .payload_offset(shape.tag, shape.cases.iter().map(|c| c.as_ref()))
                        .size_wasm32() as u32;
                    let disc = self.add_local(ValType::I32);
                    self.instruction(Instruction::LocalGet(ptr_local));
                    match tag_bytes(shape.tag) {
                        1 => self.instruction(Instruction::I32Load8U(mem_arg(offset))),
                        2 => self.instruction(Instruction::I32Load16U(mem_arg(offset))),
                        _ => self.instruction(Instruction::I32Load(mem_arg(offset))),
                    }
                    self.instruction(Instruction::LocalSet(disc));
                    self.wit_lift_variant_from(
                        resolve,
                        sizes,
                        wit_ty,
                        dovetail_ty,
                        disc,
                        Some((ptr_local, offset + payload_offset)),
                    );
                }
                TypeDefKind::Type(_) => unreachable!("aliases resolved above"),
                other => panic!("unsupported WIT type in lifting: {other:?}"),
            },
        }
    }

    /// Lift a single-flat-value result: the value has been spilled to
    /// `scalar_local`. Handles every WIT type that flattens to width 1.
    fn wit_lift_scalar(
        &mut self,
        resolve: &Resolve,
        sizes: &SizeAlign,
        wit_ty: &WitType,
        dovetail_ty: &Type,
        scalar_local: u32,
    ) {
        let wit_ty = resolve_alias(resolve, wit_ty);
        match wit_ty {
            WitType::Bool => {
                // Normalize to 0/1.
                self.instruction(Instruction::LocalGet(scalar_local));
                self.instruction(Instruction::I32Eqz);
                self.instruction(Instruction::I32Eqz);
            }
            WitType::U8 => {
                self.instruction(Instruction::LocalGet(scalar_local));
                self.instruction(Instruction::I32Const(0xff));
                self.instruction(Instruction::I32And);
            }
            WitType::S8 => {
                self.instruction(Instruction::LocalGet(scalar_local));
                self.instruction(Instruction::I32Extend8S);
            }
            WitType::U16 => {
                self.instruction(Instruction::LocalGet(scalar_local));
                self.instruction(Instruction::I32Const(0xffff));
                self.instruction(Instruction::I32And);
            }
            WitType::S16 => {
                self.instruction(Instruction::LocalGet(scalar_local));
                self.instruction(Instruction::I32Extend16S);
            }
            WitType::U32
            | WitType::S32
            | WitType::Char
            | WitType::U64
            | WitType::S64
            | WitType::F32
            | WitType::F64 => {
                self.instruction(Instruction::LocalGet(scalar_local));
            }
            WitType::String => panic!("string never flattens to a single value"),
            WitType::ErrorContext => {
                unreachable!("error-context is rejected at bindgen time")
            }
            WitType::Id(id) => match &resolve.types[*id].kind {
                TypeDefKind::Handle(_) | TypeDefKind::Flags(_) => {
                    self.instruction(Instruction::LocalGet(scalar_local));
                }
                TypeDefKind::Enum(_)
                | TypeDefKind::Variant(_)
                | TypeDefKind::Option(_)
                | TypeDefKind::Result(_) => {
                    // Width-1 variant: all cases are payload-free.
                    self.wit_lift_variant_from(
                        resolve,
                        sizes,
                        wit_ty,
                        dovetail_ty,
                        scalar_local,
                        None,
                    );
                }
                TypeDefKind::Record(record) => {
                    // Width-1 record: exactly one flattened scalar field.
                    let (struct_idx, field_info) = self.dovetail_record_info(dovetail_ty);
                    assert_eq!(field_info.len(), 1, "width-1 record has one field");
                    let (field_ty, _) = &field_info[0];
                    self.wit_lift_scalar(
                        resolve,
                        sizes,
                        &record.fields[0].ty,
                        &field_ty.clone(),
                        scalar_local,
                    );
                    self.emit_nominal_struct_new(strip_newtype(dovetail_ty), struct_idx);
                }
                TypeDefKind::Tuple(t) => {
                    let Type::Tuple(elems, _) = strip_newtype(dovetail_ty) else {
                        panic!("wit tuple must map to a Dovetail tuple")
                    };
                    assert_eq!(t.types.len(), 1, "width-1 tuple has one element");
                    self.wit_lift_scalar(resolve, sizes, &t.types[0], &elems[0], scalar_local);
                }
                other => panic!("unsupported width-1 WIT type in lifting: {other:?}"),
            },
        }
    }

    /// Construct a Dovetail enum value from a discriminant local, lifting the
    /// selected case's payload from memory when `payload_src` is given.
    fn wit_lift_variant_from(
        &mut self,
        resolve: &Resolve,
        sizes: &SizeAlign,
        wit_ty: &WitType,
        dovetail_ty: &Type,
        disc_local: u32,
        payload_src: Option<(u32, u32)>,
    ) {
        let shape = variant_shape(resolve, wit_ty).unwrap();
        let dovetail_shape = self.dovetail_enum_shape(dovetail_ty, &shape);
        let base_idx = self.codegen.type_indices[&dovetail_shape.mangled_name];

        // Result temp: nullable ref to the enum base, RefAsNonNull at the end
        // (an out-of-range discriminant traps there).
        let result_tmp = self.add_local(ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::Concrete(base_idx),
        }));

        for (i, (case_ty, (variant_name, concrete_payload, def_payload))) in
            shape.cases.iter().zip(&dovetail_shape.cases).enumerate()
        {
            let variant_idx = self.codegen.variant_type_indices
                [&(dovetail_shape.mangled_name.clone(), variant_name.clone())];
            self.instruction(Instruction::LocalGet(disc_local));
            self.instruction(Instruction::I32Const(i as i32));
            self.instruction(Instruction::I32Eq);
            self.instruction(Instruction::If(BlockType::Empty));
            {
                match (case_ty, concrete_payload) {
                    (Some(case_wit_ty), Some(concrete)) => {
                        let (ptr_local, offset) =
                            payload_src.expect("payload case requires a memory source");
                        self.wit_lift_load(
                            resolve,
                            sizes,
                            case_wit_ty,
                            concrete,
                            ptr_local,
                            offset,
                        );
                        self.box_payload_if_erased(def_payload.as_ref().unwrap(), concrete);
                    }
                    (None, Some(concrete)) => {
                        // Unit payload (bare result ok/err side): construct Unit.
                        debug_assert!(matches!(concrete, Type::Unit));
                        self.instruction(Instruction::I32Const(0));
                        self.box_payload_if_erased(def_payload.as_ref().unwrap(), concrete);
                    }
                    (_, None) => {}
                }
                self.emit_nominal_struct_new(strip_newtype(dovetail_ty), variant_idx);
                self.instruction(Instruction::LocalSet(result_tmp));
            }
            self.instruction(Instruction::End);
        }
        self.instruction(Instruction::LocalGet(result_tmp));
        self.instruction(Instruction::RefAsNonNull);
    }

    /// Lift a canonical list from `(data_ptr, data_len)` into a Dovetail
    /// `Array<T>`, leaving the array ref on the stack.
    fn wit_lift_list(
        &mut self,
        resolve: &Resolve,
        sizes: &SizeAlign,
        elem_wit: &WitType,
        dovetail_ty: &Type,
        data_ptr: u32,
        data_len: u32,
    ) {
        let Type::Array(elem_dovetail) = strip_newtype(dovetail_ty) else {
            panic!("wit list must map to a Dovetail Array, got {dovetail_ty}")
        };
        let elem_dovetail = elem_dovetail.as_ref();

        // Fast path: list<u8>-shaped elements use the existing byte copier.
        if matches!(resolve_alias(resolve, elem_wit), WitType::U8 | WitType::S8) {
            self.wasi_create_u8_array_from_bytes(data_ptr, data_len);
            return;
        }

        let elem_size = sizes.size(elem_wit).size_wasm32() as u32;
        let array_type_index = self.codegen.array_type_index(elem_dovetail);
        let elem_vt = self.codegen.single_val_type(elem_dovetail);

        // Allocate the array with a default element value.
        let array_ref = ValType::Ref(RefType {
            nullable: false,
            heap_type: HeapType::Concrete(array_type_index),
        });
        let array_local = self.add_local(array_ref);
        match elem_vt {
            ValType::I32 => self.instruction(Instruction::I32Const(0)),
            ValType::I64 => self.instruction(Instruction::I64Const(0)),
            ValType::F32 => self.instruction(Instruction::F32Const((0.0f32).into())),
            ValType::F64 => self.instruction(Instruction::F64Const((0.0f64).into())),
            ValType::Ref(_) => {
                // `$Array$ref` stores non-null `(ref any)`: use a boxed Unit
                // as the placeholder fill value (overwritten below).
                self.instruction(Instruction::I32Const(0));
                self.instruction(Instruction::StructNew(super::super::BOX_UNIT_TYPE_INDEX));
            }
            other => panic!("unsupported array element valtype {other:?}"),
        }
        self.instruction(Instruction::LocalGet(data_len));
        self.instruction(Instruction::ArrayNew(array_type_index));
        self.instruction(Instruction::LocalSet(array_local));

        // Loop: array[i] = lift(memory[data_ptr + i*elem_size])
        let idx = self.add_local(ValType::I32);
        let elem_ptr = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(idx));
        self.instruction(Instruction::Block(BlockType::Empty));
        self.instruction(Instruction::Loop(BlockType::Empty));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::LocalGet(data_len));
        self.instruction(Instruction::I32GeU);
        self.instruction(Instruction::BrIf(1));

        self.instruction(Instruction::LocalGet(data_ptr));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Const(elem_size as i32));
        self.instruction(Instruction::I32Mul);
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(elem_ptr));

        self.instruction(Instruction::LocalGet(array_local));
        self.instruction(Instruction::LocalGet(idx));
        // Tuples never appear as array elements here (Dovetail arrays store one
        // slot per element; a tuple element would be boxed) — reject for now.
        assert!(
            !self.codegen.is_tuple(elem_dovetail),
            "list of tuples is not supported yet"
        );
        self.wit_lift_load(resolve, sizes, elem_wit, elem_dovetail, elem_ptr, 0);
        self.instruction(Instruction::ArraySet(array_type_index));

        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(idx));
        self.instruction(Instruction::Br(0));
        self.instruction(Instruction::End);
        self.instruction(Instruction::End);

        self.instruction(Instruction::LocalGet(array_local));
    }

    // ------------------------------------------------------------------
    // Dovetail-side helpers
    // ------------------------------------------------------------------

    /// Struct type index + per-field (type, flattened field base) for a
    /// Dovetail record type.
    fn dovetail_record_info(&self, dovetail_ty: &Type) -> (u32, Vec<(Type, u32)>) {
        let mn = match strip_newtype(dovetail_ty) {
            Type::Record(_, mn)
            | Type::GenericRecord {
                mangled_name: mn, ..
            } => mn.clone(),
            other => panic!("wit record must map to a Dovetail record, got {other}"),
        };
        let struct_idx = self.codegen.type_indices[&mn];
        let TypeDef::Record(def) = &self.codegen.typed_module.types[&mn] else {
            panic!("record TypeDef expected")
        };
        let mut info = Vec::with_capacity(def.fields.len());
        let mut flat_base = 0u32;
        for (_, field_ty) in &def.fields {
            info.push((field_ty.clone(), flat_base));
            flat_base += self.codegen.type_to_valtypes(field_ty).len() as u32;
        }
        (struct_idx, info)
    }

    /// Dovetail enum shape for a WIT variant-like type: variant names in
    /// declaration order plus concrete and declared (possibly erased) payload
    /// types. For option/result the concrete payloads come from the
    /// instantiation's type args; a missing wit-side payload maps to Unit.
    fn dovetail_enum_shape(&self, dovetail_ty: &Type, shape: &VariantShape) -> DovetailEnumShape {
        let (mn, type_args): (MangledName, Option<Vec<Type>>) = match strip_newtype(dovetail_ty) {
            Type::Enum(_, mn) => (mn.clone(), None),
            Type::GenericEnum {
                mangled_name: mn,
                type_args,
                ..
            } => (
                mn.clone(),
                Some(type_args.iter().map(|(_, t)| t.clone()).collect()),
            ),
            other => panic!("wit variant must map to a Dovetail enum, got {other}"),
        };
        let TypeDef::Enum(def) = &self.codegen.typed_module.types[&mn] else {
            panic!("enum TypeDef expected")
        };
        assert_eq!(
            def.variants.len(),
            shape.cases.len(),
            "variant count mismatch between WIT and Dovetail enum"
        );
        // WIT-discriminant-ordered view of the Dovetail variants: by name for
        // option/result (whose Dovetail declaration order differs), positional
        // for generated enums (bindgen emits variants in WIT order).
        let ordered: Vec<&crate::typechecker::types::EnumVariantDef> = match &shape.dovetail_names {
            Some(names) => names
                .iter()
                .map(|n| {
                    def.variants
                        .iter()
                        .find(|v| v.name == *n)
                        .expect("Option/Result variant by name")
                })
                .collect(),
            None => def.variants.iter().collect(),
        };
        let cases = ordered
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let def_payload = v.payload_types.first().cloned();
                let concrete = match (&type_args, &def_payload) {
                    // Generic instantiation (Option/Result): payload = the
                    // matching type arg. Option: Some -> arg0; Result:
                    // Ok -> arg0, Error -> arg1.
                    (Some(args), Some(_)) => {
                        let arg_idx = match v.name.as_str() {
                            "Some" | "Ok" => 0,
                            "Error" => 1,
                            _ => 0,
                        };
                        Some(args[arg_idx].clone())
                    }
                    (None, Some(p)) => Some(p.clone()),
                    (_, None) => {
                        // Wit-side payload with no Dovetail payload can't
                        // happen (bindgen generates payloads 1:1); a wit-side
                        // *missing* payload for option/result maps to Unit
                        // only when Dovetail has one (Ok/Error of Unit).
                        None
                    }
                };
                // For option/result with a bare side (`result<_, s32>`), the
                // wit case has no payload but Dovetail's Ok/Error carries Unit.
                let concrete = match (&shape.cases[i], concrete) {
                    (None, Some(_)) => Some(Type::Unit),
                    (_, c) => c,
                };
                (v.name.clone(), concrete, def_payload)
            })
            .collect();
        DovetailEnumShape {
            mangled_name: mn,
            cases,
        }
    }

    /// Read a variant's payload out of the (already type-tested) enum value
    /// at local 0 of the surrounding lower — actually from the value in the
    /// enclosing `base` local: cast to the variant struct, extract the
    /// payload (unboxing erased slots), and materialize it into fresh
    /// locals. Returns the base local of the materialized payload.
    ///
    /// Precondition: the enum ref is on top of the *type test* — we re-load
    /// it from the base local captured by the caller via `LocalGet` inside
    /// this helper's instruction stream; the caller must have the enum value
    /// in a local and pass its index via `self.wit_variant_src`.
    fn materialize_variant_payload(
        &mut self,
        variant_idx: u32,
        def_payload: &Type,
        concrete: &Type,
    ) -> u32 {
        // The enum value local is recorded by the caller just before the
        // dispatch loop via `wit_variant_src`.
        let src = self
            .wit_variant_src
            .expect("variant payload materialization requires src local");
        let vts = self.codegen.type_to_valtypes(concrete);
        let tmp = self.add_value_locals(&vts);

        if super::super::Codegen::is_erased_slot(def_payload)
            || matches!(
                def_payload,
                Type::TypeVariable(_, _) | Type::GenericParam(_, _, _)
            )
        {
            // Erased single anyref slot: cast + unbox to the concrete type,
            // which leaves the flattened value on the stack.
            self.instruction(Instruction::LocalGet(src));
            self.instruction(Instruction::RefCastNonNull(HeapType::Concrete(variant_idx)));
            self.instruction(Instruction::StructGet {
                struct_type_index: variant_idx,
                field_index: self.codegen.variant_payload_prefix(variant_idx),
            });
            self.cast_back_from_erased(def_payload, concrete);
            self.store_value(tmp, &vts);
        } else {
            // Concrete flattened fields.
            for k in 0..vts.len() as u32 {
                self.instruction(Instruction::LocalGet(src));
                self.instruction(Instruction::RefCastNonNull(HeapType::Concrete(variant_idx)));
                self.instruction(Instruction::StructGet {
                    struct_type_index: variant_idx,
                    field_index: self.codegen.variant_payload_prefix(variant_idx) + k,
                });
                self.instruction(Instruction::LocalSet(tmp + k));
            }
        }
        tmp
    }

    /// Box a lifted payload if the Dovetail enum's declared slot is erased
    /// (generic Option/Result instantiations store `anyref`).
    fn box_payload_if_erased(&mut self, def_payload: &Type, concrete: &Type) {
        if matches!(
            def_payload,
            Type::TypeVariable(_, _) | Type::GenericParam(_, _, _)
        ) || super::super::Codegen::is_erased_slot(def_payload)
        {
            self.box_value_for_erased_slot(concrete);
        }
    }

    /// Materialize one array element (`array_local[idx_local]`) into fresh
    /// locals matching the element's Dovetail representation. Returns the base.
    fn materialize_array_element(
        &mut self,
        array_local: u32,
        idx_local: u32,
        elem_dovetail: &Type,
    ) -> u32 {
        assert!(
            !self.codegen.is_tuple(elem_dovetail),
            "list of tuples is not supported yet"
        );
        let array_type_index = self.codegen.array_type_index(elem_dovetail);
        let elem_vt = self.codegen.single_val_type(elem_dovetail);
        let tmp = self.add_value_locals(&[elem_vt]);
        self.instruction(Instruction::LocalGet(array_local));
        self.instruction(Instruction::LocalGet(idx_local));
        match elem_dovetail {
            Type::Int8 => self.instruction(Instruction::ArrayGetS(array_type_index)),
            Type::Uint8 => self.instruction(Instruction::ArrayGetU(array_type_index)),
            Type::Int16 => self.instruction(Instruction::ArrayGetS(array_type_index)),
            Type::Uint16 => self.instruction(Instruction::ArrayGetU(array_type_index)),
            _ => self.instruction(Instruction::ArrayGet(array_type_index)),
        }
        // Reference elements are stored as `ref null any`; cast back.
        if let ValType::Ref(rt) = elem_vt {
            self.instruction(Instruction::RefCastNonNull(rt.heap_type));
        }
        self.instruction(Instruction::LocalSet(tmp));
        tmp
    }
}

/// Transparently unwrap newtypes (bindgen never wraps composites in
/// newtypes, but resources/flags are newtypes over scalars).
fn strip_newtype(ty: &Type) -> &Type {
    match ty {
        Type::Newtype(_, inner) => strip_newtype(inner),
        Type::GenericNewtype {
            concrete_inner_type,
            ..
        } => strip_newtype(concrete_inner_type),
        _ => ty,
    }
}
