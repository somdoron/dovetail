mod common;

fn check_err_contains(source: &str, needle: &str) {
    let result = dovetail::check(source, "test.dove");
    let messages: Vec<String> = result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect();
    let combined = messages.join("\n");
    assert!(
        combined.contains(needle),
        "expected error containing '{needle}'; got:\n{combined}"
    );
}

#[test]
fn test_div_overload_string_rhs() {
    common::compile_and_run(
        r#"
package a

record Path = depth: Int32

implement Div<String> for Path =
    type Output = Path
    function div(self: Path, _rhs: String): Path = Path { depth = self.depth + 1 }

function main(): Unit =
    let root = Path { depth = 0 }
    let sub = root / "logs"
    assert sub.depth == 1
    let sub2 = sub / "more"
    assert sub2.depth == 2
"#,
    )
    .expect("Div overload, RHS=String");
}

#[test]
fn test_div_overload_self_rhs() {
    common::compile_and_run(
        r#"
package a

record Path = depth: Int32

implement Div<Path> for Path =
    type Output = Path
    function div(self: Path, rhs: Path): Path = Path { depth = self.depth + rhs.depth }

function main(): Unit =
    let a = Path { depth = 2 }
    let b = Path { depth = 3 }
    let c = a / b
    assert c.depth == 5
"#,
    )
    .expect("Div overload, RHS=Self");
}

#[test]
fn test_path_like_div_overload_both() {
    // Two `Div` impls on the same type targeting different RHS types.
    // `p / "sub"` picks `Div<String> for Path`; `p / q` picks `Div<Path> for Path`.
    common::compile_and_run(
        r#"
package a

record Path = depth: Int32

implement Div<String> for Path =
    type Output = Path
    function div(self: Path, _rhs: String): Path = Path { depth = self.depth + 1 }

implement Div<Path> for Path =
    type Output = Path
    function div(self: Path, rhs: Path): Path = Path { depth = self.depth + rhs.depth }

function main(): Unit =
    let root = Path { depth = 0 }
    let sub = root / "logs"
    assert sub.depth == 1
    let joined = sub / Path { depth = 5 }
    assert joined.depth == 6
"#,
    )
    .expect("path-like Div overload");
}

#[test]
fn test_primitive_div_still_uses_native_codegen() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    assert 5 / 2 == 2
    assert 100i64 / 3i64 == 33i64
    assert 4.0f64 / 2.0f64 == 2.0f64
"#,
    )
    .expect("primitives unaffected by trait");
}

#[test]
fn test_div_with_no_impl_errors() {
    // Two distinct failure modes for `t / r` with no matching `Div<R> for T` impl:
    // (a) types differ and no impl unifies → "operands of the same type".
    // (b) types match but operand isn't numeric → "operator '/' is not supported".
    check_err_contains(
        r#"
package a

record Bare = x: Int32

function main(): Unit =
    let b = Bare { x = 1 }
    let _ = b / "s"
    ()
"#,
        "requires operands of the same type",
    );
    check_err_contains(
        r#"
package a

record Bare = x: Int32

function main(): Unit =
    let b = Bare { x = 1 }
    let _ = b / b
    ()
"#,
        "operator '/' is not supported",
    );
}

#[test]
fn test_div_associated_output_type() {
    // Output is distinct from Self.
    common::compile_and_run(
        r#"
package a

record Box = value: Int32

implement Div<Int32> for Box =
    type Output = Int32
    function div(self: Box, rhs: Int32): Int32 = self.value / rhs

function main(): Unit =
    let b = Box { value = 100 }
    let r: Int32 = b / 4
    assert r == 25
"#,
    )
    .expect("Div with Output != Self");
}

#[test]
fn test_div_trait_bound_on_generic() {
    // Generic function with `where T: Div<Int32>` — relies on the
    // TypeVariable/GenericParam trait-bound path in `try_lower_op_to_trait`.
    // The bound explicitly requires the result to have the receiver's type.
    common::compile_and_run(
        r#"
package a

record Wrap = inner: Int32

implement Div<Int32> for Wrap =
    type Output = Wrap
    function div(self: Wrap, rhs: Int32): Wrap = Wrap { inner = self.inner / rhs }

function half<T>(x: T): T where T: Div<Int32, Output = T> = x / 2

function main(): Unit =
    let w = Wrap { inner = 40 }
    let h = half(w)
    assert h.inner == 20
"#,
    )
    .expect("Div trait bound on generic");
}
