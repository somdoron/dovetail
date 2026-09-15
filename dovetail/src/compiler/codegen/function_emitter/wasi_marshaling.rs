//! WASI marshaling helpers for linear memory interaction in intrinsic codegen.
//!
//! These methods on FunctionEmitter provide building blocks for marshaling data
//! between WASM GC types and linear memory when calling WASI imports.

use wasm_encoder::{BlockType, Instruction, ValType};

use crate::common::types::MangledName;
use crate::typechecker::types::{Type, TypedExpr};

use super::ExprContext;

/// How to load a socket-option payload out of a retptr block. `U8` covers both
/// `bool` and WIT's `u8` (hop-limit); both zero-extend into the Dovetail value.
///
/// `Unit` loads nothing: a `result<_, error-code>` has no ok payload to read, so
/// the ok branch pushes the Unit constant. It is a member here rather than a
/// separate emitter because everything else about the decode — the discriminant
/// at 0, the error-code discriminant at the payload offset, the branch shape —
/// is identical, and one of those was drifting out of step with the other.
#[derive(Clone, Copy)]
pub(super) enum WasiScalar {
    Unit,
    U8,
    I32,
    I64,
}

/// Discriminant of the `other(option<string>)` case in each p3 `error-code`,
/// i.e. the index of the `Other` name in the corresponding constructor's variant
/// table. The sockets/dns lifts need it to know which case carries a message to
/// lift into the Dovetail `Other(Option<String>)` payload; the filesystem lift
/// hands it to `wasi_free_error_message` to know when there is a message to
/// free. Point either at the wrong case and the guest leaks the message, frees
/// a discriminant, or lifts garbage.
const NETWORK_ERROR_OTHER_DISC: i32 = 14;
const DNS_ERROR_OTHER_DISC: i32 = 5;
const FS_ERROR_OTHER_DISC: i32 = 36;

/// The same thing for `descriptor-type`, which is a variant rather than an enum
/// in p3 for exactly one reason: its last case is `other(option<string>)`, the
/// same shape as the error codes above and freed by the same helper. p3 has
/// already reordered this variant once — `unknown` was dropped from the front and
/// `other` appended — while the WIT is still at an `-rc-` version.
pub(super) const DESCRIPTOR_TYPE_OTHER_DISC: i32 = 7;

/// `a == b` for string literals in const context: `PartialEq for str` is not a
/// const fn, and the alternative to spelling this out is a `debug_assert!` that a
/// release-built compiler never runs.
const fn str_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// The case at `disc` in a variant table is `Other`, and it is the last one.
///
/// Both halves matter, and the length alone is not enough: an `-rc-` that removes
/// one case and inserts another leaves the count unchanged while `other` moves,
/// which is exactly the drift these constants exist to catch. Named rather than
/// counted, and `const` rather than `debug_assert!`, so it holds in a
/// release-built compiler too.
const fn other_case_is_at(names: &[&str], disc: i32) -> bool {
    disc >= 0 && names.len() == disc as usize + 1 && str_eq(names[disc as usize], "Other")
}

/// NetworkError variant names in p3 `wasi:sockets/types` error-code order, with
/// the `other` catch-all last; its `option<string>` payload is lifted into the
/// Dovetail variant's `Option<String>` payload.
const NETWORK_ERROR_VARIANT_NAMES: [&str; 15] = [
    "AccessDenied",
    "NotSupported",
    "InvalidArgument",
    "OutOfMemory",
    "Timeout",
    "InvalidState",
    "AddressNotBindable",
    "AddressInUse",
    "RemoteUnreachable",
    "ConnectionRefused",
    "ConnectionBroken",
    "ConnectionReset",
    "ConnectionAborted",
    "DatagramTooLarge",
    "Other",
];
const _: () = assert!(other_case_is_at(
    &NETWORK_ERROR_VARIANT_NAMES,
    NETWORK_ERROR_OTHER_DISC
));

/// `wasi:sockets/ip-name-lookup` error-code order; `other(option<string>)` is
/// last and its message is lifted into the Dovetail variant's payload.
const DNS_ERROR_VARIANT_NAMES: [&str; 6] = [
    "AccessDenied",
    "InvalidArgument",
    "NameUnresolvable",
    "TemporaryResolverFailure",
    "PermanentResolverFailure",
    "Other",
];
const _: () = assert!(other_case_is_at(
    &DNS_ERROR_VARIANT_NAMES,
    DNS_ERROR_OTHER_DISC
));

/// FileSystemError variant names in p3 `wasi:filesystem/types` error-code order
/// (`would-block` was removed and a catch-all `other` appended).
const FS_ERROR_VARIANT_NAMES: [&str; 37] = [
    "Access",
    "Already",
    "BadDescriptor",
    "Busy",
    "Deadlock",
    "Quota",
    "Exist",
    "FileTooLarge",
    "IllegalByteSequence",
    "InProgress",
    "Interrupted",
    "Invalid",
    "Io",
    "IsDirectory",
    "Loop",
    "TooManyLinks",
    "MessageSize",
    "NameTooLong",
    "NoDevice",
    "NoEntry",
    "NoLock",
    "InsufficientMemory",
    "InsufficientSpace",
    "NotDirectory",
    "NotEmpty",
    "NotRecoverable",
    "Unsupported",
    "NoTty",
    "NoSuchDevice",
    "Overflow",
    "NotPermitted",
    "Pipe",
    "ReadOnly",
    "InvalidSeek",
    "TextFileBusy",
    "CrossDevice",
    "Other",
];
const _: () = assert!(other_case_is_at(
    &FS_ERROR_VARIANT_NAMES,
    FS_ERROR_OTHER_DISC
));

/// `descriptor-type` case names in p3 order: `unknown` was removed and
/// `other(option<string>)` appended.
const DESCRIPTOR_TYPE_VARIANT_NAMES: [&str; 8] = [
    "BlockDevice",
    "CharacterDevice",
    "Directory",
    "Fifo",
    "SymbolicLink",
    "RegularFile",
    "Socket",
    "Other",
];
const _: () = assert!(other_case_is_at(
    &DESCRIPTOR_TYPE_VARIANT_NAMES,
    DESCRIPTOR_TYPE_OTHER_DISC
));

impl<'a> super::FunctionEmitter<'a> {
    /// Open a marshaling window: save the scratch bump pointer (global 0) and
    /// the head of the window's pinned-scratch list into fresh locals. Paired
    /// with `scratch_restore`, this gives every window stack discipline: every
    /// block the call (and any nested call) allocates for *lowering* — scratch
    /// arena or pinned-scratch list — is reclaimed when the window closes, so
    /// neither the single scratch page nor the pinned heap is exhausted by a
    /// long-running program.
    ///
    /// It does not cover blocks the host allocated through `cabi_realloc` while
    /// lifting a result. Those outlive the window by design — the result is read
    /// after it closes — so each lift frees its own (see
    /// `wasi_free_lifted_block` / `wasi_free_lifted_list`).
    pub(super) fn scratch_save(&mut self) -> (u32, u32) {
        let saved_bump = self.add_local(ValType::I32);
        self.instruction(Instruction::GlobalGet(0));
        self.instruction(Instruction::LocalSet(saved_bump));
        let saved_pinned = self.add_local(ValType::I32);
        self.instruction(Instruction::GlobalGet(
            super::super::GLOBAL_SCRATCH_PINNED_HEAD,
        ));
        self.instruction(Instruction::LocalSet(saved_pinned));
        (saved_bump, saved_pinned)
    }

    /// Close the marshaling window opened by `scratch_save`: rewind the bump
    /// pointer and free every pinned-scratch block allocated inside it.
    pub(super) fn scratch_restore(&mut self, saved: (u32, u32)) {
        let (saved_bump, saved_pinned) = saved;
        self.instruction(Instruction::LocalGet(saved_bump));
        self.instruction(Instruction::GlobalSet(0));

        // Pop the window's pinned blocks down to the saved head. The `next`
        // link lives in the block header's second word (at `ptr - 4`), which
        // is unused while the block is allocated — so unlink before freeing,
        // since `pinned_free` reuses that word for the free list.
        let cur = self.add_local(ValType::I32);
        self.instruction(Instruction::Block(BlockType::Empty));
        self.instruction(Instruction::Loop(BlockType::Empty));
        self.instruction(Instruction::GlobalGet(
            super::super::GLOBAL_SCRATCH_PINNED_HEAD,
        ));
        self.instruction(Instruction::LocalTee(cur));
        self.instruction(Instruction::LocalGet(saved_pinned));
        self.instruction(Instruction::I32Eq);
        self.instruction(Instruction::BrIf(1));
        self.instruction(Instruction::LocalGet(cur));
        self.instruction(Instruction::I32Const(4));
        self.instruction(Instruction::I32Sub);
        self.instruction(Instruction::I32Load(wasm_encoder::MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        self.instruction(Instruction::GlobalSet(
            super::super::GLOBAL_SCRATCH_PINNED_HEAD,
        ));
        self.instruction(Instruction::LocalGet(cur));
        self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
        self.instruction(Instruction::Br(0));
        self.instruction(Instruction::End);
        self.instruction(Instruction::End);
    }

    /// Allocate `size` bytes (an i32 already on the stack) that outlive neither
    /// the enclosing marshaling window nor a suspension inside it, but whose
    /// size is not statically bounded — a marshaled string or byte list. The
    /// block comes from the pinned heap and is threaded onto the window's list
    /// so `scratch_restore` frees it. Leaves the pointer on the stack.
    fn scratch_pinned_alloc(&mut self) {
        let ptr = self.add_local(ValType::I32);
        self.instruction(Instruction::Call(self.codegen.func_pinned_alloc()));
        self.instruction(Instruction::LocalTee(ptr));
        self.instruction(Instruction::I32Const(4));
        self.instruction(Instruction::I32Sub);
        self.instruction(Instruction::GlobalGet(
            super::super::GLOBAL_SCRATCH_PINNED_HEAD,
        ));
        self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
            offset: 0,
            align: 2,
            memory_index: 0,
        }));
        self.instruction(Instruction::LocalGet(ptr));
        self.instruction(Instruction::GlobalSet(
            super::super::GLOBAL_SCRATCH_PINNED_HEAD,
        ));
        self.instruction(Instruction::LocalGet(ptr));
    }

    /// Allocate a pinned result block for an async import call.
    pub(super) fn emit_p3_pinned_retptr(&mut self, size: u32) -> u32 {
        let retptr = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(size as i32));
        self.instruction(Instruction::Call(self.codegen.func_pinned_alloc()));
        self.instruction(Instruction::LocalSet(retptr));
        retptr
    }

    /// With the async import's status on the stack, pack an `AsyncCall`:
    /// `(status as i64) << 32 | retptr`.
    pub(super) fn emit_p3_pack_async_call(&mut self, retptr: u32) {
        self.instruction(Instruction::I64ExtendI32U);
        self.instruction(Instruction::I64Const(32));
        self.instruction(Instruction::I64Shl);
        self.instruction(Instruction::LocalGet(retptr));
        self.instruction(Instruction::I64ExtendI32U);
        self.instruction(Instruction::I64Or);
    }

    /// Evaluate an `AsyncCall` argument and extract its pinned retptr into a
    /// local (for a finish intrinsic's lift).
    pub(super) fn emit_p3_unpack_retptr(&mut self, call_arg: &TypedExpr) -> u32 {
        let retptr = self.add_local(ValType::I32);
        self.emit_expr(call_arg, ExprContext::Value);
        self.instruction(Instruction::I32WrapI64);
        self.instruction(Instruction::LocalSet(retptr));
        retptr
    }

    /// Evaluate an i32-valued argument (handle/newtype) and store it into an
    /// indirect-params block at `offset`.
    pub(super) fn emit_p3_store_field(
        &mut self,
        pblock: u32,
        offset: u64,
        tmp: u32,
        arg: &TypedExpr,
    ) {
        self.emit_expr(arg, ExprContext::Value);
        self.instruction(Instruction::LocalSet(tmp));
        self.instruction(Instruction::LocalGet(pblock));
        self.instruction(Instruction::LocalGet(tmp));
        self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
            offset,
            align: 2,
            memory_index: 0,
        }));
    }

    /// Evaluate a string argument, marshal its bytes to scratch, and store
    /// (ptr, len) into an indirect-params block at `offset`/`offset + 4`.
    pub(super) fn emit_p3_store_string(&mut self, pblock: u32, offset: u64, arg: &TypedExpr) {
        self.emit_expr(arg, ExprContext::Value);
        let str_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(super::super::STRING_STRUCT_TYPE_INDEX),
        }));
        self.instruction(Instruction::LocalSet(str_local));
        let (str_ptr, str_len) = self.wasi_marshal_string_to_memory(str_local);
        self.instruction(Instruction::LocalGet(pblock));
        self.instruction(Instruction::LocalGet(str_ptr));
        self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
            offset,
            align: 2,
            memory_index: 0,
        }));
        self.instruction(Instruction::LocalGet(pblock));
        self.instruction(Instruction::LocalGet(str_len));
        self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
            offset: offset + 4,
            align: 2,
            memory_index: 0,
        }));
    }

    /// Marshal a `NewTimestamp` enum into an indirect-params block:
    /// disc U8 @offset, instant payload @offset+8 (seconds u64, nanos u32).
    pub(super) fn emit_p3_store_new_timestamp(
        &mut self,
        pblock: u32,
        offset: u64,
        arg: &TypedExpr,
    ) {
        let ts_mn = match &arg.ty {
            Type::Enum(_, mn) => mn.clone(),
            other => unreachable!("expected NewTimestamp enum, got {other}"),
        };
        let no_change_idx =
            self.codegen.variant_type_indices[&(ts_mn.clone(), "NoChange".to_string())];
        let now_idx = self.codegen.variant_type_indices[&(ts_mn.clone(), "Now".to_string())];
        let at_idx = self.codegen.variant_type_indices[&(ts_mn.clone(), "At".to_string())];

        self.emit_expr(arg, ExprContext::Value);
        let ts_base_idx = self.codegen.type_indices[&ts_mn];
        let ts_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(ts_base_idx),
        }));
        self.instruction(Instruction::LocalSet(ts_local));

        // disc
        self.instruction(Instruction::LocalGet(pblock));
        self.instruction(Instruction::LocalGet(ts_local));
        self.instruction(Instruction::RefTestNonNull(
            wasm_encoder::HeapType::Concrete(no_change_idx),
        ));
        self.instruction(Instruction::If(BlockType::Result(ValType::I32)));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::Else);
        self.instruction(Instruction::LocalGet(ts_local));
        self.instruction(Instruction::RefTestNonNull(
            wasm_encoder::HeapType::Concrete(now_idx),
        ));
        self.instruction(Instruction::If(BlockType::Result(ValType::I32)));
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::Else);
        self.instruction(Instruction::I32Const(2));
        self.instruction(Instruction::End);
        self.instruction(Instruction::End);
        self.instruction(Instruction::I32Store8(wasm_encoder::MemArg {
            offset,
            align: 0,
            memory_index: 0,
        }));

        // At(instant) payload
        self.instruction(Instruction::LocalGet(ts_local));
        self.instruction(Instruction::RefTestNonNull(
            wasm_encoder::HeapType::Concrete(at_idx),
        ));
        self.instruction(Instruction::If(BlockType::Empty));
        {
            let at_ref = self.add_local(ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(at_idx),
            }));
            self.instruction(Instruction::LocalGet(ts_local));
            self.instruction(Instruction::RefCastNonNull(
                wasm_encoder::HeapType::Concrete(at_idx),
            ));
            self.instruction(Instruction::LocalSet(at_ref));
            // Instant record ref in variant field 0
            let instant_ty = self.enum_variant_payload_types(&arg.ty, "At");
            let instant_mn = instant_ty[0].mangled_name();
            let instant_idx = self.codegen.type_indices[&instant_mn];
            let instant_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(instant_idx),
            }));
            self.instruction(Instruction::LocalGet(at_ref));
            self.instruction(Instruction::StructGet {
                struct_type_index: at_idx,
                field_index: 0,
            });
            self.instruction(Instruction::LocalSet(instant_local));
            // seconds u64 @offset+8
            self.instruction(Instruction::LocalGet(pblock));
            self.instruction(Instruction::LocalGet(instant_local));
            self.instruction(Instruction::StructGet {
                struct_type_index: instant_idx,
                field_index: 0,
            });
            self.instruction(Instruction::I64Store(wasm_encoder::MemArg {
                offset: offset + 8,
                align: 3,
                memory_index: 0,
            }));
            // nanos u32 @offset+16
            self.instruction(Instruction::LocalGet(pblock));
            self.instruction(Instruction::LocalGet(instant_local));
            self.instruction(Instruction::StructGet {
                struct_type_index: instant_idx,
                field_index: 1,
            });
            self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                offset: offset + 16,
                align: 2,
                memory_index: 0,
            }));
        }
        self.instruction(Instruction::End);
    }

    /// Marshal a `SocketAddress` enum into an indirect-params block at
    /// `offset` using the canonical memory layout of `ip-socket-address`:
    /// disc u8 @+0; v4 payload: port u16 @+4, address bytes @+6..10;
    /// v6 payload: port u16 @+4, flow-info u32 @+8, address 8*u16 @+12..28,
    /// scope-id u32 @+28. Variant size 32, align 4.
    pub(super) fn emit_p3_store_socket_address(
        &mut self,
        pblock: u32,
        offset: u64,
        arg: &TypedExpr,
    ) {
        let sa_mn = match &arg.ty {
            Type::Enum(_, mn) => mn.clone(),
            other => unreachable!("expected SocketAddress enum, got {other}"),
        };
        self.emit_expr(arg, ExprContext::Value);
        let sa_base_idx = self.codegen.type_indices[&sa_mn];
        let sa_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(sa_base_idx),
        }));
        self.instruction(Instruction::LocalSet(sa_local));
        self.emit_p3_store_socket_address_from_local(pblock, offset, &sa_mn, sa_local);
    }

    /// Store an `ip-socket-address` already held in `sa_local` into the block at
    /// `offset`. Split out of `emit_p3_store_socket_address` so the `option<...>`
    /// marshaler can reuse the payload layout without re-emitting the argument.
    ///
    /// The enum is decoded by `wasi_destructure_socket_address_flat` — the one
    /// place that knows how the Dovetail records map onto the canonical order —
    /// and this function only writes the flat locals out in the canonical
    /// memory layout of `ip-socket-address`.
    pub(super) fn emit_p3_store_socket_address_from_local(
        &mut self,
        pblock: u32,
        offset: u64,
        sa_mn: &MangledName,
        sa_local: u32,
    ) {
        let (disc_local, p_locals) = self.wasi_destructure_socket_address_flat(sa_mn, sa_local);

        let store8 = |e: &mut Self, off: u64| {
            e.instruction(Instruction::I32Store8(wasm_encoder::MemArg {
                offset: off,
                align: 0,
                memory_index: 0,
            }));
        };
        let store16 = |e: &mut Self, off: u64| {
            e.instruction(Instruction::I32Store16(wasm_encoder::MemArg {
                offset: off,
                align: 1,
                memory_index: 0,
            }));
        };
        let store32 = |e: &mut Self, off: u64| {
            e.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                offset: off,
                align: 2,
                memory_index: 0,
            }));
        };

        // disc u8 @+0 and port u16 @+4 (the port slot is shared by V4 and V6)
        self.instruction(Instruction::LocalGet(pblock));
        self.instruction(Instruction::LocalGet(disc_local));
        store8(self, offset);
        self.instruction(Instruction::LocalGet(pblock));
        self.instruction(Instruction::LocalGet(p_locals[0]));
        store16(self, offset + 4);

        self.instruction(Instruction::LocalGet(disc_local));
        self.instruction(Instruction::I32Eqz);
        self.instruction(Instruction::If(BlockType::Empty));
        {
            // V4: address bytes @+6..10 (p1..p4 = a, b, c, d)
            for i in 0..4u64 {
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(p_locals[1 + i as usize]));
                store8(self, offset + 6 + i);
            }
        }
        self.instruction(Instruction::Else);
        {
            // V6: flow-info u32 @+8 (p1)
            self.instruction(Instruction::LocalGet(pblock));
            self.instruction(Instruction::LocalGet(p_locals[1]));
            store32(self, offset + 8);
            // address 8*u16 @+12..28 (p2..p9)
            for i in 0..8u64 {
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(p_locals[2 + i as usize]));
                store16(self, offset + 12 + i * 2);
            }
            // scope-id u32 @+28 (p10)
            self.instruction(Instruction::LocalGet(pblock));
            self.instruction(Instruction::LocalGet(p_locals[10]));
            store32(self, offset + 28);
        }
        self.instruction(Instruction::End);
    }

    /// Marshal an `Option<SocketAddress>` into an indirect-params block as a
    /// canonical `option<ip-socket-address>`: discriminant (u8, but the payload's
    /// 4-byte alignment puts the address at `offset + 4`) then the address.
    /// `None` writes only the zero discriminant, leaving the payload untouched.
    pub(super) fn emit_p3_store_option_socket_address(
        &mut self,
        pblock: u32,
        offset: u64,
        arg: &TypedExpr,
    ) {
        let option_mn = match &arg.ty {
            Type::GenericEnum { mangled_name, .. } => mangled_name.clone(),
            other => unreachable!("expected Option<SocketAddress>, got {other}"),
        };
        let sa_mn = match &arg.ty {
            Type::GenericEnum { type_args, .. } => match &type_args[0].1 {
                Type::Enum(_, mn) => mn.clone(),
                other => unreachable!("expected SocketAddress in Option, got {other}"),
            },
            _ => unreachable!(),
        };
        let option_base_idx = self.codegen.type_indices[&option_mn];
        let some_idx = self.codegen.variant_type_indices[&(option_mn.clone(), "Some".to_string())];
        let sa_base_idx = self.codegen.type_indices[&sa_mn];

        self.emit_expr(arg, ExprContext::Value);
        let opt_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(option_base_idx),
        }));
        self.instruction(Instruction::LocalSet(opt_local));

        // Default the discriminant to 0 (None).
        self.instruction(Instruction::LocalGet(pblock));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::I32Store8(wasm_encoder::MemArg {
            offset,
            align: 0,
            memory_index: 0,
        }));

        self.instruction(Instruction::Block(BlockType::Empty));
        self.instruction(Instruction::LocalGet(opt_local));
        self.instruction(Instruction::RefTestNonNull(
            wasm_encoder::HeapType::Concrete(some_idx),
        ));
        self.instruction(Instruction::I32Eqz);
        self.instruction(Instruction::BrIf(0)); // None → leave disc 0

        self.instruction(Instruction::LocalGet(pblock));
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::I32Store8(wasm_encoder::MemArg {
            offset,
            align: 0,
            memory_index: 0,
        }));
        // Option's payload field is erased to anyref — cast it back before
        // storing it with the shared address layout.
        let sa_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(sa_base_idx),
        }));
        self.instruction(Instruction::LocalGet(opt_local));
        self.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(some_idx),
        ));
        self.instruction(Instruction::StructGet {
            struct_type_index: some_idx,
            field_index: self.codegen.id_prefix(&option_mn),
        });
        self.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(sa_base_idx),
        ));
        self.instruction(Instruction::LocalSet(sa_local));
        self.emit_p3_store_socket_address_from_local(pblock, offset + 4, &sa_mn, sa_local);
        self.instruction(Instruction::End);
    }

    /// Lift `result<tuple<list<u8>, ip-socket-address>, error-code>` from a
    /// pinned retptr block into a `Datagram` record: disc @0, and at +4 either
    /// the error disc or the tuple (list ptr @4, list len @8, address @12).
    /// Frees the host-allocated payload buffer after copying it into a GC array;
    /// the block itself is freed by the caller.
    pub(super) fn wasi_construct_result_datagram_network_error(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
    ) {
        let result_ref = self.result_ref(result_type);

        // Ok type is the `Datagram` record { data: Array<Uint8>, remoteAddress }.
        let datagram_type = match result_type {
            Type::GenericEnum { type_args, .. } => type_args[0].1.clone(),
            _ => unreachable!("Expected GenericEnum Result type"),
        };
        let datagram_mn = match &datagram_type {
            Type::Record(_, mn) => mn.clone(),
            other => unreachable!("Expected Datagram record, got {other}"),
        };
        let datagram_idx = self.codegen.type_indices[&datagram_mn];
        let sa_mn = self.find_record_field_type_mn(&datagram_mn, 1);

        self.wasi_i32_load8_u(retptr_local, 0);
        self.emit_if_block(BlockType::Result(result_ref));
        // then: error — disc @4, and its `other(some(_))` message with it
        self.wasi_construct_network_error(retptr_local, 4, result_type);
        self.wasi_construct_result_error(result_type, &Self::wasi_result_error_type(result_type));
        self.instruction(Instruction::Else);
        // else: ok — payload list @4/@8, peer address @12
        {
            let data_ptr = self.add_local(ValType::I32);
            let data_len = self.add_local(ValType::I32);
            self.wasi_i32_load(retptr_local, 4);
            self.instruction(Instruction::LocalSet(data_ptr));
            self.wasi_i32_load(retptr_local, 8);
            self.instruction(Instruction::LocalSet(data_len));
            self.wasi_create_u8_array_from_bytes(data_ptr, data_len);
            // Free the host-realloc'd payload now that it is copied out. Through
            // the guarded helper, not `pinned_free`: a zero-length datagram is
            // legal, and an empty list lowers as a null or small dangling
            // pointer that never went through `cabi_realloc`.
            self.wasi_free_lifted_block(data_ptr);
            self.wasi_construct_socket_address(retptr_local, 12, &sa_mn);
            self.instruction(Instruction::StructNew(datagram_idx));
            self.wasi_construct_result_ok(result_type, &Self::wasi_result_ok_type(result_type));
        }
        self.emit_end_block();
    }

    /// Post an async stream write of `bytes[offset..]`: pinned-alloc a buffer,
    /// copy that slice into it, call the async-lowered `stream.write` import,
    /// and pack the result as a `StreamOp`.
    ///
    /// The offset is taken here rather than by slicing in Dovetail: a partial
    /// completion re-posts the remainder, and re-slicing per retry made writing
    /// an n-byte array O(n²).
    /// Emit the i32 stream/future handle an intrinsic needs from `arg`.
    ///
    /// `field` is `None` when the argument already IS the handle (the fs and
    /// socket streams pass one around), and `Some(index)` when it is a record
    /// that CONTAINS it — the wasi:cli stdio streams take the whole stream value
    /// as `self`, so that one value can carry both the stream end and the
    /// one-shot result future and each intrinsic picks the half it drives.
    pub(super) fn emit_p3_stream_handle(&mut self, arg: &TypedExpr, field: Option<u32>) {
        self.emit_expr(arg, ExprContext::Value);
        if let Some(field_index) = field {
            let mangled = arg.ty.mangled_name();
            let struct_type_index = self.codegen.type_indices[&mangled];
            self.instruction(Instruction::StructGet {
                struct_type_index,
                field_index,
            });
        }
    }

    pub(super) fn emit_p3_stream_write_start(
        &mut self,
        stream_arg: &TypedExpr,
        stream_field: Option<u32>,
        slice_arg: &TypedExpr,
        write_func: u32,
    ) {
        // Evaluate arguments in source order. ReadonlySlice carries an opaque
        // array reference, offset, and length; this boundary requires Uint8 storage.
        self.emit_p3_stream_handle(stream_arg, stream_field);
        let stream = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalSet(stream));
        let array_type_index = self.codegen.array_type_index(&Type::Uint8);
        self.emit_expr(slice_arg, ExprContext::Value);
        let len = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalSet(len));
        let offset = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalSet(offset));
        let backing = self.add_local(ValType::Ref(wasm_encoder::RefType::ANYREF));
        self.instruction(Instruction::LocalSet(backing));
        self.instruction(Instruction::LocalGet(len));
        let buf = self.add_local(ValType::I32);
        self.instruction(Instruction::Call(self.codegen.func_pinned_alloc()));
        self.instruction(Instruction::LocalSet(buf));
        // Covariant empty views may retain Never-array backing. They require
        // neither a Uint8 backing cast nor a copy, but still post a zero-byte write.
        self.instruction(Instruction::LocalGet(len));
        self.emit_if_block(BlockType::Empty);
        let arr = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(array_type_index),
        }));
        self.instruction(Instruction::LocalGet(backing));
        self.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(array_type_index),
        ));
        self.instruction(Instruction::LocalSet(arr));
        self.wasi_copy_u8_array_slice_to_memory(arr, Some(offset), buf, len, array_type_index);
        self.emit_end_block();
        // post the bounded write
        self.instruction(Instruction::LocalGet(stream));
        self.instruction(Instruction::LocalGet(buf));
        self.instruction(Instruction::LocalGet(len));
        self.instruction(Instruction::Call(write_func));
        // pack StreamOp
        self.instruction(Instruction::I64ExtendI32S);
        self.instruction(Instruction::I64Const(32));
        self.instruction(Instruction::I64Shl);
        self.instruction(Instruction::LocalGet(buf));
        self.instruction(Instruction::I64ExtendI32U);
        self.instruction(Instruction::I64Or);
    }

    /// Post an async stream read: pinned-alloc a `capacity` buffer, call the
    /// async-lowered `stream.read` import for this stream's origin, and pack
    /// the result as a `StreamOp` (`(result as i64) << 32 | buffer`).
    ///
    /// `capacity` IS AN ALLOCATION SIZE, not a ceiling on the answer. The first
    /// thing emitted here is `pinned_alloc(capacity)`, and the copy the host
    /// fills is that block; a read that comes back with fewer bytes is ordinary
    /// and costs the caller the whole buffer regardless. So whatever number a
    /// Dovetail caller passes to `Async.streamRead` is a number of bytes of linear
    /// memory this component asks the host for, right now — `read(Int64.max)`
    /// meant a 2 GiB `memory.grow`, and that cost two review rounds to find,
    /// once as a silent truncation and once as a trap. Bounding it is the
    /// CALLER'S job: `AsyncInputStream.readChunkBytes` (64 KiB) is where the
    /// library does it.
    ///
    /// No ceiling is emitted here, deliberately. The value is dynamic — an
    /// arbitrary expression, so nothing can be checked at compile time — and a
    /// bound in the emitter would be a second, invisible policy competing with
    /// the library's, silently clamping (a wrong answer) or trapping at a limit
    /// no Dovetail source mentions. What is already checked, one level down in
    /// `pinned_alloc`, is the part that is not a policy: a size whose 8-byte
    /// rounding wraps traps, and so does a growth the host refuses. A negative
    /// capacity takes the first of those, which is why this emitter can treat
    /// "absurd" as someone else's problem and "negative" as impossible.
    pub(super) fn emit_p3_stream_read_start(
        &mut self,
        stream_arg: &TypedExpr,
        stream_field: Option<u32>,
        capacity_arg: &TypedExpr,
        read_func: u32,
    ) {
        let cap = self.add_local(ValType::I32);
        let buf = self.add_local(ValType::I32);
        self.emit_expr(capacity_arg, ExprContext::Value);
        self.instruction(Instruction::LocalTee(cap));
        self.instruction(Instruction::Call(self.codegen.func_pinned_alloc()));
        self.instruction(Instruction::LocalSet(buf));
        self.emit_p3_stream_handle(stream_arg, stream_field);
        self.instruction(Instruction::LocalGet(buf));
        self.instruction(Instruction::LocalGet(cap));
        self.instruction(Instruction::Call(read_func));
        // pack: high 32 = raw result (sign-extended so BLOCKED stays -1),
        // low 32 = pinned buffer
        self.instruction(Instruction::I64ExtendI32S);
        self.instruction(Instruction::I64Const(32));
        self.instruction(Instruction::I64Shl);
        self.instruction(Instruction::LocalGet(buf));
        self.instruction(Instruction::I64ExtendI32U);
        self.instruction(Instruction::I64Or);
    }

    /// Bump-allocate `size` bytes from global 0 (linear memory bump allocator)
    /// with 8-byte alignment (required by canonical ABI for i64/u64 payloads).
    /// Returns a local index holding the base pointer.
    ///
    /// Every scratch allocation goes through here, so this is the one place
    /// the arena's bound is enforced: the arena is page 0 and the pinned heap
    /// starts at `SCRATCH_LIMIT`, so an allocation that would cross it must
    /// trap rather than silently overwrite live pinned blocks (posted stream
    /// buffers of parked fibers, async-call result blocks). Only statically
    /// small, fixed-size blocks come from here — dynamically-sized payloads go
    /// through `scratch_pinned_alloc` — so a trap here means a genuine leak of
    /// window discipline, not a large payload.
    pub(super) fn wasi_bump_alloc(&mut self, size: u32) -> u32 {
        let ptr = self.add_local(ValType::I32);
        // Align global pointer to 8 bytes: ptr = (global + 7) & ~7
        self.instruction(Instruction::GlobalGet(0));
        self.instruction(Instruction::I32Const(7));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::I32Const(-8)); // ~7 = 0xFFFFFFF8
        self.instruction(Instruction::I32And);
        self.instruction(Instruction::LocalTee(ptr));
        self.instruction(Instruction::I32Const(size as i32));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::GlobalSet(0));
        self.instruction(Instruction::GlobalGet(0));
        self.instruction(Instruction::I32Const(super::super::SCRATCH_LIMIT));
        self.instruction(Instruction::I32GtU);
        self.instruction(Instruction::If(BlockType::Empty));
        self.instruction(Instruction::Unreachable);
        self.instruction(Instruction::End);
        ptr
    }

    /// Free a block the host handed us while lowering a lifted value. Those
    /// blocks come from `cabi_realloc`, i.e. from the pinned heap, and the
    /// guest owns them once the value has been copied into GC memory — without
    /// this they accumulate until `pinned_alloc` runs the heap out.
    ///
    /// The bound check keeps the free honest: an empty list or string can be
    /// lowered as a null or small dangling aligned pointer that never went
    /// through `cabi_realloc`, and `pinned_free` writes through `ptr - 8`.
    pub(super) fn wasi_free_lifted_block(&mut self, ptr_local: u32) {
        self.instruction(Instruction::LocalGet(ptr_local));
        self.instruction(Instruction::I32Const(super::super::SCRATCH_LIMIT));
        self.instruction(Instruction::I32GeU);
        self.instruction(Instruction::If(BlockType::Empty));
        self.instruction(Instruction::LocalGet(ptr_local));
        self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
        self.instruction(Instruction::End);
    }

    /// Free the message a p3 variant carried in its `other(some(msg))` case.
    ///
    /// The filesystem `error-code` is a variant whose last case is
    /// `other(option<string>)`, and the Dovetail `FileSystemError` keeps only the
    /// discriminant — so nothing downstream can ever see that string, let alone
    /// free it, and the host `cabi_realloc`'d it into the pinned heap. One
    /// leaked block per error is not much until the error is the one a retry
    /// loop keeps hitting. (The sockets and ip-name-lookup error codes used to
    /// share this fate but now lift the message into their `Other`'s
    /// `Option<String>` payload — see `wasi_construct_error_message_option` —
    /// so their lifts must not also come through here.)
    ///
    /// `descriptor-type` has exactly the same shape and the same problem, but on
    /// *ok* branches — `stat`, `get-type` and each directory entry — so this
    /// helper takes the discriminant rather than assuming an error code, and
    /// `offset` names wherever the variant sits, not just wherever an error
    /// would.
    ///
    /// Layout, from `offset` (where the caller already found the variant's
    /// discriminant): the `option<string>` payload aligns to 4, so the case
    /// discriminant is a u8 at `+0`, the option's own discriminant a u8 at `+4`,
    /// and the string is `ptr` at `+8` / `len` at `+12` — 16 bytes, align 4.
    ///
    /// wasmtime 45 only ever sends `other(none)`, so the inner block does not
    /// currently run on this host; the ABI permits `some`, and the block would
    /// be ours.
    pub(super) fn wasi_free_error_message(
        &mut self,
        retptr_local: u32,
        offset: u64,
        other_disc: i32,
    ) {
        let msg_ptr = self.add_local(ValType::I32);
        self.wasi_i32_load8_u(retptr_local, offset);
        self.instruction(Instruction::I32Const(other_disc));
        self.instruction(Instruction::I32Eq);
        // `!= 0` rather than the raw byte: this is an `and` of two conditions,
        // and and-ing a boolean with a payload byte is only accidentally right
        // when that byte is 0 or 1.
        self.wasi_i32_load8_u(retptr_local, offset + 4); // option disc: 1 = some
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::I32Ne);
        self.instruction(Instruction::I32And);
        self.instruction(Instruction::If(BlockType::Empty));
        self.wasi_i32_load(retptr_local, offset + 8);
        self.instruction(Instruction::LocalSet(msg_ptr));
        self.wasi_free_lifted_block(msg_ptr);
        self.instruction(Instruction::End);
    }

    /// Free everything a lifted host list borrowed: the string payload at each
    /// of `string_offsets` within every `stride`-byte element, then the list
    /// block itself. Call once the whole list has been copied into GC memory.
    pub(super) fn wasi_free_lifted_list(
        &mut self,
        list_ptr: u32,
        list_len: u32,
        stride: i32,
        string_offsets: &[u64],
    ) {
        let idx = self.add_local(ValType::I32);
        let elem_base = self.add_local(ValType::I32);
        let str_ptr = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(idx));
        self.instruction(Instruction::Block(BlockType::Empty));
        self.instruction(Instruction::Loop(BlockType::Empty));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::LocalGet(list_len));
        self.instruction(Instruction::I32GeU);
        self.instruction(Instruction::BrIf(1));
        self.instruction(Instruction::LocalGet(list_ptr));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Const(stride));
        self.instruction(Instruction::I32Mul);
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(elem_base));
        for &offset in string_offsets {
            self.wasi_i32_load(elem_base, offset);
            self.instruction(Instruction::LocalSet(str_ptr));
            self.wasi_free_lifted_block(str_ptr);
        }
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(idx));
        self.instruction(Instruction::Br(0));
        self.instruction(Instruction::End); // end loop
        self.instruction(Instruction::End); // end block
        self.wasi_free_lifted_block(list_ptr);
    }

    /// Emit i32.load at memory[ptr_local + offset]. Leaves i32 on stack.
    pub(super) fn wasi_i32_load(&mut self, ptr_local: u32, offset: u64) {
        self.instruction(Instruction::LocalGet(ptr_local));
        self.instruction(Instruction::I32Load(wasm_encoder::MemArg {
            offset,
            align: 0,
            memory_index: 0,
        }));
    }

    /// Emit i64.load at memory[ptr_local + offset]. Leaves i64 on stack.
    pub(super) fn wasi_i64_load(&mut self, ptr_local: u32, offset: u64) {
        self.instruction(Instruction::LocalGet(ptr_local));
        self.instruction(Instruction::I64Load(wasm_encoder::MemArg {
            offset,
            align: 0,
            memory_index: 0,
        }));
    }

    /// Emit i32.load8_u at memory[ptr_local + offset]. Leaves i32 (u8 value) on stack.
    pub(super) fn wasi_i32_load8_u(&mut self, ptr_local: u32, offset: u64) {
        self.instruction(Instruction::LocalGet(ptr_local));
        self.instruction(Instruction::I32Load8U(wasm_encoder::MemArg {
            offset,
            align: 0,
            memory_index: 0,
        }));
    }

    /// Copy u8 elements from a GC Array<Uint8> to linear memory.
    /// Used for Array<Uint8> → list<u8> marshaling (OutputStreamWrite).
    pub(super) fn wasi_copy_u8_array_to_memory(
        &mut self,
        array_local: u32,
        dest_ptr_local: u32,
        len_local: u32,
        array_type_index: u32,
    ) {
        self.wasi_copy_u8_array_slice_to_memory(
            array_local,
            None,
            dest_ptr_local,
            len_local,
            array_type_index,
        );
    }

    /// Copy `len` u8 elements starting at `src_offset_local` (or 0) from a GC
    /// `Array<Uint8>` into linear memory at `dest_ptr_local`.
    pub(super) fn wasi_copy_u8_array_slice_to_memory(
        &mut self,
        array_local: u32,
        src_offset_local: Option<u32>,
        dest_ptr_local: u32,
        len_local: u32,
        array_type_index: u32,
    ) {
        let idx = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(idx));

        self.instruction(Instruction::Block(BlockType::Empty));
        self.instruction(Instruction::Loop(BlockType::Empty));

        // if idx >= len, break
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::LocalGet(len_local));
        self.instruction(Instruction::I32GeU);
        self.instruction(Instruction::BrIf(1));

        // memory[dest + idx] = array[srcOffset + idx] (u8, zero-extended to i32)
        self.instruction(Instruction::LocalGet(dest_ptr_local));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalGet(array_local));
        self.instruction(Instruction::LocalGet(idx));
        if let Some(src_offset) = src_offset_local {
            self.instruction(Instruction::LocalGet(src_offset));
            self.instruction(Instruction::I32Add);
        }
        self.instruction(Instruction::ArrayGetU(array_type_index));
        self.instruction(Instruction::I32Store8(wasm_encoder::MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));

        // idx++
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(idx));

        self.instruction(Instruction::Br(0));
        self.instruction(Instruction::End); // end loop
        self.instruction(Instruction::End); // end block
    }

    /// Create a GC Array<Uint8> from bytes in linear memory.
    /// Used for list<u8> → Array<Uint8> marshaling (InputStreamRead).
    /// Leaves the array ref on the stack.
    pub(super) fn wasi_create_u8_array_from_bytes(&mut self, src_ptr_local: u32, len_local: u32) {
        let array_type_index = self.codegen.array_type_index(&Type::Uint8);
        let array_ref_type = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(array_type_index),
        });

        // Create array of len elements (default 0)
        let array_local = self.add_local(array_ref_type);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalGet(len_local));
        self.instruction(Instruction::ArrayNew(array_type_index));
        self.instruction(Instruction::LocalSet(array_local));

        // Copy loop: array[i] = (i32) memory[src + i]
        let idx = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(idx));

        self.instruction(Instruction::Block(BlockType::Empty));
        self.instruction(Instruction::Loop(BlockType::Empty));

        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::LocalGet(len_local));
        self.instruction(Instruction::I32GeU);
        self.instruction(Instruction::BrIf(1));

        self.instruction(Instruction::LocalGet(array_local));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::LocalGet(src_ptr_local));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::I32Load8U(wasm_encoder::MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
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

    /// Create a Dovetail String from string data in linear memory (ptr, len).
    /// Copies bytes into a GC backing array, then calls self.codegen.func_string_from_bytes().
    /// Leaves string ref on stack.
    pub(super) fn wasi_create_string_from_memory(
        &mut self,
        data_ptr_local: u32,
        data_len_local: u32,
    ) {
        let backing_type_index = super::super::U8_BACKING_TYPE_INDEX;
        let backing_ref_type = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(backing_type_index),
        });

        // Create backing array of data_len bytes (all zeros)
        let backing_local = self.add_local(backing_ref_type);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalGet(data_len_local));
        self.instruction(Instruction::ArrayNew(backing_type_index));
        self.instruction(Instruction::LocalSet(backing_local));

        // Copy loop: backing[i] = memory[ptr + i]
        let idx = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(idx));

        self.instruction(Instruction::Block(BlockType::Empty));
        self.instruction(Instruction::Loop(BlockType::Empty));

        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::LocalGet(data_len_local));
        self.instruction(Instruction::I32GeU);
        self.instruction(Instruction::BrIf(1));

        self.instruction(Instruction::LocalGet(backing_local));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::LocalGet(data_ptr_local));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::I32Load8U(wasm_encoder::MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
        self.instruction(Instruction::ArraySet(backing_type_index));

        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(idx));

        self.instruction(Instruction::Br(0));
        self.instruction(Instruction::End);
        self.instruction(Instruction::End);

        // Call string_from_bytes(backing, 0, len) → string struct
        self.instruction(Instruction::LocalGet(backing_local));
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalGet(data_len_local));
        self.instruction(Instruction::Call(self.codegen.func_string_from_bytes()));
    }

    /// Construct Result::Ok(value) where the value is already on the stack.
    /// Construct Result::Ok(value) where the value is already on the stack.
    ///
    /// Under full type erasure, Result.Ok's payload slot lowers to anyref, so primitive
    /// values must be boxed before `struct.new` so the slot's expected ref-type matches.
    /// Reference values (records, classes, enums) flow through unchanged via implicit
    /// upcast to `anyref`.
    pub(super) fn wasi_construct_result_ok(&mut self, result_type: &Type, ok_value_ty: &Type) {
        let result_mn = result_type.mangled_name();
        self.box_value_for_erased_slot(ok_value_ty);
        let ok_idx = self.codegen.variant_type_indices[&(result_mn.clone(), "Ok".to_string())];
        self.emit_nominal_struct_new(result_type, ok_idx);
    }

    /// Construct Result::Error(error) where the error value is already on the stack.
    ///
    /// Like `wasi_construct_result_ok`, Error's payload slot is anyref under erasure, so
    /// primitives must be boxed first.
    pub(super) fn wasi_construct_result_error(&mut self, result_type: &Type, err_value_ty: &Type) {
        let result_mn = result_type.mangled_name();
        self.box_value_for_erased_slot(err_value_ty);
        let error_idx =
            self.codegen.variant_type_indices[&(result_mn.clone(), "Error".to_string())];
        self.emit_nominal_struct_new(result_type, error_idx);
    }

    /// If `value_ty` is a primitive (lowers to a non-ref WASM valtype), box it via
    /// `struct.new` of the matching `Box*` type. Reference types and type parameters are
    /// already anyref-compatible and pass through untouched. A tuple's flattened values are
    /// reboxed into its `(ref $Tuple_N)` struct.
    pub(super) fn box_value_for_erased_slot(&mut self, value_ty: &Type) {
        if matches!(
            value_ty,
            Type::TypeVariable(_, _) | Type::GenericParam(_, _, _) | Type::Any
        ) {
            return;
        }
        if self.codegen.is_tuple(value_ty) {
            self.emit_rebox_tuple(value_ty);
            return;
        }
        if value_ty.is_reference_type() {
            return;
        }
        let box_idx = self.codegen.box_type_index_for(value_ty);
        self.instruction(Instruction::StructNew(box_idx));
    }

    /// Extract the Ok payload type from a `Result<Ok, Error>` type.
    pub(super) fn wasi_result_ok_type(result_type: &Type) -> Type {
        match result_type {
            Type::GenericEnum { type_args, .. } if type_args.len() == 2 => type_args[0].1.clone(),
            _ => unreachable!(
                "wasi_result_ok_type: expected Result<Ok, Error>, got {}",
                result_type
            ),
        }
    }

    /// Extract the Error payload type from a `Result<Ok, Error>` type.
    pub(super) fn wasi_result_error_type(result_type: &Type) -> Type {
        match result_type {
            Type::GenericEnum { type_args, .. } if type_args.len() == 2 => type_args[1].1.clone(),
            _ => unreachable!(
                "wasi_result_error_type: expected Result<Ok, Error>, got {}",
                result_type
            ),
        }
    }

    /// Copy bytes from a u8 backing array (GC) to linear memory.
    /// Used for marshaling Dovetail strings to WASI string params.
    pub(super) fn wasi_copy_u8_backing_to_memory(
        &mut self,
        backing_local: u32,
        dest_ptr_local: u32,
        len_local: u32,
    ) {
        let backing_type_index = super::super::U8_BACKING_TYPE_INDEX;
        let idx = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(idx));

        self.instruction(Instruction::Block(BlockType::Empty));
        self.instruction(Instruction::Loop(BlockType::Empty));

        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::LocalGet(len_local));
        self.instruction(Instruction::I32GeU);
        self.instruction(Instruction::BrIf(1));

        // memory[dest + idx] = backing[idx]
        self.instruction(Instruction::LocalGet(dest_ptr_local));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalGet(backing_local));
        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::ArrayGetU(backing_type_index));
        self.instruction(Instruction::I32Store8(wasm_encoder::MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));

        self.instruction(Instruction::LocalGet(idx));
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::LocalSet(idx));

        self.instruction(Instruction::Br(0));
        self.instruction(Instruction::End);
        self.instruction(Instruction::End);
    }

    /// Lift a host discriminant on the stack into a Dovetail enum, dispatching
    /// with a `br_table`.
    ///
    /// The four p3 enums Dovetail lifts this way — the sockets `error-code`, the
    /// ip-name-lookup `error-code`, the filesystem `error-code`, and
    /// `descriptor-type` — differ in three things and nothing else: which enum,
    /// its pinned variant table, and where that table's `other` case sits. So
    /// they share one emitter; four copies of it is four places for a fix to
    /// this dispatch to be applied three times.
    ///
    /// `other_payload`, when given as `(retptr_local, offset)` (the same offset
    /// the caller found the variant's discriminant at), lifts the `other` case's
    /// `option<string>` message into the Dovetail variant's `Option<String>`
    /// payload instead of dropping it: the host's one piece of explanatory text
    /// for an error its enum cannot name. The message block was `cabi_realloc`'d
    /// by the host, so after copying it into a GC string the block is freed here
    /// — callers must NOT also call `wasi_free_error_message` on that variant.
    /// With `None`, every case (including `other`) is constructed payload-less
    /// and any message must be freed by the caller (the filesystem paths still
    /// work this way).
    ///
    /// Shape:
    /// ```text
    /// block $out (result enum_ref)
    ///   block $b<n-1> ... block $b0
    ///     local.get disc
    ///     br_table $b0 $b1 ... $b<n-1> (default $b<other>)
    ///   end $b0: struct.new <names[0]>; br $out
    ///   ...
    /// end $out
    /// ```
    ///
    /// Branch depth i lands on variant i, so the default must name the case that
    /// exists to absorb what the table cannot name: `other`. A default of 0 sent
    /// every discriminant past the end of the table — the ones a newer host
    /// would send — to the FIRST case, reporting an unrecognised sockets error
    /// as `AccessDenied` rather than `Other`.
    ///
    /// So `other_disc` — not 0, and not "the first case" — is the only correct
    /// default here, and it is the one thing in this function that cannot be
    /// changed to match a nearby example without reintroducing that bug. The
    /// variant tables are pinned against the WIT at compile time (see the
    /// `other_case_is_at` assertions), which is what makes `other_disc` known
    /// statically rather than guessed.
    fn wasi_construct_pinned_enum(
        &mut self,
        enum_mn: &MangledName,
        variant_names: &[&str],
        other_disc: i32,
        other_payload: Option<(u32, u64)>,
    ) {
        let disc_local = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalSet(disc_local));

        let base_idx = self.codegen.type_indices[enum_mn];
        let enum_ref_type = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(base_idx),
        });

        let num_variants = variant_names.len() as u32;

        self.instruction(Instruction::Block(BlockType::Result(enum_ref_type)));
        for _ in 0..num_variants {
            self.instruction(Instruction::Block(BlockType::Empty));
        }

        self.instruction(Instruction::LocalGet(disc_local));
        let targets: Vec<u32> = (0..num_variants).collect();
        self.instruction(Instruction::BrTable(targets.into(), other_disc as u32));

        for (i, name) in variant_names.iter().enumerate() {
            self.instruction(Instruction::End); // end block $bi
            if i as i32 == other_disc
                && let Some((retptr_local, offset)) = other_payload
            {
                self.wasi_construct_error_message_option(enum_mn, retptr_local, offset);
            }
            let variant_idx =
                self.codegen.variant_type_indices[&(enum_mn.clone(), name.to_string())];
            self.instruction(Instruction::StructNew(variant_idx));
            // After ending block $bi the blocks still open are the
            // (num_variants - i - 1) remaining inner ones plus $out, so $out is
            // at that depth.
            self.instruction(Instruction::Br(num_variants - i as u32 - 1));
        }

        self.instruction(Instruction::End); // end $out
    }

    /// Lift the `other(option<string>)` message of a p3 error-code variant at
    /// `retptr_local + offset` into the `Option<String>` payload of the Dovetail
    /// enum's `Other` case, leaving the option ref on the stack.
    ///
    /// Layout is the one `wasi_free_error_message` documents: the case
    /// discriminant is a u8 at `+0`, the option's own discriminant a u8 at
    /// `+4`, the string `ptr` at `+8` / `len` at `+12`. The host allocated the
    /// string bytes through `cabi_realloc`; once they are copied into a GC
    /// string the block is freed here, so this lift replaces (never combines
    /// with) `wasi_free_error_message` on the same variant.
    ///
    /// The `Option<String>` instantiation is read off the enum's own `Other`
    /// payload declaration rather than synthesized, so its mangled name cannot
    /// drift from what monomorphization produced for the declaration.
    fn wasi_construct_error_message_option(
        &mut self,
        enum_mn: &MangledName,
        retptr_local: u32,
        offset: u64,
    ) {
        let option_type = match &self.codegen.typed_module.types[enum_mn] {
            crate::typechecker::types::TypeDef::Enum(e) => e
                .variants
                .iter()
                .find(|v| v.name == "Other")
                .unwrap()
                .payload_types[0]
                .clone(),
            _ => unreachable!(),
        };
        let option_mn = option_type.mangled_name();
        let option_base_idx = self.codegen.type_indices[&option_mn];
        let option_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(option_base_idx),
        });

        self.wasi_i32_load8_u(retptr_local, offset + 4);
        self.emit_if_block(BlockType::Result(option_ref));
        // then: disc != 0 → Some(message)
        {
            let str_ptr = self.add_local(ValType::I32);
            let str_len = self.add_local(ValType::I32);
            self.wasi_i32_load(retptr_local, offset + 8);
            self.instruction(Instruction::LocalSet(str_ptr));
            self.wasi_i32_load(retptr_local, offset + 12);
            self.instruction(Instruction::LocalSet(str_len));
            self.wasi_create_string_from_memory(str_ptr, str_len);
            self.wasi_free_lifted_block(str_ptr);
            let some_idx =
                self.codegen.variant_type_indices[&(option_mn.clone(), "Some".to_string())];
            self.emit_nominal_struct_new(&option_type, some_idx);
        }
        self.instruction(Instruction::Else);
        // else: None
        {
            let none_idx =
                self.codegen.variant_type_indices[&(option_mn.clone(), "None".to_string())];
            self.emit_nominal_struct_new(&option_type, none_idx);
        }
        self.emit_end_block();
    }

    /// Lift the `wasi:sockets` error-code variant at `retptr_local + offset`
    /// into a `NetworkError`. The WASI sockets `error-code` maps 1:1 to the
    /// pinned variant table; the `other(option<string>)` case's message is
    /// carried into `NetworkError.Other`'s `Option<String>` payload (and its
    /// host-allocated block freed), so callers must not also call
    /// `wasi_free_error_message` on it.
    /// Expects result_type to be Result<T, NetworkError> to extract the NetworkError MangledName.
    /// Leaves a NetworkError enum ref on the stack.
    pub(super) fn wasi_construct_network_error(
        &mut self,
        retptr_local: u32,
        offset: u64,
        result_type: &Type,
    ) {
        // Extract NetworkError type from Result<T, NetworkError>
        let network_error_type = match result_type {
            Type::GenericEnum { type_args, .. } => &type_args[1].1,
            _ => unreachable!("Expected GenericEnum Result type"),
        };
        let ne_mn = match network_error_type {
            Type::Enum(_, mn) => mn.clone(),
            _ => unreachable!(
                "Expected Enum NetworkError type, got: {}",
                network_error_type
            ),
        };

        self.wasi_i32_load8_u(retptr_local, offset);
        self.wasi_construct_pinned_enum(
            &ne_mn,
            &NETWORK_ERROR_VARIANT_NAMES,
            NETWORK_ERROR_OTHER_DISC,
            Some((retptr_local, offset)),
        );
    }

    /// Lift a `wasi:sockets/ip-name-lookup` error-code variant at
    /// `retptr_local + offset` into a `DnsError`. That interface has its OWN
    /// variant list, distinct from the sockets `error-code` handled by
    /// `wasi_construct_network_error` — hence a separate mapping. As there,
    /// the `other` case's message is carried into `DnsError.Other`'s
    /// `Option<String>` payload and its block freed.
    pub(super) fn wasi_construct_dns_error(
        &mut self,
        retptr_local: u32,
        offset: u64,
        result_type: &Type,
    ) {
        let dns_error_type = match result_type {
            Type::GenericEnum { type_args, .. } => &type_args[1].1,
            _ => unreachable!("Expected GenericEnum Result type"),
        };
        let de_mn = match dns_error_type {
            Type::Enum(_, mn) => mn.clone(),
            _ => unreachable!("Expected Enum DnsError type, got: {dns_error_type}"),
        };

        self.wasi_i32_load8_u(retptr_local, offset);
        self.wasi_construct_pinned_enum(
            &de_mn,
            &DNS_ERROR_VARIANT_NAMES,
            DNS_ERROR_OTHER_DISC,
            Some((retptr_local, offset)),
        );
    }

    /// Lift a flat `ip-address-family` enum return (0 = ipv4, 1 = ipv6, already
    /// on the stack) into the Dovetail `IpAddressFamily` enum. A two-case WIT
    /// enum with no catch-all, so a plain if/else rather than the pinned
    /// br_table dispatch.
    pub(super) fn wasi_construct_ip_address_family(&mut self, family_type: &Type) {
        let family_mn = match family_type {
            Type::Enum(_, mn) => mn.clone(),
            _ => unreachable!("Expected Enum IpAddressFamily type, got: {family_type}"),
        };
        let base_idx = self.codegen.type_indices[&family_mn];
        let family_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(base_idx),
        });

        self.emit_if_block(BlockType::Result(family_ref));
        // then: disc != 0 → Ipv6
        {
            let ipv6_idx =
                self.codegen.variant_type_indices[&(family_mn.clone(), "Ipv6".to_string())];
            self.instruction(Instruction::StructNew(ipv6_idx));
        }
        self.instruction(Instruction::Else);
        // else: Ipv4
        {
            let ipv4_idx =
                self.codegen.variant_type_indices[&(family_mn.clone(), "Ipv4".to_string())];
            self.instruction(Instruction::StructNew(ipv4_idx));
        }
        self.emit_end_block();
    }

    /// Lift one canonical `ip-address` at `retptr_local + base_offset` onto the
    /// stack. Layout: variant disc (u8) at +0, payload at +2 (the variant's
    /// alignment is 2, from ipv6's u16 fields). ipv4 = 4 x u8, ipv6 = 8 x u16.
    pub(super) fn wasi_construct_ip_address(
        &mut self,
        retptr_local: u32,
        base_offset: u64,
        ip_addr_mn: &MangledName,
    ) {
        let ip_base_idx = self.codegen.type_indices[ip_addr_mn];
        let ip_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(ip_base_idx),
        });

        let v4_idx = self.codegen.variant_type_indices[&(ip_addr_mn.clone(), "V4".to_string())];
        let v6_idx = self.codegen.variant_type_indices[&(ip_addr_mn.clone(), "V6".to_string())];

        self.wasi_i32_load8_u(retptr_local, base_offset);
        self.emit_if_block(BlockType::Result(ip_ref));
        // then: disc != 0 → V6 (8 x u16 at +2)
        {
            let payload_offset = base_offset + 2;
            for i in 0..8u64 {
                self.wasi_i32_load16_u(retptr_local, payload_offset + i * 2);
            }
            let ipv6_addr_mn = self.find_record_mn_for_variant(ip_addr_mn, "V6", &[]);
            let ipv6_addr_idx = self.codegen.type_indices[&ipv6_addr_mn];
            self.instruction(Instruction::StructNew(ipv6_addr_idx));
            self.instruction(Instruction::StructNew(v6_idx));
        }
        self.instruction(Instruction::Else);
        // else: disc == 0 → V4 (4 x u8 at +2)
        {
            let payload_offset = base_offset + 2;
            for i in 0..4u64 {
                self.wasi_i32_load8_u(retptr_local, payload_offset + i);
            }
            let ipv4_addr_mn = self.find_record_mn_for_variant(ip_addr_mn, "V4", &[]);
            let ipv4_addr_idx = self.codegen.type_indices[&ipv4_addr_mn];
            self.instruction(Instruction::StructNew(ipv4_addr_idx));
            self.instruction(Instruction::StructNew(v4_idx));
        }
        self.emit_end_block();
    }

    /// Lift `result<list<ip-address>, ip-name-lookup.error-code>` from a pinned
    /// retptr block: disc @0, and at +4 either the error disc or the list
    /// (ptr @4, len @8). Frees the host-allocated element buffer after copying
    /// the addresses into a GC array — the block itself is freed by the caller.
    ///
    /// Element stride is 18: disc (u8, padded to the variant's align of 2) plus
    /// the 16-byte ipv6 payload.
    pub(super) fn wasi_construct_result_ip_address_list_dns_error(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
    ) {
        const IP_ADDRESS_STRIDE: u64 = 18;

        let result_ref = self.result_ref(result_type);

        // Ok type is Array<IpAddress>; recover the element mangled name.
        let ok_type = match result_type {
            Type::GenericEnum { type_args, .. } => type_args[0].1.clone(),
            _ => unreachable!("Expected GenericEnum Result type"),
        };
        let elem_type = match &ok_type {
            Type::Array(elem) => (**elem).clone(),
            _ => unreachable!("Expected Array<IpAddress> ok type, got: {ok_type}"),
        };
        let ip_addr_mn = match &elem_type {
            Type::Enum(_, mn) => mn.clone(),
            _ => unreachable!("Expected Enum IpAddress element, got: {elem_type}"),
        };
        let array_type_idx = self.codegen.array_type_index(&elem_type);
        let array_ref_type = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(array_type_idx),
        });

        self.wasi_i32_load8_u(retptr_local, 0);
        self.emit_if_block(BlockType::Result(result_ref));
        // then: error — disc @4, its `other(some(_))` message lifted with it
        self.wasi_construct_dns_error(retptr_local, 4, result_type);
        self.wasi_construct_result_error(result_type, &Self::wasi_result_error_type(result_type));
        self.instruction(Instruction::Else);
        // else: ok — list ptr @4, len @8
        {
            let list_ptr = self.add_local(ValType::I32);
            let list_len = self.add_local(ValType::I32);
            let elem_base = self.add_local(ValType::I32);
            let arr_local = self.add_local(array_ref_type);
            self.wasi_i32_load(retptr_local, 4);
            self.instruction(Instruction::LocalSet(list_ptr));
            self.wasi_i32_load(retptr_local, 8);
            self.instruction(Instruction::LocalSet(list_len));

            self.instruction(Instruction::LocalGet(list_len));
            self.emit_if_block(BlockType::Result(array_ref_type));
            // then: at least one element — seed the array with element 0, then fill.
            {
                self.instruction(Instruction::LocalGet(list_ptr));
                self.instruction(Instruction::LocalSet(elem_base));
                self.wasi_construct_ip_address(elem_base, 0, &ip_addr_mn);
                self.instruction(Instruction::LocalGet(list_len));
                self.instruction(Instruction::ArrayNew(array_type_idx));
                self.instruction(Instruction::LocalSet(arr_local));

                let idx = self.add_local(ValType::I32);
                self.instruction(Instruction::I32Const(1));
                self.instruction(Instruction::LocalSet(idx));
                self.instruction(Instruction::Block(BlockType::Empty));
                self.instruction(Instruction::Loop(BlockType::Empty));
                self.instruction(Instruction::LocalGet(idx));
                self.instruction(Instruction::LocalGet(list_len));
                self.instruction(Instruction::I32GeU);
                self.instruction(Instruction::BrIf(1));

                self.instruction(Instruction::LocalGet(list_ptr));
                self.instruction(Instruction::LocalGet(idx));
                self.instruction(Instruction::I32Const(IP_ADDRESS_STRIDE as i32));
                self.instruction(Instruction::I32Mul);
                self.instruction(Instruction::I32Add);
                self.instruction(Instruction::LocalSet(elem_base));

                self.instruction(Instruction::LocalGet(arr_local));
                self.instruction(Instruction::LocalGet(idx));
                self.wasi_construct_ip_address(elem_base, 0, &ip_addr_mn);
                self.instruction(Instruction::ArraySet(array_type_idx));

                self.instruction(Instruction::LocalGet(idx));
                self.instruction(Instruction::I32Const(1));
                self.instruction(Instruction::I32Add);
                self.instruction(Instruction::LocalSet(idx));
                self.instruction(Instruction::Br(0));
                self.instruction(Instruction::End); // loop
                self.instruction(Instruction::End); // block
                self.instruction(Instruction::LocalGet(arr_local));
            }
            self.instruction(Instruction::Else);
            // else: empty list (the interface promises this never happens, but
            // an empty array keeps the lift total). `array.new_fixed` with size
            // 0 — NOT `array.new_default`, whose `(ref any)` element type is not
            // defaultable.
            {
                self.instruction(Instruction::ArrayNewFixed {
                    array_type_index: array_type_idx,
                    array_size: 0,
                });
            }
            self.emit_end_block();

            // Free the host-realloc'd element buffer now that the addresses are
            // copied into the GC array (the helper returns void, so the array
            // stays on the stack). Guarded rather than a raw `pinned_free`: the
            // empty-list arm above is reachable, and an empty list lowers as a
            // null or small dangling pointer that never went through
            // `cabi_realloc`.
            self.wasi_free_lifted_block(list_ptr);
            self.wasi_construct_result_ok(result_type, &Self::wasi_result_ok_type(result_type));
        }
        self.emit_end_block();
    }

    /// Emit i32.load16_u at memory[ptr_local + offset]. Leaves i32 (u16 value) on stack.
    pub(super) fn wasi_i32_load16_u(&mut self, ptr_local: u32, offset: u64) {
        self.instruction(Instruction::LocalGet(ptr_local));
        self.instruction(Instruction::I32Load16U(wasm_encoder::MemArg {
            offset,
            align: 0,
            memory_index: 0,
        }));
    }

    /// Extract the Result<T, NetworkError> mangled name from the expression's return type.
    pub(super) fn extract_result_mn(result_type: &Type) -> MangledName {
        match result_type {
            Type::GenericEnum { mangled_name, .. } => mangled_name.clone(),
            _ => unreachable!("Expected GenericEnum Result type"),
        }
    }

    /// The non-null GC ref type of the `Result` a marshaling site is building —
    /// the block type of every `if (result ...)` that picks between the ok and
    /// error arms.
    pub(super) fn result_ref(&self, result_type: &Type) -> ValType {
        let result_mn = Self::extract_result_mn(result_type);
        ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(self.codegen.type_indices[&result_mn]),
        })
    }

    /// Lift `result<T, error-code>` out of a p3 filesystem retptr and free the
    /// pinned block it came in.
    ///
    /// Every fs `*Finish` intrinsic has this shape: result disc U8 @0, and at
    /// `payload_offset` either the error-code discriminant — whose
    /// `other(some(msg))` message is freed here, because the Dovetail enum keeps
    /// only the discriminant — or the ok payload. `payload_offset` is the
    /// alignment of the widest case: 4 normally, 8 when the ok side holds u64s.
    /// Only the ok arm differs between the four, so it comes in as a closure that
    /// leaves the ok value on the stack.
    pub(super) fn emit_p3_fs_result_finish(
        &mut self,
        retptr: u32,
        result_type: &Type,
        payload_offset: u64,
        emit_ok: impl FnOnce(&mut Self),
    ) {
        self.emit_p3_fs_result(retptr, result_type, payload_offset, emit_ok);

        self.instruction(Instruction::LocalGet(retptr));
        self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
    }

    /// The lift itself, without the free — for the sites whose block is not the
    /// caller's to release.
    ///
    /// Two kinds. A caller that frees the retptr on its own line
    /// (`P3FsOpenAtFinish` and friends), and — the reason this is a separate
    /// function rather than a style choice — the `future.read` arms
    /// `P3FsReadReadResult` / `P3FsReadWriteResult` / `P3FsReadEntryResult` /
    /// `P3FsReadAppendResult` (and their sockets twins
    /// `P3TcpReadReceiveResult` / `P3TcpReadSendResult`, which go through
    /// `wasi_construct_result_unit_network_error`). Those take their retptr from
    /// `wasi_bump_alloc(24)` — the scratch arena, not the pinned heap — because
    /// the future has already resolved and the block only has to outlive the
    /// lift. Handing a scratch pointer to `pinned_free` writes a free-list link
    /// through `ptr - 8`, i.e. into the arena, and corrupts the heap.
    ///
    /// They are the only bump-allocated blocks that reach a lifting helper at
    /// all: every `pinned_free` call site in codegen frees a block that came
    /// from `pinned_alloc`, either directly or through `emit_p3_pinned_retptr` /
    /// `emit_p3_unpack_retptr` (the low half of a packed `AsyncCall`/`StreamOp`).
    pub(super) fn emit_p3_fs_result(
        &mut self,
        retptr: u32,
        result_type: &Type,
        payload_offset: u64,
        emit_ok: impl FnOnce(&mut Self),
    ) {
        let result_ref = self.result_ref(result_type);

        self.wasi_i32_load8_u(retptr, 0);
        self.emit_if_block(BlockType::Result(result_ref));
        self.wasi_free_error_message(retptr, payload_offset, FS_ERROR_OTHER_DISC);
        self.wasi_i32_load8_u(retptr, payload_offset);
        self.wasi_construct_fs_error(result_type);
        self.wasi_construct_result_error(result_type, &Self::wasi_result_error_type(result_type));
        self.instruction(Instruction::Else);
        emit_ok(self);
        self.wasi_construct_result_ok(result_type, &Self::wasi_result_ok_type(result_type));
        self.emit_end_block();
    }

    /// Common pattern: construct Result<Unit, NetworkError> from retptr.
    /// p3 retptr layout: disc u8 @0, err disc u8 @4 (error-code variant
    /// aligns 4; its `option<string>` payload, at @8..20, is lifted into the
    /// Dovetail `Other` payload). Alloc 24.
    pub(super) fn wasi_construct_result_unit_network_error(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
    ) {
        self.wasi_construct_result_scalar_network_error(
            retptr_local,
            result_type,
            4,
            WasiScalar::Unit,
        );
    }

    /// Construct `Result<Int32, NetworkError>` from a retptr — the `I32`
    /// instance of `wasi_construct_result_scalar_network_error`, kept as a name
    /// because that is what the socket-handle-returning intrinsics mean.
    pub(super) fn wasi_construct_result_i32_network_error(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
    ) {
        self.wasi_construct_result_scalar_network_error(
            retptr_local,
            result_type,
            4,
            WasiScalar::I32,
        );
    }

    /// Construct `Result<scalar, NetworkError>` from a retptr block.
    ///
    /// Every socket-option getter has this shape. The payload offset is not
    /// fixed: a `result` puts all its cases at one shared offset, aligned to the
    /// widest case, and `error-code` is a *variant* (its `other(option<string>)`
    /// case makes it align 4, size 16). So `result<bool, error-code>` puts both
    /// the bool and the error-code discriminant at 4, while `result<u64, ...>`
    /// pushes both to 8. Callers pass the offset they computed.
    pub(super) fn wasi_construct_result_scalar_network_error(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
        payload_offset: u64,
        payload: WasiScalar,
    ) {
        let result_ref = self.result_ref(result_type);

        self.wasi_i32_load8_u(retptr_local, 0);
        self.emit_if_block(BlockType::Result(result_ref));
        // then: error — the error-code discriminant shares the payload offset,
        // and its `other(some(_))` message is lifted with it
        self.wasi_construct_network_error(retptr_local, payload_offset, result_type);
        self.wasi_construct_result_error(result_type, &Self::wasi_result_error_type(result_type));
        self.instruction(Instruction::Else);
        // else: ok — the scalar, at the same offset
        match payload {
            WasiScalar::Unit => self.instruction(Instruction::I32Const(0)),
            WasiScalar::U8 => self.wasi_i32_load8_u(retptr_local, payload_offset),
            WasiScalar::I32 => self.wasi_i32_load(retptr_local, payload_offset),
            WasiScalar::I64 => self.wasi_i64_load(retptr_local, payload_offset),
        }
        self.wasi_construct_result_ok(result_type, &Self::wasi_result_ok_type(result_type));
        self.emit_end_block();
    }

    /// Marshal a Dovetail SocketAddress enum into 12 flattened i32 params on the stack.
    /// Canonical ABI flattened ip-socket-address:
    ///   [disc, p0, p1, p2, p3, p4, p5, p6, p7, p8, p9, p10]
    ///   V4: disc=0, p0=port, p1=a, p2=b, p3=c, p4=d, p5..p10=0
    ///   V6: disc=1, p0=port, p1=flowInfo, p2..p9=addr(8 x u16), p10=scopeId
    ///
    /// The SocketAddress GC value must already be on the stack.
    pub(super) fn wasi_marshal_socket_address_flat(&mut self, socket_addr_mn: &MangledName) {
        let sa_base_idx = self.codegen.type_indices[socket_addr_mn];
        let sa_ref_type = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(sa_base_idx),
        });

        // Save the socket address value
        let sa_local = self.add_local(sa_ref_type);
        self.instruction(Instruction::LocalSet(sa_local));

        let (disc_local, p_locals) =
            self.wasi_destructure_socket_address_flat(socket_addr_mn, sa_local);

        // Push all 12 params onto the stack
        self.instruction(Instruction::LocalGet(disc_local));
        for &p in &p_locals {
            self.instruction(Instruction::LocalGet(p));
        }
    }

    /// Destructure a `SocketAddress` GC value held in `sa_local` into the 12
    /// canonical-ABI flattened i32 locals `[disc, p0..p10]` (see
    /// `wasi_marshal_socket_address_flat` for the per-variant layout; unused
    /// slots are zero). This is the ONE place that knows how the Dovetail enum's
    /// records in `wasi/src/net` map onto the canonical order — both the flat
    /// marshaler and the indirect-params store variant consume the locals it
    /// fills, so a field reorder in the Dovetail source is fixed here alone.
    fn wasi_destructure_socket_address_flat(
        &mut self,
        socket_addr_mn: &MangledName,
        sa_local: u32,
    ) -> (u32, [u32; 11]) {
        let v4_idx = self.codegen.variant_type_indices[&(socket_addr_mn.clone(), "V4".to_string())];
        let v6_idx = self.codegen.variant_type_indices[&(socket_addr_mn.clone(), "V6".to_string())];

        let ipv4_sa_mn = self.find_record_mn_for_variant(socket_addr_mn, "V4", &[]);
        let ipv4_sa_idx = self.codegen.type_indices[&ipv4_sa_mn];
        let ipv6_sa_mn = self.find_record_mn_for_variant(socket_addr_mn, "V6", &[]);
        let ipv6_sa_idx = self.codegen.type_indices[&ipv6_sa_mn];

        // Look up Ipv4Address and Ipv6Address record types (payload of Ipv4SocketAddress.address and Ipv6SocketAddress.address)
        let ipv4_addr_mn = self.find_record_field_type_mn(&ipv4_sa_mn, 1); // field 1 = address
        let ipv4_addr_idx = self.codegen.type_indices[&ipv4_addr_mn];
        let ipv6_addr_mn = self.find_record_field_type_mn(&ipv6_sa_mn, 2); // field 2 = address
        let ipv6_addr_idx = self.codegen.type_indices[&ipv6_addr_mn];

        // Allocate 12 locals for the flattened params
        let disc_local = self.add_local(ValType::I32);
        let mut p_locals = [0u32; 11];
        for p in &mut p_locals {
            *p = self.add_local(ValType::I32);
        }

        // Initialize all to 0
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(disc_local));
        for &p in &p_locals {
            self.instruction(Instruction::I32Const(0));
            self.instruction(Instruction::LocalSet(p));
        }

        // Check variant: try to cast to V4 first
        self.instruction(Instruction::Block(BlockType::Empty)); // outer block
        self.instruction(Instruction::Block(BlockType::Empty)); // v4 block

        // ref.test for V4
        self.instruction(Instruction::LocalGet(sa_local));
        self.instruction(Instruction::RefTestNonNull(
            wasm_encoder::HeapType::Concrete(v4_idx),
        ));
        self.instruction(Instruction::I32Eqz);
        self.instruction(Instruction::BrIf(0)); // not V4, skip to V6

        // V4 path
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(disc_local));

        // Cast to V4 variant, get Ipv4SocketAddress record
        self.instruction(Instruction::LocalGet(sa_local));
        self.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(v4_idx),
        ));
        let v4_record_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(ipv4_sa_idx),
        }));
        // V4 variant struct field 0 = Ipv4SocketAddress record
        self.instruction(Instruction::StructGet {
            struct_type_index: v4_idx,
            field_index: 0,
        });
        self.instruction(Instruction::LocalSet(v4_record_local));

        // p0 = port (field 0)
        self.instruction(Instruction::LocalGet(v4_record_local));
        self.instruction(Instruction::StructGet {
            struct_type_index: ipv4_sa_idx,
            field_index: 0,
        });
        self.instruction(Instruction::LocalSet(p_locals[0]));

        // Get Ipv4Address record (field 1)
        let v4_addr_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(ipv4_addr_idx),
        }));
        self.instruction(Instruction::LocalGet(v4_record_local));
        self.instruction(Instruction::StructGet {
            struct_type_index: ipv4_sa_idx,
            field_index: 1,
        });
        self.instruction(Instruction::LocalSet(v4_addr_local));

        // p1..p4 = a, b, c, d (fields 0..3 of Ipv4Address)
        for i in 0..4 {
            self.instruction(Instruction::LocalGet(v4_addr_local));
            self.instruction(Instruction::StructGet {
                struct_type_index: ipv4_addr_idx,
                field_index: i,
            });
            self.instruction(Instruction::LocalSet(p_locals[1 + i as usize]));
        }

        self.instruction(Instruction::Br(1)); // break to outer block
        self.instruction(Instruction::End); // end v4 block

        // V6 path
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::LocalSet(disc_local));

        self.instruction(Instruction::LocalGet(sa_local));
        self.instruction(Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(v6_idx),
        ));
        let v6_record_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(ipv6_sa_idx),
        }));
        self.instruction(Instruction::StructGet {
            struct_type_index: v6_idx,
            field_index: 0,
        });
        self.instruction(Instruction::LocalSet(v6_record_local));

        // p0 = port (field 0)
        self.instruction(Instruction::LocalGet(v6_record_local));
        self.instruction(Instruction::StructGet {
            struct_type_index: ipv6_sa_idx,
            field_index: 0,
        });
        self.instruction(Instruction::LocalSet(p_locals[0]));

        // p1 = flowInfo (field 1)
        self.instruction(Instruction::LocalGet(v6_record_local));
        self.instruction(Instruction::StructGet {
            struct_type_index: ipv6_sa_idx,
            field_index: 1,
        });
        self.instruction(Instruction::LocalSet(p_locals[1]));

        // Get Ipv6Address record (field 2)
        let v6_addr_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(ipv6_addr_idx),
        }));
        self.instruction(Instruction::LocalGet(v6_record_local));
        self.instruction(Instruction::StructGet {
            struct_type_index: ipv6_sa_idx,
            field_index: 2,
        });
        self.instruction(Instruction::LocalSet(v6_addr_local));

        // p2..p9 = a..h (fields 0..7 of Ipv6Address)
        for i in 0..8 {
            self.instruction(Instruction::LocalGet(v6_addr_local));
            self.instruction(Instruction::StructGet {
                struct_type_index: ipv6_addr_idx,
                field_index: i,
            });
            self.instruction(Instruction::LocalSet(p_locals[2 + i as usize]));
        }

        // p10 = scopeId (field 3)
        self.instruction(Instruction::LocalGet(v6_record_local));
        self.instruction(Instruction::StructGet {
            struct_type_index: ipv6_sa_idx,
            field_index: 3,
        });
        self.instruction(Instruction::LocalSet(p_locals[10]));

        self.instruction(Instruction::End); // end outer block

        (disc_local, p_locals)
    }

    /// Construct a SocketAddress enum from retptr memory (ip-socket-address layout).
    /// Layout at retptr + base_offset:
    ///   @0: addr disc (u8, 0=V4, 1=V6)
    ///   V4: port u16 @4, a u8 @6, b @7, c @8, d @9
    ///   V6: port u16 @4, pad @6..8, flowInfo u32 @8, addr 8×u16 @12..28, scopeId u32 @28
    /// Leaves a SocketAddress enum ref on the stack.
    pub(super) fn wasi_construct_socket_address(
        &mut self,
        retptr_local: u32,
        base_offset: u64,
        socket_addr_mn: &MangledName,
    ) {
        let sa_base_idx = self.codegen.type_indices[socket_addr_mn];
        let sa_ref_type = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(sa_base_idx),
        });

        let v4_idx = self.codegen.variant_type_indices[&(socket_addr_mn.clone(), "V4".to_string())];
        let v6_idx = self.codegen.variant_type_indices[&(socket_addr_mn.clone(), "V6".to_string())];

        let ipv4_sa_mn = self.find_record_mn_for_variant(socket_addr_mn, "V4", &[]);
        let ipv4_sa_idx = self.codegen.type_indices[&ipv4_sa_mn];
        let ipv6_sa_mn = self.find_record_mn_for_variant(socket_addr_mn, "V6", &[]);
        let ipv6_sa_idx = self.codegen.type_indices[&ipv6_sa_mn];

        let ipv4_addr_mn = self.find_record_field_type_mn(&ipv4_sa_mn, 1);
        let ipv4_addr_idx = self.codegen.type_indices[&ipv4_addr_mn];
        let ipv6_addr_mn = self.find_record_field_type_mn(&ipv6_sa_mn, 2);
        let ipv6_addr_idx = self.codegen.type_indices[&ipv6_addr_mn];

        // Read addr discriminant
        self.wasi_i32_load8_u(retptr_local, base_offset);
        self.emit_if_block(BlockType::Result(sa_ref_type));
        // then: disc != 0 → V6
        {
            // port u16 @4
            self.wasi_i32_load16_u(retptr_local, base_offset + 4);
            // flowInfo u32 @8
            self.wasi_i32_load(retptr_local, base_offset + 8);
            // Ipv6Address: 8 x u16 starting @12
            for i in 0..8u64 {
                self.wasi_i32_load16_u(retptr_local, base_offset + 12 + i * 2);
            }
            self.instruction(Instruction::StructNew(ipv6_addr_idx));
            // scopeId u32 @28
            self.wasi_i32_load(retptr_local, base_offset + 28);
            // Create Ipv6SocketAddress(port, flowInfo, address, scopeId)
            self.instruction(Instruction::StructNew(ipv6_sa_idx));
            self.instruction(Instruction::StructNew(v6_idx));
        }
        self.instruction(Instruction::Else);
        // else: disc == 0 → V4
        {
            // port u16 @4
            self.wasi_i32_load16_u(retptr_local, base_offset + 4);
            // Ipv4Address: a @6, b @7, c @8, d @9
            for i in 0..4u64 {
                self.wasi_i32_load8_u(retptr_local, base_offset + 6 + i);
            }
            self.instruction(Instruction::StructNew(ipv4_addr_idx));
            // Create Ipv4SocketAddress(port, address)
            self.instruction(Instruction::StructNew(ipv4_sa_idx));
            self.instruction(Instruction::StructNew(v4_idx));
        }
        self.emit_end_block();
    }

    /// Construct Result<SocketAddress, NetworkError> from retptr.
    /// retptr layout (36 bytes read; both callers allocate 40, which is only
    /// this rounded up to the allocators' 8-byte granularity — 36 is the ABI
    /// size, so a 36-byte block would also be correct):
    ///   @0: result disc (u8, 0=ok, 1=err)
    ///   @4: payload
    ///     err: error-code u8 @4, its `other` message ptr @12
    ///     ok: ip-socket-address starting @4 (32 bytes, so @4..36)
    pub(super) fn wasi_construct_result_socket_address_network_error(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
    ) {
        let result_ref = self.result_ref(result_type);

        // Extract SocketAddress type from Result<SocketAddress, NetworkError>
        let socket_addr_type = match result_type {
            Type::GenericEnum { type_args, .. } => &type_args[0].1,
            _ => unreachable!(),
        };
        let socket_addr_mn = match socket_addr_type {
            Type::Enum(_, mn) => mn.clone(),
            _ => unreachable!("Expected Enum SocketAddress type"),
        };

        self.wasi_i32_load8_u(retptr_local, 0);
        self.emit_if_block(BlockType::Result(result_ref));
        // then: error — disc @4, its `other(some(_))` message lifted with it
        self.wasi_construct_network_error(retptr_local, 4, result_type);
        self.wasi_construct_result_error(result_type, &Self::wasi_result_error_type(result_type));
        self.instruction(Instruction::Else);
        // else: ok — construct SocketAddress from retptr+4
        self.wasi_construct_socket_address(retptr_local, 4, &socket_addr_mn);
        self.wasi_construct_result_ok(result_type, &Self::wasi_result_ok_type(result_type));
        self.emit_end_block();
    }

    /// Extract the SocketAddress MangledName from a function argument's type.
    pub(super) fn extract_socket_address_mn(addr_type: &Type) -> MangledName {
        match addr_type {
            Type::Enum(_, mn) => mn.clone(),
            _ => unreachable!("Expected Enum SocketAddress type"),
        }
    }

    /// Find the MangledName of the record type used as a field in a record type.
    /// Looks up the record type definition and returns the type of the given field index.
    pub(super) fn find_record_field_type_mn(
        &self,
        record_mn: &MangledName,
        field_index: usize,
    ) -> MangledName {
        use crate::typechecker::types::TypeDef;
        for (name, type_def) in &self.codegen.typed_module.types {
            if name == record_mn
                && let TypeDef::Record(r) = type_def
                && let Some(field) = r.fields.get(field_index)
            {
                return field.1.mangled_name();
            }
        }
        unreachable!(
            "Could not find field {} type for record {}",
            field_index, record_mn
        )
    }

    /// Find the full Type of a record field by index.
    /// Used when the field type is a generic (e.g. Option<SocketAddress>) and we need
    /// the full Type, not just the MangledName.
    pub(super) fn find_record_field_type(
        &self,
        record_mn: &MangledName,
        field_index: usize,
    ) -> Type {
        use crate::typechecker::types::TypeDef;
        for (name, type_def) in &self.codegen.typed_module.types {
            if name == record_mn
                && let TypeDef::Record(r) = type_def
                && let Some(field) = r.fields.get(field_index)
            {
                return field.1.clone();
            }
        }
        unreachable!(
            "Could not find field {} type for record {}",
            field_index, record_mn
        )
    }

    /// Find the MangledName of the record type used as payload for an enum variant.
    /// Looks up the variant in the typed module's type definitions.
    ///
    /// For canonical (erased) generic enums, the stored payload type may still be a
    /// `TypeVariable`. We use the optional `type_args` (paired with the enum's `type_params`)
    /// to substitute concrete types before extracting the mangled name. For non-generic
    /// enums, `type_args` is empty and substitution is a no-op.
    fn find_record_mn_for_variant(
        &self,
        enum_mn: &MangledName,
        variant_name: &str,
        type_args: &[Type],
    ) -> MangledName {
        use crate::typechecker::types::TypeDef;
        for (name, type_def) in &self.codegen.typed_module.types {
            if name == enum_mn
                && let TypeDef::Enum(e) = type_def
            {
                for variant in &e.variants {
                    if variant.name == variant_name
                        && let Some(payload_type) = variant.payload_types.first()
                    {
                        let sub: std::collections::BTreeMap<
                            crate::common::types::TypeParamName,
                            Type,
                        > = e
                            .type_params
                            .iter()
                            .cloned()
                            .zip(type_args.iter().cloned())
                            .collect();
                        let resolved = if sub.is_empty() {
                            payload_type.clone()
                        } else {
                            crate::compiler::monomorphize::substitute::apply_type_substitution(
                                payload_type,
                                &sub,
                            )
                        };
                        return resolved.mangled_name();
                    }
                }
            }
        }
        unreachable!(
            "Could not find record type for variant {}.{}",
            enum_mn, variant_name
        )
    }

    // --- Filesystem marshaling helpers ---

    /// Construct a FileSystemError enum variant from an error-code i32 on the stack.
    /// The WASI filesystem error-code has 36 variants (0-35) matching FileSystemError.
    /// Expects result_type to be Result<T, FileSystemError>.
    /// Leaves a FileSystemError enum ref on the stack.
    pub(super) fn wasi_construct_fs_error(&mut self, result_type: &Type) {
        let fs_error_type = match result_type {
            Type::GenericEnum { type_args, .. } => &type_args[1].1,
            _ => unreachable!("Expected GenericEnum Result type"),
        };
        let ne_mn = match fs_error_type {
            Type::Enum(_, mn) => mn.clone(),
            _ => unreachable!("Expected Enum FileSystemError type, got: {}", fs_error_type),
        };

        self.wasi_construct_pinned_enum(&ne_mn, &FS_ERROR_VARIANT_NAMES, FS_ERROR_OTHER_DISC, None);
    }

    /// Construct `Result<Unit, CliIoError>` from a stdio write-result block.
    ///
    /// The wasi:cli `error-code` is a plain 3-case **enum**, size 1 / align 1 —
    /// not the `other(option<string>)` variant the sockets and filesystem
    /// worlds use — so the result's payload sits at offset 1, where those put
    /// theirs at 4. `CliIoError` is a newtype over the discriminant, so the
    /// loaded byte is the value.
    pub(super) fn wasi_construct_result_unit_cli_error(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
    ) {
        let result_ref = self.result_ref(result_type);

        self.wasi_i32_load8_u(retptr_local, 0);
        self.emit_if_block(BlockType::Result(result_ref));
        // then: error — error-code discriminant @1
        self.wasi_i32_load8_u(retptr_local, 1);
        self.wasi_construct_result_error(result_type, &Self::wasi_result_error_type(result_type));
        self.instruction(Instruction::Else);
        // else: ok(unit)
        self.instruction(Instruction::I32Const(0));
        self.wasi_construct_result_ok(result_type, &Self::wasi_result_ok_type(result_type));
        self.emit_end_block();
    }

    /// Construct Result<Unit, FileSystemError> from retptr.
    /// p3 layout: disc U8 @0, err: error-code variant @4 (disc @4, message
    /// ptr @12 / len @16). Callers allocate 24.
    pub(super) fn wasi_construct_result_unit_fs_error(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
    ) {
        // The error side is the shape every fs result has — see
        // `emit_p3_fs_result`; the ok side is a unit, which lowers to nothing
        // and lifts from nothing.
        self.emit_p3_fs_result(retptr_local, result_type, 4, |e| {
            e.instruction(Instruction::I32Const(0));
        });
    }

    /// Construct Result<i32, FileSystemError> from retptr.
    /// p3 layout: disc U8 @0, ok: i32 @4 / err: error-code variant @4 (disc @4,
    /// message ptr @12 / len @16) — 20 bytes touched, and callers allocate 24.
    pub(super) fn wasi_construct_result_i32_fs_error(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
    ) {
        self.emit_p3_fs_result(retptr_local, result_type, 4, |e| {
            e.wasi_i32_load(retptr_local, 4);
        });
    }

    /// Marshal a Dovetail String to linear memory, returning (ptr_local, len_local).
    /// Extracts the backing array and copies the bytes into a pinned-scratch
    /// block owned by the enclosing marshaling window — a string's length is
    /// caller-controlled, so it cannot come from the fixed scratch arena.
    pub(super) fn wasi_marshal_string_to_memory(&mut self, string_local: u32) -> (u32, u32) {
        let backing_type_index = super::super::U8_BACKING_TYPE_INDEX;
        let backing_ref_type = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(backing_type_index),
        });

        // Extract backing array
        self.instruction(Instruction::LocalGet(string_local));
        self.instruction(Instruction::StructGet {
            struct_type_index: super::super::STRING_STRUCT_TYPE_INDEX,
            field_index: 0,
        });
        let backing_local = self.add_local(backing_ref_type);
        self.instruction(Instruction::LocalSet(backing_local));

        // Get byte length
        let byte_len = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalGet(backing_local));
        self.instruction(Instruction::ArrayLen);
        self.instruction(Instruction::LocalSet(byte_len));

        // Allocate space in linear memory
        let str_ptr = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalGet(byte_len));
        self.scratch_pinned_alloc();
        self.instruction(Instruction::LocalSet(str_ptr));

        // Copy string bytes to linear memory
        self.wasi_copy_u8_backing_to_memory(backing_local, str_ptr, byte_len);

        (str_ptr, byte_len)
    }

    /// Convert a simple payload-less Dovetail enum on stack to its i32 discriminant.
    /// Uses ref.test against each variant type to find the matching discriminant.
    /// Leaves i32 discriminant on stack.
    pub(super) fn wasi_fs_enum_to_disc(&mut self, enum_type: &Type) {
        let enum_mn = match enum_type {
            Type::Enum(_, mn) => mn.clone(),
            _ => unreachable!("Expected Enum type for enum_to_disc"),
        };

        let enum_base_idx = self.codegen.type_indices[&enum_mn];
        let enum_ref_type = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(enum_base_idx),
        });

        // Store enum ref in a local, then test variants in order
        let enum_local = self.add_local(enum_ref_type);
        self.instruction(Instruction::LocalSet(enum_local));

        // Get variant names from codegen's variant_type_indices
        // We need to iterate in the correct order. Since we know the variant names
        // for each enum type, we look them up from the type registry.
        let variants: Vec<(String, u32)> = self
            .codegen
            .variant_type_indices
            .iter()
            .filter(|((mn, _), _)| *mn == enum_mn)
            .map(|((_, name), &idx)| (name.clone(), idx))
            .collect();

        // Sort by variant type index to get declaration order
        let mut sorted_variants = variants;
        sorted_variants.sort_by_key(|(_, idx)| *idx);

        // Use if-else chain: if ref.test variant0 → 0, elif ref.test variant1 → 1, ...
        // Outer block returns i32
        self.instruction(Instruction::Block(BlockType::Result(ValType::I32)));
        for (i, (_, variant_idx)) in sorted_variants.iter().enumerate() {
            self.instruction(Instruction::LocalGet(enum_local));
            self.instruction(Instruction::RefTestNonNull(
                wasm_encoder::HeapType::Concrete(*variant_idx),
            ));
            self.emit_if_block(BlockType::Empty);
            self.instruction(Instruction::I32Const(i as i32));
            self.instruction(Instruction::Br(1)); // break to outer block
            self.emit_end_block();
        }
        // Default: 0
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::End); // end outer block
    }

    /// Construct a DescriptorType enum from an i32 discriminant on the stack.
    /// 8 p3 cases: BlockDevice(0), CharacterDevice(1), Directory(2), Fifo(3),
    /// SymbolicLink(4), RegularFile(5), Socket(6), Other(7) — see
    /// `DESCRIPTOR_TYPE_OTHER_DISC` for why the last one is load-bearing.
    /// Expects result_type to contain DescriptorType inside Result<DescriptorType, FileSystemError>.
    pub(super) fn wasi_fs_construct_descriptor_type(&mut self, result_type: &Type) {
        // Extract DescriptorType from Result<T, E> or Result<Option<DirectoryEntry>, E>
        let dt_type = self.wasi_fs_find_descriptor_type(result_type);
        self.wasi_fs_construct_descriptor_type_direct(&dt_type);
    }

    /// Construct a DescriptorType value from the p3 discriminant on the stack.
    pub(super) fn wasi_fs_construct_descriptor_type_direct(&mut self, dt_type: &Type) {
        let dt_mn = match &dt_type {
            Type::Enum(_, mn) => mn.clone(),
            _ => unreachable!("Expected Enum DescriptorType type, got: {}", dt_type),
        };

        // `descriptor-type` is not an error code, but it has an `other` case of
        // its own and so takes the same fallback.
        self.wasi_construct_pinned_enum(
            &dt_mn,
            &DESCRIPTOR_TYPE_VARIANT_NAMES,
            DESCRIPTOR_TYPE_OTHER_DISC,
            None,
        );
    }

    /// Find the DescriptorType enum type from various result type structures.
    fn wasi_fs_find_descriptor_type(&self, result_type: &Type) -> Type {
        // Result<DescriptorType, E>
        let ok_type = match result_type {
            Type::GenericEnum { type_args, .. } => &type_args[0].1,
            _ => unreachable!(),
        };
        match ok_type {
            Type::Enum(_, _) => ok_type.clone(),
            // Result<Option<DirectoryEntry>, E> — DescriptorType is inside DirectoryEntry
            Type::GenericEnum { type_args, .. } => {
                // Option<DirectoryEntry> → DirectoryEntry
                let inner = &type_args[0].1;
                match inner {
                    Type::Record(_, _) => {
                        // DirectoryEntry.entryType is field 0
                        let inner_mn = inner.mangled_name();
                        self.find_record_field_type(&inner_mn, 0)
                    }
                    _ => unreachable!("Expected Record DirectoryEntry"),
                }
            }
            Type::Record(_, _) => {
                // FileStat.descriptorType is field 0
                let ok_mn = ok_type.mangled_name();
                self.find_record_field_type(&ok_mn, 0)
            }
            _ => unreachable!("Cannot find DescriptorType in: {}", ok_type),
        }
    }

    /// Convert FileFlags record on stack to i32 bitmask.
    /// FileFlags: read(bit0), write(bit1), fileIntegritySync(bit2), dataIntegritySync(bit3),
    /// requestedWriteSync(bit4), mutateDirectory(bit5).
    pub(super) fn wasi_fs_file_flags_to_bitmask(&mut self, flags_type: &Type) {
        let flags_mn = flags_type.mangled_name();
        let flags_idx = self.codegen.type_indices[&flags_mn];
        let flags_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(flags_idx),
        });
        let flags_local = self.add_local(flags_ref);
        self.instruction(Instruction::LocalSet(flags_local));

        let result_local = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(result_local));

        for bit in 0..6u32 {
            self.instruction(Instruction::LocalGet(flags_local));
            self.instruction(Instruction::StructGet {
                struct_type_index: flags_idx,
                field_index: bit,
            });
            self.emit_if_block(BlockType::Empty);
            self.instruction(Instruction::LocalGet(result_local));
            self.instruction(Instruction::I32Const(1 << bit as i32));
            self.instruction(Instruction::I32Or);
            self.instruction(Instruction::LocalSet(result_local));
            self.emit_end_block();
        }

        self.instruction(Instruction::LocalGet(result_local));
    }

    /// Convert i32 bitmask on stack to FileFlags record.
    /// Leaves FileFlags record ref on stack.
    pub(super) fn wasi_fs_bitmask_to_file_flags(&mut self, result_type: &Type) {
        let ok_type = match result_type {
            Type::GenericEnum { type_args, .. } => &type_args[0].1,
            _ => unreachable!(),
        };
        let flags_mn = ok_type.mangled_name();
        let flags_idx = self.codegen.type_indices[&flags_mn];

        let bitmask = self.add_local(ValType::I32);
        self.instruction(Instruction::LocalSet(bitmask));

        // Create 6 bool fields from bitmask
        for bit in 0..6u32 {
            self.instruction(Instruction::LocalGet(bitmask));
            self.instruction(Instruction::I32Const(1 << bit as i32));
            self.instruction(Instruction::I32And);
            self.instruction(Instruction::I32Const(0));
            self.instruction(Instruction::I32Ne);
        }

        self.instruction(Instruction::StructNew(flags_idx));
    }

    /// Convert PathFlags record on stack to i32 bitmask.
    /// PathFlags: symlinkFollow(bit0).
    pub(super) fn wasi_fs_path_flags_to_bitmask(&mut self, flags_type: &Type) {
        let flags_mn = flags_type.mangled_name();
        let flags_idx = self.codegen.type_indices[&flags_mn];
        let flags_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(flags_idx),
        });
        let flags_local = self.add_local(flags_ref);
        self.instruction(Instruction::LocalSet(flags_local));

        // Just field 0 (symlinkFollow)
        self.instruction(Instruction::LocalGet(flags_local));
        self.instruction(Instruction::StructGet {
            struct_type_index: flags_idx,
            field_index: 0,
        });
    }

    /// Convert OpenFlags record on stack to i32 bitmask.
    /// OpenFlags: create(bit0), directory(bit1), exclusive(bit2), truncate(bit3).
    pub(super) fn wasi_fs_open_flags_to_bitmask(&mut self, flags_type: &Type) {
        let flags_mn = flags_type.mangled_name();
        let flags_idx = self.codegen.type_indices[&flags_mn];
        let flags_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(flags_idx),
        });
        let flags_local = self.add_local(flags_ref);
        self.instruction(Instruction::LocalSet(flags_local));

        let result_local = self.add_local(ValType::I32);
        self.instruction(Instruction::I32Const(0));
        self.instruction(Instruction::LocalSet(result_local));

        for bit in 0..4u32 {
            self.instruction(Instruction::LocalGet(flags_local));
            self.instruction(Instruction::StructGet {
                struct_type_index: flags_idx,
                field_index: bit,
            });
            self.emit_if_block(BlockType::Empty);
            self.instruction(Instruction::LocalGet(result_local));
            self.instruction(Instruction::I32Const(1 << bit as i32));
            self.instruction(Instruction::I32Or);
            self.instruction(Instruction::LocalSet(result_local));
            self.emit_end_block();
        }

        self.instruction(Instruction::LocalGet(result_local));
    }

    /// Construct Result<FileStat, FileSystemError> from retptr.
    /// Canonical ABI layout, p3 (112 bytes — callers must allocate that much;
    /// p2's block was 104, and a retptr sized to the old number would have the
    /// host write the last 8 bytes of the status-change option straight over the
    /// next block's `[size][next]` header):
    ///   @0: U8 result disc (0=ok, 1=err)
    ///   ok payload (starts at @8, max_case_alignment=8):
    ///     @8:  descriptor-type (a 16-byte variant in p3, not a 1-byte enum:
    ///          case disc @8, and for `other`, option disc @12 / ptr @16 / len @20)
    ///     @24: u64 link-count
    ///     @32: u64 size
    ///     @40: option<datetime> data-access (24 bytes)
    ///     @64: option<datetime> data-modification (24 bytes)
    ///     @88: option<datetime> status-change (24 bytes)
    ///   err payload:
    ///     @8: U8 error-code (its own `other` message at @16, freed below)
    pub(super) fn wasi_fs_construct_result_file_stat(
        &mut self,
        retptr_local: u32,
        result_type: &Type,
    ) {
        // The payload aligns to 8 here (the ok case holds u64s), so the
        // error-code sits at 8 and its message pointer at 16.
        self.emit_p3_fs_result(retptr_local, result_type, 8, |e| {
            let ok_type = match result_type {
                Type::GenericEnum { type_args, .. } => &type_args[0].1,
                _ => unreachable!(),
            };
            let stat_mn = ok_type.mangled_name();
            let stat_idx = e.codegen.type_indices[&stat_mn];

            // The type field is a variant, so a stat of a device or FIFO can
            // arrive as `other(some(msg))` — a host block the Dovetail FileStat
            // has no field for. Same shape, same helper as the error codes.
            e.wasi_free_error_message(retptr_local, 8, DESCRIPTOR_TYPE_OTHER_DISC);

            // Field 0: descriptorType (case disc U8 @8)
            e.wasi_i32_load8_u(retptr_local, 8);
            e.wasi_fs_construct_descriptor_type(result_type);

            // p3 layout: the type field is a 16-byte variant (payload case
            // `other(option<string>)`), shifting later fields by 8 vs p2.
            // Field 1: linkCount (u64 @24)
            e.wasi_i64_load(retptr_local, 24);

            // Field 2: size (u64 @32)
            e.wasi_i64_load(retptr_local, 32);

            let option_instant_type = e.find_record_field_type(&stat_mn, 3);

            // Field 3: dataAccessTimestamp (option<instant> @40)
            e.wasi_fs_construct_option_instant(retptr_local, 40, &option_instant_type);

            // Field 4: dataModificationTimestamp (option<instant> @64)
            e.wasi_fs_construct_option_instant(retptr_local, 64, &option_instant_type);

            // Field 5: statusChangeTimestamp (option<instant> @88)
            e.wasi_fs_construct_option_instant(retptr_local, 88, &option_instant_type);

            e.instruction(Instruction::StructNew(stat_idx));
        });
    }

    /// Construct Option<Instant> from linear memory at retptr+base_offset.
    /// Canonical ABI layout: disc U8 @0, seconds u64 @8, nanoseconds u32 @16.
    fn wasi_fs_construct_option_instant(
        &mut self,
        retptr_local: u32,
        base_offset: u64,
        option_type: &Type,
    ) {
        // Under full erasure, the canonical Option's `Some` variant has payload type
        // `TypeVariable("T")` — we recover the concrete payload (`Instant`) from the
        // `Option<Instant>` Type's type_args.
        let (option_mn, type_args): (&MangledName, Vec<Type>) = match option_type {
            Type::GenericEnum {
                mangled_name,
                type_args,
                ..
            } => (
                mangled_name,
                type_args.iter().map(|(_, t)| t.clone()).collect(),
            ),
            Type::Enum(_, mangled_name) => (mangled_name, Vec::new()),
            _ => unreachable!("wasi_fs_construct_option_instant expects an Option enum type"),
        };
        let option_base_idx = self.codegen.type_indices[option_mn];
        let option_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(option_base_idx),
        });

        let some_idx = self.codegen.variant_type_indices[&(option_mn.clone(), "Some".to_string())];
        let none_idx = self.codegen.variant_type_indices[&(option_mn.clone(), "None".to_string())];

        self.wasi_i32_load8_u(retptr_local, base_offset);
        self.emit_if_block(BlockType::Result(option_ref));
        // Some(Instant)
        {
            self.wasi_i64_load(retptr_local, base_offset + 8);
            self.wasi_i32_load(retptr_local, base_offset + 16);
            let instant_mn = self.find_record_mn_for_variant(option_mn, "Some", &type_args);
            let instant_idx = self.codegen.type_indices[&instant_mn];
            self.instruction(Instruction::StructNew(instant_idx));
            self.emit_nominal_struct_new(option_type, some_idx);
        }
        self.instruction(Instruction::Else);
        // None
        {
            self.emit_nominal_struct_new(option_type, none_idx);
        }
        self.emit_end_block();
    }

    /// Construct Option<T> where T is a newtype over i32 from retptr in linear memory.
    ///
    /// Canonical ABI layout: disc U8 @0 (0=None, 1=Some), payload i32 @4 — align 4,
    /// size 8. The discriminant is ONE byte; @1..3 is padding the host never writes,
    /// so it must be read with `i32.load8_u`. A 4-byte load folds in whatever the
    /// previous user of this scratch block left there — `wasi_bump_alloc` never
    /// zeroes the arena and the bump pointer is rewound and reused on every call —
    /// which turns a host `none` into `Some(<stale handle>)`.
    pub(super) fn wasi_construct_option_i32(&mut self, retptr_local: u32, option_type: &Type) {
        let option_mn = option_type.mangled_name();
        let option_base_idx = self.codegen.type_indices[&option_mn];
        let option_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(option_base_idx),
        });

        self.wasi_i32_load8_u(retptr_local, 0);
        self.emit_if_block(BlockType::Result(option_ref));
        // then: disc != 0 → Some(handle)
        {
            self.wasi_i32_load(retptr_local, 4);
            // Under full erasure, Some's payload slot is anyref; box the i32 handle.
            self.instruction(Instruction::StructNew(
                self.codegen.box_type_index_for(&Type::Int32),
            ));
            let some_idx =
                self.codegen.variant_type_indices[&(option_mn.clone(), "Some".to_string())];
            self.emit_nominal_struct_new(option_type, some_idx);
        }
        self.instruction(Instruction::Else);
        // else: None
        {
            let none_idx =
                self.codegen.variant_type_indices[&(option_mn.clone(), "None".to_string())];
            self.emit_nominal_struct_new(option_type, none_idx);
        }
        self.emit_end_block();
    }

    /// Construct Option<String> from retptr in linear memory.
    ///
    /// Canonical ABI layout: disc U8 @0 (0=None, 1=Some), str_ptr @4, str_len @8 —
    /// align 4, size 12. One-byte discriminant, @1..3 padding the host never writes;
    /// see `wasi_construct_option_i32` for why a 4-byte load of it is a bug. Here the
    /// consequence is worse than a stale handle: the `Some` arm lifts a string from a
    /// garbage pointer/length and then hands that pointer to `pinned_free`.
    pub(super) fn wasi_construct_option_string(&mut self, retptr_local: u32, option_type: &Type) {
        let option_mn = option_type.mangled_name();
        let option_base_idx = self.codegen.type_indices[&option_mn];
        let option_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(option_base_idx),
        });

        self.wasi_i32_load8_u(retptr_local, 0);
        self.emit_if_block(BlockType::Result(option_ref));
        // then: disc != 0 → Some(string)
        {
            let str_ptr = self.add_local(ValType::I32);
            let str_len = self.add_local(ValType::I32);
            self.wasi_i32_load(retptr_local, 4);
            self.instruction(Instruction::LocalSet(str_ptr));
            self.wasi_i32_load(retptr_local, 8);
            self.instruction(Instruction::LocalSet(str_len));
            self.wasi_create_string_from_memory(str_ptr, str_len);
            // The host allocated these bytes through `cabi_realloc`; the GC
            // string owns a copy now.
            self.wasi_free_lifted_block(str_ptr);
            let some_idx =
                self.codegen.variant_type_indices[&(option_mn.clone(), "Some".to_string())];
            self.emit_nominal_struct_new(option_type, some_idx);
        }
        self.instruction(Instruction::Else);
        // else: None
        {
            let none_idx =
                self.codegen.variant_type_indices[&(option_mn.clone(), "None".to_string())];
            self.emit_nominal_struct_new(option_type, none_idx);
        }
        self.emit_end_block();
    }
}
