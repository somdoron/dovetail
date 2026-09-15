mod common;

// ── Empty array literal with expected type ──────────────────────────────

#[test]
fn test_empty_array_with_type_annotation() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Array<Int32> = [||]
    assert x.length == 0
"#,
    )
    .expect("empty array with type annotation");
}

#[test]
fn test_empty_array_without_annotation_still_errors() {
    let errors = common::compile_expecting_errors(
        r#"
package a

function main(): Unit =
    let x = [||]
    assert x.length == 0
"#,
    );
    assert!(
        errors
            .iter()
            .any(|e| e.contains("cannot infer element type for empty array literal")),
        "expected error about empty array, got: {:?}",
        errors
    );
}

// ── If branches ─────────────────────────────────────────────────────────

#[test]
fn test_if_branch_empty_array() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Array<Int32> = if true then [||] else [|1|]
    assert x.length == 0
"#,
    )
    .expect("if branch with empty array");
}

#[test]
fn test_if_else_branch_empty_array() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Array<Int32> = if false then [|1, 2|] else [||]
    assert x.length == 0
"#,
    )
    .expect("if else branch with empty array");
}

// ── Match arms ──────────────────────────────────────────────────────────

#[test]
fn test_match_arm_empty_array() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Array<Int32> =
        match 1 with
            case 1 => [||]
            case _ => [|42|]
    assert x.length == 0
"#,
    )
    .expect("match arm with empty array");
}

#[test]
fn test_match_arm_empty_array_non_first() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Array<Int32> =
        match 2 with
            case 1 => [|10|]
            case _ => [||]
    assert x.length == 0
"#,
    )
    .expect("match arm empty array non-first");
}

// ── Block propagation ───────────────────────────────────────────────────

#[test]
fn test_block_propagates_expected_type() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Array<Int32> =
        let y = 42
        [||]
    assert x.length == 0
"#,
    )
    .expect("block propagates expected type to last expr");
}

// ── Assignment ──────────────────────────────────────────────────────────

#[test]
fn test_assignment_propagates_expected_type() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let mutable x: Array<Int32> = [|1, 2, 3|]
    x = [||]
    assert x.length == 0
"#,
    )
    .expect("assignment propagates expected type");
}

// ── Record fields ───────────────────────────────────────────────────────

#[test]
fn test_record_field_empty_array() {
    common::compile_and_run(
        r#"
package a

record Holder =
    items: Array<Int32>

function main(): Unit =
    let h = Holder { items = [||] }
    assert h.items.length == 0
"#,
    )
    .expect("record field with empty array");
}

// ── Record with ─────────────────────────────────────────────────────────

#[test]
fn test_record_with_empty_array() {
    common::compile_and_run(
        r#"
package a

record Holder =
    items: Array<Int32>

function main(): Unit =
    let h = Holder { items = [|1, 2, 3|] }
    let h2 = h with items = [||]
    assert h2.items.length == 0
"#,
    )
    .expect("record with empty array field");
}

// ── Generic function with expected-type fallback ────────────────────────

#[test]
fn test_generic_function_expected_type_fallback() {
    common::compile_and_run(
        r#"
package a

function identity<T>(x: T): T = x

function main(): Unit =
    let x: Array<Int32> = identity(Array<Int32>.empty())
    assert x.length == 0
"#,
    )
    .expect("generic function with expected type fallback");
}

#[test]
fn test_generic_function_return_type_inference() {
    common::compile_and_run(
        r#"
package a

function wrap<T>(x: T): Array<T> = [|x|]

function main(): Unit =
    let x: Array<Int32> = wrap(42)
    assert x.length == 1
    assert x[0] == 42
"#,
    )
    .expect("generic function return type inference");
}

// ── Nested bidirectional typing ─────────────────────────────────────────

#[test]
fn test_nested_if_in_block_empty_array() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let x: Array<Int32> =
        let flag = true
        if flag then [||] else [|1|]
    assert x.length == 0
"#,
    )
    .expect("nested if in block with empty array");
}

#[test]
fn test_generic_record_field_empty_array() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

function main(): Unit =
    let b = Box<Array<Int32>> { value = [||] }
    assert b.value.length == 0
"#,
    )
    .expect("generic record field with empty array");
}

// ── Generic record with — field type propagation ────────────────────────

#[test]
fn test_generic_record_with_empty_array() {
    common::compile_and_run(
        r#"
package a

record Box<T> =
    value: T

function main(): Unit =
    let b = Box<Array<Int32>> { value = [|1, 2, 3|] }
    let b2 = b with value = [||]
    assert b2.value.length == 0
"#,
    )
    .expect("generic record with empty array field");
}
