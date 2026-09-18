mod common;

#[test]
fn eager_arguments_execute_before_later_awaits() {
    common::compile_and_run_async(
        r#"
package a
function subtract(left: Int32, right: Int32): Int32 = left - right
async function calculate(next: () => Int32): Async<Int32, Never> =
    subtract(right = next(), left = await Async.Succeed(next()))
function main(): Unit =
    let mutable count = 0
    let next: () => Int32 = () =>
        count = count + 1
        count
    match calculate(next = next).evaluate() with
    case Async.Succeed(value) => assert value == 1 && count == 2
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "unexpected failure"
"#,
    )
    .expect("an eager argument precedes a later written await");
}

#[test]
fn awaited_virtual_receivers_execute_once() {
    common::compile_and_run_async(
        r#"
package a
class Counter(public number: Int32) =
    public function add(self: Counter, amount: Int32): Int32 = self.number + amount
async function calculate(make: () => Counter): Async<Int32, Never> =
    (await Async.Succeed(make())).add(amount = 3)
function main(): Unit =
    let mutable count = 0
    let make: () => Counter = () =>
        count = count + 1
        Counter(count)
    match calculate(make = make).evaluate() with
    case Async.Succeed(value) => assert value == 4 && count == 1
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "unexpected failure"
"#,
    )
    .expect("a virtual receiver containing await executes once");
}

#[test]
fn named_await_calls_preserve_receivers_nested_effects_and_lazy_arguments() {
    common::compile_and_run_async(
        r#"
package a
class Counter(public number: Int32) =
    public function subtract(self: Counter, amount: Int32): Int32 = amount - self.number
function choose(fallback: ByName<Int32>, value: Int32): Int32 = value
function fail(): Int32 = panic "lazy argument evaluated"
function identity(value: Int32): Int32 = value
async function calculate(next: () => Int32): Async<Int32, Never> =
    identity(value = Counter(next()).subtract(amount = await Async.Succeed(next()))) +
        choose(fallback = fail(), value = await Async.Succeed(10))
function main(): Unit =
    let mutable count = 0
    let next: () => Int32 = () =>
        count = count + 1
        count
    match calculate(next = next).evaluate() with
    case Async.Succeed(value) => assert value == 11 && count == 2
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "unexpected failure"
"#,
    )
    .expect(
        "named calls preserve receiver order, nested effects, and ByName laziness around await",
    );
}

#[test]
fn explicit_interface_await_calls_preserve_receiver_and_argument_order() {
    common::compile_and_run_async(r#"
package a
interface Subtractor =
    function subtract(self, left: Int32, right: Int32): Int32
record Worker = number: Int32
implement Subtractor for Worker =
    function subtract(self, left: Int32, right: Int32): Int32 = left - right + self.number
async function calculate(make: () => Subtractor, next: () => Int32): Async<Int32, Never> =
    Subtractor.subtract(await Async.Succeed(make()), right = next(), left = await Async.Succeed(next()))
function main(): Unit =
    let mutable count = 0
    let make: () => Subtractor = () =>
        count = count + 1
        Worker { number = count }
    let next: () => Int32 = () =>
        count = count + 1
        count
    match calculate(next = next, make = make).evaluate() with
    case Async.Succeed(value) => assert value == 2 && count == 3
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "unexpected failure"
"#).expect("explicit interface receiver and arguments preserve written order around awaits");
}

#[test]
fn generic_lazy_arguments_remain_deferred_around_await() {
    common::compile_and_run_async(
        r#"
package a
function choose<T>(fallback: ByName<T>, value: T): T = value
function fail(): String = panic "lazy generic argument evaluated"
async function calculate(): Async<String, Never> =
    choose(fallback = fail(), value = await Async.Succeed("ready"))
function main(): Unit =
    match calculate().evaluate() with
    case Async.Succeed(value) => assert value == "ready"
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "unexpected failure"
"#,
    )
    .expect("generic ByName parameters remain deferred when another argument awaits");
}

#[test]
fn lazy_arguments_are_forced_only_by_the_callee_after_await() {
    common::compile_and_run_async(
        r#"
package a
function force<T>(first: ByName<T>, second: Int32): T = first.get
async function calculate(next: () => Int32): Async<Int32, Never> =
    force(first = next(), second = await Async.Succeed(next()))
function main(): Unit =
    let mutable count = 0
    let next: () => Int32 = () =>
        count = count + 1
        count
    match calculate(next = next).evaluate() with
    case Async.Succeed(value) => assert value == 2 && count == 2
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "unexpected failure"
"#,
    )
    .expect("lazy source operands are forced after the awaited eager operand");
}

#[test]
fn synchronous_virtual_named_calls_preserve_lazy_arguments() {
    common::compile_and_run(
        r#"
package a
function fail(): Int32 = panic "lazy virtual argument evaluated"
class Choice() =
    public function choose(self: Choice, fallback: ByName<Int32>, value: Int32): Int32 = value
    public function force<T>(self: Choice, value: ByName<T>): T = value.get
function main(): Unit =
    assert Choice().choose(fallback = fail(), value = 7) == 7
    let mutable count = 0
    let next: () => Int32 = () =>
        count = count + 1
        count
    assert Choice().force(value = next()) == 1
    assert count == 1
"#,
    )
    .expect("named virtual arguments remain lazy and can be forced by the method");
}

#[test]
fn synchronous_named_constructors_preserve_lazy_arguments() {
    common::compile_and_run(
        r#"
package a
function fail(): Int32 = panic "lazy constructor argument evaluated"
class Choice(public fallback: ByName<Int32>, public value: Int32)
class GenericChoice<T>(public value: ByName<T>)
function main(): Unit =
    assert Choice(fallback = fail(), value = 7).value == 7
    let mutable count = 0
    let next: () => Int32 = () =>
        count = count + 1
        count
    let choice = GenericChoice(value = next())
    assert count == 0
    assert choice.value.get == 1
    assert count == 1
"#,
    )
    .expect("named constructor arguments remain lazy until explicitly forced");
}

#[test]
fn await_inside_an_implicit_lazy_argument_is_diagnosed() {
    let errors = common::compile_expecting_errors_async(
        r#"
package a
function choose(fallback: ByName<Int32>, value: Int32): Int32 = value
async function calculate(): Async<Int32, Never> =
    choose(fallback = await Async.Succeed(1), value = 7)
function main(): Unit = ()
"#,
    );
    assert!(
        errors
            .iter()
            .any(|error| error.contains("await is not allowed in a deferred ByName argument")),
        "{errors:?}"
    );
}

#[test]
fn lazy_arguments_can_contain_explicit_async_scopes() {
    common::compile_and_run_async(
        r#"
package a
function force(value: ByName<Async<Int32, Never>>): Async<Int32, Never> = value.get
function main(): Unit =
    let result = force(value = async do
        await Async.Succeed(7)
    )
    match result.evaluate() with
    case Async.Succeed(value) => assert value == 7
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "unexpected failure"
"#,
    )
    .expect("explicit async scopes inside deferred arguments remain valid");
}

#[test]
fn explicit_generic_lazy_parameter_types_are_preserved() {
    common::compile_and_run_async(
        r#"
package a
function force<T>(value: ByName<T>, ignored: Int32): T = value.get
async function calculate(): Async<Any, Never> =
    force<Any>(value = 7, ignored = await Async.Succeed(0))
function main(): Unit =
    assert force<Any>(value = 7, ignored = 0) is Int32
    match calculate().evaluate() with
    case Async.Succeed(value) => assert value is Int32
    case Async.Deferred(_) => panic "unexpected deferred computation"
    case Async.Fail(_) => panic "unexpected failure"
"#,
    )
    .expect("explicit generic ByName coercions preserve the declared return type");
}
