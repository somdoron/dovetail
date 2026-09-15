mod common;

#[test]
fn generic_impl_sibling_interface_applications_keep_distinct_vtables() {
    common::compile_and_run(
        r#"
package a

record Rec<U> =
    x: Int32

interface Alpha<T> =
    function get(self): T
    function dup(self): Self

implement <U> Alpha<Int32> for Rec<U> =
    function get(self): Int32 = self.x
    function dup(self): Rec<U> = self

implement <V> Alpha<String> for Rec<V> =
    function get(self): String = "str"
    function dup(self): Rec<V> = self

function main(): Unit =
    let r = Rec<Int32> { x = 9 }
    let asx: Alpha<String> = r
    let aix: Alpha<Int32> = r
    assert asx.get() == "str"
    assert aix.get() == 9
    assert asx.dup().get() == "str"
    assert aix.dup().get() == 9
"#,
    )
    .expect("generic sibling applications retain their own implementation");
}

#[test]
fn sibling_super_applications_rebox_through_their_direct_implementations() {
    common::compile_and_run(
        r#"package a
record Rec =
    x: Int32
interface Alpha<T> =
    function get(self): T
    function dup(self): Self
interface Beta<T> extends Alpha<T> =
    function extra(self): Int32
implement Alpha<Int32> for Rec =
    function get(self): Int32 = 1
    function dup(self): Rec = self
implement Alpha<String> for Rec =
    function get(self): String = "direct"
    function dup(self): Rec = self
implement Beta<Int32> for Rec =
    function get(self): Int32 = 2
    function dup(self): Rec = self
    function extra(self): Int32 = 0
implement Beta<String> for Rec =
    function get(self): String = "via"
    function dup(self): Rec = self
    function extra(self): Int32 = 0
function main(): Unit =
    let r = Rec { x = 0 }
    let bi: Beta<Int32> = r
    let bs: Beta<String> = r
    assert bi.dup().get() == 1
    assert bs.dup().get() == "direct"
"#,
    )
    .expect("each inherited Self return selects its corresponding direct application");
}

#[test]
fn bound_calls_preserve_the_selected_intersection_component() {
    common::compile_and_run(
        r#"package a
record Rec =
    item: Int32
interface Conv<U> =
    function tag(self): Int32
interface Alpha extends Conv<Int32> =
    function alpha(self): Int32
interface Beta extends Conv<String> =
    function beta(self): Int32
implement Alpha for Rec =
    function tag(self): Int32 = 10
    function alpha(self): Int32 = 1
implement Beta for Rec =
    function tag(self): Int32 = 20
    function beta(self): Int32 = 2
function call<T>(v: T): Int32 where T: Beta = v.tag()
function main(): Unit =
    let v: Alpha and Beta = Rec { item = 0 }
    assert call(v) == 20
"#,
    )
    .expect("bound dispatch uses its applicable implementation");
}

#[test]
fn generic_direct_application_wins_during_monomorphization() {
    common::compile_and_run(
        r#"package a
record Wrap<T> =
    item: T
trait Alpha<U> =
    function alpha(self): Int32
trait Beta<U> extends Alpha<U> =
    function beta(self): Int32
implement <T> Alpha<T> for Wrap<T> =
    function alpha(self): Int32 = 10
implement <T> Beta<T> for Wrap<T> =
    function alpha(self): Int32 = 20
    function beta(self): Int32 = 2
function bound<T>(v: T): Int32 where T: Alpha<Int32> = v.alpha()
function main(): Unit =
    let w = Wrap<Int32> { item = 1 }
    assert bound(w) == 10
    assert Alpha<Int32>.alpha(w) == 10
"#,
    )
    .expect("bound dispatch uses its applicable implementation");
}

#[test]
fn inapplicable_direct_impl_does_not_own_a_bound_call() {
    common::compile_and_run(
        r#"
package a
record Wrap<T> =
    item: T
trait Required =
    function required(self): Int32
trait Alpha =
    function alpha(self): Int32
trait Beta extends Alpha =
    function beta(self): Int32
implement <T> Alpha for Wrap<T> where T: Required =
    function alpha(self): Int32 = 1
implement <T> Beta for Wrap<T> =
    function alpha(self): Int32 = 2
    function beta(self): Int32 = 3
function call<T>(v: T): Int32 where T: Alpha = v.alpha()
function main(): Unit =
    let w = Wrap<Int32> { item = 1 }
    assert call(w) == 2
"#,
    )
    .expect("bound dispatch uses its applicable implementation");
}
