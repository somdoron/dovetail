use wasm_encoder::{BlockType, Function, Instruction, RefType, ValType};

use super::p3_imports::{
    FUNC_P3_CLI_STDERR_ASYNC_STREAM_WRITE_0_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDERR_FUTURE_DROP_READABLE_1_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDERR_STREAM_NEW_0_WRITE_VIA_STREAM, FUNC_P3_CLI_STDERR_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDOUT_ASYNC_STREAM_WRITE_0_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDOUT_FUTURE_DROP_READABLE_1_WRITE_VIA_STREAM,
    FUNC_P3_CLI_STDOUT_STREAM_NEW_0_WRITE_VIA_STREAM, FUNC_P3_CLI_STDOUT_WRITE_VIA_STREAM,
    FUNC_P3_ROOT_WAITABLE_JOIN, FUNC_P3_ROOT_WAITABLE_SET_NEW, FUNC_P3_ROOT_WAITABLE_SET_WAIT,
};
use super::{
    GLOBAL_FLUSH_SET, GLOBAL_STDERR_WRITABLE, GLOBAL_STDOUT_WRITABLE, STRING_STRUCT_TYPE_INDEX,
    U8_BACKING_TYPE_INDEX,
};

/// Generate the string equality function.
/// Signature: (ref $string_struct, ref $string_struct) -> i32
/// Returns 1 if equal, 0 if not equal.
/// Extracts backing arrays, compares lengths first (fast path), then byte-by-byte.
pub(super) fn generate_string_eq() -> Function {
    let backing_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
    });

    // Params: 0=str_a (struct), 1=str_b (struct)
    // Locals: 2=arr_a, 3=arr_b, 4=len, 5=idx
    let mut f = Function::new(vec![(2, backing_ref), (2, ValType::I32)]);

    let str_a = 0;
    let str_b = 1;
    let arr_a = 2;
    let arr_b = 3;
    let len = 4;
    let idx = 5;

    // Extract backing arrays
    f.instruction(&Instruction::LocalGet(str_a));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 0,
    });
    f.instruction(&Instruction::LocalSet(arr_a));

    f.instruction(&Instruction::LocalGet(str_b));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 0,
    });
    f.instruction(&Instruction::LocalSet(arr_b));

    // Fast path: compare lengths
    // len = arr_a.len
    f.instruction(&Instruction::LocalGet(arr_a));
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::LocalTee(len));

    // arr_b.len
    f.instruction(&Instruction::LocalGet(arr_b));
    f.instruction(&Instruction::ArrayLen);

    // if lengths differ
    f.instruction(&Instruction::I32Ne);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));

    // lengths differ → return false (0)
    f.instruction(&Instruction::I32Const(0));

    f.instruction(&Instruction::Else);

    // idx = 0
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(idx));

    // block (break target)
    f.instruction(&Instruction::Block(BlockType::Empty));
    // loop (continue target)
    f.instruction(&Instruction::Loop(BlockType::Empty));

    // if idx >= len, break (all bytes matched)
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::LocalGet(len));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::BrIf(1)); // break out of block

    // compare arr_a[idx] vs arr_b[idx]
    f.instruction(&Instruction::LocalGet(arr_a));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));

    f.instruction(&Instruction::LocalGet(arr_b));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));

    f.instruction(&Instruction::I32Ne);
    f.instruction(&Instruction::If(BlockType::Empty));
    // mismatch → push false (0) and exit
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::Br(3)); // exit: if(0) + loop(1) + block(2) + else(3)
    f.instruction(&Instruction::End); // end if (mismatch check)

    // idx++
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(idx));

    // continue loop
    f.instruction(&Instruction::Br(0));

    f.instruction(&Instruction::End); // end loop
    f.instruction(&Instruction::End); // end block

    // all bytes matched → true (1)
    f.instruction(&Instruction::I32Const(1));

    f.instruction(&Instruction::End); // end if/else

    f.instruction(&Instruction::End); // end function

    f
}

/// Generate the string concatenation function.
/// Signature: (ref $string_struct, ref $string_struct) -> ref $string_struct
/// Extracts backing arrays, allocates a new array of len_a + len_b, copies both,
/// computes UTF-8 metadata, returns new string struct.
pub(super) fn generate_string_concat() -> Function {
    let backing_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
    });
    let backing_ref_nullable = ValType::Ref(RefType {
        nullable: true,
        heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
    });

    // Params: 0=str_a (struct), 1=str_b (struct)
    // Locals: 2=arr_a, 3=arr_b, 4=len_a, 5=len_b, 6=result_backing (nullable),
    //         7=utf8_a, 8=utf8_b, 9=result_utf8
    let mut f = Function::new(vec![
        (2, backing_ref),
        (2, ValType::I32),
        (1, backing_ref_nullable),
        (3, ValType::I32),
    ]);

    let str_a = 0;
    let str_b = 1;
    let arr_a = 2;
    let arr_b = 3;
    let len_a = 4;
    let len_b = 5;
    let result_backing = 6;
    let utf8_a = 7;
    let utf8_b = 8;
    let result_utf8 = 9;

    // Extract backing arrays
    f.instruction(&Instruction::LocalGet(str_a));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 0,
    });
    f.instruction(&Instruction::LocalSet(arr_a));

    f.instruction(&Instruction::LocalGet(str_b));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 0,
    });
    f.instruction(&Instruction::LocalSet(arr_b));

    // Extract UTF-8 metadata
    f.instruction(&Instruction::LocalGet(str_a));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 1,
    });
    f.instruction(&Instruction::LocalSet(utf8_a));

    f.instruction(&Instruction::LocalGet(str_b));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 1,
    });
    f.instruction(&Instruction::LocalSet(utf8_b));

    // len_a = arr_a.len
    f.instruction(&Instruction::LocalGet(arr_a));
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::LocalSet(len_a));

    // len_b = arr_b.len
    f.instruction(&Instruction::LocalGet(arr_b));
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::LocalSet(len_b));

    // result_backing = array.new(0, len_a + len_b)
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalGet(len_a));
    f.instruction(&Instruction::LocalGet(len_b));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::ArrayNew(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalSet(result_backing));

    // array.copy: result[0..len_a] = arr_a[0..len_a]
    f.instruction(&Instruction::LocalGet(result_backing));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalGet(arr_a));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalGet(len_a));
    f.instruction(&Instruction::ArrayCopy {
        array_type_index_dst: U8_BACKING_TYPE_INDEX,
        array_type_index_src: U8_BACKING_TYPE_INDEX,
    });

    // array.copy: result[len_a..len_a+len_b] = arr_b[0..len_b]
    f.instruction(&Instruction::LocalGet(result_backing));
    f.instruction(&Instruction::LocalGet(len_a));
    f.instruction(&Instruction::LocalGet(arr_b));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalGet(len_b));
    f.instruction(&Instruction::ArrayCopy {
        array_type_index_dst: U8_BACKING_TYPE_INDEX,
        array_type_index_src: U8_BACKING_TYPE_INDEX,
    });

    // Compute result UTF-8 metadata:
    // result_utf8_len = (utf8_a & 0x7FFFFFFF) + (utf8_b & 0x7FFFFFFF)
    // ascii_flag = (utf8_a & utf8_b) & 0x80000000
    // result_utf8 = result_utf8_len | ascii_flag
    f.instruction(&Instruction::LocalGet(utf8_a));
    f.instruction(&Instruction::I32Const(0x7FFFFFFF));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::LocalGet(utf8_b));
    f.instruction(&Instruction::I32Const(0x7FFFFFFF));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Add);
    // Stack: result_utf8_len
    f.instruction(&Instruction::LocalGet(utf8_a));
    f.instruction(&Instruction::LocalGet(utf8_b));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(i32::MIN)); // 0x80000000
    f.instruction(&Instruction::I32And);
    // Stack: result_utf8_len, ascii_flag
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::LocalSet(result_utf8));

    // Wrap in struct: StructNew(result_backing, result_utf8)
    f.instruction(&Instruction::LocalGet(result_backing));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::LocalGet(result_utf8));
    f.instruction(&Instruction::StructNew(STRING_STRUCT_TYPE_INDEX));

    f.instruction(&Instruction::End);

    f
}

/// Generate the string comparison function (lexicographic, unsigned byte ordering).
/// Signature: (ref $string_struct, ref $string_struct) -> i32
/// Returns -1 if a < b, 0 if a == b, 1 if a > b.
pub(super) fn generate_string_cmp() -> Function {
    let backing_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
    });

    // Params: 0=str_a (struct), 1=str_b (struct)
    // Locals: 2=arr_a, 3=arr_b, 4=len_a, 5=len_b, 6=min_len, 7=idx
    let mut f = Function::new(vec![(2, backing_ref), (4, ValType::I32)]);

    let str_a = 0;
    let str_b = 1;
    let arr_a = 2;
    let arr_b = 3;
    let len_a = 4;
    let len_b = 5;
    let min_len = 6;
    let idx = 7;

    // Extract backing arrays
    f.instruction(&Instruction::LocalGet(str_a));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 0,
    });
    f.instruction(&Instruction::LocalSet(arr_a));

    f.instruction(&Instruction::LocalGet(str_b));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 0,
    });
    f.instruction(&Instruction::LocalSet(arr_b));

    // len_a = arr_a.len
    f.instruction(&Instruction::LocalGet(arr_a));
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::LocalSet(len_a));

    // len_b = arr_b.len
    f.instruction(&Instruction::LocalGet(arr_b));
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::LocalSet(len_b));

    // min_len = min(len_a, len_b)
    f.instruction(&Instruction::LocalGet(len_a));
    f.instruction(&Instruction::LocalGet(len_b));
    f.instruction(&Instruction::LocalGet(len_a));
    f.instruction(&Instruction::LocalGet(len_b));
    f.instruction(&Instruction::I32LeU);
    f.instruction(&Instruction::Select);
    f.instruction(&Instruction::LocalSet(min_len));

    // idx = 0
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(idx));

    // block (break target)
    f.instruction(&Instruction::Block(BlockType::Empty));
    // loop (continue target)
    f.instruction(&Instruction::Loop(BlockType::Empty));

    // if idx >= min_len, break
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::LocalGet(min_len));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::BrIf(1));

    // if arr_a[idx] < arr_b[idx], return -1
    f.instruction(&Instruction::LocalGet(arr_a));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalGet(arr_b));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(-1));
    f.instruction(&Instruction::Br(3)); // exit: if + loop + block
    f.instruction(&Instruction::End);

    // if arr_a[idx] > arr_b[idx], return 1
    f.instruction(&Instruction::LocalGet(arr_a));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalGet(arr_b));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32GtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::Br(3)); // exit: if + loop + block
    f.instruction(&Instruction::End);

    // idx++
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(idx));

    // continue loop
    f.instruction(&Instruction::Br(0));

    f.instruction(&Instruction::End); // end loop
    f.instruction(&Instruction::End); // end block

    // All bytes matched up to min_len — compare lengths
    // if len_a < len_b, return -1; if len_a > len_b, return 1; else return 0
    f.instruction(&Instruction::LocalGet(len_a));
    f.instruction(&Instruction::LocalGet(len_b));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    f.instruction(&Instruction::I32Const(-1));
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::LocalGet(len_a));
    f.instruction(&Instruction::LocalGet(len_b));
    f.instruction(&Instruction::I32GtU);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::End); // end inner if
    f.instruction(&Instruction::End); // end outer if

    f.instruction(&Instruction::End); // end function

    f
}

/// Generate string_from_bytes: creates a new string from a byte array slice.
/// Signature: (ref $backing, i32, i32) -> ref $string_struct
/// Params: buf, start, length. Copies buf[start..start+length] into a new backing array,
/// scans to compute UTF-8 metadata, wraps in string struct.
pub(super) fn generate_string_from_bytes() -> Function {
    let backing_ref_nullable = ValType::Ref(RefType {
        nullable: true,
        heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
    });

    // Params: 0=buf, 1=start, 2=length
    // Locals: 3=result (ref null backing), 4=utf8_count, 5=is_ascii, 6=scan_idx, 7=byte
    let mut f = Function::new(vec![(1, backing_ref_nullable), (4, ValType::I32)]);

    let buf = 0;
    let start = 1;
    let length = 2;
    let result = 3;
    let utf8_count = 4;
    let is_ascii = 5;
    let scan_idx = 6;
    let byte = 7;

    // Create result array: ArrayNew(0, length)
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalGet(length));
    f.instruction(&Instruction::ArrayNew(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalTee(result));

    // ArrayCopy: buf[start..start+length] -> result[0..length]
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalGet(buf));
    f.instruction(&Instruction::LocalGet(start));
    f.instruction(&Instruction::LocalGet(length));
    f.instruction(&Instruction::ArrayCopy {
        array_type_index_dst: U8_BACKING_TYPE_INDEX,
        array_type_index_src: U8_BACKING_TYPE_INDEX,
    });

    // Scan to compute UTF-8 metadata
    // utf8_count = 0, is_ascii = 1 (assume ASCII), scan_idx = 0
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(utf8_count));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::LocalSet(is_ascii));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(scan_idx));

    // loop over bytes
    f.instruction(&Instruction::Block(BlockType::Empty));
    f.instruction(&Instruction::Loop(BlockType::Empty));

    // if scan_idx >= length, break
    f.instruction(&Instruction::LocalGet(scan_idx));
    f.instruction(&Instruction::LocalGet(length));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::BrIf(1));

    // byte = result[scan_idx]
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::LocalGet(scan_idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalSet(byte));

    // if (byte & 0xC0) != 0x80, it's a start byte → increment utf8_count
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0xC0));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::I32Ne);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(utf8_count));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(utf8_count));
    f.instruction(&Instruction::End);

    // if byte >= 0x80, clear is_ascii
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(is_ascii));
    f.instruction(&Instruction::End);

    // scan_idx++
    f.instruction(&Instruction::LocalGet(scan_idx));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(scan_idx));

    // continue loop
    f.instruction(&Instruction::Br(0));

    f.instruction(&Instruction::End); // end loop
    f.instruction(&Instruction::End); // end block

    // Compute utf8_field = utf8_count | (is_ascii ? 0x80000000 : 0)
    // result_backing, utf8_field → StructNew
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::LocalGet(utf8_count));
    f.instruction(&Instruction::LocalGet(is_ascii));
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    f.instruction(&Instruction::I32Const(i32::MIN)); // 0x80000000
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::StructNew(STRING_STRUCT_TYPE_INDEX));

    f.instruction(&Instruction::End);

    f
}

/// Generate char_to_string: converts a Unicode code point (i32) to a UTF-8 string struct.
/// Handles 1-4 byte encodings. Wraps result in string struct with UTF-8 metadata.
/// Signature: (i32) -> ref $string_struct
pub(super) fn generate_char_to_string() -> Function {
    let backing_ref_nullable = ValType::Ref(RefType {
        nullable: true,
        heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
    });

    // Params: 0=code_point (i32)
    // Locals: 1=result (ref null $backing)
    let mut f = Function::new(vec![(1, backing_ref_nullable)]);

    let cp = 0;
    let result = 1;

    // 1-byte: cp <= 0x7F (ASCII)
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    // backing = [cp]
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::ArrayNew(U8_BACKING_TYPE_INDEX));
    // utf8_field = 1 | 0x80000000 (1 char, ASCII)
    f.instruction(&Instruction::I32Const(1 | i32::MIN));
    f.instruction(&Instruction::StructNew(STRING_STRUCT_TYPE_INDEX));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    // 2-byte: cp <= 0x7FF
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(0x800));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::ArrayNew(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalSet(result));
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Const(0xC0));
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(6));
    f.instruction(&Instruction::I32ShrU);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::ArraySet(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::ArraySet(U8_BACKING_TYPE_INDEX));
    // Wrap: backing, utf8_field=1 (1 char, not ASCII)
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::StructNew(STRING_STRUCT_TYPE_INDEX));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    // 3-byte: cp <= 0xFFFF
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(0x10000));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Const(3));
    f.instruction(&Instruction::ArrayNew(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalSet(result));
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Const(0xE0));
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(12));
    f.instruction(&Instruction::I32ShrU);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::ArraySet(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(6));
    f.instruction(&Instruction::I32ShrU);
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::ArraySet(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::ArraySet(U8_BACKING_TYPE_INDEX));
    // Wrap: backing, utf8_field=1 (1 char, not ASCII)
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::StructNew(STRING_STRUCT_TYPE_INDEX));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    // 4-byte: cp > 0xFFFF
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Const(4));
    f.instruction(&Instruction::ArrayNew(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalSet(result));
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Const(0xF0));
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(18));
    f.instruction(&Instruction::I32ShrU);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::ArraySet(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(12));
    f.instruction(&Instruction::I32ShrU);
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::ArraySet(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(6));
    f.instruction(&Instruction::I32ShrU);
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::ArraySet(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::I32Const(3));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::ArraySet(U8_BACKING_TYPE_INDEX));
    // Wrap: backing, utf8_field=1 (1 char, not ASCII)
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::RefAsNonNull);
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::StructNew(STRING_STRUCT_TYPE_INDEX));

    f.instruction(&Instruction::End);

    f
}

/// Generate debug_print: print a GC string to stdout via WASI, appending a newline.
/// Signature: (ref $string_struct) -> ()
/// Copies the GC string backing array into linear memory, appends 0x0A,
/// then calls get-stdout + blocking-write-and-flush.
/// Generate a runtime function that writes a GC string to a WASI stream.
/// `stream_func` is the WASI import to get the stream handle (get-stdout or get-stderr).
/// Which standard output stream a runtime print function targets.
struct StdioTarget {
    /// `[stream-new-0]write-via-stream` import for this stream.
    stream_new: u32,
    /// `write-via-stream` import (sync; hands the readable end to the host).
    write_via: u32,
    /// `[stream-write-0]write-via-stream` import.
    stream_write: u32,
    /// `[future-drop-readable-1]write-via-stream` import.
    future_drop_readable: u32,
    /// Mutable i32 global caching the writable end (0 = uninitialized).
    writable_global: u32,
}

const STDOUT_TARGET: StdioTarget = StdioTarget {
    stream_new: FUNC_P3_CLI_STDOUT_STREAM_NEW_0_WRITE_VIA_STREAM,
    write_via: FUNC_P3_CLI_STDOUT_WRITE_VIA_STREAM,
    stream_write: FUNC_P3_CLI_STDOUT_ASYNC_STREAM_WRITE_0_WRITE_VIA_STREAM,
    future_drop_readable: FUNC_P3_CLI_STDOUT_FUTURE_DROP_READABLE_1_WRITE_VIA_STREAM,
    writable_global: GLOBAL_STDOUT_WRITABLE,
};

const STDERR_TARGET: StdioTarget = StdioTarget {
    stream_new: FUNC_P3_CLI_STDERR_STREAM_NEW_0_WRITE_VIA_STREAM,
    write_via: FUNC_P3_CLI_STDERR_WRITE_VIA_STREAM,
    stream_write: FUNC_P3_CLI_STDERR_ASYNC_STREAM_WRITE_0_WRITE_VIA_STREAM,
    future_drop_readable: FUNC_P3_CLI_STDERR_FUTURE_DROP_READABLE_1_WRITE_VIA_STREAM,
    writable_global: GLOBAL_STDERR_WRITABLE,
};

/// Emit the p3 "post + flush" write of `byte_len + extra` bytes at `ptr` to a
/// stdio stream: lazily create the flush waitable-set and the stream pair
/// (handing the readable end to the host via `write-via-stream`), then loop
/// `stream.write`; on BLOCKED, wait on the private flush set for the
/// completion event and continue with the remainder (partial completions
/// re-post).
///
/// Best-effort by design: a DROPPED completion (the reader went away) abandons
/// the rest of the payload and returns normally. `debug` and panic reports are
/// the only callers, and neither has anywhere to report a stdio failure — so
/// killing the component over a closed pipe would lose the very message it was
/// asked to print. Print errors were ignored outright under P2; this at least
/// stops writing.
///
/// The stream pair created here is PRIVATE to this path: nothing else opens it,
/// joins it to another waitable-set, or posts on its writable end.
/// `standard.io.Console` calls `write-via-stream` again for its own pair (the
/// host explicitly allows many stdio writers, and for inherited stdio each one
/// writes straight through to the same fd). That separation is what makes the
/// two correctness claims below true:
///
///  - **The next event on the flush set is always ours.** The writable end is
///    joined to `GLOBAL_FLUSH_SET` at creation and never leaves it, and runtime
///    prints fully drain before returning, so at most one write is in flight.
///    Sharing the end with `Console` broke both halves: a parked `Console` write
///    moved the waitable into the fiber runtime's set — so this wait could never
///    be satisfied — and a second post on the same end is a concurrent-operation
///    trap.
///  - **A payload of any size completes.** Past the host's buffer the write comes
///    back BLOCKED, waits here, and resumes with the remainder (covered by
///    `tests/stdio_blocking.rs`).
///
/// Blocking in `waitable-set.wait` is legal because every Dovetail export is
/// async-lifted (may-block is type-based). What this path cannot do is let
/// anything else run while it waits, which is why it is confined to panic
/// reporting and `debug`, where there is no runtime to hand the write to in the
/// first place. `Console` posts through the fiber runtime instead, so
/// backpressure parks one fiber rather than the whole component. The cost of the
/// split is that output from the two paths can interleave at chunk boundaries
/// when both are in flight at once — acceptable for a debug/panic channel, and
/// strictly better than trapping.
#[allow(clippy::too_many_arguments)]
fn emit_flush_write(
    f: &mut Function,
    target: &StdioTarget,
    ptr: u32,
    byte_len: u32,
    extra: i32,
    evtbuf: u32,
    wptr: u32,
    wlen: u32,
    tmp: u32,
    pair: u32,
) {
    // evtbuf = align4(ptr + byte_len + extra) — the scratch tail area
    f.instruction(&Instruction::LocalGet(ptr));
    f.instruction(&Instruction::LocalGet(byte_len));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(extra + 3));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(-4));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::LocalSet(evtbuf));

    // Everything below is skipped once the reader is gone: -1 is the latch the
    // DROPPED arm sets. `stream.write` on an end whose readable half has been
    // dropped is itself a trap ("cannot write after being notified that the
    // readable end dropped"), so the second `debug` after a broken pipe has to
    // be answered here rather than by the host.
    f.instruction(&Instruction::GlobalGet(target.writable_global));
    f.instruction(&Instruction::I32Const(-1));
    f.instruction(&Instruction::I32Ne);
    f.instruction(&Instruction::If(BlockType::Empty));

    // if flush_set == 0 { flush_set = waitable-set.new() }
    f.instruction(&Instruction::GlobalGet(GLOBAL_FLUSH_SET));
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Call(FUNC_P3_ROOT_WAITABLE_SET_NEW));
    f.instruction(&Instruction::GlobalSet(GLOBAL_FLUSH_SET));
    f.instruction(&Instruction::End);

    // if writable == 0 { create pair, hand readable to host, keep writable }
    f.instruction(&Instruction::GlobalGet(target.writable_global));
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Call(target.stream_new));
    f.instruction(&Instruction::LocalTee(pair));
    // writable = high 32 bits
    f.instruction(&Instruction::I64Const(32));
    f.instruction(&Instruction::I64ShrU);
    f.instruction(&Instruction::I32WrapI64);
    f.instruction(&Instruction::GlobalSet(target.writable_global));
    // hand readable (low 32 bits) to the host and drop the result future it
    // returns: this path traps on any non-COMPLETED status and has nobody to
    // report a late failure to, so keeping the handle alive would only give the
    // runtime a waitable it might mistake for one of its own.
    f.instruction(&Instruction::LocalGet(pair));
    f.instruction(&Instruction::I32WrapI64);
    f.instruction(&Instruction::Call(target.write_via));
    f.instruction(&Instruction::Call(target.future_drop_readable));
    // keep the writable end joined to the flush set permanently
    f.instruction(&Instruction::GlobalGet(target.writable_global));
    f.instruction(&Instruction::GlobalGet(GLOBAL_FLUSH_SET));
    f.instruction(&Instruction::Call(FUNC_P3_ROOT_WAITABLE_JOIN));
    f.instruction(&Instruction::End);

    // wptr = ptr; wlen = byte_len + extra
    f.instruction(&Instruction::LocalGet(ptr));
    f.instruction(&Instruction::LocalSet(wptr));
    f.instruction(&Instruction::LocalGet(byte_len));
    f.instruction(&Instruction::I32Const(extra));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(wlen));

    f.instruction(&Instruction::Block(BlockType::Empty)); // $done
    f.instruction(&Instruction::Loop(BlockType::Empty)); // $w
    // if wlen == 0 break
    f.instruction(&Instruction::LocalGet(wlen));
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::BrIf(1));
    // tmp = stream.write(writable, wptr, wlen)
    f.instruction(&Instruction::GlobalGet(target.writable_global));
    f.instruction(&Instruction::LocalGet(wptr));
    f.instruction(&Instruction::LocalGet(wlen));
    f.instruction(&Instruction::Call(target.stream_write));
    f.instruction(&Instruction::LocalSet(tmp));
    // BLOCKED → wait for the completion event; its payload uses the same
    // `code | n<<4` packing as the direct return value
    f.instruction(&Instruction::LocalGet(tmp));
    f.instruction(&Instruction::I32Const(-1));
    f.instruction(&Instruction::I32Eq);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::GlobalGet(GLOBAL_FLUSH_SET));
    f.instruction(&Instruction::LocalGet(evtbuf));
    f.instruction(&Instruction::Call(FUNC_P3_ROOT_WAITABLE_SET_WAIT));
    // The wait returns the event code and stores the waitable index at
    // evtbuf+0, the payload at evtbuf+4. The invariants above say the only
    // event this private set can deliver is a write completion (code 3,
    // STREAM_WRITE) for the end being flushed — so check both before trusting
    // the payload. A mismatch is an impossible host state (or a hole in those
    // invariants), and the policy for those is loud failure: same bare
    // `unreachable` as the impossible-status arm below.
    f.instruction(&Instruction::I32Const(3));
    f.instruction(&Instruction::I32Ne);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::LocalGet(evtbuf));
    f.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
        offset: 0,
        align: 2,
        memory_index: 0,
    }));
    f.instruction(&Instruction::GlobalGet(target.writable_global));
    f.instruction(&Instruction::I32Ne);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::LocalGet(evtbuf));
    f.instruction(&Instruction::I32Load(wasm_encoder::MemArg {
        offset: 4,
        align: 2,
        memory_index: 0,
    }));
    f.instruction(&Instruction::LocalSet(tmp));
    f.instruction(&Instruction::End);
    // DROPPED (code 1): the reader is gone — `dovetail run app | head` after head
    // exits. There is nothing to write to and nobody to report to, so latch the
    // stream as dead, stop, and let the caller carry on. Trapping here would
    // kill the component over a closed pipe, and on the panic path it would
    // replace the panic report with a bare stdio trap.
    f.instruction(&Instruction::LocalGet(tmp));
    f.instruction(&Instruction::I32Const(0xF));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Eq);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(-1));
    f.instruction(&Instruction::GlobalSet(target.writable_global));
    f.instruction(&Instruction::Br(2)); // out of $done
    f.instruction(&Instruction::End);
    // Any other non-COMPLETED code means the canonical ABI handed us a status
    // this call cannot produce (CANCELLED, with no cancel outstanding), which is
    // a broken host or a corrupted handle rather than a gone reader.
    f.instruction(&Instruction::LocalGet(tmp));
    f.instruction(&Instruction::I32Const(0xF));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);
    // wptr += n; wlen -= n (n = tmp >> 4)
    f.instruction(&Instruction::LocalGet(wptr));
    f.instruction(&Instruction::LocalGet(tmp));
    f.instruction(&Instruction::I32Const(4));
    f.instruction(&Instruction::I32ShrU);
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(wptr));
    f.instruction(&Instruction::LocalGet(wlen));
    f.instruction(&Instruction::LocalGet(tmp));
    f.instruction(&Instruction::I32Const(4));
    f.instruction(&Instruction::I32ShrU);
    f.instruction(&Instruction::I32Sub);
    f.instruction(&Instruction::LocalSet(wlen));
    f.instruction(&Instruction::Br(0));
    f.instruction(&Instruction::End); // end loop
    f.instruction(&Instruction::End); // end block $done
    f.instruction(&Instruction::End); // end "reader still there"
}

/// Write a string to a stdio stream via the p3 posted-write path, appending
/// a newline when requested. Frees its buffer before returning.
///
/// The buffer is pinned-allocated rather than taken from the scratch arena:
/// its size is the caller's string length, which is unbounded, and the arena
/// is one page sitting directly below the pinned heap. Under P2 an oversized
/// scratch write trapped on a memory bound; now that memory grows for the
/// pinned heap it would instead silently overwrite live pinned blocks.
fn generate_write_string_to_stream(
    target: &StdioTarget,
    append_newline: bool,
    pinned_alloc: u32,
    pinned_free: u32,
) -> Function {
    let backing_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
    });

    // Params: 0=str (string struct)
    let mut f = Function::new(vec![(1, backing_ref), (7, ValType::I32), (1, ValType::I64)]);

    let str_param = 0;
    let backing = 1;
    let byte_len = 2;
    let ptr = 3;
    let idx = 4;
    let evtbuf = 5;
    let wptr = 6;
    let wlen = 7;
    let tmp = 8;
    let pair = 9;

    // Extract backing array
    f.instruction(&Instruction::LocalGet(str_param));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 0,
    });
    f.instruction(&Instruction::LocalSet(backing));

    // byte_len = backing.len
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::LocalSet(byte_len));

    let extra = if append_newline { 1 } else { 0 };

    // ptr = pinned_alloc(align4(byte_len + extra) + 16) — the trailing 16 bytes
    // are the event buffer `emit_flush_write` reads completions into.
    f.instruction(&Instruction::LocalGet(byte_len));
    f.instruction(&Instruction::I32Const(extra + 3));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(-4));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(16));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::Call(pinned_alloc));
    f.instruction(&Instruction::LocalSet(ptr));

    // Copy loop: memory[ptr + i] = backing[i] for i in 0..byte_len
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(idx));

    f.instruction(&Instruction::Block(BlockType::Empty));
    f.instruction(&Instruction::Loop(BlockType::Empty));

    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::LocalGet(byte_len));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::BrIf(1));

    f.instruction(&Instruction::LocalGet(ptr));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32Store8(wasm_encoder::MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));

    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(idx));

    f.instruction(&Instruction::Br(0));
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::End);

    if append_newline {
        f.instruction(&Instruction::LocalGet(ptr));
        f.instruction(&Instruction::LocalGet(byte_len));
        f.instruction(&Instruction::I32Add);
        f.instruction(&Instruction::I32Const(0x0A));
        f.instruction(&Instruction::I32Store8(wasm_encoder::MemArg {
            offset: 0,
            align: 0,
            memory_index: 0,
        }));
    }

    emit_flush_write(
        &mut f, target, ptr, byte_len, extra, evtbuf, wptr, wlen, tmp, pair,
    );

    // The write has fully drained by here, so the buffer is dead.
    f.instruction(&Instruction::LocalGet(ptr));
    f.instruction(&Instruction::Call(pinned_free));

    f.instruction(&Instruction::End); // end function

    f
}

pub(super) fn generate_debug_print(pinned_alloc: u32, pinned_free: u32) -> Function {
    generate_write_string_to_stream(&STDOUT_TARGET, true, pinned_alloc, pinned_free)
}

/// Console.print: write string to stdout without newline.
pub(super) fn generate_console_print(pinned_alloc: u32, pinned_free: u32) -> Function {
    generate_write_string_to_stream(&STDOUT_TARGET, false, pinned_alloc, pinned_free)
}

/// Console.eprint: write string to stderr without newline.
pub(super) fn generate_console_eprint(pinned_alloc: u32, pinned_free: u32) -> Function {
    generate_write_string_to_stream(&STDERR_TARGET, false, pinned_alloc, pinned_free)
}

/// Console.eprintln: write string to stderr with newline.
pub(super) fn generate_console_eprintln(pinned_alloc: u32, pinned_free: u32) -> Function {
    generate_write_string_to_stream(&STDERR_TARGET, true, pinned_alloc, pinned_free)
}

/// Generate string_get_char: get the Unicode code point at a UTF-8 character index.
/// Signature: (ref $string_struct, i32) -> i32
/// Params: s (string struct), index (character index).
/// Returns the code point at the given character index.
/// ASCII fast path: O(1) direct array access.
/// Non-ASCII: O(n) scan counting start bytes.
pub(super) fn generate_string_get_char() -> Function {
    let backing_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
    });

    // Params: 0=s (string struct), 1=index (i32)
    // Locals: 2=backing, 3=utf8_field, 4=utf8_len, 5=byte_idx, 6=char_count, 7=byte, 8=cp
    let mut f = Function::new(vec![(1, backing_ref), (6, ValType::I32)]);

    let s = 0;
    let index = 1;
    let backing = 2;
    let utf8_field = 3;
    let utf8_len = 4;
    let byte_idx = 5;
    let char_count = 6;
    let byte = 7;
    let cp = 8;

    // Extract backing and utf8_field
    f.instruction(&Instruction::LocalGet(s));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 0,
    });
    f.instruction(&Instruction::LocalSet(backing));

    f.instruction(&Instruction::LocalGet(s));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 1,
    });
    f.instruction(&Instruction::LocalSet(utf8_field));

    // utf8_len = utf8_field & 0x7FFFFFFF
    f.instruction(&Instruction::LocalGet(utf8_field));
    f.instruction(&Instruction::I32Const(0x7FFFFFFF));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::LocalSet(utf8_len));

    // Bounds check: if index < 0 || index >= utf8_len → trap
    f.instruction(&Instruction::LocalGet(index));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32LtS);
    f.instruction(&Instruction::LocalGet(index));
    f.instruction(&Instruction::LocalGet(utf8_len));
    f.instruction(&Instruction::I32GeS);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);

    // ASCII fast path: if utf8_field < 0 (sign bit set = is_ascii)
    f.instruction(&Instruction::LocalGet(utf8_field));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32LtS);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    // return backing[index] (direct O(1) access)
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(index));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::Else);

    // Non-ASCII path: scan bytes counting start bytes until char_count == index
    // byte_idx = 0, char_count = 0
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(byte_idx));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(char_count));

    // Scan loop: find the byte position of the index-th character
    f.instruction(&Instruction::Block(BlockType::Empty));
    f.instruction(&Instruction::Loop(BlockType::Empty));

    // if char_count == index, break (byte_idx points to the start of the target char)
    f.instruction(&Instruction::LocalGet(char_count));
    f.instruction(&Instruction::LocalGet(index));
    f.instruction(&Instruction::I32Eq);
    f.instruction(&Instruction::BrIf(1));

    // Skip current character (1-4 bytes)
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalSet(byte));

    // Determine byte length from lead byte and advance byte_idx
    // if byte < 0x80 → 1 byte
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(byte_idx));
    f.instruction(&Instruction::Else);
    // if byte < 0xE0 → 2 bytes
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0xE0));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(byte_idx));
    f.instruction(&Instruction::Else);
    // if byte < 0xF0 → 3 bytes
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0xF0));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(3));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(byte_idx));
    f.instruction(&Instruction::Else);
    // 4 bytes
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(4));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(byte_idx));
    f.instruction(&Instruction::End); // end 0xF0 check
    f.instruction(&Instruction::End); // end 0xE0 check
    f.instruction(&Instruction::End); // end 0x80 check

    // char_count++
    f.instruction(&Instruction::LocalGet(char_count));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(char_count));

    // continue loop
    f.instruction(&Instruction::Br(0));
    f.instruction(&Instruction::End); // end loop
    f.instruction(&Instruction::End); // end block

    // Now byte_idx points to the start byte of the target character.
    // Decode the UTF-8 sequence.
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::LocalSet(byte));

    // 1-byte: byte < 0x80
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0x80));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::Else);

    // 2-byte: byte < 0xE0
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0xE0));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    // cp = (byte & 0x1F) << 6
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0x1F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(6));
    f.instruction(&Instruction::I32Shl);
    // | (backing[byte_idx+1] & 0x3F)
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::Else);

    // 3-byte: byte < 0xF0
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0xF0));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Result(ValType::I32)));
    // cp = (byte & 0x0F) << 12
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0x0F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(12));
    f.instruction(&Instruction::I32Shl);
    f.instruction(&Instruction::LocalSet(cp));
    // | (backing[byte_idx+1] & 0x3F) << 6
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(6));
    f.instruction(&Instruction::I32Shl);
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::LocalSet(cp));
    // | (backing[byte_idx+2] & 0x3F)
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::Else);

    // 4-byte
    // cp = (byte & 0x07) << 18
    f.instruction(&Instruction::LocalGet(byte));
    f.instruction(&Instruction::I32Const(0x07));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(18));
    f.instruction(&Instruction::I32Shl);
    f.instruction(&Instruction::LocalSet(cp));
    // | (backing[byte_idx+1] & 0x3F) << 12
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(12));
    f.instruction(&Instruction::I32Shl);
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::LocalSet(cp));
    // | (backing[byte_idx+2] & 0x3F) << 6
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(2));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(6));
    f.instruction(&Instruction::I32Shl);
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::LocalSet(cp));
    // | (backing[byte_idx+3] & 0x3F)
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(byte_idx));
    f.instruction(&Instruction::I32Const(3));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32Const(0x3F));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::LocalGet(cp));
    f.instruction(&Instruction::I32Or);

    f.instruction(&Instruction::End); // end 0xF0 check
    f.instruction(&Instruction::End); // end 0xE0 check
    f.instruction(&Instruction::End); // end 0x80 check

    f.instruction(&Instruction::End); // end ASCII fast path if/else

    f.instruction(&Instruction::End); // end function

    f
}

/// Generate panic_with_message: print "Panic: " + message to stderr, then trap.
/// Signature: (ref $string_struct) -> ()
/// Copies the GC string backing array into linear memory with "Panic: " prefix and newline,
/// then calls get-stderr + blocking-write-and-flush, then unreachable.
pub(super) fn generate_panic_with_message(pinned_alloc: u32) -> Function {
    let backing_ref = ValType::Ref(RefType {
        nullable: false,
        heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
    });

    // Params: 0=str (string struct)
    // Locals: 1=backing, 2=byte_len, 3=ptr, 4=idx, 5=evtbuf, 6=wptr, 7=wlen, 8=tmp, 9=pair
    let mut f = Function::new(vec![(1, backing_ref), (7, ValType::I32), (1, ValType::I64)]);

    let str_param = 0;
    let backing = 1;
    let byte_len = 2;
    let ptr = 3;
    let idx = 4;
    let evtbuf = 5;
    let wptr = 6;
    let wlen = 7;
    let tmp = 8;
    let pair = 9;

    // Extract backing array
    f.instruction(&Instruction::LocalGet(str_param));
    f.instruction(&Instruction::StructGet {
        struct_type_index: STRING_STRUCT_TYPE_INDEX,
        field_index: 0,
    });
    f.instruction(&Instruction::LocalSet(backing));

    // byte_len = backing.len
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::ArrayLen);
    f.instruction(&Instruction::LocalSet(byte_len));

    // ptr = pinned_alloc(align4(7 + byte_len + 1) + 16)
    // 7 for "Panic: ", 1 for newline, 16 for the event buffer. Pinned rather
    // than scratch because the message length is unbounded — see
    // `generate_write_string_to_stream`. Never freed: this function traps.
    f.instruction(&Instruction::LocalGet(byte_len));
    f.instruction(&Instruction::I32Const(8)); // +7 prefix +1 newline
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(3)); // align up to 4: (x + 3) & ~3
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(-4)); // ~3 = 0xFFFFFFFC
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(16)); // return area
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::Call(pinned_alloc));
    f.instruction(&Instruction::LocalSet(ptr));

    // Store "Panic: " prefix (7 bytes) at ptr
    // 'P'=0x50, 'a'=0x61, 'n'=0x6E, 'i'=0x69, 'c'=0x63, ':'=0x3A, ' '=0x20
    let prefix = [0x50u8, 0x61, 0x6E, 0x69, 0x63, 0x3A, 0x20];
    for (i, &byte) in prefix.iter().enumerate() {
        f.instruction(&Instruction::LocalGet(ptr));
        f.instruction(&Instruction::I32Const(byte as i32));
        f.instruction(&Instruction::I32Store8(wasm_encoder::MemArg {
            offset: i as u64,
            align: 0,
            memory_index: 0,
        }));
    }

    // Copy loop: memory[ptr + 7 + i] = backing[i] for i in 0..byte_len
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(idx));

    f.instruction(&Instruction::Block(BlockType::Empty));
    f.instruction(&Instruction::Loop(BlockType::Empty));

    // if idx >= byte_len, break
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::LocalGet(byte_len));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::BrIf(1));

    // memory[ptr + 7 + idx] = backing[idx]
    f.instruction(&Instruction::LocalGet(ptr));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(7));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalGet(backing));
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::ArrayGetU(U8_BACKING_TYPE_INDEX));
    f.instruction(&Instruction::I32Store8(wasm_encoder::MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));

    // idx++
    f.instruction(&Instruction::LocalGet(idx));
    f.instruction(&Instruction::I32Const(1));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(idx));

    // continue
    f.instruction(&Instruction::Br(0));
    f.instruction(&Instruction::End); // end loop
    f.instruction(&Instruction::End); // end block

    // Store newline at ptr + 7 + byte_len
    f.instruction(&Instruction::LocalGet(ptr));
    f.instruction(&Instruction::LocalGet(byte_len));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(7));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(0x0A));
    f.instruction(&Instruction::I32Store8(wasm_encoder::MemArg {
        offset: 0,
        align: 0,
        memory_index: 0,
    }));

    // Flush "Panic: " + message + newline (7 + byte_len + 1 bytes at ptr)
    // to stderr, then trap.
    emit_flush_write(
        &mut f,
        &STDERR_TARGET,
        ptr,
        byte_len,
        8,
        evtbuf,
        wptr,
        wlen,
        tmp,
        pair,
    );

    // Trap
    f.instruction(&Instruction::Unreachable);

    f.instruction(&Instruction::End); // end function

    f
}
