mod common;

#[test]
fn slice_views_and_copying() {
    common::compile_and_run(
        r#"
package a

function main(): Unit =
    let a = [|0, 1, 2, 3, 4|]
    let s = a[|1..4|]
    assert s.length == 3
    assert s[0] == 1
    s[1] = 20
    assert a[2] == 20
    a[1] = 10
    assert s[0] == 10
    assert s[|1..|][0] == 20
    assert s[|..2|].length == 2
    assert s[|..|] == s
    assert a[|..=2|].length == 3
    assert s[|1..=2|].length == 2
    assert a[|5..|].isEmpty()
    assert a[|2..2|].isEmpty()
    let empty: Array<Int32> = Array.empty()
    assert empty[|..|].toArray().length == 0
    let copied = s.toArray()
    copied[0] = 99
    assert s[0] == 10
    assert s == [|10, 20, 3|][|..|]
    assert s != [|10, 20|][|..|]
    assert s != [|10, 20, 4|][|..|]
    let words = [|"left", "middle", "right"|][|1..2|]
    assert words.toArray()[0] == "middle"
    let targets = [|"", ""|]
    words.copyTo(targets[|1..|])
    assert targets[1] == "middle"
    assert [|1.5, 2.5|][|1..|][0] == 2.5
    assert "abc"[1] == 'b'
    assert s.format() == "[10,20,3,]"
    let mutable total = 0
    for n in s do
        total = total + n
    assert total == 33
    let overlap = [|1, 2, 3, 4, 5|]
    overlap[|..4|].copyTo(overlap[|1..|])
    assert overlap[|..|] == [|1, 1, 2, 3, 4|][|..|]
    overlap[|1..|].copyTo(overlap[|..4|])
    assert overlap[|..|] == [|1, 2, 3, 4, 4|][|..|]
"#,
    )
    .expect("slice views and overlap-safe copying");
}

#[test]
fn slice_evaluation_order_and_storage_boundaries() {
    common::compile_and_run(
        r#"
package a

record Saved = view: Slice<Int32>
class Holder(public mutable view: Slice<Int32>)

function source(events: Array<Int32>): Array<Int32> =
    events[0] = events[0] * 10 + 1
    [|10, 20, 30|]

function bound(events: Array<Int32>, n: Int32): Int32 =
    events[0] = events[0] * 10 + n
    n - 2

function peel(s: Slice<Int32>): Slice<Int32> = s[|1..|]
function identity<T>(value: T): T = value

function main(): Unit =
    let events = [|0|]
    let s = source(events)[|bound(events, 2)..=bound(events, 3)|]
    assert events[0] == 123
    assert s.length == 2
    assert peel(s)[0] == 20
    let saved = Saved { view = s }
    assert saved.view[1] == 20
    let holder = Holder(s)
    holder.view = peel(s)
    assert holder.view[0] == 20
    let erased: Option<Slice<Int32>> = Some(identity(s))
    match erased with
        case Some(view) => assert view[0] == 10
        case None => panic "missing slice"
    let f: () => Int32 = () => s[1]
    assert f() == 20
    let views = [|s|]
    assert views[0][1] == 20
"#,
    )
    .expect("slice evaluation and boxing boundaries");
}

#[test]
fn invalid_slice_bounds_panic() {
    for expression in [
        "Slice.make(a, -1, 1)",
        "Slice.make(a, 0, -1)",
        "Slice.make(a, 1, 2147483647)",
        "a[|-1..|]",
        "a[|2..1|]",
        "a[|..4|]",
        "a[|..=2147483647|]",
        "a[|..=3|]",
        "a[|..2|][2]",
        "a[|..2|][-1]",
        "a[|..1|][1] = 9",
        "a[|..|].copyTo(a[|..1|])",
        "a[|..1|][|..2|]",
    ] {
        let source = format!(
            "package a\nfunction main(): Unit =\n    let a = [|1, 2, 3|]\n    let _ = {expression}\n    ()\n"
        );
        common::compile_and_expect_trap(&source);
    }
}

#[test]
fn slice_syntax_and_privacy_diagnostics() {
    for expression in [
        "a[|1|]",
        "a[1..2]",
        "a[|1..=|]",
        "1..3",
        "\"abc\"[|..|]",
        "a[|true..|]",
        "a[|..|].value",
        "Slice((a, 0, 3))",
        "a[|..|] = a[|..|]",
    ] {
        let source = format!(
            "package a\nfunction main(): Unit =\n    let a = [|1, 2, 3|]\n    let _ = {expression}\n    ()\n"
        );
        assert!(
            !common::compile_expecting_errors(&source).is_empty(),
            "{expression}"
        );
    }
}

#[test]
fn slice_any_storage_roundtrip() {
    common::compile_and_run(
        r#"
package a
record Saved = value: Any
function erase(value: Slice<Int32>): Any = value
function main(): Unit =
    let view = [|1, 2, 3|][|..|]
    let mutable stored: Any = view
    assert (stored as Slice<Int32>).length == 3
    stored = view[|1..|]
    assert (stored as Slice<Int32>)[0] == 2
    let saved = Saved { value = view }
    assert (saved.value as Slice<Int32>)[2] == 3
    assert (erase(view) as Slice<Int32>)[1] == 2
    let boxed: Any = view
    let values: Array<Any> = [|boxed|]
    assert (values[0] as Slice<Int32>).length == 3
    let f: () => Any = () => view
    assert (f() as Slice<Int32>)[0] == 1
    let pair: Any = (10, 20)
    assert (pair as (Int32, Int32))._1 == 20
"#,
    )
    .expect("slices and tuples box once when stored as Any");
}

#[test]
fn slice_interface_dispatch_and_self_return() {
    common::compile_and_run(
        r#"
package a
interface View =
    function size(self): Int32
    function dropFirst(self): Self
implement View for Slice<Int32> =
    public function size(self): Int32 = self.length
    public function dropFirst(self): Slice<Int32> = self[|1..|]
function measure(value: View): Int32 = value.size()
function main(): Unit =
    let slice = [|1, 2, 3|][|..|]
    assert measure(slice) == 3
    let view: View = slice
    assert view.dropFirst().size() == 2
"#,
    )
    .expect("slice receivers box across interface dispatch and Self returns");
}

#[test]
fn slice_field_pattern_binding() {
    common::compile_and_run(
        r#"
package a
record Wrap<T> = view: Slice<T>
function main(): Unit =
    let wrapped = Wrap { view = [|10, 20|][|..|] }
    match wrapped with
        case Wrap { view = s } => assert s[1] == 20
"#,
    )
    .expect("record patterns restore a slice field's concrete array type");
}

#[test]
fn slice_dynamic_tests_and_patterns_validate_bounds() {
    common::compile_and_run(
        r#"
package a
function check(raw: Any, valid: Bool): Unit =
    assert (raw is Slice<Int32>) == valid
    let mutable matched = false
    match raw with
        case s: Slice<Int32> =>
            matched = true
            assert s.length == 0 || s[0] == 20
        case _ => ()
    assert matched == valid
function restore<T>(raw: Any): T = raw as T
function main(): Unit =
    let a = [|10, 20|]
    check(a[|1..|], true)
    check((a, 1, 1), true)
    check((a, 2, 0), true)
    check((a, -1, 2), false)
    check((a, 0, -1), false)
    check((a, 3, 0), false)
    check((a, 1, 2147483647), false)
    check((a, 2147483647, 2147483647), false)
    check((a, "bad", 1), false)
    check(("bad", 0, 1), false)
    check((a, 0), false)
    check(42, false)
    let nested: Any = (99, a[|1..|])
    assert nested is (Int32, Slice<Int32>)
    assert (nested as (Int32, Slice<Int32>))._1[0] == 20
    let malformed: Any = (99, (a, -1, 2))
    assert !(malformed is (Int32, Slice<Int32>))
    let restored: Slice<Int32> = restore((a, 1, 1))
    assert restored[0] == 20
"#,
    )
    .expect("dynamic slice tests reject malformed storage without trapping");
}

#[test]
fn invalid_dynamic_slice_casts_trap() {
    for (raw, target) in [
        ("(a, -1, 2)", "Slice<Int32>"),
        ("(a, 0, -1)", "Slice<Int32>"),
        ("(a, 3, 0)", "Slice<Int32>"),
        ("(a, 1, 2147483647)", "Slice<Int32>"),
        ("(a, \"bad\", 1)", "Slice<Int32>"),
        ("(99, (a, -1, 2))", "(Int32, Slice<Int32>)"),
    ] {
        let source = format!(
            "package a\nfunction main(): Unit =\n    let a = [|10, 20|]\n    let raw: Any = {raw}\n    let _ = raw as {target}\n    ()\n"
        );
        common::compile_and_expect_trap(&source);
    }
}
