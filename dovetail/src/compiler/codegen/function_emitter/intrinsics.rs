//! Intrinsic function codegen: WASI imports, string/array ops, networking, TCP.

use wasm_encoder::{BlockType, Instruction, ValType};

use crate::typechecker::types::{IntrinsicKind, Type, TypedExpr};

use super::super::p3_imports::{
    FUNC_P3_CLI_ENVIRONMENT_GET_ARGUMENTS, FUNC_P3_CLI_ENVIRONMENT_GET_ENVIRONMENT,
    FUNC_P3_CLI_ENVIRONMENT_GET_INITIAL_CWD, FUNC_P3_CLI_EXIT_EXIT,
    FUNC_P3_CLI_STDERR_ASYNC_FUTURE_READ_1_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDERR_ASYNC_STREAM_WRITE_0_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDERR_FUTURE_CANCEL_READ_1_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDERR_STREAM_CANCEL_WRITE_0_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDERR_STREAM_NEW_0_WRITE_VIA_STREAM, FUNC_P3_CLI_STDERR_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDIN_ASYNC_FUTURE_READ_1_READ_VIA_STREAM,
    FUNC_P3_CLI_STDIN_ASYNC_STREAM_READ_0_READ_VIA_STREAM,
    FUNC_P3_CLI_STDIN_FUTURE_CANCEL_READ_1_READ_VIA_STREAM,
    FUNC_P3_CLI_STDIN_FUTURE_DROP_READABLE_1_READ_VIA_STREAM, FUNC_P3_CLI_STDIN_READ_VIA_STREAM,
    FUNC_P3_CLI_STDIN_STREAM_CANCEL_READ_0_READ_VIA_STREAM,
    FUNC_P3_CLI_STDIN_STREAM_DROP_READABLE_0_READ_VIA_STREAM,
    FUNC_P3_CLI_STDOUT_ASYNC_FUTURE_READ_1_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDOUT_ASYNC_STREAM_WRITE_0_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDOUT_FUTURE_CANCEL_READ_1_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDOUT_STREAM_CANCEL_WRITE_0_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDOUT_STREAM_NEW_0_WRITE_VIA_STREAM, FUNC_P3_CLI_STDOUT_WRITE_VIA_STREAM,
    FUNC_P3_CLI_TERMINAL_STDERR_GET_TERMINAL_STDERR, FUNC_P3_CLI_TERMINAL_STDIN_GET_TERMINAL_STDIN,
    FUNC_P3_CLI_TERMINAL_STDOUT_GET_TERMINAL_STDOUT, FUNC_P3_CLOCKS_MONOTONIC_CLOCK_ASYNC_WAIT_FOR,
    FUNC_P3_CLOCKS_MONOTONIC_CLOCK_ASYNC_WAIT_UNTIL, FUNC_P3_CLOCKS_MONOTONIC_CLOCK_NOW,
    FUNC_P3_CLOCKS_SYSTEM_CLOCK_NOW, FUNC_P3_FILESYSTEM_PREOPENS_GET_DIRECTORIES,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_FUTURE_READ_1_METHOD_DESCRIPTOR_APPEND_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_FUTURE_READ_1_METHOD_DESCRIPTOR_READ_DIRECTORY,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_FUTURE_READ_1_METHOD_DESCRIPTOR_READ_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_FUTURE_READ_1_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_ADVISE,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_CREATE_DIRECTORY_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_GET_FLAGS,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_GET_TYPE,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_IS_SAME_OBJECT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_LINK_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_METADATA_HASH,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_METADATA_HASH_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_OPEN_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_READLINK_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_REMOVE_DIRECTORY_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_RENAME_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SET_SIZE,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SET_TIMES,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SET_TIMES_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_STAT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_STAT_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SYMLINK_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SYNC,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SYNC_DATA,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_UNLINK_FILE_AT,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_STREAM_READ_0_METHOD_DESCRIPTOR_READ_DIRECTORY,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_STREAM_READ_0_METHOD_DESCRIPTOR_READ_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_STREAM_WRITE_0_METHOD_DESCRIPTOR_APPEND_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_ASYNC_STREAM_WRITE_0_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_DROP_DESCRIPTOR,
    FUNC_P3_FILESYSTEM_TYPES_FUTURE_CANCEL_READ_1_METHOD_DESCRIPTOR_APPEND_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_FUTURE_CANCEL_READ_1_METHOD_DESCRIPTOR_READ_DIRECTORY,
    FUNC_P3_FILESYSTEM_TYPES_FUTURE_CANCEL_READ_1_METHOD_DESCRIPTOR_READ_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_FUTURE_CANCEL_READ_1_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_FUTURE_DROP_READABLE_1_METHOD_DESCRIPTOR_APPEND_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_FUTURE_DROP_READABLE_1_METHOD_DESCRIPTOR_READ_DIRECTORY,
    FUNC_P3_FILESYSTEM_TYPES_FUTURE_DROP_READABLE_1_METHOD_DESCRIPTOR_READ_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_FUTURE_DROP_READABLE_1_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_METHOD_DESCRIPTOR_APPEND_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_METHOD_DESCRIPTOR_READ_DIRECTORY,
    FUNC_P3_FILESYSTEM_TYPES_METHOD_DESCRIPTOR_READ_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_STREAM_CANCEL_READ_0_METHOD_DESCRIPTOR_READ_DIRECTORY,
    FUNC_P3_FILESYSTEM_TYPES_STREAM_CANCEL_READ_0_METHOD_DESCRIPTOR_READ_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_STREAM_CANCEL_WRITE_0_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_STREAM_DROP_READABLE_0_METHOD_DESCRIPTOR_READ_DIRECTORY,
    FUNC_P3_FILESYSTEM_TYPES_STREAM_DROP_READABLE_0_METHOD_DESCRIPTOR_READ_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_STREAM_DROP_WRITABLE_0_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_STREAM_NEW_0_METHOD_DESCRIPTOR_APPEND_VIA_STREAM,
    FUNC_P3_FILESYSTEM_TYPES_STREAM_NEW_0_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
    FUNC_P3_RANDOM_RANDOM_GET_RANDOM_BYTES, FUNC_P3_RANDOM_RANDOM_GET_RANDOM_U64,
    FUNC_P3_ROOT_SUBTASK_CANCEL, FUNC_P3_ROOT_SUBTASK_DROP, FUNC_P3_ROOT_WAITABLE_JOIN,
    FUNC_P3_ROOT_WAITABLE_SET_DROP, FUNC_P3_ROOT_WAITABLE_SET_NEW, FUNC_P3_ROOT_WAITABLE_SET_WAIT,
    FUNC_P3_SOCKETS_IP_NAME_LOOKUP_ASYNC_RESOLVE_ADDRESSES,
    FUNC_P3_SOCKETS_TYPES_ASYNC_FUTURE_READ_1_METHOD_TCP_SOCKET_RECEIVE,
    FUNC_P3_SOCKETS_TYPES_ASYNC_FUTURE_READ_1_METHOD_TCP_SOCKET_SEND,
    FUNC_P3_SOCKETS_TYPES_ASYNC_METHOD_TCP_SOCKET_CONNECT,
    FUNC_P3_SOCKETS_TYPES_ASYNC_METHOD_UDP_SOCKET_RECEIVE,
    FUNC_P3_SOCKETS_TYPES_ASYNC_METHOD_UDP_SOCKET_SEND,
    FUNC_P3_SOCKETS_TYPES_ASYNC_STREAM_READ_0_METHOD_TCP_SOCKET_LISTEN,
    FUNC_P3_SOCKETS_TYPES_ASYNC_STREAM_READ_0_METHOD_TCP_SOCKET_RECEIVE,
    FUNC_P3_SOCKETS_TYPES_ASYNC_STREAM_WRITE_0_METHOD_TCP_SOCKET_SEND,
    FUNC_P3_SOCKETS_TYPES_DROP_TCP_SOCKET, FUNC_P3_SOCKETS_TYPES_DROP_UDP_SOCKET,
    FUNC_P3_SOCKETS_TYPES_FUTURE_CANCEL_READ_1_METHOD_TCP_SOCKET_RECEIVE,
    FUNC_P3_SOCKETS_TYPES_FUTURE_CANCEL_READ_1_METHOD_TCP_SOCKET_SEND,
    FUNC_P3_SOCKETS_TYPES_FUTURE_DROP_READABLE_1_METHOD_TCP_SOCKET_RECEIVE,
    FUNC_P3_SOCKETS_TYPES_FUTURE_DROP_READABLE_1_METHOD_TCP_SOCKET_SEND,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_BIND,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_ADDRESS_FAMILY,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_HOP_LIMIT,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_IS_LISTENING,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_KEEP_ALIVE_COUNT,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_KEEP_ALIVE_ENABLED,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_KEEP_ALIVE_IDLE_TIME,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_KEEP_ALIVE_INTERVAL,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_LOCAL_ADDRESS,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_RECEIVE_BUFFER_SIZE,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_REMOTE_ADDRESS,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_SEND_BUFFER_SIZE,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_LISTEN,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_RECEIVE, FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SEND,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_HOP_LIMIT,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_KEEP_ALIVE_COUNT,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_KEEP_ALIVE_ENABLED,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_KEEP_ALIVE_IDLE_TIME,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_KEEP_ALIVE_INTERVAL,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_LISTEN_BACKLOG_SIZE,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_RECEIVE_BUFFER_SIZE,
    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_SEND_BUFFER_SIZE,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_BIND, FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_CONNECT,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_DISCONNECT,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_ADDRESS_FAMILY,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_LOCAL_ADDRESS,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_RECEIVE_BUFFER_SIZE,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_REMOTE_ADDRESS,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_SEND_BUFFER_SIZE,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_UNICAST_HOP_LIMIT,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_SET_RECEIVE_BUFFER_SIZE,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_SET_SEND_BUFFER_SIZE,
    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_SET_UNICAST_HOP_LIMIT,
    FUNC_P3_SOCKETS_TYPES_STATIC_TCP_SOCKET_CREATE, FUNC_P3_SOCKETS_TYPES_STATIC_UDP_SOCKET_CREATE,
    FUNC_P3_SOCKETS_TYPES_STREAM_CANCEL_READ_0_METHOD_TCP_SOCKET_LISTEN,
    FUNC_P3_SOCKETS_TYPES_STREAM_CANCEL_READ_0_METHOD_TCP_SOCKET_RECEIVE,
    FUNC_P3_SOCKETS_TYPES_STREAM_CANCEL_WRITE_0_METHOD_TCP_SOCKET_SEND,
    FUNC_P3_SOCKETS_TYPES_STREAM_DROP_READABLE_0_METHOD_TCP_SOCKET_LISTEN,
    FUNC_P3_SOCKETS_TYPES_STREAM_DROP_READABLE_0_METHOD_TCP_SOCKET_RECEIVE,
    FUNC_P3_SOCKETS_TYPES_STREAM_DROP_WRITABLE_0_METHOD_TCP_SOCKET_SEND,
    FUNC_P3_SOCKETS_TYPES_STREAM_NEW_0_METHOD_TCP_SOCKET_SEND,
};
use super::wasi_marshaling::{DESCRIPTOR_TYPE_OTHER_DISC, WasiScalar};
use super::{ExprContext, FunctionEmitter};

impl FunctionEmitter<'_> {
    fn emit_identity_hash(&mut self, value: &TypedExpr) {
        use super::super::GLOBAL_IDENTITY_HASH_COUNTER;
        let class_name = match &value.ty {
            Type::Class(_, name)
            | Type::GenericClass {
                mangled_name: name, ..
            } => name,
            _ => unreachable!("identity hash must be specialized to a class"),
        };
        let class_index = self.codegen.type_indices[class_name];
        let object = self.add_local(self.codegen.single_val_type(&value.ty));
        let hash = self.add_local(ValType::I32);
        self.emit_expr(value, ExprContext::Value);
        self.instruction(Instruction::LocalTee(object));
        self.instruction(Instruction::StructGet {
            struct_type_index: class_index,
            field_index: self.codegen.class_hash_field(class_name),
        });
        self.instruction(Instruction::LocalTee(hash));
        self.instruction(Instruction::I32Eqz);
        self.emit_if_block(BlockType::Empty);
        self.instruction(Instruction::GlobalGet(GLOBAL_IDENTITY_HASH_COUNTER));
        self.instruction(Instruction::LocalSet(hash));
        self.instruction(Instruction::LocalGet(object));
        self.instruction(Instruction::LocalGet(hash));
        self.instruction(Instruction::StructSet {
            struct_type_index: class_index,
            field_index: self.codegen.class_hash_field(class_name),
        });
        self.instruction(Instruction::LocalGet(hash));
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::I32Add);
        self.instruction(Instruction::GlobalSet(GLOBAL_IDENTITY_HASH_COUNTER));
        self.instruction(Instruction::GlobalGet(GLOBAL_IDENTITY_HASH_COUNTER));
        self.instruction(Instruction::I32Eqz);
        self.emit_if_block(BlockType::Empty);
        self.instruction(Instruction::I32Const(1));
        self.instruction(Instruction::GlobalSet(GLOBAL_IDENTITY_HASH_COUNTER));
        self.emit_end_block();
        self.emit_end_block();
        self.instruction(Instruction::LocalGet(hash));
        self.instruction(Instruction::I64ExtendI32U);
    }

    /// Emit an intrinsic call, bracketing WASI intrinsics with a
    /// save/restore of the scratch bump pointer (global 0) so per-call
    /// marshaling memory is reclaimed (see `scratch_save`).
    pub(super) fn emit_intrinsic_call(
        &mut self,
        intrinsic: &IntrinsicKind,
        args: &[TypedExpr],
        expr: &crate::typechecker::types::TypedExpr,
        ctx: ExprContext,
    ) {
        let scratch = if intrinsic_uses_scratch(intrinsic) {
            Some(self.scratch_save())
        } else {
            None
        };
        self.emit_intrinsic_call_inner(intrinsic, args, expr, ctx);
        if let Some(saved) = scratch {
            self.scratch_restore(saved);
        }
    }

    fn emit_intrinsic_call_inner(
        &mut self,
        intrinsic: &IntrinsicKind,
        args: &[TypedExpr],
        expr: &crate::typechecker::types::TypedExpr,
        ctx: ExprContext,
    ) {
        match intrinsic {
            IntrinsicKind::TupleProjection(_) => {
                unreachable!("tuple projection must be resolved before emission")
            }
            IntrinsicKind::BinaryOperator(op) => {
                let binary = TypedExpr {
                    kind: crate::typechecker::types::TypedExprKind::BinaryOp {
                        op: *op,
                        left: Box::new(args[0].clone()),
                        right: Box::new(args[1].clone()),
                    },
                    ty: expr.ty.clone(),
                    span: expr.span.clone(),
                };
                self.emit_expr(&binary, ctx);
            }

            IntrinsicKind::ClassIdentityEquals => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                self.instruction(Instruction::RefEq);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::ClassIdentityHash => {
                self.emit_identity_hash(&args[0]);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::StringUnsafeBytes => {
                // String backing array is the SAME WASM type as Array<Uint8>.
                // Zero-copy: just extract field 0 (backing) from the string struct.
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::StructGet {
                    struct_type_index: super::super::STRING_STRUCT_TYPE_INDEX,
                    field_index: 0,
                });
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::ReadonlySliceMake
            | IntrinsicKind::ReadonlySliceLength
            | IntrinsicKind::ReadonlySliceGet
            | IntrinsicKind::ReadonlySliceSlice
            | IntrinsicKind::ReadonlySliceCopyTo => {
                self.emit_readonly_slice(intrinsic, args, expr, ctx);
            }
            IntrinsicKind::ArrayGet => {
                // args[0] = array, args[1] = index
                let elem_type = match &args[0].ty {
                    Type::Array(elem) => elem.as_ref().clone(),
                    _ => unreachable!("ArrayGet receiver must have Array type"),
                };
                let array_type_index = self.codegen.array_type_index(&elem_type);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                // Reads the element as its single value type, casting back from `(ref any)` for the
                // shared `$Array$ref`.
                self.emit_array_element_get(&elem_type, array_type_index);
                // A boxed tuple/Uint128 element explodes into its values (only when the value is
                // wanted; in statement context the boxed ref stays and is dropped by drop_if_statement).
                if ctx == ExprContext::Value && self.codegen.is_tuple(&elem_type) {
                    self.emit_unbox_tuple(&elem_type);
                } else if ctx == ExprContext::Value && self.codegen.is_uint128(&elem_type) {
                    self.emit_unbox_uint128();
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::ArraySet => {
                // args[0] = array, args[1] = index, args[2] = value
                let elem_type = match &args[0].ty {
                    Type::Array(elem) => elem.as_ref().clone(),
                    _ => unreachable!("ArraySet receiver must have Array type"),
                };
                let array_type_index = self.codegen.array_type_index(&elem_type);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                self.emit_expr(&args[2], ExprContext::Value);
                // The element slot is boxed; a tuple value reboxes its values, a Uint128 its `[lo, hi]`.
                if self.codegen.is_tuple(&elem_type) {
                    self.emit_rebox_tuple(&elem_type);
                } else if self.codegen.is_uint128(&elem_type) {
                    self.emit_rebox_uint128();
                }
                self.instruction(Instruction::ArraySet(array_type_index));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }
            IntrinsicKind::ArrayLength => {
                // args[0] = array
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::ArrayLen);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::ArrayClone => {
                // args[0] = source array
                let elem_type = match &args[0].ty {
                    Type::Array(elem) => elem.as_ref().clone(),
                    _ => unreachable!("ArrayClone receiver must have Array type"),
                };
                self.emit_array_clone(&elem_type, &args[0]);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::ArrayFill => {
                // args[0] = length, args[1] = value
                let elem = match &expr.ty {
                    Type::Array(elem) => elem.as_ref(),
                    _ => unreachable!("ArrayFill must have Array type"),
                };
                let array_type_index = self.codegen.array_type_index(elem);
                let elem = elem.clone();
                self.emit_expr(&args[1], ExprContext::Value);
                // The fill value occupies a boxed element slot; rebox a tuple's values / a Uint128.
                if self.codegen.is_tuple(&elem) {
                    self.emit_rebox_tuple(&elem);
                } else if self.codegen.is_uint128(&elem) {
                    self.emit_rebox_uint128();
                }
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::ArrayNew(array_type_index));
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::ArrayEmpty => {
                let elem = match &expr.ty {
                    Type::Array(elem) => elem.as_ref(),
                    _ => unreachable!("ArrayEmpty must have Array type"),
                };
                let array_type_index = self.codegen.array_type_index(elem);
                self.instruction(Instruction::ArrayNewFixed {
                    array_type_index,
                    array_size: 0,
                });
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::NumericConvert(target) => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_numeric_convert(&args[0].ty, target);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::Uint128Multiply => {
                // Widening multiply: u64 × u64 → u128. The two `Uint64` args are each one `i64`;
                // `i64.mul_wide_u` consumes them and leaves the 128-bit product as `[lo, hi]` —
                // exactly the flattened `Uint128` representation.
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                self.instruction(Instruction::I64MulWideU);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::Uint128Make => {
                // Construct a Uint128 from (lo, hi). Each `Uint64` arg is one `i64`; pushing
                // `lo` then `hi` leaves `[lo, hi]` on the stack — exactly the flattened
                // `Uint128` representation. No instruction beyond evaluating the args.
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::Uint128High => {
                // args[0] = Uint128 -> stack `[lo, hi]` with `hi` on top. Keep `hi` and
                // drop `lo` using a scratch local (no shift). The low word is `toUint64()`,
                // which is just a `Drop` of the high word.
                self.emit_expr(&args[0], ExprContext::Value);
                let tmp = self.add_local(wasm_encoder::ValType::I64);
                self.instruction(Instruction::LocalSet(tmp));
                self.instruction(Instruction::Drop);
                self.instruction(Instruction::LocalGet(tmp));
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::ArrayExtend => {
                // args[0] = self (source array), args[1] = fill value, args[2] = newSize
                // Inlined: create new array with fill, copy min(src.len, newSize) from src
                let elem_type = match &args[0].ty {
                    Type::Array(elem) => elem.as_ref().clone(),
                    _ => unreachable!("ArrayExtend receiver must have Array type"),
                };
                self.emit_inline_array_extend(&elem_type, args);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::ArrayConcat => {
                // args[0] = self, args[1] = other
                // Inlined: allocate new array of len_a + len_b, copy both
                let elem_type = match &args[0].ty {
                    Type::Array(elem) => elem.as_ref().clone(),
                    _ => unreachable!("ArrayConcat receiver must have Array type"),
                };
                self.emit_inline_array_concat(&elem_type, args);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::ArrayCopy => {
                // args[0] = self (source), args[1] = sourceOffset, args[2] = destination,
                // args[3] = destinationOffset, args[4] = length
                // Returns destination array.
                let elem_type = match &args[0].ty {
                    Type::Array(elem) => elem.as_ref().clone(),
                    _ => unreachable!("ArrayCopy receiver must have Array type"),
                };
                let array_type_index = self.codegen.array_type_index(&elem_type);

                // Save destination ref so we can return it
                self.emit_expr(&args[2], ExprContext::Value);
                let dst_ref_type = wasm_encoder::ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(array_type_index),
                });
                let dst_local = self.add_local(dst_ref_type);
                self.instruction(Instruction::LocalSet(dst_local));

                // array.copy: dst, dstOffset, src, srcOffset, length
                self.instruction(Instruction::LocalGet(dst_local));
                self.emit_expr(&args[3], ExprContext::Value); // destinationOffset
                self.emit_expr(&args[0], ExprContext::Value); // source array
                self.emit_expr(&args[1], ExprContext::Value); // sourceOffset
                self.emit_expr(&args[4], ExprContext::Value); // length
                self.instruction(Instruction::ArrayCopy {
                    array_type_index_dst: array_type_index,
                    array_type_index_src: array_type_index,
                });

                // Return destination array
                self.instruction(Instruction::LocalGet(dst_local));
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::StringFromBytes => {
                // args[0] = buf (Array<Uint8>), args[1] = start, args[2] = length
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                self.emit_expr(&args[2], ExprContext::Value);
                self.instruction(Instruction::Call(self.codegen.func_string_from_bytes()));
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::StringFromChar => {
                // args[0] = char
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(self.codegen.func_char_to_string()));
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::StringLength => {
                // Extract utf8 field, mask off ASCII flag to get char count
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::StructGet {
                    struct_type_index: super::super::STRING_STRUCT_TYPE_INDEX,
                    field_index: 1,
                });
                self.instruction(Instruction::I32Const(0x7FFFFFFF));
                self.instruction(Instruction::I32And);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::StringByteLength => {
                // Extract backing array, get its length
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::StructGet {
                    struct_type_index: super::super::STRING_STRUCT_TYPE_INDEX,
                    field_index: 0,
                });
                self.instruction(Instruction::ArrayLen);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::StringGetChar => {
                // args[0] = string, args[1] = index
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                self.instruction(Instruction::Call(self.codegen.func_string_get_char()));
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::DebugPrint => {
                // args[0] is already a String
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(self.codegen.func_debug_print()));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }
            IntrinsicKind::DebugPrintDisplay { format_method } => {
                // args[0] is value of type T; call format_method(value) → String, then debug_print
                self.emit_expr(&args[0], ExprContext::Value);
                let format_idx = self.codegen.function_indices[format_method];
                self.instruction(Instruction::Call(format_idx));
                self.instruction(Instruction::Call(self.codegen.func_debug_print()));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }
            IntrinsicKind::ConsolePrint => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(self.codegen.func_console_print()));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }
            IntrinsicKind::ConsolePrintln => {
                // Reuses debug_print (stdout + newline)
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(self.codegen.func_debug_print()));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }
            IntrinsicKind::ConsoleEprint => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(self.codegen.func_console_eprint()));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }
            IntrinsicKind::ConsoleEprintln => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(self.codegen.func_console_eprintln()));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }
            IntrinsicKind::StringIsAscii => {
                // Extract utf8 field, check sign bit (negative = is_ascii)
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::StructGet {
                    struct_type_index: super::super::STRING_STRUCT_TYPE_INDEX,
                    field_index: 1,
                });
                self.instruction(Instruction::I32Const(0));
                self.instruction(Instruction::I32LtS);
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::FloatToBits => {
                self.emit_expr(&args[0], ExprContext::Value);
                match &args[0].ty {
                    Type::Float64 => self.instruction(Instruction::I64ReinterpretF64),
                    Type::Float32 => self.instruction(Instruction::I32ReinterpretF32),
                    _ => unreachable!("FloatToBits on non-float type"),
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::BitsToFloat => {
                self.emit_expr(&args[0], ExprContext::Value);
                match &args[0].ty {
                    Type::Int64 => self.instruction(Instruction::F64ReinterpretI64),
                    Type::Int32 => self.instruction(Instruction::F32ReinterpretI32),
                    _ => unreachable!("BitsToFloat on non-integer type"),
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::MathFloor => {
                self.emit_expr(&args[0], ExprContext::Value);
                match &args[0].ty {
                    Type::Float64 => self.instruction(Instruction::F64Floor),
                    Type::Float32 => self.instruction(Instruction::F32Floor),
                    _ => unreachable!("MathFloor on non-float type"),
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::MathTrunc => {
                self.emit_expr(&args[0], ExprContext::Value);
                match &args[0].ty {
                    Type::Float64 => self.instruction(Instruction::F64Trunc),
                    Type::Float32 => self.instruction(Instruction::F32Trunc),
                    _ => unreachable!("MathTrunc on non-float type"),
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::MathAbs => {
                self.emit_expr(&args[0], ExprContext::Value);
                match &args[0].ty {
                    Type::Float64 => self.instruction(Instruction::F64Abs),
                    Type::Float32 => self.instruction(Instruction::F32Abs),
                    _ => unreachable!("MathAbs on non-float type"),
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::MathFmod => {
                // x - trunc(x / y) * y
                let is_f64 = matches!(&args[0].ty, Type::Float64);
                let val_type = if is_f64 {
                    wasm_encoder::ValType::F64
                } else {
                    wasm_encoder::ValType::F32
                };
                let tmp_x = self.add_local(val_type);
                let tmp_y = self.add_local(val_type);
                // Save x and y into locals
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalSet(tmp_x));
                self.emit_expr(&args[1], ExprContext::Value);
                self.instruction(Instruction::LocalSet(tmp_y));
                // x - trunc(x / y) * y
                self.instruction(Instruction::LocalGet(tmp_x));
                self.instruction(Instruction::LocalGet(tmp_x));
                self.instruction(Instruction::LocalGet(tmp_y));
                if is_f64 {
                    self.instruction(Instruction::F64Div);
                    self.instruction(Instruction::F64Trunc);
                } else {
                    self.instruction(Instruction::F32Div);
                    self.instruction(Instruction::F32Trunc);
                }
                self.instruction(Instruction::LocalGet(tmp_y));
                if is_f64 {
                    self.instruction(Instruction::F64Mul);
                    self.instruction(Instruction::F64Sub);
                } else {
                    self.instruction(Instruction::F32Mul);
                    self.instruction(Instruction::F32Sub);
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::MathIsNan => {
                // x != x (NaN is the only value where this is true)
                let is_f64 = matches!(&args[0].ty, Type::Float64);
                let val_type = if is_f64 {
                    wasm_encoder::ValType::F64
                } else {
                    wasm_encoder::ValType::F32
                };
                let tmp = self.add_local(val_type);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalTee(tmp));
                self.instruction(Instruction::LocalGet(tmp));
                if is_f64 {
                    self.instruction(Instruction::F64Ne);
                } else {
                    self.instruction(Instruction::F32Ne);
                }
                self.drop_if_statement(ctx, &expr.ty);
            }
            IntrinsicKind::MathIsInfinity => {
                // abs(x) == infinity
                self.emit_expr(&args[0], ExprContext::Value);
                let is_f64 = matches!(&args[0].ty, Type::Float64);
                if is_f64 {
                    self.instruction(Instruction::F64Abs);
                    self.instruction(Instruction::F64Const((f64::INFINITY).into()));
                    self.instruction(Instruction::F64Eq);
                } else {
                    self.instruction(Instruction::F32Abs);
                    self.instruction(Instruction::F32Const((f32::INFINITY).into()));
                    self.instruction(Instruction::F32Eq);
                }
                self.drop_if_statement(ctx, &expr.ty);
            }

            // --- IO intrinsics ---
            IntrinsicKind::P3ThreadYield => {
                self.instruction(Instruction::Call(
                    crate::codegen::p3_imports::FUNC_P3_ROOT_THREAD_YIELD,
                ));
                self.instruction(Instruction::Drop);
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0));
                }
            }

            IntrinsicKind::P3WaitableSetNew => {
                self.instruction(Instruction::Call(FUNC_P3_ROOT_WAITABLE_SET_NEW));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3WaitableSetJoin => {
                // canonical waitable.join(waitable, set): waitable first
                self.emit_expr(&args[1], ExprContext::Value); // waitable
                self.emit_expr(&args[0], ExprContext::Value); // set (self)
                self.instruction(Instruction::Call(FUNC_P3_ROOT_WAITABLE_JOIN));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3WaitableRemove => {
                // waitable.join(waitable, 0) removes it from its current set
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::I32Const(0));
                self.instruction(Instruction::Call(FUNC_P3_ROOT_WAITABLE_JOIN));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3WaitableSetWait | IntrinsicKind::P3WaitableSetPoll => {
                // waitable-set.wait(set, evtbuf) -> event code; evtbuf: index @0, payload @4.
                // Blocking is legal: every Dovetail export is async-typed.
                let evtbuf = self.wasi_bump_alloc(8);
                // Poll leaves the payload unspecified when it returns no event.
                self.instruction(Instruction::LocalGet(evtbuf));
                self.instruction(Instruction::I64Const(0));
                self.instruction(Instruction::I64Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                self.emit_expr(&args[0], ExprContext::Value); // set (self)
                self.instruction(Instruction::LocalGet(evtbuf));
                let function = if matches!(intrinsic, IntrinsicKind::P3WaitableSetPoll) {
                    crate::codegen::p3_imports::FUNC_P3_ROOT_WAITABLE_SET_POLL
                } else {
                    FUNC_P3_ROOT_WAITABLE_SET_WAIT
                };
                self.instruction(Instruction::Call(function));
                // Construct WaitableEvent { event, index, payload } — event
                // (the call result) is already on the stack.
                self.wasi_i32_load(evtbuf, 0);
                self.wasi_i32_load(evtbuf, 4);
                let mn = expr.ty.mangled_name();
                let type_idx = self.codegen.type_indices[&mn];
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3WaitableSetDrop => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(FUNC_P3_ROOT_WAITABLE_SET_DROP));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3SubtaskDrop => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(FUNC_P3_ROOT_SUBTASK_DROP));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3SubtaskCancel => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(FUNC_P3_ROOT_SUBTASK_CANCEL));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3MonotonicNow => {
                self.instruction(Instruction::Call(FUNC_P3_CLOCKS_MONOTONIC_CLOCK_NOW));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3WaitForStart | IntrinsicKind::P3WaitUntilStart => {
                // async-lowered wait-for(nanoseconds)/wait-until(mark) -> status
                // (no result buffer). Pack into AsyncCall: status << 32 | 0.
                self.emit_expr(&args[0], ExprContext::Value); // i64
                let func = if matches!(intrinsic, IntrinsicKind::P3WaitForStart) {
                    FUNC_P3_CLOCKS_MONOTONIC_CLOCK_ASYNC_WAIT_FOR
                } else {
                    FUNC_P3_CLOCKS_MONOTONIC_CLOCK_ASYNC_WAIT_UNTIL
                };
                self.instruction(Instruction::Call(func));
                // pack AsyncCall: (status as i64) << 32 (retptr = 0)
                self.instruction(Instruction::I64ExtendI32U);
                self.instruction(Instruction::I64Const(32));
                self.instruction(Instruction::I64Shl);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3StdinOpen => {
                // read-via-stream(retptr) -> (); retptr: stream @0, future @4
                let retptr = self.wasi_bump_alloc(8);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(FUNC_P3_CLI_STDIN_READ_VIA_STREAM));
                self.wasi_i32_load(retptr, 0);
                self.wasi_i32_load(retptr, 4);
                let mn = expr.ty.mangled_name();
                let type_idx = self.codegen.type_indices[&mn];
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3StdoutOpen | IntrinsicKind::P3StderrOpen => {
                // Open a fresh stdout/stderr write stream and hand back
                // (writable, result-future): make a stream pair, give the host
                // the readable end via `write-via-stream`, keep the writable end.
                //
                // Deliberately NOT shared with the compiler-owned blocking path
                // in `string_functions::emit_flush_write`, which opens its own
                // pair. p3 allows exactly one outstanding operation per stream
                // end, so one shared end would let a `debug` post a second write
                // under a parked `Console` write (a trap) and would let either
                // path's completion be delivered to whichever waitable-set
                // currently holds the handle (corrupt write-progress accounting).
                // The host explicitly supports several stdio writers — for
                // inherited stdio each one writes straight through to the same
                // fd — so the only cost is that the two paths' output can
                // interleave at chunk boundaries when both are in flight.
                //
                // Every call opens a new stream; `standard.io.Console` caches the
                // one instance the library needs in a module-level binding.
                let is_stdout = matches!(intrinsic, IntrinsicKind::P3StdoutOpen);
                let (stream_new, write_via) = if is_stdout {
                    (
                        FUNC_P3_CLI_STDOUT_STREAM_NEW_0_WRITE_VIA_STREAM,
                        FUNC_P3_CLI_STDOUT_WRITE_VIA_STREAM,
                    )
                } else {
                    (
                        FUNC_P3_CLI_STDERR_STREAM_NEW_0_WRITE_VIA_STREAM,
                        FUNC_P3_CLI_STDERR_WRITE_VIA_STREAM,
                    )
                };
                let pair = self.add_local(ValType::I64);
                let writable = self.add_local(ValType::I32);
                self.instruction(Instruction::Call(stream_new));
                self.instruction(Instruction::LocalTee(pair));
                // writable = high 32 bits
                self.instruction(Instruction::I64Const(32));
                self.instruction(Instruction::I64ShrU);
                self.instruction(Instruction::I32WrapI64);
                self.instruction(Instruction::LocalSet(writable));
                self.instruction(Instruction::LocalGet(writable));
                // hand the readable end (low 32 bits) to the host, keep the future
                self.instruction(Instruction::LocalGet(pair));
                self.instruction(Instruction::I32WrapI64);
                self.instruction(Instruction::Call(write_via));
                let mn = expr.ty.mangled_name();
                let type_idx = self.codegen.type_indices[&mn];
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3StdoutWriteStart | IntrinsicKind::P3StderrWriteStart => {
                let func = if matches!(intrinsic, IntrinsicKind::P3StdoutWriteStart) {
                    FUNC_P3_CLI_STDOUT_ASYNC_STREAM_WRITE_0_WRITE_VIA_STREAM
                } else {
                    FUNC_P3_CLI_STDERR_ASYNC_STREAM_WRITE_0_WRITE_VIA_STREAM
                };
                self.emit_p3_stream_write_start(&args[0], Some(0), &args[1], func);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3StdoutCancelWrite | IntrinsicKind::P3StderrCancelWrite => {
                let func = if matches!(intrinsic, IntrinsicKind::P3StdoutCancelWrite) {
                    FUNC_P3_CLI_STDOUT_STREAM_CANCEL_WRITE_0_WRITE_VIA_STREAM
                } else {
                    FUNC_P3_CLI_STDERR_STREAM_CANCEL_WRITE_0_WRITE_VIA_STREAM
                };
                self.emit_p3_stream_handle(&args[0], Some(0));
                self.instruction(Instruction::Call(func));
                // The packed status is returned, not dropped: a cancel can lose the
                // race with a completion that already copied bytes out, and its
                // count is the only report that they went.
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3StdoutReadResult
            | IntrinsicKind::P3StderrReadResult
            | IntrinsicKind::P3StdinReadResult => {
                // future.read(future, retptr) -> status, reading a resolved
                // `result<_, error-code>`. The wasi:cli `error-code` is a plain
                // 3-case ENUM (not the variant the sockets and filesystem worlds
                // use), so it is size 1 / align 1 and the result is disc @0 +
                // payload @1 = 2 bytes. Same pre-init-Ok / cancel-if-blocked
                // shape as the socket and filesystem result reads.
                //
                // stdin's future comes from `read-via-stream` rather than
                // `write-via-stream`, and says how the INPUT ended: a mid-read
                // host error (a failing redirected file, a reset pipe) resolves
                // it with that error instead of resolving cleanly, which is the
                // only thing that distinguishes a truncated stdin from a
                // complete one.
                let (read_func, cancel_func) = match intrinsic {
                    IntrinsicKind::P3StdoutReadResult => (
                        FUNC_P3_CLI_STDOUT_ASYNC_FUTURE_READ_1_WRITE_VIA_STREAM,
                        FUNC_P3_CLI_STDOUT_FUTURE_CANCEL_READ_1_WRITE_VIA_STREAM,
                    ),
                    IntrinsicKind::P3StderrReadResult => (
                        FUNC_P3_CLI_STDERR_ASYNC_FUTURE_READ_1_WRITE_VIA_STREAM,
                        FUNC_P3_CLI_STDERR_FUTURE_CANCEL_READ_1_WRITE_VIA_STREAM,
                    ),
                    _ => (
                        FUNC_P3_CLI_STDIN_ASYNC_FUTURE_READ_1_READ_VIA_STREAM,
                        FUNC_P3_CLI_STDIN_FUTURE_CANCEL_READ_1_READ_VIA_STREAM,
                    ),
                };
                let future_local = self.add_local(ValType::I32);
                self.emit_p3_stream_handle(&args[0], Some(1));
                self.instruction(Instruction::LocalSet(future_local));
                let retptr = self.wasi_bump_alloc(8);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::I32Const(0));
                self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                self.instruction(Instruction::LocalGet(future_local));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(read_func));
                self.instruction(Instruction::I32Const(0));
                self.instruction(Instruction::I32LtS);
                self.instruction(Instruction::If(BlockType::Empty));
                self.instruction(Instruction::LocalGet(future_local));
                self.instruction(Instruction::Call(cancel_func));
                self.instruction(Instruction::Drop);
                self.instruction(Instruction::End);
                self.wasi_construct_result_unit_cli_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3StdinReadStart => {
                self.emit_p3_stream_read_start(
                    &args[0],
                    Some(0),
                    &args[1],
                    FUNC_P3_CLI_STDIN_ASYNC_STREAM_READ_0_READ_VIA_STREAM,
                );
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3StdinDropReadable => {
                self.emit_p3_stream_handle(&args[0], Some(0));
                self.instruction(Instruction::Call(
                    FUNC_P3_CLI_STDIN_STREAM_DROP_READABLE_0_READ_VIA_STREAM,
                ));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3StdinDropResultFuture => {
                // Dropped only after the result has been read: the future
                // resolves once the host sees end-of-input, so reading it has to
                // sit between dropping the readable end and dropping this.
                self.emit_p3_stream_handle(&args[0], Some(1));
                self.instruction(Instruction::Call(
                    FUNC_P3_CLI_STDIN_FUTURE_DROP_READABLE_1_READ_VIA_STREAM,
                ));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3StreamReadFinish => {
                // Copy count bytes from the op's pinned buffer, then free it.
                self.emit_expr(&args[0], ExprContext::Value); // op (i64)
                let ptr = self.add_local(ValType::I32);
                self.instruction(Instruction::I32WrapI64);
                self.instruction(Instruction::LocalSet(ptr));
                self.emit_expr(&args[1], ExprContext::Value); // count
                let len = self.add_local(ValType::I32);
                self.instruction(Instruction::LocalSet(len));
                self.wasi_create_u8_array_from_bytes(ptr, len);
                self.instruction(Instruction::LocalGet(ptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3StreamDiscard => {
                self.emit_expr(&args[0], ExprContext::Value); // op (i64)
                self.instruction(Instruction::I32WrapI64);
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            // ── WASI p3 filesystem (wave 1) ──
            IntrinsicKind::P3FsOpenAtStart => {
                // open-at has 6 flat params (> MAX_FLAT_ASYNC_PARAMS), so the
                // async-lowered core sig is (params-ptr, retptr) -> status with
                // params stored indirect: self @0, path-flags @4, path.ptr @8,
                // path.len @12, open-flags @16, descriptor-flags @17.
                //
                // Note @17, NOT @20. A `flags` type of 8 bits or fewer is ONE
                // BYTE in memory, aligned to 1 — `path-flags` (1 bit),
                // `open-flags` (4) and `descriptor-flags` (6) all are. Everywhere
                // else in this file a flags param is followed by a 4-aligned
                // field, so the padding hides the difference; open-at is the only
                // call with two of them adjacent, and there it does not. Writing
                // descriptor-flags at 20 left the host reading the zero byte at
                // 17, so every file opened here was opened WITHOUT write access —
                // invisible whenever `create` or `truncate` was also set, because
                // those give the host its own reason to open for writing. The
                // tuple is 18 bytes, aligned to 4.
                //
                // The host lifts arguments during the call, so the param block
                // and string bytes may live in scratch; only the retptr is pinned.
                // result<descriptor, error-code>: err carries option<string> -> 24 bytes.
                let retptr = self.emit_p3_pinned_retptr(24);
                let pblock = self.wasi_bump_alloc(24);
                let tmp = self.add_local(ValType::I32);
                self.emit_p3_store_field(pblock, 0, tmp, &args[0]);
                self.emit_expr(&args[1], ExprContext::Value);
                self.wasi_fs_path_flags_to_bitmask(&args[1].ty);
                self.instruction(Instruction::LocalSet(tmp));
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(tmp));
                self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }));
                self.emit_p3_store_string(pblock, 8, &args[2]);
                self.emit_expr(&args[3], ExprContext::Value);
                self.wasi_fs_open_flags_to_bitmask(&args[3].ty);
                self.instruction(Instruction::LocalSet(tmp));
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(tmp));
                self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 16,
                    align: 2,
                    memory_index: 0,
                }));
                self.emit_expr(&args[4], ExprContext::Value);
                self.wasi_fs_file_flags_to_bitmask(&args[4].ty);
                // One byte, at 17. The i32 store of open-flags just above cleared
                // 17..19, and this must therefore come after it.
                self.instruction(Instruction::LocalSet(tmp));
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(tmp));
                self.instruction(Instruction::I32Store8(wasm_encoder::MemArg {
                    offset: 17,
                    align: 0,
                    memory_index: 0,
                }));
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_OPEN_AT,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsOpenAtFinish => {
                // Lift Result<FileDescriptor, FileSystemError> from the pinned
                // block, then free it.
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                self.wasi_construct_result_i32_fs_error(retptr, &expr.ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsStatStart => {
                // full result<descriptor-stat, error-code> block (p3 layout)
                let retptr = self.emit_p3_pinned_retptr(112);
                self.emit_expr(&args[0], ExprContext::Value); // self
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_STAT,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsStatFinish => {
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                self.wasi_fs_construct_result_file_stat(retptr, &expr.ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsCreateDirectoryAtStart
            | IntrinsicKind::P3FsUnlinkFileAtStart
            | IntrinsicKind::P3FsRemoveDirectoryAtStart => {
                // (self, path_ptr, path_len, retptr) -> status
                let func = match intrinsic {
                    IntrinsicKind::P3FsCreateDirectoryAtStart => {
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_CREATE_DIRECTORY_AT
                    }
                    IntrinsicKind::P3FsUnlinkFileAtStart => {
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_UNLINK_FILE_AT
                    }
                    _ => FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_REMOVE_DIRECTORY_AT,
                };
                // result<_, error-code>: err carries option<string> -> 24 bytes.
                let retptr = self.emit_p3_pinned_retptr(24);
                self.emit_expr(&args[0], ExprContext::Value); // self
                self.emit_expr(&args[1], ExprContext::Value);
                let str_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(
                        super::super::STRING_STRUCT_TYPE_INDEX,
                    ),
                }));
                self.instruction(Instruction::LocalSet(str_local));
                let (str_ptr, str_len) = self.wasi_marshal_string_to_memory(str_local);
                self.instruction(Instruction::LocalGet(str_ptr));
                self.instruction(Instruction::LocalGet(str_len));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(func));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsUnitFinish => {
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                self.wasi_construct_result_unit_fs_error(retptr, &expr.ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsReadViaStream => {
                // read-via-stream(self, offset, retptr) -> (); retptr: stream @0, future @4
                let retptr = self.wasi_bump_alloc(8);
                self.emit_expr(&args[0], ExprContext::Value); // self
                self.emit_expr(&args[1], ExprContext::Value); // offset i64
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_METHOD_DESCRIPTOR_READ_VIA_STREAM,
                ));
                self.wasi_i32_load(retptr, 0);
                self.wasi_i32_load(retptr, 4);
                let mn = expr.ty.mangled_name();
                let type_idx = self.codegen.type_indices[&mn];
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsWriteViaStream | IntrinsicKind::P3FsAppendViaStream => {
                // Create the stream pair, hand the readable end to the host,
                // return { writable, result-future }.
                let is_write = matches!(intrinsic, IntrinsicKind::P3FsWriteViaStream);
                let pair = self.add_local(ValType::I64);
                self.instruction(Instruction::Call(if is_write {
                    FUNC_P3_FILESYSTEM_TYPES_STREAM_NEW_0_METHOD_DESCRIPTOR_WRITE_VIA_STREAM
                } else {
                    FUNC_P3_FILESYSTEM_TYPES_STREAM_NEW_0_METHOD_DESCRIPTOR_APPEND_VIA_STREAM
                }));
                self.instruction(Instruction::LocalSet(pair));
                self.emit_expr(&args[0], ExprContext::Value); // self
                // readable end (low 32)
                self.instruction(Instruction::LocalGet(pair));
                self.instruction(Instruction::I32WrapI64);
                if is_write {
                    self.emit_expr(&args[1], ExprContext::Value); // offset i64
                    self.instruction(Instruction::Call(
                        FUNC_P3_FILESYSTEM_TYPES_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
                    ));
                } else {
                    self.instruction(Instruction::Call(
                        FUNC_P3_FILESYSTEM_TYPES_METHOD_DESCRIPTOR_APPEND_VIA_STREAM,
                    ));
                }
                // stack: future handle; build { writable (high 32), future }
                let future = self.add_local(ValType::I32);
                self.instruction(Instruction::LocalSet(future));
                self.instruction(Instruction::LocalGet(pair));
                self.instruction(Instruction::I64Const(32));
                self.instruction(Instruction::I64ShrU);
                self.instruction(Instruction::I32WrapI64);
                self.instruction(Instruction::LocalGet(future));
                let mn = expr.ty.mangled_name();
                let type_idx = self.codegen.type_indices[&mn];
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsClose => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(FUNC_P3_FILESYSTEM_TYPES_DROP_DESCRIPTOR));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3FsStreamReadStart => {
                self.emit_p3_stream_read_start(
                    &args[0],
                    None,
                    &args[1],
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_STREAM_READ_0_METHOD_DESCRIPTOR_READ_VIA_STREAM,
                );
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsStreamWriteStart | IntrinsicKind::P3FsStreamAppendWriteStart => {
                let func = if matches!(intrinsic, IntrinsicKind::P3FsStreamWriteStart) {
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_STREAM_WRITE_0_METHOD_DESCRIPTOR_WRITE_VIA_STREAM
                } else {
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_STREAM_WRITE_0_METHOD_DESCRIPTOR_APPEND_VIA_STREAM
                };
                self.emit_p3_stream_write_start(&args[0], None, &args[1], func);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsDropReadable => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_STREAM_DROP_READABLE_0_METHOD_DESCRIPTOR_READ_VIA_STREAM,
                ));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3FsDropWritable => {
                // Serves append streams too: `stream.drop-writable` is keyed by
                // the `stream<u8>` type shared by write-via-stream and
                // append-via-stream, so one built-in drops either writable end.
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_STREAM_DROP_WRITABLE_0_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
                ));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3FsDropReadResult
            | IntrinsicKind::P3FsDropWriteResult
            | IntrinsicKind::P3FsDropAppendResult => {
                let func = match intrinsic {
                    IntrinsicKind::P3FsDropReadResult => {
                        FUNC_P3_FILESYSTEM_TYPES_FUTURE_DROP_READABLE_1_METHOD_DESCRIPTOR_READ_VIA_STREAM
                    }
                    IntrinsicKind::P3FsDropWriteResult => {
                        FUNC_P3_FILESYSTEM_TYPES_FUTURE_DROP_READABLE_1_METHOD_DESCRIPTOR_WRITE_VIA_STREAM
                    }
                    _ => FUNC_P3_FILESYSTEM_TYPES_FUTURE_DROP_READABLE_1_METHOD_DESCRIPTOR_APPEND_VIA_STREAM,
                };
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(func));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            // ── WASI p3 filesystem (wave 2) ──
            IntrinsicKind::P3FsStatAtStart | IntrinsicKind::P3FsMetadataHashAtStart => {
                // (self, path-flags, path_ptr, path_len, retptr) -> status
                let (func, retsize) = if matches!(intrinsic, IntrinsicKind::P3FsStatAtStart) {
                    (
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_STAT_AT,
                        112,
                    )
                } else {
                    (
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_METADATA_HASH_AT,
                        24,
                    )
                };
                let retptr = self.emit_p3_pinned_retptr(retsize);
                self.emit_expr(&args[0], ExprContext::Value); // self
                self.emit_expr(&args[1], ExprContext::Value);
                self.wasi_fs_path_flags_to_bitmask(&args[1].ty);
                self.emit_expr(&args[2], ExprContext::Value);
                let str_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(
                        super::super::STRING_STRUCT_TYPE_INDEX,
                    ),
                }));
                self.instruction(Instruction::LocalSet(str_local));
                let (str_ptr, str_len) = self.wasi_marshal_string_to_memory(str_local);
                self.instruction(Instruction::LocalGet(str_ptr));
                self.instruction(Instruction::LocalGet(str_len));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(func));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsSetSizeStart => {
                // (self, size, retptr) -> status
                let retptr = self.emit_p3_pinned_retptr(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value); // i64
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SET_SIZE,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsSyncStart
            | IntrinsicKind::P3FsSyncDataStart
            | IntrinsicKind::P3FsGetFlagsStart
            | IntrinsicKind::P3FsGetTypeStart
            | IntrinsicKind::P3FsMetadataHashStart => {
                // (self, retptr) -> status
                let func = match intrinsic {
                    IntrinsicKind::P3FsSyncStart => {
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SYNC
                    }
                    IntrinsicKind::P3FsSyncDataStart => {
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SYNC_DATA
                    }
                    IntrinsicKind::P3FsGetFlagsStart => {
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_GET_FLAGS
                    }
                    IntrinsicKind::P3FsGetTypeStart => {
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_GET_TYPE
                    }
                    _ => FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_METADATA_HASH,
                };
                let retptr = self.emit_p3_pinned_retptr(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(func));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsAdviseStart => {
                // (self, offset, length, advice-disc, retptr) -> status
                let retptr = self.emit_p3_pinned_retptr(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value); // i64
                self.emit_expr(&args[2], ExprContext::Value); // i64
                self.emit_expr(&args[3], ExprContext::Value); // FileAdvice enum
                self.wasi_fs_enum_to_disc(&args[3].ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_ADVISE,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsIsSameObjectStart => {
                // (self, other, retptr) -> status; plain bool result @0
                let retptr = self.emit_p3_pinned_retptr(8);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_IS_SAME_OBJECT,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsIsSameObjectFinish => {
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                self.wasi_i32_load8_u(retptr, 0);
                self.instruction(Instruction::I32Const(0));
                self.instruction(Instruction::I32Ne);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsGetFlagsFinish => {
                // ok: `descriptor-flags` U8 @4 — six flags, so one byte.
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                let ty = &expr.ty;
                self.emit_p3_fs_result_finish(retptr, ty, 4, |e| {
                    e.wasi_i32_load8_u(retptr, 4);
                    e.wasi_fs_bitmask_to_file_flags(ty);
                });
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsGetTypeFinish => {
                // ok is a `descriptor-type`, a variant whose `other` case carries
                // an `option<string>`: case disc @4, option disc @8, ptr @12 /
                // len @16 — the whole 16-byte payload fits the 24-byte retptr.
                // The Dovetail enum keeps only the discriminant, so free the
                // message here or never.
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                let ty = &expr.ty;
                self.emit_p3_fs_result_finish(retptr, ty, 4, |e| {
                    e.wasi_free_error_message(retptr, 4, DESCRIPTOR_TYPE_OTHER_DISC);
                    e.wasi_i32_load8_u(retptr, 4);
                    e.wasi_fs_construct_descriptor_type(ty);
                });
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsMetadataHashFinish => {
                // ok: { lower u64 @8, upper u64 @16 } — u64s, so the payload
                // (and with it the error discriminant) aligns to 8, not 4.
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                let ty = &expr.ty;
                let ok_type = match ty {
                    Type::GenericEnum { type_args, .. } => &type_args[0].1,
                    _ => unreachable!(),
                };
                let hash_idx = self.codegen.type_indices[&ok_type.mangled_name()];
                self.emit_p3_fs_result_finish(retptr, ty, 8, |e| {
                    e.wasi_i64_load(retptr, 8);
                    e.wasi_i64_load(retptr, 16);
                    e.instruction(Instruction::StructNew(hash_idx));
                });
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsSymlinkAtStart => {
                // indirect params: self@0, old ptr@4 len@8, new ptr@12 len@16
                let retptr = self.emit_p3_pinned_retptr(24);
                let pblock = self.wasi_bump_alloc(20);
                let tmp = self.add_local(ValType::I32);
                self.emit_p3_store_field(pblock, 0, tmp, &args[0]);
                self.emit_p3_store_string(pblock, 4, &args[1]);
                self.emit_p3_store_string(pblock, 12, &args[2]);
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SYMLINK_AT,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsRenameAtStart => {
                // indirect params: self@0, old ptr@4 len@8, new-desc@12, new ptr@16 len@20
                let retptr = self.emit_p3_pinned_retptr(24);
                let pblock = self.wasi_bump_alloc(24);
                let tmp = self.add_local(ValType::I32);
                self.emit_p3_store_field(pblock, 0, tmp, &args[0]);
                self.emit_p3_store_string(pblock, 4, &args[1]);
                self.emit_p3_store_field(pblock, 12, tmp, &args[2]);
                self.emit_p3_store_string(pblock, 16, &args[3]);
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_RENAME_AT,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsLinkAtStart => {
                // indirect params: self@0, old-path-flags@4, old ptr@8 len@12,
                // new-desc@16, new ptr@20 len@24
                let retptr = self.emit_p3_pinned_retptr(24);
                let pblock = self.wasi_bump_alloc(28);
                let tmp = self.add_local(ValType::I32);
                self.emit_p3_store_field(pblock, 0, tmp, &args[0]);
                self.emit_expr(&args[1], ExprContext::Value);
                self.wasi_fs_path_flags_to_bitmask(&args[1].ty);
                self.instruction(Instruction::LocalSet(tmp));
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(tmp));
                self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }));
                self.emit_p3_store_string(pblock, 8, &args[2]);
                self.emit_p3_store_field(pblock, 16, tmp, &args[3]);
                self.emit_p3_store_string(pblock, 20, &args[4]);
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_LINK_AT,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            // ── WASI p3 filesystem (wave 3) ──
            IntrinsicKind::P3FsReadlinkAtStart => {
                // (self, path_ptr, path_len, retptr) -> status
                let retptr = self.emit_p3_pinned_retptr(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                let str_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(
                        super::super::STRING_STRUCT_TYPE_INDEX,
                    ),
                }));
                self.instruction(Instruction::LocalSet(str_local));
                let (str_ptr, str_len) = self.wasi_marshal_string_to_memory(str_local);
                self.instruction(Instruction::LocalGet(str_ptr));
                self.instruction(Instruction::LocalGet(str_len));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_READLINK_AT,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsReadlinkAtFinish => {
                // ok: the link target as (str ptr @4, len @8), realloc'd by the
                // host into the pinned heap and freed once copied.
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                let ty = &expr.ty;
                let sptr = self.add_local(ValType::I32);
                let slen = self.add_local(ValType::I32);
                self.emit_p3_fs_result_finish(retptr, ty, 4, |e| {
                    e.wasi_i32_load(retptr, 4);
                    e.instruction(Instruction::LocalSet(sptr));
                    e.wasi_i32_load(retptr, 8);
                    e.instruction(Instruction::LocalSet(slen));
                    e.wasi_create_string_from_memory(sptr, slen);
                    // Back through the guarded helper, not `pinned_free`: an
                    // empty string lowers as a null or dangling pointer that
                    // never went through `cabi_realloc`.
                    e.wasi_free_lifted_block(sptr);
                });
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsSetTimesStart => {
                // indirect params: self @0, ts1 @8..32, ts2 @32..56
                let retptr = self.emit_p3_pinned_retptr(24);
                let pblock = self.wasi_bump_alloc(56);
                let tmp = self.add_local(ValType::I32);
                self.emit_p3_store_field(pblock, 0, tmp, &args[0]);
                self.emit_p3_store_new_timestamp(pblock, 8, &args[1]);
                self.emit_p3_store_new_timestamp(pblock, 32, &args[2]);
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SET_TIMES,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsSetTimesAtStart => {
                // indirect params: self @0, path-flags @4, ptr @8, len @12,
                // ts1 @16..40, ts2 @40..64
                let retptr = self.emit_p3_pinned_retptr(24);
                let pblock = self.wasi_bump_alloc(64);
                let tmp = self.add_local(ValType::I32);
                self.emit_p3_store_field(pblock, 0, tmp, &args[0]);
                self.emit_expr(&args[1], ExprContext::Value);
                self.wasi_fs_path_flags_to_bitmask(&args[1].ty);
                self.instruction(Instruction::LocalSet(tmp));
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(tmp));
                self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }));
                self.emit_p3_store_string(pblock, 8, &args[2]);
                self.emit_p3_store_new_timestamp(pblock, 16, &args[3]);
                self.emit_p3_store_new_timestamp(pblock, 40, &args[4]);
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_METHOD_DESCRIPTOR_SET_TIMES_AT,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsReadDirectory => {
                // read-directory(self, retptr) -> (); retptr: stream @0, future @4
                let retptr = self.wasi_bump_alloc(8);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_METHOD_DESCRIPTOR_READ_DIRECTORY,
                ));
                self.wasi_i32_load(retptr, 0);
                self.wasi_i32_load(retptr, 4);
                let mn = expr.ty.mangled_name();
                let type_idx = self.codegen.type_indices[&mn];
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsEntryReadStart => {
                // one 24-byte directory-entry per read
                let buf = self.add_local(ValType::I32);
                self.instruction(Instruction::I32Const(24));
                self.instruction(Instruction::Call(self.codegen.func_pinned_alloc()));
                self.instruction(Instruction::LocalSet(buf));
                self.emit_expr(&args[0], ExprContext::Value); // stream
                self.instruction(Instruction::LocalGet(buf));
                self.instruction(Instruction::I32Const(1));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_ASYNC_STREAM_READ_0_METHOD_DESCRIPTOR_READ_DIRECTORY,
                ));
                self.instruction(Instruction::I64ExtendI32S);
                self.instruction(Instruction::I64Const(32));
                self.instruction(Instruction::I64Shl);
                self.instruction(Instruction::LocalGet(buf));
                self.instruction(Instruction::I64ExtendI32U);
                self.instruction(Instruction::I64Or);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsEntryReadFinish => {
                // `directory-entry { type: descriptor-type, name: string }`, laid
                // out in the 24-byte buffer as:
                //
                //   @0  descriptor-type discriminant (u8, 8 cases)
                //   @4  payload of the one case that has one — `other`'s
                //       `option<string>`: option discriminant, then
                //   @8  its string ptr / @12 len
                //   @16 name ptr / @20 len
                //
                // (descriptor-type is size 16 / align 4, so the record is 24.)
                // Both strings were realloc'd by the host into the pinned heap
                // and are the guest's to free once copied.
                let buf = self.emit_p3_unpack_retptr(&args[0]);
                let entry_mn = expr.ty.mangled_name();
                let entry_idx = self.codegen.type_indices[&entry_mn];
                let dt_type = self.find_record_field_type(&entry_mn, 0);
                self.wasi_i32_load8_u(buf, 0);
                self.wasi_fs_construct_descriptor_type_direct(&dt_type);
                let sptr = self.add_local(ValType::I32);
                let slen = self.add_local(ValType::I32);
                self.wasi_i32_load(buf, 16);
                self.instruction(Instruction::LocalSet(sptr));
                self.wasi_i32_load(buf, 20);
                self.instruction(Instruction::LocalSet(slen));
                self.wasi_create_string_from_memory(sptr, slen);
                // Host-lifted string: guarded, like the `other(some(_))` string
                // freed a few lines below.
                self.wasi_free_lifted_block(sptr);
                self.instruction(Instruction::StructNew(entry_idx));
                // The Dovetail surface keeps only the discriminant of
                // `descriptor-type`, so nothing downstream will ever see — let
                // alone free — the string inside `other(some(_))`. Free it here,
                // while the block is still readable.
                self.wasi_free_error_message(buf, 0, DESCRIPTOR_TYPE_OTHER_DISC);
                self.instruction(Instruction::LocalGet(buf));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3FsDropEntryReadable => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_STREAM_DROP_READABLE_0_METHOD_DESCRIPTOR_READ_DIRECTORY,
                ));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3FsDropEntryResult => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_TYPES_FUTURE_DROP_READABLE_1_METHOD_DESCRIPTOR_READ_DIRECTORY,
                ));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            // ── WASI p3 TCP ──
            IntrinsicKind::P3TcpCreate => {
                // [static]tcp-socket.create(family-disc, retptr) -> ()
                let retptr = self.wasi_bump_alloc(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.wasi_fs_enum_to_disc(&args[0].ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_STATIC_TCP_SOCKET_CREATE,
                ));
                self.wasi_construct_result_i32_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpBind => {
                // bind(self, addr-flat x12, retptr) -> (); result<_, error-code>
                let retptr = self.wasi_bump_alloc(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                let sa_mn = Self::extract_socket_address_mn(&args[1].ty);
                self.wasi_marshal_socket_address_flat(&sa_mn);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_BIND,
                ));
                self.wasi_construct_result_unit_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpConnectStart => {
                // async indirect: params block self @0, ip-socket-address @4..36
                let retptr = self.emit_p3_pinned_retptr(24);
                let pblock = self.wasi_bump_alloc(36);
                let tmp = self.add_local(ValType::I32);
                self.emit_p3_store_field(pblock, 0, tmp, &args[0]);
                self.emit_p3_store_socket_address(pblock, 4, &args[1]);
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_ASYNC_METHOD_TCP_SOCKET_CONNECT,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpConnectFinish => {
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                self.wasi_construct_result_unit_network_error(retptr, &expr.ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpListen => {
                // listen(self, retptr) -> (); result<stream, error-code>
                let retptr = self.wasi_bump_alloc(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_LISTEN,
                ));
                self.wasi_construct_result_i32_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpAcceptStart => {
                // read ONE socket handle (4-byte element) from the accept stream
                let buf = self.add_local(ValType::I32);
                self.instruction(Instruction::I32Const(4));
                self.instruction(Instruction::Call(self.codegen.func_pinned_alloc()));
                self.instruction(Instruction::LocalSet(buf));
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalGet(buf));
                self.instruction(Instruction::I32Const(1));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_ASYNC_STREAM_READ_0_METHOD_TCP_SOCKET_LISTEN,
                ));
                self.instruction(Instruction::I64ExtendI32S);
                self.instruction(Instruction::I64Const(32));
                self.instruction(Instruction::I64Shl);
                self.instruction(Instruction::LocalGet(buf));
                self.instruction(Instruction::I64ExtendI32U);
                self.instruction(Instruction::I64Or);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpAcceptFinish => {
                let buf = self.emit_p3_unpack_retptr(&args[0]);
                self.wasi_i32_load(buf, 0);
                self.instruction(Instruction::LocalGet(buf));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpSend | IntrinsicKind::P3TcpReceive => {
                if matches!(intrinsic, IntrinsicKind::P3TcpSend) {
                    // create pair; send(self, readable) -> future; keep writable
                    let pair = self.add_local(ValType::I64);
                    self.instruction(Instruction::Call(
                        FUNC_P3_SOCKETS_TYPES_STREAM_NEW_0_METHOD_TCP_SOCKET_SEND,
                    ));
                    self.instruction(Instruction::LocalSet(pair));
                    self.emit_expr(&args[0], ExprContext::Value);
                    self.instruction(Instruction::LocalGet(pair));
                    self.instruction(Instruction::I32WrapI64);
                    self.instruction(Instruction::Call(
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SEND,
                    ));
                    let future = self.add_local(ValType::I32);
                    self.instruction(Instruction::LocalSet(future));
                    self.instruction(Instruction::LocalGet(pair));
                    self.instruction(Instruction::I64Const(32));
                    self.instruction(Instruction::I64ShrU);
                    self.instruction(Instruction::I32WrapI64);
                    self.instruction(Instruction::LocalGet(future));
                } else {
                    // receive(self, retptr) -> (); retptr: stream @0, future @4
                    let retptr = self.wasi_bump_alloc(8);
                    self.emit_expr(&args[0], ExprContext::Value);
                    self.instruction(Instruction::LocalGet(retptr));
                    self.instruction(Instruction::Call(
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_RECEIVE,
                    ));
                    self.wasi_i32_load(retptr, 0);
                    self.wasi_i32_load(retptr, 4);
                }
                let mn = expr.ty.mangled_name();
                let type_idx = self.codegen.type_indices[&mn];
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpSendWriteStart => {
                self.emit_p3_stream_write_start(
                    &args[0],
                    None,
                    &args[1],
                    FUNC_P3_SOCKETS_TYPES_ASYNC_STREAM_WRITE_0_METHOD_TCP_SOCKET_SEND,
                );
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpReceiveReadStart => {
                self.emit_p3_stream_read_start(
                    &args[0],
                    None,
                    &args[1],
                    FUNC_P3_SOCKETS_TYPES_ASYNC_STREAM_READ_0_METHOD_TCP_SOCKET_RECEIVE,
                );
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpDropSendWritable
            | IntrinsicKind::P3TcpDropReceiveReadable
            | IntrinsicKind::P3TcpDropSendResult
            | IntrinsicKind::P3TcpDropReceiveResult
            | IntrinsicKind::P3TcpDropAcceptStream
            | IntrinsicKind::P3TcpClose => {
                let func = match intrinsic {
                    IntrinsicKind::P3TcpDropSendWritable => {
                        FUNC_P3_SOCKETS_TYPES_STREAM_DROP_WRITABLE_0_METHOD_TCP_SOCKET_SEND
                    }
                    IntrinsicKind::P3TcpDropReceiveReadable => {
                        FUNC_P3_SOCKETS_TYPES_STREAM_DROP_READABLE_0_METHOD_TCP_SOCKET_RECEIVE
                    }
                    IntrinsicKind::P3TcpDropSendResult => {
                        FUNC_P3_SOCKETS_TYPES_FUTURE_DROP_READABLE_1_METHOD_TCP_SOCKET_SEND
                    }
                    IntrinsicKind::P3TcpDropReceiveResult => {
                        FUNC_P3_SOCKETS_TYPES_FUTURE_DROP_READABLE_1_METHOD_TCP_SOCKET_RECEIVE
                    }
                    IntrinsicKind::P3TcpDropAcceptStream => {
                        FUNC_P3_SOCKETS_TYPES_STREAM_DROP_READABLE_0_METHOD_TCP_SOCKET_LISTEN
                    }
                    _ => FUNC_P3_SOCKETS_TYPES_DROP_TCP_SOCKET,
                };
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(func));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3TcpCancelAcceptRead => {
                // stream.cancel-read(accept-stream) -> status. Unlike the byte
                // streams this one RETURNS the status: a cancel can race a
                // completion that already transferred an owned socket handle
                // into the pinned buffer, and only the caller can tell (from
                // `count`) whether it must close that socket instead of
                // discarding the buffer.
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_STREAM_CANCEL_READ_0_METHOD_TCP_SOCKET_LISTEN,
                ));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpCancelReceiveRead
            | IntrinsicKind::P3FsCancelRead
            | IntrinsicKind::P3StdinCancelRead => {
                // stream.cancel-read(handle) -> status. Cancels an in-flight read
                // so the stream can be dropped, consuming any raced completion —
                // and a raced completion may already have copied bytes into the
                // op's pinned buffer. The packed status (`code | count << 4`) is
                // the only way the caller can learn they are there, so unlike the
                // cancel-write paths it is returned rather than dropped.
                let func = match intrinsic {
                    IntrinsicKind::P3TcpCancelReceiveRead => {
                        FUNC_P3_SOCKETS_TYPES_STREAM_CANCEL_READ_0_METHOD_TCP_SOCKET_RECEIVE
                    }
                    IntrinsicKind::P3FsCancelRead => {
                        FUNC_P3_FILESYSTEM_TYPES_STREAM_CANCEL_READ_0_METHOD_DESCRIPTOR_READ_VIA_STREAM
                    }
                    _ => FUNC_P3_CLI_STDIN_STREAM_CANCEL_READ_0_READ_VIA_STREAM,
                };
                // stdin passes its whole stream value as `self`; the fs and
                // socket streams pass the handle itself.
                let field = match intrinsic {
                    IntrinsicKind::P3StdinCancelRead => Some(0),
                    _ => None,
                };
                self.emit_p3_stream_handle(&args[0], field);
                self.instruction(Instruction::Call(func));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpCancelSendWrite
            | IntrinsicKind::P3FsCancelWrite
            | IntrinsicKind::P3FsCancelEntryRead => {
                // stream.cancel-read/write(handle) -> status. Cancels an in-flight
                // write (or directory-entry read) so the stream can be dropped,
                // consuming any raced completion. The packed status carries the
                // element count of a completion the cancel lost the race with, so
                // it is returned to the caller (see below), not dropped here.
                let func = match intrinsic {
                    IntrinsicKind::P3TcpCancelSendWrite => {
                        FUNC_P3_SOCKETS_TYPES_STREAM_CANCEL_WRITE_0_METHOD_TCP_SOCKET_SEND
                    }
                    IntrinsicKind::P3FsCancelWrite => {
                        // Also cancels append streams: `stream.cancel-write` is
                        // keyed by the `stream<u8>` type, and write-via-stream and
                        // append-via-stream produce the same one, so this built-in
                        // serves both writable ends.
                        FUNC_P3_FILESYSTEM_TYPES_STREAM_CANCEL_WRITE_0_METHOD_DESCRIPTOR_WRITE_VIA_STREAM
                    }
                    _ => FUNC_P3_FILESYSTEM_TYPES_STREAM_CANCEL_READ_0_METHOD_DESCRIPTOR_READ_DIRECTORY,
                };
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(func));
                // The packed status is returned, not dropped: a cancel can lose
                // the race with a completion that already copied elements into the
                // buffer, and the count is the only report that they are there.
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpReadReceiveResult
            | IntrinsicKind::P3TcpReadSendResult
            | IntrinsicKind::P3FsReadReadResult
            | IntrinsicKind::P3FsReadWriteResult
            | IntrinsicKind::P3FsReadEntryResult
            | IntrinsicKind::P3FsReadAppendResult => {
                // future.read(future, retptr) -> status, reading a resolved
                // `result<_, error-code>`. Both the sockets and the filesystem
                // `error-code` are variants whose last case is
                // `other(option<string>)`, so error-code is size 16 / align 4
                // and the result is disc @0 + payload @4..20 = 20 bytes. Round
                // to 24 for the allocator's 8-byte alignment, as the socket
                // option getters do. Pre-init the block to Ok(0) so a
                // not-yet-ready read reports a clean close; a resolved error
                // overwrites it. If the read BLOCKS (future not resolved yet) we
                // cancel the pending read so the future can then be dropped
                // without tripping the "busy stream" trap.
                let is_tcp = matches!(
                    intrinsic,
                    IntrinsicKind::P3TcpReadReceiveResult | IntrinsicKind::P3TcpReadSendResult
                );
                let read_func = match intrinsic {
                    IntrinsicKind::P3TcpReadReceiveResult => {
                        FUNC_P3_SOCKETS_TYPES_ASYNC_FUTURE_READ_1_METHOD_TCP_SOCKET_RECEIVE
                    }
                    IntrinsicKind::P3TcpReadSendResult => {
                        FUNC_P3_SOCKETS_TYPES_ASYNC_FUTURE_READ_1_METHOD_TCP_SOCKET_SEND
                    }
                    IntrinsicKind::P3FsReadReadResult => {
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_FUTURE_READ_1_METHOD_DESCRIPTOR_READ_VIA_STREAM
                    }
                    IntrinsicKind::P3FsReadEntryResult => {
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_FUTURE_READ_1_METHOD_DESCRIPTOR_READ_DIRECTORY
                    }
                    IntrinsicKind::P3FsReadAppendResult => {
                        FUNC_P3_FILESYSTEM_TYPES_ASYNC_FUTURE_READ_1_METHOD_DESCRIPTOR_APPEND_VIA_STREAM
                    }
                    _ => FUNC_P3_FILESYSTEM_TYPES_ASYNC_FUTURE_READ_1_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
                };
                let cancel_func = match intrinsic {
                    IntrinsicKind::P3TcpReadReceiveResult => {
                        FUNC_P3_SOCKETS_TYPES_FUTURE_CANCEL_READ_1_METHOD_TCP_SOCKET_RECEIVE
                    }
                    IntrinsicKind::P3TcpReadSendResult => {
                        FUNC_P3_SOCKETS_TYPES_FUTURE_CANCEL_READ_1_METHOD_TCP_SOCKET_SEND
                    }
                    IntrinsicKind::P3FsReadReadResult => {
                        FUNC_P3_FILESYSTEM_TYPES_FUTURE_CANCEL_READ_1_METHOD_DESCRIPTOR_READ_VIA_STREAM
                    }
                    IntrinsicKind::P3FsReadEntryResult => {
                        FUNC_P3_FILESYSTEM_TYPES_FUTURE_CANCEL_READ_1_METHOD_DESCRIPTOR_READ_DIRECTORY
                    }
                    IntrinsicKind::P3FsReadAppendResult => {
                        FUNC_P3_FILESYSTEM_TYPES_FUTURE_CANCEL_READ_1_METHOD_DESCRIPTOR_APPEND_VIA_STREAM
                    }
                    _ => FUNC_P3_FILESYSTEM_TYPES_FUTURE_CANCEL_READ_1_METHOD_DESCRIPTOR_WRITE_VIA_STREAM,
                };
                let future_local = self.add_local(ValType::I32);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalSet(future_local));
                let retptr = self.wasi_bump_alloc(24);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::I32Const(0));
                self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 0,
                    align: 2,
                    memory_index: 0,
                }));
                self.instruction(Instruction::LocalGet(future_local));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(read_func));
                // status < 0 → BLOCKED: cancel the pending read so drop is safe.
                self.instruction(Instruction::I32Const(0));
                self.instruction(Instruction::I32LtS);
                self.instruction(Instruction::If(BlockType::Empty));
                self.instruction(Instruction::LocalGet(future_local));
                self.instruction(Instruction::Call(cancel_func));
                self.instruction(Instruction::Drop);
                self.instruction(Instruction::End);
                if is_tcp {
                    self.wasi_construct_result_unit_network_error(retptr, &expr.ty);
                } else {
                    self.wasi_construct_result_unit_fs_error(retptr, &expr.ty);
                }
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3AsyncCallDiscard => {
                // Free the call's pinned result block. `retptr` is the low 32
                // bits of the packed AsyncCall. Guard against 0 (imports with no
                // result buffer, e.g. the clock timers): `pinned_free` writes
                // through `ptr - 8`, so a null pointer would corrupt memory.
                let retptr = self.add_local(ValType::I32);
                self.emit_expr(&args[0], ExprContext::Value); // AsyncCall (i64)
                self.instruction(Instruction::I32WrapI64);
                self.instruction(Instruction::LocalSet(retptr));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::If(BlockType::Empty));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.instruction(Instruction::End);
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3DnsResolveStart => {
                // async direct: resolve-addresses(name-ptr, name-len, retptr) -> status.
                // The result block is pinned (it must survive the suspension);
                // the name bytes are only read while the host lifts the call's
                // params, so scratch is fine (same as the fs *Start paths).
                let retptr = self.emit_p3_pinned_retptr(24);
                self.emit_expr(&args[0], ExprContext::Value);
                let name_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(
                        super::super::STRING_STRUCT_TYPE_INDEX,
                    ),
                }));
                self.instruction(Instruction::LocalSet(name_local));
                let (name_ptr, name_len) = self.wasi_marshal_string_to_memory(name_local);
                self.instruction(Instruction::LocalGet(name_ptr));
                self.instruction(Instruction::LocalGet(name_len));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_IP_NAME_LOOKUP_ASYNC_RESOLVE_ADDRESSES,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3DnsResolveFinish => {
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                self.wasi_construct_result_ip_address_list_dns_error(retptr, &expr.ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3UdpCreate => {
                // [static]udp-socket.create(family-disc, retptr) -> ()
                let retptr = self.wasi_bump_alloc(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.wasi_fs_enum_to_disc(&args[0].ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_STATIC_UDP_SOCKET_CREATE,
                ));
                self.wasi_construct_result_i32_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3UdpBind | IntrinsicKind::P3UdpConnect => {
                // bind/connect(self, addr-flat x12, retptr) -> (); result<_, error-code>
                let func = if matches!(intrinsic, IntrinsicKind::P3UdpBind) {
                    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_BIND
                } else {
                    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_CONNECT
                };
                let retptr = self.wasi_bump_alloc(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                let sa_mn = Self::extract_socket_address_mn(&args[1].ty);
                self.wasi_marshal_socket_address_flat(&sa_mn);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(func));
                self.wasi_construct_result_unit_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3UdpDisconnect => {
                let retptr = self.wasi_bump_alloc(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_DISCONNECT,
                ));
                self.wasi_construct_result_unit_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3UdpSendStart => {
                // async indirect: params block self @0, data ptr @4 / len @8,
                // option<ip-socket-address> @12 (disc @12, address @16..48).
                let retptr = self.emit_p3_pinned_retptr(24);
                let pblock = self.wasi_bump_alloc(48);
                let tmp = self.add_local(ValType::I32);
                self.emit_p3_store_field(pblock, 0, tmp, &args[0]);

                // Copy the payload into a pinned buffer: the host reads it while
                // lifting the call's params, and this frame's scratch is rewound
                // out from under it, so it has to be pinned rather than bumped.
                //
                // This site owns that block and frees it below, before the Start
                // even returns — NOT in `sendFinish`, which owns the retptr and
                // nothing else. The host lifts the whole params block during the
                // call, so by the time the call returns its status the payload
                // has already been consumed and there is nothing left to wait
                // for. Freeing it in the Finish instead would strand it for a
                // send that is never finished, and doing it in BOTH places is a
                // double free that corrupts the pinned free list.
                let data_local = self.add_local(ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(
                        self.codegen.array_type_index(&Type::Uint8),
                    ),
                }));
                self.emit_expr(&args[1], ExprContext::Value);
                self.instruction(Instruction::LocalSet(data_local));
                let data_len = self.add_local(ValType::I32);
                self.instruction(Instruction::LocalGet(data_local));
                self.instruction(Instruction::ArrayLen);
                self.instruction(Instruction::LocalSet(data_len));
                let data_ptr = self.add_local(ValType::I32);
                self.instruction(Instruction::LocalGet(data_len));
                self.instruction(Instruction::Call(self.codegen.func_pinned_alloc()));
                self.instruction(Instruction::LocalSet(data_ptr));
                let u8_array_idx = self.codegen.array_type_index(&Type::Uint8);
                self.wasi_copy_u8_array_to_memory(data_local, data_ptr, data_len, u8_array_idx);
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(data_ptr));
                self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 4,
                    align: 2,
                    memory_index: 0,
                }));
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(data_len));
                self.instruction(Instruction::I32Store(wasm_encoder::MemArg {
                    offset: 8,
                    align: 2,
                    memory_index: 0,
                }));

                self.emit_p3_store_option_socket_address(pblock, 12, &args[2]);
                self.instruction(Instruction::LocalGet(pblock));
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_ASYNC_METHOD_UDP_SOCKET_SEND,
                ));
                // Free the payload copy here, not in the Finish: the host lifted
                // it during the call above, so its lifetime ends with the call
                // rather than with the send.
                self.instruction(Instruction::LocalGet(data_ptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3UdpSendFinish => {
                // One free, the retptr — like every other `*Finish`. The Start's
                // pinned payload copy is not this site's to release: the host
                // finished lifting it during the call, and the Start freed it
                // there.
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                self.wasi_construct_result_unit_network_error(retptr, &expr.ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3UdpReceiveStart => {
                // async direct: receive(self, retptr) -> status. Result block:
                // disc @0; ok = list ptr @4 / len @8 + address @12..44; err @4.
                let retptr = self.emit_p3_pinned_retptr(48);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_ASYNC_METHOD_UDP_SOCKET_RECEIVE,
                ));
                self.emit_p3_pack_async_call(retptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3UdpReceiveFinish => {
                let retptr = self.emit_p3_unpack_retptr(&args[0]);
                self.wasi_construct_result_datagram_network_error(retptr, &expr.ty);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(self.codegen.func_pinned_free()));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3UdpLocalAddress | IntrinsicKind::P3UdpRemoteAddress => {
                let func = if matches!(intrinsic, IntrinsicKind::P3UdpLocalAddress) {
                    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_LOCAL_ADDRESS
                } else {
                    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_REMOTE_ADDRESS
                };
                let retptr = self.wasi_bump_alloc(40);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(func));
                self.wasi_construct_result_socket_address_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3UdpClose => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(FUNC_P3_SOCKETS_TYPES_DROP_UDP_SOCKET));
                if ctx == ExprContext::Value {
                    self.instruction(Instruction::I32Const(0)); // Unit
                }
            }

            IntrinsicKind::P3TcpLocalAddress | IntrinsicKind::P3TcpRemoteAddress => {
                // get-*-address(self, retptr) -> (); result<ip-socket-address, error-code>
                // p3: payload @4 (addr variant align 4, 32 bytes) -> block 40
                let func = if matches!(intrinsic, IntrinsicKind::P3TcpLocalAddress) {
                    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_LOCAL_ADDRESS
                } else {
                    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_REMOTE_ADDRESS
                };
                let retptr = self.wasi_bump_alloc(40);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(func));
                self.wasi_construct_result_socket_address_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::P3TcpSetListenBacklogSize => {
                // (self, size i64, retptr) -> ()
                let retptr = self.wasi_bump_alloc(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_LISTEN_BACKLOG_SIZE,
                ));
                self.wasi_construct_result_unit_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            // --- TCP socket options -------------------------------------------
            //
            // `get-is-listening` is the odd one out: a plain `bool` return, no
            // result and no retptr.
            IntrinsicKind::P3TcpIsListening => {
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(
                    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_IS_LISTENING,
                ));
                self.drop_if_statement(ctx, &expr.ty);
            }

            // `get-address-family` is the same shape: a plain flat enum return
            // (0 = ipv4, 1 = ipv6), no result and no retptr. The disc is lifted
            // into the Dovetail `IpAddressFamily` enum.
            IntrinsicKind::P3TcpAddressFamily | IntrinsicKind::P3UdpAddressFamily => {
                let func = if matches!(intrinsic, IntrinsicKind::P3TcpAddressFamily) {
                    FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_ADDRESS_FAMILY
                } else {
                    FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_ADDRESS_FAMILY
                };
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::Call(func));
                self.wasi_construct_ip_address_family(&expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            // Getters: `(self, retptr) -> ()` writing `result<scalar, error-code>`.
            // The payload offset is the alignment of the widest case — 4 when the
            // value is bool/u8/u32 (error-code itself is align 4), 8 for u64 — and
            // both the ok value and the error discriminant sit there.
            IntrinsicKind::P3TcpKeepAliveEnabled
            | IntrinsicKind::P3TcpKeepAliveCount
            | IntrinsicKind::P3TcpHopLimit
            | IntrinsicKind::P3TcpKeepAliveIdleTime
            | IntrinsicKind::P3TcpKeepAliveInterval
            | IntrinsicKind::P3TcpReceiveBufferSize
            | IntrinsicKind::P3TcpSendBufferSize
            | IntrinsicKind::P3UdpUnicastHopLimit
            | IntrinsicKind::P3UdpReceiveBufferSize
            | IntrinsicKind::P3UdpSendBufferSize => {
                let (func, offset, payload) = match intrinsic {
                    // UDP carries the same three options as TCP, with the same
                    // payload widths, hence the same offsets: u8 at 4, u64 at 8.
                    IntrinsicKind::P3UdpUnicastHopLimit => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_UNICAST_HOP_LIMIT,
                        4,
                        WasiScalar::U8,
                    ),
                    IntrinsicKind::P3UdpReceiveBufferSize => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_RECEIVE_BUFFER_SIZE,
                        8,
                        WasiScalar::I64,
                    ),
                    IntrinsicKind::P3UdpSendBufferSize => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_GET_SEND_BUFFER_SIZE,
                        8,
                        WasiScalar::I64,
                    ),
                    IntrinsicKind::P3TcpKeepAliveEnabled => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_KEEP_ALIVE_ENABLED,
                        4,
                        WasiScalar::U8,
                    ),
                    IntrinsicKind::P3TcpKeepAliveCount => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_KEEP_ALIVE_COUNT,
                        4,
                        WasiScalar::I32,
                    ),
                    IntrinsicKind::P3TcpHopLimit => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_HOP_LIMIT,
                        4,
                        WasiScalar::U8,
                    ),
                    IntrinsicKind::P3TcpKeepAliveIdleTime => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_KEEP_ALIVE_IDLE_TIME,
                        8,
                        WasiScalar::I64,
                    ),
                    IntrinsicKind::P3TcpKeepAliveInterval => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_KEEP_ALIVE_INTERVAL,
                        8,
                        WasiScalar::I64,
                    ),
                    IntrinsicKind::P3TcpReceiveBufferSize => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_RECEIVE_BUFFER_SIZE,
                        8,
                        WasiScalar::I64,
                    ),
                    _ => (
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_GET_SEND_BUFFER_SIZE,
                        8,
                        WasiScalar::I64,
                    ),
                };
                let retptr = self.wasi_bump_alloc(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(func));
                self.wasi_construct_result_scalar_network_error(retptr, &expr.ty, offset, payload);
                self.drop_if_statement(ctx, &expr.ty);
            }

            // Setters: `(self, value, retptr) -> ()` writing `result<_, error-code>`.
            IntrinsicKind::P3TcpSetKeepAliveEnabled
            | IntrinsicKind::P3TcpSetKeepAliveCount
            | IntrinsicKind::P3TcpSetHopLimit
            | IntrinsicKind::P3TcpSetKeepAliveIdleTime
            | IntrinsicKind::P3TcpSetKeepAliveInterval
            | IntrinsicKind::P3TcpSetReceiveBufferSize
            | IntrinsicKind::P3TcpSetSendBufferSize
            | IntrinsicKind::P3UdpSetUnicastHopLimit
            | IntrinsicKind::P3UdpSetReceiveBufferSize
            | IntrinsicKind::P3UdpSetSendBufferSize => {
                let func = match intrinsic {
                    IntrinsicKind::P3UdpSetUnicastHopLimit => {
                        FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_SET_UNICAST_HOP_LIMIT
                    }
                    IntrinsicKind::P3UdpSetReceiveBufferSize => {
                        FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_SET_RECEIVE_BUFFER_SIZE
                    }
                    IntrinsicKind::P3UdpSetSendBufferSize => {
                        FUNC_P3_SOCKETS_TYPES_METHOD_UDP_SOCKET_SET_SEND_BUFFER_SIZE
                    }
                    IntrinsicKind::P3TcpSetKeepAliveEnabled => {
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_KEEP_ALIVE_ENABLED
                    }
                    IntrinsicKind::P3TcpSetKeepAliveCount => {
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_KEEP_ALIVE_COUNT
                    }
                    IntrinsicKind::P3TcpSetHopLimit => {
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_HOP_LIMIT
                    }
                    IntrinsicKind::P3TcpSetKeepAliveIdleTime => {
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_KEEP_ALIVE_IDLE_TIME
                    }
                    IntrinsicKind::P3TcpSetKeepAliveInterval => {
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_KEEP_ALIVE_INTERVAL
                    }
                    IntrinsicKind::P3TcpSetReceiveBufferSize => {
                        FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_RECEIVE_BUFFER_SIZE
                    }
                    _ => FUNC_P3_SOCKETS_TYPES_METHOD_TCP_SOCKET_SET_SEND_BUFFER_SIZE,
                };
                let retptr = self.wasi_bump_alloc(24);
                self.emit_expr(&args[0], ExprContext::Value);
                self.emit_expr(&args[1], ExprContext::Value);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(func));
                self.wasi_construct_result_unit_network_error(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::WallClockNow => {
                // wall-clock now(retptr) -> (), retptr: seconds @0 (i64), nanoseconds @8 (i32)
                let retptr = self.wasi_bump_alloc(16);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(FUNC_P3_CLOCKS_SYSTEM_CLOCK_NOW));
                // epochSecond = i64_load(retptr, 0)
                self.wasi_i64_load(retptr, 0);
                // nano = i32_load(retptr, 8)
                self.wasi_i32_load(retptr, 8);
                let mn = expr.ty.mangled_name();
                let type_idx = self.codegen.type_indices[&mn];
                self.instruction(Instruction::StructNew(type_idx));
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::RandomBytes => {
                // get-random-bytes(len: i64, retptr: i32) -> (); retptr: ptr @0, len @4
                let retptr = self.wasi_bump_alloc(8);
                self.emit_expr(&args[0], ExprContext::Value); // len: i64
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(FUNC_P3_RANDOM_RANDOM_GET_RANDOM_BYTES));
                let data_ptr = self.add_local(ValType::I32);
                let data_len = self.add_local(ValType::I32);
                self.wasi_i32_load(retptr, 0);
                self.instruction(Instruction::LocalSet(data_ptr));
                self.wasi_i32_load(retptr, 4);
                self.instruction(Instruction::LocalSet(data_len));
                self.wasi_create_u8_array_from_bytes(data_ptr, data_len);
                // Free the host-realloc'd list<u8> buffer (allocated through our
                // pinned cabi_realloc) now that its bytes are copied into the GC
                // array — otherwise every call leaks it. The helper returns void,
                // leaving the array on the stack. Guarded, because a zero-length
                // request is legal and the host lowers the empty list it returns
                // as a null or dangling pointer that never went through
                // `cabi_realloc`.
                self.wasi_free_lifted_block(data_ptr);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::RandomInt64 => {
                // get-random-u64() -> i64
                self.instruction(Instruction::Call(FUNC_P3_RANDOM_RANDOM_GET_RANDOM_U64));
                self.drop_if_statement(ctx, &expr.ty);
            }

            // --- CLI intrinsics (Phase 8) ---
            IntrinsicKind::CliTerminalStdin => {
                // get-terminal-stdin(retptr) -> ()
                // retptr (8 bytes): disc @0 (0=None, 1=Some), handle @4
                let retptr = self.wasi_bump_alloc(8);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_CLI_TERMINAL_STDIN_GET_TERMINAL_STDIN,
                ));
                self.wasi_construct_option_i32(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::CliTerminalStdout => {
                let retptr = self.wasi_bump_alloc(8);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_CLI_TERMINAL_STDOUT_GET_TERMINAL_STDOUT,
                ));
                self.wasi_construct_option_i32(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::CliTerminalStderr => {
                let retptr = self.wasi_bump_alloc(8);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_CLI_TERMINAL_STDERR_GET_TERMINAL_STDERR,
                ));
                self.wasi_construct_option_i32(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::CliGetEnvironment => {
                // get-environment(retptr) -> ()
                // retptr (8 bytes): list_ptr @0, list_len @4
                // Each element: (str1_ptr i32, str1_len i32, str2_ptr i32, str2_len i32) = 16 bytes
                let retptr = self.wasi_bump_alloc(8);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(FUNC_P3_CLI_ENVIRONMENT_GET_ENVIRONMENT));

                let list_ptr = self.add_local(ValType::I32);
                let list_len = self.add_local(ValType::I32);
                self.wasi_i32_load(retptr, 0);
                self.instruction(Instruction::LocalSet(list_ptr));
                self.wasi_i32_load(retptr, 4);
                self.instruction(Instruction::LocalSet(list_len));

                // Build Array<(String, String)>
                let array_elem = match &expr.ty {
                    Type::Array(elem) => elem.as_ref(),
                    _ => unreachable!("Expected Array type"),
                };
                let array_type_idx = self.codegen.array_type_index(array_elem);
                let tuple_ty = array_elem.clone();

                let array_ref_type = ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(array_type_idx),
                });

                let elem_base = self.add_local(ValType::I32);
                let str_ptr = self.add_local(ValType::I32);
                let str_len = self.add_local(ValType::I32);

                self.instruction(Instruction::LocalGet(list_len));
                self.emit_if_block(BlockType::Result(array_ref_type));
                // then: list_len > 0
                {
                    // First element (index 0) as default for ArrayNew
                    self.instruction(Instruction::LocalGet(list_ptr));
                    self.instruction(Instruction::LocalSet(elem_base));

                    // key string @0, @4
                    self.wasi_i32_load(elem_base, 0);
                    self.instruction(Instruction::LocalSet(str_ptr));
                    self.wasi_i32_load(elem_base, 4);
                    self.instruction(Instruction::LocalSet(str_len));
                    self.wasi_create_string_from_memory(str_ptr, str_len);
                    // value string @8, @12
                    self.wasi_i32_load(elem_base, 8);
                    self.instruction(Instruction::LocalSet(str_ptr));
                    self.wasi_i32_load(elem_base, 12);
                    self.instruction(Instruction::LocalSet(str_len));
                    self.wasi_create_string_from_memory(str_ptr, str_len);
                    self.emit_rebox_tuple(&tuple_ty);

                    let arr_local = self.add_local(array_ref_type);
                    self.instruction(Instruction::LocalGet(list_len));
                    self.instruction(Instruction::ArrayNew(array_type_idx));
                    self.instruction(Instruction::LocalSet(arr_local));

                    // Loop idx=1..list_len
                    let idx = self.add_local(ValType::I32);
                    self.instruction(Instruction::I32Const(1));
                    self.instruction(Instruction::LocalSet(idx));

                    self.instruction(Instruction::Block(BlockType::Empty));
                    self.instruction(Instruction::Loop(BlockType::Empty));
                    self.instruction(Instruction::LocalGet(idx));
                    self.instruction(Instruction::LocalGet(list_len));
                    self.instruction(Instruction::I32GeU);
                    self.instruction(Instruction::BrIf(1));

                    // elem_base = list_ptr + idx * 16
                    self.instruction(Instruction::LocalGet(list_ptr));
                    self.instruction(Instruction::LocalGet(idx));
                    self.instruction(Instruction::I32Const(16));
                    self.instruction(Instruction::I32Mul);
                    self.instruction(Instruction::I32Add);
                    self.instruction(Instruction::LocalSet(elem_base));

                    self.instruction(Instruction::LocalGet(arr_local));
                    self.instruction(Instruction::LocalGet(idx));
                    // key string @0, @4
                    self.wasi_i32_load(elem_base, 0);
                    self.instruction(Instruction::LocalSet(str_ptr));
                    self.wasi_i32_load(elem_base, 4);
                    self.instruction(Instruction::LocalSet(str_len));
                    self.wasi_create_string_from_memory(str_ptr, str_len);
                    // value string @8, @12
                    self.wasi_i32_load(elem_base, 8);
                    self.instruction(Instruction::LocalSet(str_ptr));
                    self.wasi_i32_load(elem_base, 12);
                    self.instruction(Instruction::LocalSet(str_len));
                    self.wasi_create_string_from_memory(str_ptr, str_len);
                    self.emit_rebox_tuple(&tuple_ty);
                    self.instruction(Instruction::ArraySet(array_type_idx));

                    // idx++
                    self.instruction(Instruction::LocalGet(idx));
                    self.instruction(Instruction::I32Const(1));
                    self.instruction(Instruction::I32Add);
                    self.instruction(Instruction::LocalSet(idx));
                    self.instruction(Instruction::Br(0));
                    self.instruction(Instruction::End); // end loop
                    self.instruction(Instruction::End); // end block

                    self.instruction(Instruction::LocalGet(arr_local));
                }
                self.instruction(Instruction::Else);
                // else: empty array
                self.instruction(Instruction::ArrayNewFixed {
                    array_type_index: array_type_idx,
                    array_size: 0,
                });
                self.emit_end_block(); // end if

                // The list block and every key/value string block came from
                // `cabi_realloc` (the pinned heap) and have now been copied
                // into GC strings.
                self.wasi_free_lifted_list(list_ptr, list_len, 16, &[0, 8]);

                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::CliGetArguments => {
                // get-arguments(retptr) -> ()
                // retptr (8 bytes): list_ptr @0, list_len @4
                // Each element: (str_ptr i32, str_len i32) = 8 bytes
                let retptr = self.wasi_bump_alloc(8);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(FUNC_P3_CLI_ENVIRONMENT_GET_ARGUMENTS));

                let list_ptr = self.add_local(ValType::I32);
                let list_len = self.add_local(ValType::I32);
                self.wasi_i32_load(retptr, 0);
                self.instruction(Instruction::LocalSet(list_ptr));
                self.wasi_i32_load(retptr, 4);
                self.instruction(Instruction::LocalSet(list_len));

                // Build Array<String>
                let string_array_type_idx = self.codegen.array_type_index(&Type::String);
                let array_ref_type = ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(string_array_type_idx),
                });

                let elem_base = self.add_local(ValType::I32);
                let str_ptr = self.add_local(ValType::I32);
                let str_len = self.add_local(ValType::I32);

                self.instruction(Instruction::LocalGet(list_len));
                self.emit_if_block(BlockType::Result(array_ref_type));
                // then: list_len > 0
                {
                    // First element
                    self.instruction(Instruction::LocalGet(list_ptr));
                    self.instruction(Instruction::LocalSet(elem_base));
                    self.wasi_i32_load(elem_base, 0);
                    self.instruction(Instruction::LocalSet(str_ptr));
                    self.wasi_i32_load(elem_base, 4);
                    self.instruction(Instruction::LocalSet(str_len));
                    self.wasi_create_string_from_memory(str_ptr, str_len);

                    let arr_local = self.add_local(array_ref_type);
                    self.instruction(Instruction::LocalGet(list_len));
                    self.instruction(Instruction::ArrayNew(string_array_type_idx));
                    self.instruction(Instruction::LocalSet(arr_local));

                    // Loop idx=1..list_len
                    let idx = self.add_local(ValType::I32);
                    self.instruction(Instruction::I32Const(1));
                    self.instruction(Instruction::LocalSet(idx));

                    self.instruction(Instruction::Block(BlockType::Empty));
                    self.instruction(Instruction::Loop(BlockType::Empty));
                    self.instruction(Instruction::LocalGet(idx));
                    self.instruction(Instruction::LocalGet(list_len));
                    self.instruction(Instruction::I32GeU);
                    self.instruction(Instruction::BrIf(1));

                    // elem_base = list_ptr + idx * 8
                    self.instruction(Instruction::LocalGet(list_ptr));
                    self.instruction(Instruction::LocalGet(idx));
                    self.instruction(Instruction::I32Const(8));
                    self.instruction(Instruction::I32Mul);
                    self.instruction(Instruction::I32Add);
                    self.instruction(Instruction::LocalSet(elem_base));

                    self.instruction(Instruction::LocalGet(arr_local));
                    self.instruction(Instruction::LocalGet(idx));
                    self.wasi_i32_load(elem_base, 0);
                    self.instruction(Instruction::LocalSet(str_ptr));
                    self.wasi_i32_load(elem_base, 4);
                    self.instruction(Instruction::LocalSet(str_len));
                    self.wasi_create_string_from_memory(str_ptr, str_len);
                    self.instruction(Instruction::ArraySet(string_array_type_idx));

                    // idx++
                    self.instruction(Instruction::LocalGet(idx));
                    self.instruction(Instruction::I32Const(1));
                    self.instruction(Instruction::I32Add);
                    self.instruction(Instruction::LocalSet(idx));
                    self.instruction(Instruction::Br(0));
                    self.instruction(Instruction::End); // end loop
                    self.instruction(Instruction::End); // end block

                    self.instruction(Instruction::LocalGet(arr_local));
                }
                self.instruction(Instruction::Else);
                // else: empty array
                self.instruction(Instruction::ArrayNewFixed {
                    array_type_index: string_array_type_idx,
                    array_size: 0,
                });
                self.emit_end_block(); // end if

                self.wasi_free_lifted_list(list_ptr, list_len, 8, &[0]);

                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::CliInitialCwd => {
                // initial-cwd(retptr) -> ()
                // retptr (12 bytes): disc @0 (0=None, 1=Some), str_ptr @4, str_len @8
                let retptr = self.wasi_bump_alloc(12);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(FUNC_P3_CLI_ENVIRONMENT_GET_INITIAL_CWD));
                self.wasi_construct_option_string(retptr, &expr.ty);
                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::CliExit => {
                // Process.exit(status: Result<Unit, Unit>)
                // Use ref.test to check if Error variant (exit code 1) or Ok (exit code 0)
                self.emit_expr(&args[0], ExprContext::Value);
                let result_mn = Self::extract_result_mn(&args[0].ty);
                let error_idx =
                    self.codegen.variant_type_indices[&(result_mn, "Error".to_string())];
                self.instruction(Instruction::RefTestNonNull(
                    wasm_encoder::HeapType::Concrete(error_idx),
                ));
                self.instruction(Instruction::Call(FUNC_P3_CLI_EXIT_EXIT));
                self.instruction(Instruction::Unreachable);
            }

            IntrinsicKind::FsPreopensGetDirectories => {
                // get-directories(retptr) -> ()
                // retptr (8 bytes): list_ptr i32 @0, list_len i32 @4
                // Each element in linear memory: (descriptor_handle i32, string_ptr i32, string_len i32) = 12 bytes
                let retptr = self.wasi_bump_alloc(8);
                self.instruction(Instruction::LocalGet(retptr));
                self.instruction(Instruction::Call(
                    FUNC_P3_FILESYSTEM_PREOPENS_GET_DIRECTORIES,
                ));

                let list_ptr = self.add_local(ValType::I32);
                let list_len = self.add_local(ValType::I32);
                self.wasi_i32_load(retptr, 0);
                self.instruction(Instruction::LocalSet(list_ptr));
                self.wasi_i32_load(retptr, 4);
                self.instruction(Instruction::LocalSet(list_len));

                // Build Array<(FileDescriptor, String)>
                let array_elem = match &expr.ty {
                    Type::Array(elem) => elem.as_ref(),
                    _ => unreachable!("Expected Array type"),
                };
                let array_type_idx = self.codegen.array_type_index(array_elem);

                // Get the inner tuple type
                let tuple_ty = array_elem.clone();

                let array_ref_type = ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(array_type_idx),
                });

                let elem_base = self.add_local(ValType::I32);
                let name_ptr = self.add_local(ValType::I32);
                let name_len = self.add_local(ValType::I32);

                // Array element type is non-nullable ref, so we can't use RefNull for initialization.
                // Branch on empty vs non-empty (same pattern as UDP datagram receive).
                self.instruction(Instruction::LocalGet(list_len));
                self.emit_if_block(BlockType::Result(array_ref_type));
                // then: list_len > 0
                {
                    // Construct first element (index 0) as default for ArrayNew
                    self.instruction(Instruction::LocalGet(list_ptr));
                    self.instruction(Instruction::LocalSet(elem_base));

                    // Read descriptor handle @0
                    self.wasi_i32_load(elem_base, 0);
                    // Read string ptr @4 and len @8, create String
                    self.instruction(Instruction::LocalGet(elem_base));
                    self.instruction(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));
                    self.instruction(Instruction::LocalSet(name_ptr));
                    self.instruction(Instruction::LocalGet(elem_base));
                    self.instruction(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 8,
                        align: 2,
                        memory_index: 0,
                    }));
                    self.instruction(Instruction::LocalSet(name_len));
                    self.wasi_create_string_from_memory(name_ptr, name_len);
                    // Create tuple struct (descriptor_handle i32, string ref)
                    self.emit_rebox_tuple(&tuple_ty);

                    // ArrayNew with first element as default
                    let arr_local = self.add_local(array_ref_type);
                    self.instruction(Instruction::LocalGet(list_len));
                    self.instruction(Instruction::ArrayNew(array_type_idx));
                    self.instruction(Instruction::LocalSet(arr_local));

                    // Loop idx=1..list_len: construct remaining elements
                    let idx = self.add_local(ValType::I32);
                    self.instruction(Instruction::I32Const(1));
                    self.instruction(Instruction::LocalSet(idx));

                    self.instruction(Instruction::Block(BlockType::Empty));
                    self.instruction(Instruction::Loop(BlockType::Empty));
                    self.instruction(Instruction::LocalGet(idx));
                    self.instruction(Instruction::LocalGet(list_len));
                    self.instruction(Instruction::I32GeU);
                    self.instruction(Instruction::BrIf(1));

                    // elem_base = list_ptr + idx * 12
                    self.instruction(Instruction::LocalGet(list_ptr));
                    self.instruction(Instruction::LocalGet(idx));
                    self.instruction(Instruction::I32Const(12));
                    self.instruction(Instruction::I32Mul);
                    self.instruction(Instruction::I32Add);
                    self.instruction(Instruction::LocalSet(elem_base));

                    // arr[idx] = new tuple
                    self.instruction(Instruction::LocalGet(arr_local));
                    self.instruction(Instruction::LocalGet(idx));
                    // descriptor handle @0
                    self.wasi_i32_load(elem_base, 0);
                    // string from @4, @8
                    self.instruction(Instruction::LocalGet(elem_base));
                    self.instruction(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 4,
                        align: 2,
                        memory_index: 0,
                    }));
                    self.instruction(Instruction::LocalSet(name_ptr));
                    self.instruction(Instruction::LocalGet(elem_base));
                    self.instruction(Instruction::I32Load(wasm_encoder::MemArg {
                        offset: 8,
                        align: 2,
                        memory_index: 0,
                    }));
                    self.instruction(Instruction::LocalSet(name_len));
                    self.wasi_create_string_from_memory(name_ptr, name_len);
                    self.emit_rebox_tuple(&tuple_ty);
                    self.instruction(Instruction::ArraySet(array_type_idx));

                    // idx++
                    self.instruction(Instruction::LocalGet(idx));
                    self.instruction(Instruction::I32Const(1));
                    self.instruction(Instruction::I32Add);
                    self.instruction(Instruction::LocalSet(idx));
                    self.instruction(Instruction::Br(0));
                    self.instruction(Instruction::End); // end loop
                    self.instruction(Instruction::End); // end block

                    self.instruction(Instruction::LocalGet(arr_local));
                }
                self.instruction(Instruction::Else);
                // else: empty array
                self.instruction(Instruction::ArrayNewFixed {
                    array_type_index: array_type_idx,
                    array_size: 0,
                });
                self.emit_end_block(); // end if

                self.wasi_free_lifted_list(list_ptr, list_len, 12, &[4]);

                self.drop_if_statement(ctx, &expr.ty);
            }

            IntrinsicKind::ResourceBytes {
                resource_name,
                declaring_root,
            } => {
                // Embedded resource: produce an `Array<Uint8>` from the
                // passive WASM data segment that was pre-allocated for this
                // resource at codegen-init time. Codegen looked it up by
                // `(declaring_root, resource_name)`; the same key resolves
                // the segment index here. `array.new_data` initializes a
                // fresh array of the right element size from `[offset=0,
                // length=segment_byte_length)`.
                let key = (declaring_root.clone(), resource_name.clone());
                let segment_idx = *self
                    .codegen
                    .resource_data_segment_indices
                    .get(&key)
                    .unwrap_or_else(|| {
                        panic!(
                            "resource `{}` for project `{}` not registered with codegen",
                            resource_name, declaring_root
                        )
                    });
                let byte_len = self
                    .codegen
                    .resource_byte_lengths
                    .get(&key)
                    .copied()
                    .unwrap_or(0);
                let array_type_index = self.codegen.array_type_index(&Type::Uint8);
                self.instruction(Instruction::I32Const(0));
                self.instruction(Instruction::I32Const(byte_len as i32));
                self.instruction(Instruction::ArrayNewData {
                    array_type_index,
                    array_data_index: segment_idx,
                });
                self.drop_if_statement(ctx, &expr.ty);
            }
        }
    }
}

/// Whether an intrinsic marshals through the linear-memory scratch arena
/// (global 0). These are exactly the WASI import intrinsics; pure intrinsics
/// (arrays, strings, boxes, ...) never touch linear memory and skip the
/// save/restore bracket. Console/debug printing goes through runtime
/// functions that restore the pointer themselves.
fn intrinsic_uses_scratch(kind: &IntrinsicKind) -> bool {
    matches!(
        kind,
        IntrinsicKind::P3WaitableSetWait
            | IntrinsicKind::P3WaitableSetPoll
            | IntrinsicKind::P3StdinOpen
            | IntrinsicKind::P3StdoutOpen
            | IntrinsicKind::P3StderrOpen
            | IntrinsicKind::P3StdoutReadResult
            | IntrinsicKind::P3StderrReadResult
            | IntrinsicKind::P3StdinReadResult
            | IntrinsicKind::P3FsOpenAtStart
            | IntrinsicKind::P3FsCreateDirectoryAtStart
            | IntrinsicKind::P3FsUnlinkFileAtStart
            | IntrinsicKind::P3FsRemoveDirectoryAtStart
            | IntrinsicKind::P3FsReadViaStream
            | IntrinsicKind::P3FsStatAtStart
            | IntrinsicKind::P3FsMetadataHashAtStart
            | IntrinsicKind::P3FsSymlinkAtStart
            | IntrinsicKind::P3FsRenameAtStart
            | IntrinsicKind::P3FsLinkAtStart
            | IntrinsicKind::P3FsReadlinkAtStart
            | IntrinsicKind::P3FsSetTimesStart
            | IntrinsicKind::P3FsSetTimesAtStart
            | IntrinsicKind::P3FsReadDirectory
            | IntrinsicKind::P3TcpCreate
            | IntrinsicKind::P3TcpBind
            | IntrinsicKind::P3TcpConnectStart
            | IntrinsicKind::P3TcpListen
            | IntrinsicKind::P3TcpReceive
            | IntrinsicKind::P3TcpLocalAddress
            | IntrinsicKind::P3TcpRemoteAddress
            | IntrinsicKind::P3TcpSetListenBacklogSize
            | IntrinsicKind::P3TcpKeepAliveEnabled
            | IntrinsicKind::P3TcpSetKeepAliveEnabled
            | IntrinsicKind::P3TcpKeepAliveIdleTime
            | IntrinsicKind::P3TcpSetKeepAliveIdleTime
            | IntrinsicKind::P3TcpKeepAliveInterval
            | IntrinsicKind::P3TcpSetKeepAliveInterval
            | IntrinsicKind::P3TcpKeepAliveCount
            | IntrinsicKind::P3TcpSetKeepAliveCount
            | IntrinsicKind::P3TcpHopLimit
            | IntrinsicKind::P3TcpSetHopLimit
            | IntrinsicKind::P3TcpReceiveBufferSize
            | IntrinsicKind::P3TcpSetReceiveBufferSize
            | IntrinsicKind::P3TcpSendBufferSize
            | IntrinsicKind::P3TcpSetSendBufferSize
            | IntrinsicKind::P3UdpUnicastHopLimit
            | IntrinsicKind::P3UdpSetUnicastHopLimit
            | IntrinsicKind::P3UdpReceiveBufferSize
            | IntrinsicKind::P3UdpSetReceiveBufferSize
            | IntrinsicKind::P3UdpSendBufferSize
            | IntrinsicKind::P3UdpSetSendBufferSize
            | IntrinsicKind::WallClockNow
            | IntrinsicKind::RandomBytes
            | IntrinsicKind::P3DnsResolveStart
            | IntrinsicKind::P3UdpCreate
            | IntrinsicKind::P3UdpBind
            | IntrinsicKind::P3UdpConnect
            | IntrinsicKind::P3UdpDisconnect
            | IntrinsicKind::P3UdpSendStart
            | IntrinsicKind::P3UdpLocalAddress
            | IntrinsicKind::P3UdpRemoteAddress
            | IntrinsicKind::P3TcpReadReceiveResult
            | IntrinsicKind::P3TcpReadSendResult
            | IntrinsicKind::P3FsReadReadResult
            | IntrinsicKind::P3FsReadWriteResult
            | IntrinsicKind::P3FsReadEntryResult
            | IntrinsicKind::P3FsReadAppendResult
            | IntrinsicKind::FsPreopensGetDirectories
            | IntrinsicKind::CliTerminalStdin
            | IntrinsicKind::CliTerminalStdout
            | IntrinsicKind::CliTerminalStderr
            | IntrinsicKind::CliGetEnvironment
            | IntrinsicKind::CliGetArguments
            | IntrinsicKind::CliInitialCwd
            | IntrinsicKind::CliExit
    )
}
