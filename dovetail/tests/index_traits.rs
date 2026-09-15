mod common;

#[test]
fn indexing_traits_support_generic_bounds_and_independent_capabilities() {
    common::compile_and_run(r#"
package a
newtype Lookup = Array<Int32>
implement Index<String> for Lookup =
    type Output = Int32
    function get(self: Lookup, key: String): Int32 = if key == "first" then self.value[0] else self.value[1]
newtype Writer = Array<Int32>
implement IndexSet<String> for Writer =
    type Value = Int32
    function set(self: Writer, key: String, value: Int32): Unit = self.value[if key == "first" then 0 else 1] = value
function read<C, K, O>(c: C, k: K): O where C: Index<K, Output = O> = c[k]
function write<C, K, V>(c: C, k: K, v: V): Unit where C: IndexSet<K, Value = V> = c[k] = v
function forward<C, K, O>(c: C, k: K): O where C: Index<K, Output = O> = read(c, k)
function main(): Unit =
    let a = [|10, 20|]
    let s = a[|..|]
    write(s, 1, 30)
    assert read(s, 1) == 30
    assert forward(s.readonly, 1) == 30
    let lookup = Lookup(a)
    let writer = Writer(a)
    writer["first"] = 40
    assert lookup["first"] == 40
    write(writer, "second", 50)
    assert read(lookup, "second") == 50
    assert Index.get(s, 0) == 40
    IndexSet.set(s, 0, 60)
    assert s.get(0) == 60
    s.set(0, 70)
    assert s[0] == 70
    assert "abc"[1] == 'b'
"#).unwrap();
}

#[test]
fn indexing_evaluates_receiver_key_and_value_once_in_order() {
    common::compile_and_run(
        r#"
package a
function receiver(s: Slice<Int32>, log: Array<Int32>): Slice<Int32> =
    log[0] = log[0] * 10 + 1
    s
function key(log: Array<Int32>): Int32 =
    log[0] = log[0] * 10 + 2
    0
function value(log: Array<Int32>): Int32 =
    log[0] = log[0] * 10 + 3
    99
function main(): Unit =
    let s = [|1|][|..|]
    let log = [|0|]
    receiver(s, log)[key(log)] = value(log)
    assert log[0] == 123
    log[0] = 0
    assert receiver(s, log)[key(log)] == 99
    assert log[0] == 12
"#,
    )
    .unwrap();
}

#[test]
fn indexing_requires_traits_and_correct_associated_types() {
    for (source, expected) in [
        ("function main(): Unit = [|1|].readonly[0] = 2", "IndexSet"),
        (
            "function main(): Unit = [|1|][|..|][0] = true",
            "type mismatch",
        ),
        (
            "function main(): Unit =\n    let _ = [|1|].readonly[true]\n    ()",
            "Index<Bool>",
        ),
        (
            "function read<C>(c: C): Int32 where C: Index<Int32> = c[0]\nfunction main(): Unit = ()",
            "associated type constraint",
        ),
        (
            "function write<C>(c: C): Unit where C: IndexSet<Int32> = c[0] = 1\nfunction main(): Unit = ()",
            "associated type constraint",
        ),
        (
            "newtype Plain = Int32\nmodule Plain =\n    function get(self, i: Int32): Int32 = self.value\nfunction main(): Unit =\n    let _ = Plain(1)[0]\n    ()",
            "Index<Int32>",
        ),
        (
            "newtype OnlyRead = Int32\nimplement Index<Int32> for OnlyRead =\n    type Output = Int32\n    function get(self: OnlyRead, i: Int32): Int32 = self.value\nfunction main(): Unit = OnlyRead(1)[0] = 2",
            "IndexSet",
        ),
    ] {
        let errors = common::compile_expecting_errors(&format!("package a\n{source}\n"));
        assert!(
            errors.iter().any(|m| m.contains(expected)),
            "expected {expected}: {errors:?}"
        );
    }
}

#[test]
fn overlapping_index_implementations_cannot_be_selected_by_result_or_value() {
    for (trait_name, associated, method, expression) in [
        (
            "Index",
            "Output",
            "function get(self: Box<T>, i: Int32): Int32 = 1",
            "let n: Int32 = box[0]",
        ),
        (
            "IndexSet",
            "Value",
            "function set(self: Box<T>, i: Int32, v: Int32): Unit = ()",
            "box[0] = 1",
        ),
    ] {
        let source = format!(
            "package a\nnewtype Box<T> = T\nimplement <T> {trait_name}<Int32> for Box<T> =\n    type {associated} = Int32\n    {method}\nimplement {trait_name}<Int32> for Box<Int32> =\n    type {associated} = String\n    {}\nfunction main(): Unit =\n    let box = Box(1)\n    {expression}\n    ()\n",
            if trait_name == "Index" {
                "function get(self: Box<Int32>, i: Int32): String = \"x\""
            } else {
                "function set(self: Box<Int32>, i: Int32, v: String): Unit = ()"
            }
        );
        let errors = common::compile_expecting_errors(&source);
        assert!(
            errors
                .iter()
                .any(|m| m.contains("ambiguous index operator")),
            "{errors:?}"
        );
    }
}

#[test]
fn index_outputs_preserve_tuples_and_multiple_key_implementations() {
    common::compile_and_run(
        r#"
package a
newtype Table<T> = Array<T>
implement <T> Index<Int32> for Table<T> =
    type Output = (T, Int32)
    function get(self: Table<T>, key: Int32): (T, Int32) = (self.value[key], key)
implement <T> Index<String> for Table<T> =
    type Output = T
    function get(self: Table<T>, key: String): T = self.value[0]
function read<C, K, O>(c: C, k: K): O where C: Index<K, Output = O> = c[k]
function main(): Unit =
    let table = Table([|"first", "second"|])
    assert table[1]._0 == "second"
    assert table[1]._1 == 1
    assert table["name"] == "first"
    assert read(table, 1)._0 == "second"
    assert read(table, "name") == "first"
    let slices = [|[|1, 2|].readonly, [|3, 4|].readonly|][|..|]
    assert read(slices, 1)[0] == 3
"#,
    )
    .unwrap();
}

#[test]
fn bottom_typed_keys_are_valid_for_slice_reads_and_writes() {
    let source = r#"
package a
function stop(): Never = panic "stop"
function read(s: Slice<Int32>): Int32 = s[stop()]
function readOnly(s: ReadonlySlice<Int32>): Int32 = s[stop()]
function write(s: Slice<Int32>): Unit = s[stop()] = 1
function genericRead<C>(c: C): Int32 where C: Index<Int32, Output = Int32> = c[stop()]
function genericWrite<C>(c: C): Unit where C: IndexSet<Int32, Value = Int32> = c[stop()] = 1
function main(): Unit = ()
"#;
    let result = dovetail::check(source, "test.dove");
    let errors: Vec<_> = result.diagnostics.iter().map(|d| &d.message).collect();
    assert!(!result.diagnostics.has_errors(), "{errors:?}");
}

#[test]
fn indexing_accepts_keys_assignable_to_the_declared_key_type() {
    common::compile_and_run(r#"
package a
newtype Store = Array<Int32>
implement Index<Any> for Store =
    type Output = Int32
    function get(self: Store, key: Any): Int32 = self.value[if key is String then 0 else 1]
implement IndexSet<Any> for Store =
    type Value = Int32
    function set(self: Store, key: Any, value: Int32): Unit = self.value[if key is String then 0 else 1] = value
function read<C>(c: C, key: String): Int32 where C: Index<Any, Output = Int32> = c[key]
function write<C>(c: C, key: Int32, value: Int32): Unit where C: IndexSet<Any, Value = Int32> = c[key] = value
function main(): Unit =
    let store = Store([|10, 20|])
    assert store["name"] == 10
    store[42] = 30
    assert store[42] == 30
    assert read(store, "name") == 10
    write(store, 42, 40)
    assert store[42] == 40
"#).unwrap();
}

#[test]
fn interface_and_superclass_keys_use_normal_argument_coercions() {
    common::compile_and_run(r#"
package a
interface Key =
    function position(self: Self): Int32
newtype NamedKey = Int32
implement Key for NamedKey =
    function position(self: NamedKey): Int32 = self.value
class BaseKey(public index: Int32)
class ChildKey(index: Int32) extends BaseKey(index)
newtype Store<T> = Array<T>
implement <T> Index<Key> for Store<T> =
    type Output = T
    function get(self: Store<T>, key: Key): T = self.value[key.position()]
implement <T> IndexSet<BaseKey> for Store<T> =
    type Value = T
    function set(self: Store<T>, key: BaseKey, value: T): Unit = self.value[key.index] = value
function read<C>(c: C, key: NamedKey): Int32 where C: Index<Key, Output = Int32> = c[key]
function write<C>(c: C, key: ChildKey, value: Int32): Unit where C: IndexSet<BaseKey, Value = Int32> = c[key] = value
function main(): Unit =
    let store = Store([|10, 20|])
    assert store[NamedKey(1)] == 20
    store[ChildKey(1)] = 30
    assert read(store, NamedKey(1)) == 30
    write(store, ChildKey(0), 40)
    assert store[NamedKey(0)] == 40
"#).unwrap();
}

#[test]
fn bottom_keys_do_not_disambiguate_multiple_key_capabilities() {
    let errors = common::compile_expecting_errors(
        r#"
package a
function stop(): Never = panic "stop"
function read<C>(c: C): Int32 where C: Index<Int32, Output = Int32> + Index<String, Output = Int32> = c[stop()]
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("ambiguous index operator")),
        "{errors:?}"
    );
}

#[test]
fn generic_index_bounds_require_the_exact_declared_application() {
    for (trait_name, associated, method, use_bound) in [
        (
            "Index",
            "Output",
            "function get(self: Store, key: Any): Int32 = self.value[0]",
            "function useBound<C>(c: C): Int32 where C: Index<String, Output = Int32> = c[\"x\"]",
        ),
        (
            "IndexSet",
            "Value",
            "function set(self: Store, key: Any, value: Int32): Unit = self.value[0] = value",
            "function useBound<C>(c: C): Unit where C: IndexSet<String, Value = Int32> = c[\"x\"] = 1",
        ),
    ] {
        let source = format!(
            "package a\nnewtype Store = Array<Int32>\nimplement {trait_name}<Any> for Store =\n    type {associated} = Int32\n    {method}\n{use_bound}\nfunction main(): Unit =\n    let _ = useBound(Store([|1|]))\n    ()\n"
        );
        let result = dovetail::check(&source, "test.dove");
        let errors: Vec<_> = result.diagnostics.iter().map(|d| &d.message).collect();
        assert!(
            result.diagnostics.has_errors(),
            "expected incompatible {trait_name} application: {errors:?}"
        );
    }
}

#[test]
fn exact_index_bound_ignores_other_assignable_key_applications() {
    common::compile_and_run(
        r#"
package a
newtype Store = Int32
implement Index<Any> for Store =
    type Output = Int32
    function get(self: Store, key: Any): Int32 = 1
implement Index<String> for Store =
    type Output = Int32
    function get(self: Store, key: String): Int32 = 2
function read<C>(c: C): Int32 where C: Index<String, Output = Int32> = c["x"]
function main(): Unit = assert read(Store(0)) == 2
"#,
    )
    .unwrap();
}
