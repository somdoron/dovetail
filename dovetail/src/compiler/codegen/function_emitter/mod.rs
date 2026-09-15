mod expressions;
mod intrinsics;
mod match_expression;
mod readonly_slice;
mod runtime_types;
mod wasi_marshaling;
mod wit_marshaling;

use std::collections::BTreeMap;

use wasm_encoder::{Instruction, ValType};

use crate::codegen::{Codegen, FunctionDebugInfo, FunctionSourceMapping};
use crate::common::span::Span;
use crate::common::types::VarName;
use crate::typechecker::types::{CapturedVar, TypedClosureParam, TypedExpr, TypedParam};

/// Whether an expression's value is needed or can be discarded.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ExprContext {
    /// The expression's value is needed on the WASM stack.
    Value,
    /// The expression is used as a statement; no value should remain on the stack.
    Statement,
}

/// A lexical scope mapping variable names to WASM local indices.
struct Scope {
    locals: BTreeMap<VarName, u32>,
}

impl Scope {
    fn new() -> Self {
        Self {
            locals: BTreeMap::new(),
        }
    }

    fn define(&mut self, name: VarName, index: u32) {
        self.locals.insert(name, index);
    }

    fn lookup(&self, name: &VarName) -> Option<u32> {
        self.locals.get(name).copied()
    }
}

/// Labels for a loop's WASM block/loop pair.
struct LoopLabels {
    /// Absolute wasm_block_depth of the outer `block` (break target).
    break_depth: u32,
    /// Absolute wasm_block_depth of the inner `loop` (continue target).
    continue_depth: u32,
}

/// Accumulates locals and instructions for a single WASM function.
///
/// This allows a single-pass approach: as we walk the typed AST, we can
/// add locals and emit instructions without needing a separate counting pass.
///
/// WASM function parameters are implicit locals at indices 0..N-1.
/// Declared locals (from let bindings) start at index N.
pub(super) struct FunctionEmitter<'a> {
    /// Set while emitting an interface-object wrapper: the wrapper's coercion
    /// group key (`$via$`-tagged for provider-backed super groups). Bare-`Self`
    /// re-boxes prefer the direct (un-tagged) vtable global and fall back to
    /// this group's own standalone.
    pub(super) self_return_group_key: Option<crate::common::types::MangledName>,
    next_local_index: u32,
    locals: Vec<(u32, ValType)>,
    instructions: Vec<Instruction<'static>>,
    scopes: Vec<Scope>,
    codegen: &'a Codegen<'a>,
    /// Tracks nesting of WASM structured blocks (block/loop/if).
    wasm_block_depth: u32,
    /// Stack of active loops for break/continue label computation.
    loop_contexts: Vec<LoopLabels>,
    /// Absolute wasm_block_depth of the function's outer block (for Return).
    return_block_depth: u32,
    /// When the function/closure returns a flattenable tuple, the outer return block is an empty
    /// block and the body's (and each early `Return`'s) value is spilled into these temp locals
    /// before branching out, then reloaded after the block. Avoids multi-value block types.
    /// `(base, valtypes)`.
    return_spill: Option<(u32, smallvec::SmallVec<[ValType; 2]>)>,
    /// When a `match` *expression* yields a flattenable tuple, the outer match block is an empty
    /// block and each arm body spills its values into these temp locals before branching out, then
    /// they're reloaded after the block — mirroring `if`-expression tuple results. Saved/restored
    /// around nested matches. `(base, valtypes)`.
    match_result_spill: Option<(u32, smallvec::SmallVec<[ValType; 2]>)>,
    /// True when this emitter is for a lifted closure body. Closures are always erased to the
    /// Func_N ABI (a single `anyref` result), so the body value and every early `Return` are boxed
    /// to anyref rather than returned as a flattened tuple.
    in_closure: bool,
    /// Source mappings: (instruction_vec_index, span).
    source_mappings: Vec<(usize, Span)>,
    /// WIT-import marshaling: the local holding the enum value being lowered,
    /// so a variant case can materialize its payload out of it. Set/restored
    /// around each variant dispatch by `wit_marshaling`; `None` otherwise.
    wit_variant_src: Option<u32>,
}

impl<'a> FunctionEmitter<'a> {
    /// Tuple params always flatten into a sequence of consecutive WASM params (direct-call *and* vtable
    /// ABI), so each binding reserves the full flattened width up front. Param indices follow the
    /// *WASM* layout: `slot_params` (the erased vtable slot signature) when this is a vtable method,
    /// else the concrete param types. A vtable method's body rebinds any partially-erased param to a
    /// concrete shadow values via `emit_param_coerce_prologue`.
    pub fn new(
        params: &[TypedParam],
        slot_params: Option<&[crate::typechecker::types::Type]>,
        codegen: &'a Codegen<'a>,
    ) -> Self {
        let mut emitter = Self {
            next_local_index: 0,
            locals: Vec::new(),
            instructions: Vec::new(),
            scopes: vec![Scope::new()],
            codegen,
            wasm_block_depth: 0,
            loop_contexts: Vec::new(),
            return_block_depth: 0,
            return_spill: None,
            match_result_spill: None,
            in_closure: false,
            source_mappings: Vec::new(),
            wit_variant_src: None,
            self_return_group_key: None,
        };

        for (i, param) in params.iter().enumerate() {
            let layout_ty = slot_params.and_then(|s| s.get(i)).unwrap_or(&param.ty);
            let width = emitter.codegen.type_to_valtypes(layout_ty).len() as u32;
            emitter.define_param(VarName(param.name.clone()), width);
        }

        emitter
    }

    /// Register a function parameter occupying `width` consecutive WASM param indices, binding the
    /// name to the base. Params occupy indices 0..N but are not declared in the locals section.
    fn define_param(&mut self, name: VarName, width: u32) {
        let base = self.next_local_index;
        self.next_local_index += width;
        self.scopes
            .last_mut()
            .expect("define_param called with no scope")
            .define(name, base);
    }

    /// Add a declared local variable, returning its WASM local index.
    fn add_local(&mut self, ty: ValType) -> u32 {
        let index = self.next_local_index;
        self.next_local_index += 1;
        self.locals.push((1, ty));
        index
    }

    /// Reserve consecutive locals for a value's flattened slots (one per valtype), returning the
    /// base index. A tuple occupies `valtypes.len()` consecutive locals; a non-tuple is width-1 and
    /// identical to `add_local`. The slot count at any use site is recovered from the binding's
    /// Dovetail type via `type_to_valtypes`, so the scope only needs to store `base`.
    fn add_value_locals(&mut self, valtypes: &[ValType]) -> u32 {
        let base = self.next_local_index;
        for vt in valtypes {
            self.next_local_index += 1;
            self.locals.push((1, *vt));
        }
        base
    }

    /// Define a named local variable, returning its WASM local index.
    pub fn define_local(&mut self, name: VarName, ty: ValType) -> u32 {
        self.define_value_locals(name, &[ty])
    }

    /// Bind a name to an existing local base (aliasing — no new locals reserved). Used when a
    /// binding should refer to a slice of locals that already hold its value (e.g. a parent
    /// constructor param aliasing a temp, or a destructured element aliasing a sub-range).
    pub fn bind_name(&mut self, name: VarName, base: u32) {
        self.scopes
            .last_mut()
            .expect("bind_name called with no scope")
            .define(name, base);
    }

    /// Define a named binding occupying `valtypes.len()` consecutive locals (its flattened slots).
    pub fn define_value_locals(&mut self, name: VarName, valtypes: &[ValType]) -> u32 {
        let base = self.add_value_locals(valtypes);
        self.scopes
            .last_mut()
            .expect("define_value_locals called with no scope")
            .define(name, base);
        base
    }

    /// Pop a value's flattened slots (one per `valtypes` entry) off the stack into locals
    /// `base..base+valtypes.len()`. They sit on the stack with the last on top, so locals are
    /// filled in reverse index order.
    fn store_value(&mut self, base: u32, valtypes: &[ValType]) {
        for k in (0..valtypes.len() as u32).rev() {
            self.instruction(Instruction::LocalSet(base + k));
        }
    }

    /// Push a value's flattened slots (one per `valtypes` entry) from locals
    /// `base..base+valtypes.len()` onto the stack, in order.
    fn load_value(&mut self, base: u32, valtypes: &[ValType]) {
        for k in 0..valtypes.len() as u32 {
            self.instruction(Instruction::LocalGet(base + k));
        }
    }

    /// Reserve a *result-temp* locals for spilling a tuple across `if`/`match`/return control flow.
    /// Reference elements are declared **nullable** so the locals are defaultable: WASM's
    /// local-initialization check does not treat a non-defaultable local as initialized after an
    /// `if` even when both arms set it, so a non-nullable spill temp set only inside branches would
    /// fail validation. `load_result_temp` recovers the non-null values with `ref.as_non_null`.
    fn add_result_temp(&mut self, valtypes: &[ValType]) -> u32 {
        let base = self.next_local_index;
        for vt in valtypes {
            self.next_local_index += 1;
            let decl = match vt {
                ValType::Ref(rt) => ValType::Ref(wasm_encoder::RefType {
                    nullable: true,
                    heap_type: rt.heap_type,
                }),
                other => *other,
            };
            self.locals.push((1, decl));
        }
        base
    }

    /// Load a result-temp locals (see `add_result_temp`), restoring non-null refs.
    fn load_result_temp(&mut self, base: u32, valtypes: &[ValType]) {
        for (k, vt) in valtypes.iter().enumerate() {
            self.instruction(Instruction::LocalGet(base + k as u32));
            if matches!(vt, ValType::Ref(_)) {
                self.instruction(Instruction::RefAsNonNull);
            }
        }
    }

    /// Look up a named local variable, returning its WASM local index.
    pub fn lookup_local(&self, name: &VarName) -> u32 {
        for scope in self.scopes.iter().rev() {
            if let Some(index) = scope.lookup(name) {
                return index;
            }
        }
        unreachable!("undefined local: {}", name)
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Emit an instruction.
    pub fn instruction(&mut self, inst: Instruction<'static>) {
        self.instructions.push(inst);
    }

    /// Explode a boxed tuple `(ref $TupleN)` on top of the stack into its flattened values of N
    /// WASM values (Phase 2 multi-value returns). `ty` must be a flattenable tuple.
    pub fn emit_unbox_tuple(&mut self, ty: &crate::typechecker::types::Type) {
        let base = self.next_local_index;
        let (instrs, temps) = self.codegen.tuple_unbox_instrs(ty, base);
        for t in temps {
            self.add_local(t);
        }
        for i in instrs {
            self.instruction(i);
        }
    }

    /// Test a dynamic value without trapping on malformed bounded views.
    pub(super) fn emit_runtime_type_test(&mut self, target: &crate::typechecker::types::Type) {
        if self.emit_reified_type_test(target) {
            return;
        }
        if self.codegen.slice_offsets(target).is_empty() {
            let index = self.codegen.wasm_type_index_for_any_cast(target);
            self.instruction(Instruction::RefTestNonNull(
                wasm_encoder::HeapType::Concrete(index),
            ));
            return;
        }
        let (instrs, temps) = self
            .codegen
            .tuple_slice_test_instrs(target, self.next_local_index);
        for ty in temps {
            self.add_local(ty);
        }
        for instruction in instrs {
            self.instruction(instruction);
        }
    }

    /// Reassemble a flattened `[i64, i64]` (lo, hi) on top of the stack into a boxed
    /// `(ref $Uint128)`. The struct's two fields are raw `i64`, so this is a single `struct.new`
    /// (no per-leaf anyref boxing); the values land lo→field0, hi→field1.
    pub fn emit_rebox_uint128(&mut self) {
        self.instruction(Instruction::StructNew(super::UINT128_STRUCT_TYPE_INDEX));
    }

    /// Explode a boxed `(ref $Uint128)` on top of the stack into its flattened `[i64, i64]`
    /// (lo, hi). The ref is spilled to a temp local so both fields can be read. (Reboxing is
    /// the generic primitive path: `box_type_index_for(Uint128)` is `$Uint128` and `struct.new`
    /// pops the two i64 already on the stack — see `emit_box_to_any`.)
    pub fn emit_unbox_uint128(&mut self) {
        let ref_ty = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(super::UINT128_STRUCT_TYPE_INDEX),
        });
        let tmp = self.next_local_index;
        self.add_local(ref_ty);
        self.instruction(Instruction::LocalSet(tmp));
        self.instruction(Instruction::LocalGet(tmp));
        self.instruction(Instruction::StructGet {
            struct_type_index: super::UINT128_STRUCT_TYPE_INDEX,
            field_index: 0,
        });
        self.instruction(Instruction::LocalGet(tmp));
        self.instruction(Instruction::StructGet {
            struct_type_index: super::UINT128_STRUCT_TYPE_INDEX,
            field_index: 1,
        });
    }

    /// Reassemble a flattened tuple on top of the stack back into a boxed `(ref $TupleN)`.
    /// `ty` must be a flattenable tuple.
    pub fn emit_rebox_tuple(&mut self, ty: &crate::typechecker::types::Type) {
        let base = self.next_local_index;
        let (instrs, temps) = self.codegen.tuple_rebox_instrs(ty, base);
        for t in temps {
            self.add_local(t);
        }
        for i in instrs {
            self.instruction(i);
        }
    }

    /// Store a freshly-emitted value (its flattened slots on the stack) into a **new** heap mut-box,
    /// leaving the box reference on the stack. A tuple's mutable `$Tuple_N` is the box; every other
    /// type wraps in a single-field `$MutBox` (see `Codegen::mut_box_type_index_for`).
    pub fn emit_mut_box_new(&mut self, ty: &crate::typechecker::types::Type) {
        if self.codegen.is_tuple(ty) {
            self.emit_rebox_tuple(ty);
        } else {
            self.instruction(Instruction::StructNew(
                self.codegen.mut_box_type_index_for(ty),
            ));
        }
    }

    /// Read the value held in the mut-box `box_local`, leaving its flattened slots on the stack.
    pub fn emit_mut_box_load(&mut self, box_local: u32, ty: &crate::typechecker::types::Type) {
        self.instruction(Instruction::LocalGet(box_local));
        if self.codegen.is_tuple(ty) {
            self.emit_unbox_tuple(ty);
        } else if self.codegen.is_uint128(ty) {
            // The `$Uint128` mut-box doubles as the box; explode it into `[lo, hi]`.
            self.emit_unbox_uint128();
        } else {
            let box_idx = self.codegen.mut_box_type_index_for(ty);
            self.instruction(Instruction::StructGet {
                struct_type_index: box_idx,
                field_index: 0,
            });
            // The shared `$MutBox` stores `anyref`; cast back to the concrete ref type.
            if box_idx == super::MUT_BOX_REF_TYPE_INDEX
                && let ValType::Ref(rt) = self.codegen.single_val_type(ty)
            {
                self.instruction(Instruction::RefCastNonNull(rt.heap_type));
            }
        }
    }

    /// Write a freshly-emitted value (its flattened slots on the stack) into the mut-box `box_local`
    /// **in place** — visible through every captured reference. A tuple sets its N fields; every
    /// other type sets the single `$MutBox` field.
    pub fn emit_mut_box_store(&mut self, box_local: u32, ty: &crate::typechecker::types::Type) {
        let box_idx = self.codegen.mut_box_type_index_for(ty);
        if self.codegen.is_tuple(ty) {
            let leaves = self.codegen.flatten_to_leaf_types(ty);
            let valtypes = self.codegen.type_to_valtypes(ty);
            let base = self.add_value_locals(&valtypes);
            self.store_value(base, &valtypes);
            for (k, leaf) in leaves.iter().enumerate() {
                self.instruction(Instruction::LocalGet(box_local));
                self.instruction(Instruction::LocalGet(base + k as u32));
                self.emit_box_to_any(leaf);
                self.instruction(Instruction::StructSet {
                    struct_type_index: box_idx,
                    field_index: k as u32,
                });
            }
        } else if self.codegen.is_uint128(ty) {
            // The `$Uint128` mut-box has two raw `i64` fields; set them in place (no boxing).
            let valtypes = self.codegen.type_to_valtypes(ty);
            let base = self.add_value_locals(&valtypes);
            self.store_value(base, &valtypes);
            for k in 0..valtypes.len() as u32 {
                self.instruction(Instruction::LocalGet(box_local));
                self.instruction(Instruction::LocalGet(base + k));
                self.instruction(Instruction::StructSet {
                    struct_type_index: box_idx,
                    field_index: k,
                });
            }
        } else {
            // Spill the single value, then set the box's field (box ref must sit below it).
            let tmp = self.add_local(self.codegen.single_val_type(ty));
            self.instruction(Instruction::LocalSet(tmp));
            self.instruction(Instruction::LocalGet(box_local));
            self.instruction(Instruction::LocalGet(tmp));
            self.instruction(Instruction::StructSet {
                struct_type_index: box_idx,
                field_index: 0,
            });
        }
    }

    /// Emit a WASM `block` instruction and track its depth.
    pub fn emit_block(&mut self, block_type: wasm_encoder::BlockType) {
        self.instructions.push(Instruction::Block(block_type));
        self.wasm_block_depth += 1;
    }

    /// Emit a WASM `if` block and track its depth.
    pub fn emit_if_block(&mut self, block_type: wasm_encoder::BlockType) {
        self.instructions.push(Instruction::If(block_type));
        self.wasm_block_depth += 1;
    }

    /// Emit a WASM `block`+`loop` pair for a while loop and push loop context.
    pub fn emit_while_block(&mut self) {
        self.instructions
            .push(Instruction::Block(wasm_encoder::BlockType::Empty));
        self.wasm_block_depth += 1;
        self.instructions
            .push(Instruction::Loop(wasm_encoder::BlockType::Empty));
        self.wasm_block_depth += 1;
        self.loop_contexts.push(LoopLabels {
            break_depth: self.wasm_block_depth - 2,
            continue_depth: self.wasm_block_depth - 1,
        });
    }

    /// Close a while loop's `block`+`loop` pair and pop loop context.
    pub fn emit_end_while_block(&mut self) {
        self.loop_contexts.pop();
        // end loop
        self.wasm_block_depth -= 1;
        self.instructions.push(Instruction::End);
        // end block
        self.wasm_block_depth -= 1;
        self.instructions.push(Instruction::End);
    }

    /// Emit an `end` instruction and untrack the block depth.
    pub fn emit_end_block(&mut self) {
        self.wasm_block_depth -= 1;
        self.instructions.push(Instruction::End);
    }

    /// Compute the relative br depth to break out of the innermost loop.
    pub fn break_label(&self) -> u32 {
        let ctx = self.loop_contexts.last().expect("break outside loop");
        self.wasm_block_depth - ctx.break_depth - 1
    }

    /// Compute the relative br depth to continue the innermost loop.
    pub fn continue_label(&self) -> u32 {
        let ctx = self.loop_contexts.last().expect("continue outside loop");
        self.wasm_block_depth - ctx.continue_depth - 1
    }

    /// Emit a ref.cast prologue to cast param 0 from root base class to concrete class.
    /// Creates a new local with the concrete type and remaps the "self" binding to it.
    pub fn emit_self_cast_prologue(&mut self, concrete_valtype: ValType, concrete_type_idx: u32) {
        let cast_local = self.add_local(concrete_valtype);
        self.instruction(Instruction::LocalGet(0));
        self.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(concrete_type_idx),
        ));
        self.instruction(Instruction::LocalSet(cast_local));
        // Remap "self" in the current scope to the new cast local
        let self_name = VarName("self".to_string());
        self.scopes
            .last_mut()
            .expect("emit_self_cast_prologue called with no scope")
            .define(self_name, cast_local);
    }

    /// Rewrite a flattened value on the stack from `from_ty`'s flattened layout to `to_ty`'s. The two types are
    /// the same shape up to erasure (the typechecker guarantees `from ~ to`). A no-op when the
    /// layouts are already identical (the concrete hot path). Otherwise one of:
    /// - `to` is a single erased `anyref` (a bare type parameter / `Any`): box the whole `from`
    ///   value into it (`emit_box_to_any` — reboxes a flattened tuple into `(ref $Tuple_N)`).
    /// - `from` is a single erased `anyref`: cast it back to `to` (`emit_cast_back_from_any`).
    /// - both are tuples of equal arity: recurse element-by-element (handles width-changing nested
    ///   cases, e.g. a concrete sub-tuple boxed into a type-parameter element).
    pub fn coerce_value(
        &mut self,
        from_ty: &crate::typechecker::types::Type,
        to_ty: &crate::typechecker::types::Type,
    ) {
        let from_vts = self.codegen.type_to_valtypes(from_ty);
        let to_vts = self.codegen.type_to_valtypes(to_ty);
        if from_vts == to_vts {
            return;
        }
        // Class inheritance is represented by WASM struct subtyping. An
        // accepted subclass-to-superclass coercion needs no instructions.
        if from_ty.is_class_type()
            && to_ty.is_class_type()
            && crate::typechecker::subtyping::is_subtype(self.codegen.registry, from_ty, to_ty)
        {
            return;
        }
        // A type that lowers to a single erased ref is an erased slot: `(ref any)` for a bare type
        // parameter / `Any` / a `InterfaceObject<T>`, or the abstract `(ref array)` for a fully-generic
        // `Array<T>`. Boxing into either is an upcast (the concrete value is already a subtype);
        // reading back `ref.cast`s to the concrete type. (The reverse store — a bare `(ref any)`
        // into a `(ref array)` slot — never occurs: an erased array slot only ever receives a
        // concrete array.)
        let any_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::ANY,
        });
        let array_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Array,
            },
        });
        let is_one_erased_ref = |vts: &smallvec::SmallVec<[ValType; 2]>| {
            vts.len() == 1 && (vts[0] == any_ref || vts[0] == array_ref)
        };
        if is_one_erased_ref(&to_vts) {
            self.emit_box_to_any(from_ty);
            return;
        }
        if is_one_erased_ref(&from_vts) {
            self.emit_cast_back_from_any(to_ty);
            return;
        }
        // Concrete value → interface object: a vtable slot with a bare-`Self`
        // return types the slot as the declaring interface's object type; the
        // wrapper re-boxes its concrete return into the interface here ("Self
        // is observed as the interface type"). The (concrete, interface)
        // vtable global exists because this wrapper exists.
        if let crate::typechecker::types::Type::InterfaceObject { mangled_name, .. } = to_ty
            && !matches!(
                from_ty,
                crate::typechecker::types::Type::InterfaceObject { .. }
            )
        {
            // Box every shape — primitives box, references upcast free,
            // and flattened tuple returns rebox into their `(ref $Tuple)`.
            self.emit_box_to_any(from_ty);
            let type_key = super::instance_key(from_ty);
            // Direct global first — a re-boxed super object is
            // a direct-super context — but ONLY when the type directly
            // implements the SAME super application this wrapper's group
            // backs (the coercion pass authorizes those pairs). The
            // base key is application-erased, so an unrelated direct
            // coercion of a different application must not hijack a via
            // wrapper's re-box. Keep the application's full group key.
            let direct_key = match self.self_return_group_key.as_ref() {
                Some(k) if k.0.contains("$via$") => self
                    .codegen
                    .direct_rebox_authorized
                    .get(&(type_key.clone(), k.clone())),
                Some(k) => Some(k),
                None => Some(mangled_name),
            };
            let direct = direct_key.and_then(|key| {
                self.codegen
                    .vtable_global_indices
                    .get(&(type_key.clone(), key.clone()))
                    .copied()
                    .filter(|&idx| idx != u32::MAX)
            });
            let via = self.self_return_group_key.as_ref().and_then(|group_key| {
                self.codegen
                    .vtable_global_indices
                    .get(&(type_key.clone(), group_key.clone()))
                    .copied()
                    .filter(|&idx| idx != u32::MAX)
            });
            let vtable_global = direct.or(via).unwrap_or_else(|| {
                    panic!("missing vtable global for Self-return re-box: type={type_key} interface={mangled_name}")
                });
            self.instruction(Instruction::GlobalGet(vtable_global));
            let traitobj_type = self.codegen.interface_object_type_indices[mangled_name];
            self.instruction(Instruction::StructNew(traitobj_type));
            return;
        }
        // Both are tuples of equal arity (the only remaining structural mismatch).
        let from_elems = self
            .codegen
            .resolve_tuple(from_ty)
            .unwrap_or_else(|| panic!("coerce_value: {from_ty} !~ {to_ty}"))
            .0
            .to_vec();
        let to_elems = self
            .codegen
            .resolve_tuple(to_ty)
            .unwrap_or_else(|| panic!("coerce_value: {from_ty} !~ {to_ty}"))
            .0
            .to_vec();
        debug_assert_eq!(from_elems.len(), to_elems.len());
        let base = self.add_value_locals(&from_vts);
        self.store_value(base, &from_vts);
        for i in 0..from_elems.len() {
            let (start, width) = self.codegen.tuple_elem_offset(&from_elems, i);
            self.load_value(
                base + start,
                &from_vts[start as usize..(start + width) as usize],
            );
            self.coerce_value(&from_elems[i], &to_elems[i]);
        }
    }

    /// For a vtable method emitted with the erased slot signature, rebind each non-self parameter
    /// whose slot layout differs from its concrete layout (a type-parameter scalar, or a tuple with
    /// type-parameter leaves) to a concrete shadow values via `coerce_value`. A no-op for fully-concrete
    /// params, which already arrive in their exact layout.
    pub fn emit_param_coerce_prologue(
        &mut self,
        slot_params: &[crate::typechecker::types::Type],
        concrete_params: &[TypedParam],
    ) {
        for (i, concrete) in concrete_params.iter().enumerate() {
            if i == 0 {
                continue; // self — handled by emit_self_cast_prologue
            }
            let Some(slot_ty) = slot_params.get(i) else {
                continue;
            };
            let from_vts = self.codegen.type_to_valtypes(slot_ty);
            let to_vts = self.codegen.type_to_valtypes(&concrete.ty);
            if from_vts == to_vts {
                continue; // concrete param — arrives in its exact layout
            }
            let name = VarName(concrete.name.clone());
            let param_base = self.lookup_local(&name);
            self.load_value(param_base, &from_vts);
            self.coerce_value(slot_ty, &concrete.ty);
            let shadow_base = self.add_value_locals(&to_vts);
            self.store_value(shadow_base, &to_vts);
            self.bind_name(name, shadow_base);
        }
    }

    /// Emit a interface-object dispatch wrapper body. The wrapper's WASM params are the erased
    /// vtable-slot signature — `anyref` self followed by each non-self param in its slot (possibly
    /// erased) layout. The body casts/coerces every param to the concrete impl ABI via
    /// `coerce_value` (so a partially-erased tuple param — `(Int32, T)` → flattened concrete — is
    /// handled, not just bare-`anyref` params), calls the impl, then coerces the concrete return
    /// back into the slot's layout (boxing erased leaves). Create the emitter with **no** params
    /// (`FunctionEmitter::new(&[], None, …)`); this method reserves the param locals itself.
    pub fn emit_interface_object_wrapper(
        &mut self,
        self_concrete_ty: &crate::typechecker::types::Type,
        params: &[(
            crate::typechecker::types::Type,
            crate::typechecker::types::Type,
        )],
        impl_func_idx: u32,
        concrete_return: &crate::typechecker::types::Type,
        slot_return: &crate::typechecker::types::Type,
    ) {
        use crate::typechecker::types::Type;
        // Reserve param locals up front (self at 0, then each slot param) so `coerce_value`'s temp
        // locals are allocated *after* them.
        self.next_local_index = 1; // self occupies local 0 (a single anyref).
        let mut param_bases: Vec<u32> = Vec::with_capacity(params.len());
        for (slot_ty, _) in params {
            param_bases.push(self.next_local_index);
            self.next_local_index += self.codegen.type_to_valtypes(slot_ty).len() as u32;
        }

        // self: anyref at local 0 → cast back to the concrete impl type.
        self.instruction(Instruction::LocalGet(0));
        self.coerce_value(&Type::Any, self_concrete_ty);

        // Each non-self param: load its slot values, coerce to the concrete impl layout.
        for (i, (slot_ty, concrete_ty)) in params.iter().enumerate() {
            let slot_vts = self.codegen.type_to_valtypes(slot_ty);
            self.load_value(param_bases[i], &slot_vts);
            self.coerce_value(slot_ty, concrete_ty);
        }

        self.instruction(Instruction::Call(impl_func_idx));

        // Coerce the concrete return into the (possibly erased) slot return layout.
        self.coerce_value(concrete_return, slot_return);
    }

    /// Create a FunctionEmitter for a lifted closure function under always-erased closures.
    ///
    /// WASM signature is `Func_N = (param anyref) ... N+1 times (result anyref)`. All params
    /// arrive as `anyref` and the function returns `anyref`. Param 0 is the env (unnamed —
    /// `emit_closure_env_prologue` casts it to the concrete capture struct). Params 1..N are
    /// the declared closure params, also arriving as anyref; their names are NOT bound here —
    /// `emit_closure_param_prologue` casts each one back to its declared type and binds the
    /// name to the shadow local.
    pub fn new_for_closure(params: &[TypedClosureParam], codegen: &'a Codegen<'a>) -> Self {
        let mut emitter = Self {
            next_local_index: 0,
            locals: Vec::new(),
            instructions: Vec::new(),
            scopes: vec![Scope::new()],
            codegen,
            wasm_block_depth: 0,
            loop_contexts: Vec::new(),
            return_block_depth: 0,
            return_spill: None,
            match_result_spill: None,
            in_closure: true,
            source_mappings: Vec::new(),
            wit_variant_src: None,
            self_return_group_key: None,
        };

        // Reserve indices 0..N for env + N anyref WASM params. Names are bound later in
        // `emit_closure_param_prologue` (which produces a shadow local of the declared type).
        emitter.next_local_index = 1 + params.len() as u32;
        emitter
    }

    /// For each declared closure param at WASM index `i+1` (param 0 is env), cast the anyref
    /// value to the declared type — unboxing primitives, `ref.cast` for ref types, identity
    /// for anyref-equivalent types (`Any` / type parameters) — store in a shadow local, and
    /// bind the declared param name to that shadow in the current scope. Must be called after
    /// `new_for_closure` (and after `emit_closure_env_prologue` if there are captures).
    pub fn emit_closure_param_prologue(&mut self, params: &[TypedClosureParam]) {
        for (i, param) in params.iter().enumerate() {
            let wasm_param_idx = (i + 1) as u32;
            let declared_ty = &param.ty;

            // anyref-equivalent declared type: bind the name directly to the WASM param index.
            // Type::Never is included here because closures with `(Never) => …` params
            // can never actually be invoked, so a runtime cast to a concrete Never type
            // would be both impossible (Never has no values) and dead code (the body
            // must diverge or never reference the param).
            if matches!(
                declared_ty,
                crate::typechecker::types::Type::Any
                    | crate::typechecker::types::Type::Never
                    | crate::typechecker::types::Type::TypeVariable(..)
                    | crate::typechecker::types::Type::GenericParam(..)
            ) {
                self.scopes
                    .last_mut()
                    .expect("emit_closure_param_prologue called with no scope")
                    .define(param.name.clone(), wasm_param_idx);
                continue;
            }

            // A tuple param arrives boxed as anyref; cast to `(ref $Tuple)`, explode into its flattened values,
            // and bind the name to the values (the body reads it like any other tuple binding).
            if self.codegen.is_tuple(declared_ty) {
                let valtypes = self.codegen.type_to_valtypes(declared_ty);
                let base = self.add_value_locals(&valtypes);
                self.instruction(Instruction::LocalGet(wasm_param_idx));
                let type_idx = self.codegen.wasm_type_index_for_any_cast(declared_ty);
                self.instruction(Instruction::RefCastNonNull(
                    wasm_encoder::HeapType::Concrete(type_idx),
                ));
                self.emit_unbox_tuple(declared_ty);
                self.store_value(base, &valtypes);
                self.bind_name(param.name.clone(), base);
                continue;
            }

            // A `Uint128` closure param arrives boxed as `(ref $Uint128)`; cast, explode into
            // `[lo, hi]`, and bind the name to the run.
            if self.codegen.is_uint128(declared_ty) {
                let valtypes = self.codegen.type_to_valtypes(declared_ty);
                let base = self.add_value_locals(&valtypes);
                self.instruction(Instruction::LocalGet(wasm_param_idx));
                let type_idx = self.codegen.wasm_type_index_for_any_cast(declared_ty);
                self.instruction(Instruction::RefCastNonNull(
                    wasm_encoder::HeapType::Concrete(type_idx),
                ));
                self.emit_unbox_uint128();
                self.store_value(base, &valtypes);
                self.bind_name(param.name.clone(), base);
                continue;
            }

            let concrete_valtype = self.codegen.single_val_type(declared_ty);
            let shadow_local = self.add_local(concrete_valtype);
            self.instruction(Instruction::LocalGet(wasm_param_idx));
            if declared_ty.is_reference_type() {
                let type_idx = self.codegen.wasm_type_index_for_any_cast(declared_ty);
                self.instruction(Instruction::RefCastNonNull(
                    wasm_encoder::HeapType::Concrete(type_idx),
                ));
            } else {
                let box_idx = self.codegen.box_type_index_for(declared_ty);
                self.instruction(Instruction::RefCastNonNull(
                    wasm_encoder::HeapType::Concrete(box_idx),
                ));
                self.instruction(Instruction::StructGet {
                    struct_type_index: box_idx,
                    field_index: 0,
                });
            }
            self.instruction(Instruction::LocalSet(shadow_local));
            self.scopes
                .last_mut()
                .expect("emit_closure_param_prologue called with no scope")
                .define(param.name.clone(), shadow_local);
        }
    }

    /// Emit a closure body. The closure's WASM function returns `anyref` (the canonical
    /// `Func_N` signature), but the user's body has the declared return type. We wrap the
    /// body in an inner block whose result is the declared type — so `Return` expressions
    /// can `br` to it carrying a declared-typed value — and then `emit_box_to_any` after
    /// the block end to lift the value into `anyref` for the outer function return.
    pub fn emit_closure_body(
        &mut self,
        body: &TypedExpr,
        declared_return: &crate::typechecker::types::Type,
    ) {
        let _ = declared_return;
        // The Func_N ABI returns a single `anyref` (closures are always erased). Box the body value
        // — using its *own* type, which may be more specific than the declared return, e.g. a
        // concrete tuple where the closure type says `Any` — to anyref inside the block; each early
        // `Return` boxes to anyref too (see `in_closure`, set by `new_for_closure`).
        let anyref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::ANY,
        });
        self.emit_block(wasm_encoder::BlockType::Result(anyref));
        self.return_block_depth = self.wasm_block_depth - 1;
        self.push_scope();
        self.emit_expr(body, ExprContext::Value);
        self.emit_box_to_any(&body.ty);
        self.pop_scope();
        self.emit_end_block();
    }

    /// Box a value of type `value_ty` (currently on top of the stack) into `anyref`.
    /// No-op for anyref-equivalent types and reference types (which upcast implicitly).
    /// Wraps primitives in their `Box*` struct. Emits `unreachable` for `Never`/`Error`
    /// (the value was produced by code that can't actually reach this point).
    pub fn emit_box_to_any(&mut self, value_ty: &crate::typechecker::types::Type) {
        use crate::typechecker::types::Type;
        if matches!(
            value_ty,
            Type::Any | Type::TypeVariable(..) | Type::GenericParam(..)
        ) {
            return;
        }
        if matches!(value_ty, Type::Never | Type::Error) {
            self.instruction(Instruction::Unreachable);
            return;
        }
        // A flattened tuple on the stack is reboxed into its `(ref $Tuple)` (an anyref
        // subtype). `is_reference_type(Tuple)` is false (the stack holds a flattened run,
        // not a ref), so this `is_tuple` branch is what boxes it.
        if self.codegen.is_tuple(value_ty) {
            self.emit_rebox_tuple(value_ty);
            return;
        }
        if value_ty.is_reference_type() {
            // Implicit upcast to anyref — nothing to emit.
            return;
        }
        let box_idx = self.codegen.box_type_index_for(value_ty);
        self.instruction(Instruction::StructNew(box_idx));
    }

    /// Cast an `anyref` value (on top of the stack) back to the static type `target_ty` —
    /// `ref.cast` for ref types, `ref.cast` + `struct.get` for primitives (unbox), identity
    /// for anyref-equivalents. Emits `unreachable` for `Never`/`Error` targets so the
    /// downstream consumer's stack is polymorphic.
    pub fn emit_cast_back_from_any(&mut self, target_ty: &crate::typechecker::types::Type) {
        use crate::typechecker::types::Type;
        if matches!(
            target_ty,
            Type::Any | Type::TypeVariable(..) | Type::GenericParam(..)
        ) {
            return;
        }
        if matches!(target_ty, Type::Never | Type::Error) {
            self.instruction(Instruction::Unreachable);
            return;
        }
        // A boxed tuple arriving as anyref: cast to `(ref $Tuple)` then explode to a flattened value.
        if self.codegen.is_tuple(target_ty) {
            let target_idx = self.codegen.wasm_type_index_for_any_cast(target_ty);
            self.instruction(Instruction::RefCastNonNull(
                wasm_encoder::HeapType::Concrete(target_idx),
            ));
            self.emit_unbox_tuple(target_ty);
            return;
        }
        // A boxed `Uint128` arriving as anyref: cast to `(ref $Uint128)` then explode to `[lo, hi]`.
        if self.codegen.is_uint128(target_ty) {
            self.instruction(Instruction::RefCastNonNull(
                wasm_encoder::HeapType::Concrete(super::UINT128_STRUCT_TYPE_INDEX),
            ));
            self.emit_unbox_uint128();
            return;
        }
        let target_idx = self.codegen.wasm_type_index_for_any_cast(target_ty);
        self.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(target_idx),
        ));
        if !target_ty.is_reference_type() {
            self.instruction(Instruction::StructGet {
                struct_type_index: target_idx,
                field_index: 0,
            });
        }
    }

    /// Emit prologue code for a lifted closure: cast env param to concrete struct,
    /// extract captures into named locals.
    pub fn emit_closure_env_prologue(&mut self, env_type_index: u32, captures: &[CapturedVar]) {
        let env_ref_type = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(env_type_index),
        });
        let env_local = self.add_local(env_ref_type);

        // Cast param 0 (anyref) → (ref $env_struct)
        self.instruction(Instruction::LocalGet(0));
        self.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(env_type_index),
        ));
        self.instruction(Instruction::LocalSet(env_local));

        // Extract each capture from the env struct into a named local. Captures splice: an
        // immutable tuple occupies a run of `width` fields, so track a running field offset
        // rather than indexing 1:1.
        let mut field_offset: u32 = 0;
        for cap in captures.iter() {
            if cap.mutable {
                // Mutable capture: one field holding the mut-box ref (a `(ref $Tuple_N)` for a tuple,
                // else a `(ref $MutBox)`).
                let box_ref_type = self.codegen.mut_box_valtype(&cap.ty);
                let local = self.define_local(cap.name.clone(), box_ref_type);
                self.instruction(Instruction::LocalGet(env_local));
                self.instruction(Instruction::StructGet {
                    struct_type_index: env_type_index,
                    field_index: field_offset,
                });
                self.instruction(Instruction::LocalSet(local));
                field_offset += 1;
            } else {
                // Immutable capture: bind as a flattened value, read directly from its run of
                // spliced fields (no unbox — the tuple's leaves are stored field-by-field).
                let valtypes = self.codegen.type_to_valtypes(&cap.ty);
                let base = self.define_value_locals(cap.name.clone(), &valtypes);
                for k in 0..valtypes.len() as u32 {
                    self.instruction(Instruction::LocalGet(env_local));
                    self.instruction(Instruction::StructGet {
                        struct_type_index: env_type_index,
                        field_index: field_offset + k,
                    });
                }
                self.store_value(base, &valtypes);
                field_offset += valtypes.len() as u32;
            }
        }
    }

    /// Emit the body of a function (params already in scope from new()), leaving the body's value
    /// in the representation of `return_type` (a flattened values for a tuple, a single value
    /// otherwise). Wraps the body in an outer WASM block so that `Return` expressions can `br` to
    /// the function exit. A tuple return uses an empty block + a return-run temp (see `return_spill`)
    /// to avoid multi-value block types; the values is reloaded after the block.
    pub fn emit_body(&mut self, body: &TypedExpr, return_type: &crate::typechecker::types::Type) {
        // A tuple or `Uint128` return is multi-value; use an empty block + a return-run temp to
        // avoid multi-value block types, reloading the run after the block.
        if self.codegen.is_tuple(return_type) || self.codegen.is_uint128(return_type) {
            let valtypes = self.codegen.type_to_valtypes(return_type);
            let base = self.add_result_temp(&valtypes);
            self.return_spill = Some((base, valtypes.clone()));
            self.emit_block(wasm_encoder::BlockType::Empty);
            self.return_block_depth = self.wasm_block_depth - 1;
            self.push_scope();
            self.emit_expr(body, ExprContext::Value);
            self.store_value(base, &valtypes);
            self.pop_scope();
            self.emit_end_block();
            self.load_result_temp(base, &valtypes);
        } else {
            let return_valtype = self.codegen.single_val_type(return_type);
            self.emit_block(wasm_encoder::BlockType::Result(return_valtype));
            self.return_block_depth = self.wasm_block_depth - 1;
            self.push_scope();
            self.emit_expr(body, ExprContext::Value);
            self.pop_scope();
            self.emit_end_block();
        }
    }

    /// Build the final wasm_encoder::Function with debug info.
    pub fn build(self) -> (wasm_encoder::Function, FunctionDebugInfo) {
        let mut func = wasm_encoder::Function::new(self.locals);

        // Build a map: instruction_vec_index → byte_offset within function body
        let mut inst_byte_offsets: Vec<u32> = Vec::with_capacity(self.instructions.len());
        for inst in &self.instructions {
            inst_byte_offsets.push(func.byte_len() as u32);
            func.instruction(inst);
        }
        func.instruction(&Instruction::End);
        let body_byte_len = func.byte_len();

        // Convert source_mappings from instruction indices to byte offsets
        let mappings: Vec<FunctionSourceMapping> = self
            .source_mappings
            .into_iter()
            .filter_map(|(inst_idx, span)| {
                inst_byte_offsets
                    .get(inst_idx)
                    .map(|&offset| FunctionSourceMapping {
                        byte_offset: offset,
                        span,
                    })
            })
            .collect();

        (
            func,
            FunctionDebugInfo {
                body_byte_len,
                mappings,
            },
        )
    }
}
