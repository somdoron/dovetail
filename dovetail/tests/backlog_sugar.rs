mod common;

#[test]
fn for_loop_resolves_an_iterable_type_parameter_bound() {
    common::compile_and_run(
        r#"
package a
function total<C>(values: C): Int32 where C: Iterable<Int32> =
    let mutable sum = 0
    for value in values do
        sum = sum + value
    sum
function main(): Unit = assert total([1, 2, 3]) == 6
"#,
    )
    .unwrap();
}

#[test]
fn try_resolves_a_bound_with_an_associated_failure_type() {
    common::compile_and_run(r#"
package a
function increment<C>(value: C): Option<Int32> where C: EarlyReturn<Int32, OnFailure = Option<Never>> =
    let number = try value
    Some(number + 1)
function main(): Unit =
    assert increment(Some(4)).or(0) == 5
    let absent: Option<Int32> = None
    assert increment(absent).isNone
"#).unwrap();
}

#[test]
fn bound_use_rejects_an_incompatible_associated_wrapper() {
    let errors = common::compile_expecting_errors(
        r#"
package a
record Resource = value: Int32
implement Usable<Int32, Never> for Resource =
    type Wrapped<U, E2> = List<U>
    function use<U, E2>(self, f: Int32 => List<U>, errorF: Never => E2): List<U> = f(self.value)
function increment<C>(holder: C): Int32 where C: Usable<Int32, Never> =
    let value = use holder
    value + 1
function main(): Unit = assert increment(Resource { value = 4 }) == 5
"#,
    );
    assert!(
        errors.iter().any(|message| message.contains(
            "generic use requires a continuation returning 'C.Wrapped<U, Never>', found 'Int32'"
        )),
        "{errors:?}"
    );
}

#[test]
fn for_loop_resolves_inherited_bounds_in_a_default_body() {
    common::compile_and_run(
        r#"
package a
trait Numbers extends Iterable<Int32> =
    function total(self): Int32 =
        let mutable sum = 0
        for value in self do
            sum = sum + value
        sum
record Values = items: List<Int32>
implement Numbers for Values =
    function iterator(self): Iterator<Int32> = self.items.iterator()
function main(): Unit = assert Values { items = [2, 3] }.total() == 5
"#,
    )
    .unwrap();
}

#[test]
fn for_loop_rejects_distinct_direct_and_inherited_element_types() {
    let errors = common::compile_expecting_errors(
        r#"
package a
trait TextValues extends Iterable<String>
function visit<C>(values: C): Unit where C: Iterable<Int32> + TextValues =
    for value in values do ()
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|message| message.contains("a for-loop needs a unique element type")),
        "{errors:?}"
    );
}

#[test]
fn any_preserves_the_boxed_interface_value() {
    common::compile_and_run(
        r#"
package a
interface Read = function read(self): Int32
record Rec = value: Int32
implement Read for Rec = function read(self): Int32 = self.value
function main(): Unit =
    let object: Read = Rec { value = 7 }
    let erased: Any = object
    assert !(erased is Rec)
    let concrete: Any = Rec { value = 7 }
    assert concrete is Rec
    assert (concrete as Rec).value == 7
"#,
    )
    .unwrap();
}
