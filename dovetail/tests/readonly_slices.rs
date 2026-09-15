mod common;

#[test]
fn readonly_views_ranges_aliases_and_copies() {
    common::compile_and_run(
        r#"
package a
function main(): Unit =
    let a = [|0, 1, 2, 3, 4|]
    let s = a[|1..4|]
    let r = s.readonly
    assert r.length == 3
    assert r[0] == 1
    assert a.readonly.length == 5
    assert r.readonly == r
    s[1] = 20
    assert r[1] == 20
    a[1] = 10
    assert r[0] == 10
    assert r[|1..|][0] == 20
    assert r[|..2|].length == 2
    assert r[|..|] == r
    assert r[|..=1|].length == 2
    assert r[|1..=2|][1] == 3
    assert r[|1..2|][0] == 20
    assert r[|3..|].isEmpty()
    assert r[|2..2|].isEmpty()
    let copied = r.toArray()
    copied[0] = 99
    assert r[0] == 10
    let fullCopy = a.readonly.toArray()
    fullCopy[0] = 99
    assert a[0] == 0
    let mapped = r.map(x => x + 1)
    assert mapped == [|11, 21, 4|].readonly
    a[1] = 5
    assert mapped[0] == 11
    let empty: Array<Int32> = Array.empty()
    assert empty.readonly.toArray().length == 0
    assert empty.readonly.map(x => x + 1).isEmpty()
    assert r.format() == "[5,20,3,]"
    let mutable sum = 0
    for n in r do
        sum = sum + n
    assert sum == 28
    let overlap = [|1, 2, 3, 4, 5|]
    overlap.readonly[|..4|].copyTo(overlap[|1..|])
    assert overlap.readonly == [|1, 1, 2, 3, 4|].readonly
    overlap.readonly[|1..|].copyTo(overlap[|..4|])
    assert overlap.readonly == [|1, 2, 3, 4, 4|].readonly
    let words = [|"left", "middle", "right"|].readonly[|1..2|]
    assert words.toArray()[0] == "middle"
    assert [|1.5, 2.5|].readonly[1] == 2.5
    let bytes: Array<Uint8> = [|1u8, 2u8, 255u8|]
    assert bytes.readonly[|1..|].toArray()[1] == 255u8
    assert bytes.readonly.map(x => x)[2] == 255u8
"#,
    )
    .unwrap();
}

#[test]
fn readonly_api_rejects_writes_and_private_representation_access() {
    for expression in [
        "r[0] = 2",
        "r[|..|][0] = 2",
        "r.set(0, 2)",
        "r.value",
        "ReadonlySlice((a, 0, 3))",
        "r[|..|] = r",
        "r[|true..|]",
    ] {
        let source = format!(
            "package a\nfunction main(): Unit =\n    let a = [|1, 2, 3|]\n    let r = a.readonly\n    let _ = {expression}\n    ()\n"
        );
        assert!(
            !common::compile_expecting_errors(&source).is_empty(),
            "{expression}"
        );
    }
}

#[test]
fn readonly_bounds_fail_instead_of_exposing_other_elements() {
    for expression in [
        "ReadonlySlice.make(a, -1, 1)",
        "ReadonlySlice.make(a, 0, -1)",
        "ReadonlySlice.make(a, 1, 2147483647)",
        "ReadonlySlice.make(a, 4, 0)",
        "r[-1]",
        "r[2]",
        "r[|0..3|]",
        "r[|..=2|]",
        "r.drop(-1)",
        "r.take(3)",
        "r.copyTo(a[|..1|])",
    ] {
        let source = format!(
            "package a\nfunction main(): Unit =\n    let a = [|1, 2, 3|]\n    let r = a.readonly[|1..|]\n    let _ = {expression}\n    ()\n"
        );
        common::compile_and_expect_trap(&source);
    }
}

#[test]
fn readonly_slice_dynamic_tests_and_patterns_validate_bounds() {
    common::compile_and_run(
        r#"
package a
function check(raw: Any, valid: Bool): Unit =
    assert (raw is ReadonlySlice<Int32>) == valid
    let mutable matched = false
    match raw with
        case s: ReadonlySlice<Int32> =>
            matched = true
            assert s.length == 0 || s[0] == 20
        case _ => ()
    assert matched == valid
function restore<T>(raw: Any): T = raw as T
function main(): Unit =
    let a = [|10, 20|]
    check(a.readonly[|1..|], true)
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
    let nested: Any = (99, a.readonly[|1..|])
    assert nested is (Int32, ReadonlySlice<Int32>)
    assert (nested as (Int32, ReadonlySlice<Int32>))._1[0] == 20
    let malformed: Any = (99, (a, -1, 2))
    assert !(malformed is (Int32, ReadonlySlice<Int32>))
    let restored: ReadonlySlice<Int32> = restore((a, 1, 1))
    assert restored[0] == 20
"#,
    )
    .expect("dynamic slice tests reject malformed storage without trapping");
}

#[test]
fn invalid_dynamic_readonly_slice_casts_trap() {
    for (raw, target) in [
        ("(a, -1, 2)", "ReadonlySlice<Int32>"),
        ("(a, 0, -1)", "ReadonlySlice<Int32>"),
        ("(a, 3, 0)", "ReadonlySlice<Int32>"),
        ("(a, 1, 2147483647)", "ReadonlySlice<Int32>"),
        ("(a, \"bad\", 1)", "ReadonlySlice<Int32>"),
        ("(99, (a, -1, 2))", "(Int32, ReadonlySlice<Int32>)"),
    ] {
        let source = format!(
            "package a\nfunction main(): Unit =\n    let a = [|10, 20|]\n    let raw: Any = {raw}\n    let _ = raw as {target}\n    ()\n"
        );
        common::compile_and_expect_trap(&source);
    }
}

#[test]
fn readonly_storage_and_range_evaluation() {
    common::compile_and_run(
        r#"
package a
record Saved = view: ReadonlySlice<Int32>
class Holder(public mutable view: ReadonlySlice<Int32>)
function identity<T>(value: T): T = value
function receiver(r: ReadonlySlice<Int32>, log: Array<Int32>): ReadonlySlice<Int32> =
    log[0] = log[0] * 10 + 1
    r
function bound(log: Array<Int32>, value: Int32): Int32 =
    log[0] = log[0] * 10 + value
    value
function main(): Unit =
    let r = [|10, 20, 30, 40|].readonly
    let log = [|0|]
    let sub = receiver(r, log)[|bound(log, 2)..bound(log, 3)|]
    assert log[0] == 123
    assert sub[0] == 30
    let saved = Saved { view = sub }
    let holder = Holder(r)
    holder.view = saved.view
    assert holder.view[0] == 30
    let erased: Option<ReadonlySlice<Int32>> = Some(identity(sub))
    assert erased.require[0] == 30
    let views = [|r, sub|]
    assert views[1][0] == 30
    let captured = () => sub
    assert captured()[0] == 30
    let mutable mutableView = r
    let indirect = () => mutableView
    mutableView = sub
    assert indirect()[0] == 30
    let raw: Any = sub
    assert (raw as ReadonlySlice<Int32>)[0] == 30
"#,
    )
    .unwrap();
}

#[test]
fn covariant_primitive_views_preserve_storage_and_box_identity() {
    let values = [
        ("Unit", "()"),
        ("Bool", "true"),
        ("Char", "'x'"),
        ("Int8", "-127i8"),
        ("Uint8", "255u8"),
        ("Int16", "-32767i16"),
        ("Uint16", "65535u16"),
        ("Int32", "-123"),
        ("Uint32", "4294967295u32"),
        ("Int64", "-123i64"),
        ("Uint64", "18446744073709551615u64"),
        ("Float32", "1.25f32"),
        ("Float64", "2.5"),
        ("Uint128", "123u128"),
    ];
    let mut source = String::from(
        r#"package a
function main(): Unit =
"#,
    );
    for (i, (ty, value)) in values.iter().enumerate() {
        source.push_str(&format!(
            r#"    let a{i} = Array.fill(3, {value})
    let r{i}: ReadonlySlice<Any> = a{i}.readonly[|1..3|]
    assert r{i}.length == 2
    assert r{i}[0] is {ty}
    let copy{i} = r{i}.toArray()
    assert copy{i}[0] is {ty}
    let erased{i}: Any = r{i}
    assert erased{i} is ReadonlySlice<{ty}>
    assert erased{i} is ReadonlySlice<Any>
    let raw{i}: Any = a{i}
"#
        ));
        if *ty != "Unit" {
            source.push_str(&format!(
                r#"    assert (r{i}[1] as {ty}) == {value}
    assert (copy{i}[1] as {ty}) == {value}
"#
            ));
        }
        for (other, _) in &values {
            if other != ty {
                source.push_str(&format!(
                    r#"    assert !(erased{i} is ReadonlySlice<{other}>)
    assert !(raw{i} is Array<{other}>)
"#
                ));
            }
        }
    }
    common::compile_and_run(&source).unwrap();
}

#[test]
fn covariant_views_alias_through_generic_and_closure_boundaries() {
    common::compile_and_run(
        r#"
package a
class Parent(public value: Int32)
class Child(value: Int32) extends Parent(value)
record Holder = view: ReadonlySlice<Any>
function identity<T>(value: T): T = value
function main(): Unit =
    let array = [|1u8, 2u8, 3u8|]
    let wide: ReadonlySlice<Any> = identity(array.readonly)
    let holder = Holder { view = wide }
    let reader = () => holder.view[1]
    array[1] = 255u8
    assert (reader() as Uint8) == 255u8
    let destination: Array<Any> = Array<Any>.fill(3, ())
    wide.copyTo(destination[|..|])
    assert (destination[1] as Uint8) == 255u8
    let children = [|Child(7)|]
    let parents: ReadonlySlice<Parent> = children.readonly
    assert parents[0].value == 7
    let pairs: ReadonlySlice<Any> = [|(1, 2)|].readonly
    assert (pairs[0] as (Int32, Int32))._1 == 2
"#,
    )
    .unwrap();
}

#[test]
fn intrinsic_declarations_and_writable_variance_are_restricted() {
    for declaration in [
        "type Unknown = intrinsic",
        "type Unknown<out T> = intrinsic",
        "type Alias<out T> = Array<T>",
    ] {
        let source = format!(
            r#"package a
{declaration}
function main(): Unit = ()
"#
        );
        assert!(!common::compile_expecting_errors(&source).is_empty());
    }
    for declaration in [
        "let result: Array<Any> = [|1|]",
        "let result: Slice<Any> = [|1|][|..|]",
        "let result: ReadonlySlice<Int32> = Array<Any>.fill(1, 1).readonly",
    ] {
        let source = format!(
            r#"package a
function main(): Unit =
    {declaration}
    ()
"#
        );
        assert!(!common::compile_expecting_errors(&source).is_empty());
    }
}

#[test]
fn packed_newtype_views_use_signed_and_unsigned_reads() {
    common::compile_and_run(
        r#"
package a
newtype SignedByte = Int8
newtype UnsignedByte = Uint8
newtype SignedWord = Int16
newtype UnsignedWord = Uint16
newtype Wrapped<T> = T
function main(): Unit =
    let signed = [|SignedByte(-127i8)|].readonly
    let unsigned = [|UnsignedByte(255u8)|].readonly
    let word = [|SignedWord(-32767i16)|].readonly
    let unsignedWord = [|UnsignedWord(65535u16)|].readonly
    assert signed[0].value == -127i8
    assert unsigned[0].value == 255u8
    assert word[0].value == -32767i16
    assert unsignedWord[0].value == 65535u16
    assert [|Wrapped(SignedByte(-1i8))|].readonly[0].value.value == -1i8
    assert signed.toArray()[0].value == -127i8
    assert unsigned.toArray()[0].value == 255u8
"#,
    )
    .unwrap();
}

#[test]
fn covariant_newtype_elements_use_erased_storage_dispatch() {
    common::compile_and_run(
        r#"
package a
newtype Wrapper<out T> = T
function main(): Unit =
    let source = [|Wrapper(12)|].readonly
    let wide: ReadonlySlice<Wrapper<Any>> = source
    assert (wide[0].value as Int32) == 12
    let erased: Any = wide
    assert erased is ReadonlySlice<Wrapper<Any>>
    let recovered = erased as ReadonlySlice<Wrapper<Any>>
    assert (recovered[0].value as Int32) == 12
    let copied = wide.toArray()
    assert (copied[0].value as Int32) == 12
    let nested: ReadonlySlice<Wrapper<Wrapper<Any>>> = [|Wrapper(Wrapper(255u8))|].readonly
    assert (nested[0].value.value as Uint8) == 255u8
    let nestedCopy = nested.toArray()
    assert (nestedCopy[0].value.value as Uint8) == 255u8
"#,
    )
    .unwrap();
}
