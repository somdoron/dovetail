mod common;

#[test]
fn construction_and_patterns_preserve_the_record_value() {
    common::compile_and_run(
        r#"
package a

record Placement =
    placedAt: Int32
    total: Int32

enum Status =
    Draft
    Placed(Placement)

function read(status: Status): Int32 =
    match status with
        case Placed { placedAt = 1, total } if total > 0 => total
        case Status.Placed(Placement { total }) => total
        case Draft => 0

function main(): Unit =
    let placement = Placement { placedAt = 1; total = 20 }
    let first = Status.Placed(placement)
    let second = Status.Placed { total = 30; placedAt = 2 }
    assert read(first) == 20
    assert read(second) == 30
    match second with
        case Placed(value) => assert value.total == 30
        case Draft => assert false
    match first with
        case Placed {} => ()
        case Draft => assert false
"#,
    )
    .expect("single record payload supports both spellings");
}

#[test]
fn generic_payloads_infer_from_fields_and_expected_types() {
    common::compile_and_run(
        r#"
package a

record Data<A> =
    value: A
    other: Option<A>

enum Envelope<T> =
    Data(Data<T>)

function wrap<T>(value: T): Envelope<T> =
    Envelope.Data { value = value; other = None }

function read<T>(envelope: Envelope<T>): T =
    match envelope with
        case Data { value } => value

function main(): Unit =
    let inferred = Envelope.Data { value = 42; other = Some(5) }
    let contextual: Envelope<Int32> = Envelope.Data { other = None; value = 12 }
    assert read(inferred) == 42
    assert read(contextual) == 12
    assert read(wrap("hello")) == "hello"
    let option: Option<Data<Int32>> = Some { value = 7; other = None }
    match option with
        case Some { value; other = None } => assert value == 7
        case Some { other = Some(_) } => assert false
        case None => assert false
    let result: Result<Data<Int32>, String> = Ok { value = 8; other = None }
    match result with
        case Ok { value } => assert value == 8
        case Error(_) => assert false
    let failure: Result<Int32, Data<Int32>> = Error { value = 9; other = None }
    match failure with
        case Error { value } => assert value == 9
        case Ok(_) => assert false
"#,
    )
    .expect("generic field inference and context are preserved");
}

#[test]
fn later_fields_resolve_earlier_contextual_values() {
    common::compile_and_run(
        r#"
package a

record Data<A> =
    value: A
    other: Option<A>
    nested: Option<Option<A>>

enum Envelope<T> =
    Data(Data<T>)

class Counter(public mutable count: Int32)

function read<T>(envelope: Envelope<T>): T =
    match envelope with
        case Data { value; other = None; nested = Some(None) } => value
        case _ => panic "expected empty options"

function main(): Unit =
    let counter = Counter(0)
    let first = Envelope.Data {
        other =
            counter.count = counter.count + 1
            None
        nested = Some(None)
        value = 12
    }
    let second = Envelope.Data { value = "hello"; other = None; nested = Some(None) }
    assert read(first) == 12
    assert read(second) == "hello"
    assert counter.count == 1
"#,
    )
    .expect("later fields resolve earlier contextual expressions without duplicating evaluation");
}

#[test]
fn contextual_closures_wait_for_other_fields() {
    common::compile_and_run(
        r#"
package a

record Data<A> =
    value: A
    transform: A => A
    other: Option<A => A>

enum Envelope<T> =
    Data(Data<T>)

function read<T>(envelope: Envelope<T>): T =
    match envelope with
        case Data { value; transform; other = Some(next) } => next(transform(value))
        case _ => panic "expected another transform"

function main(): Unit =
    let first = Envelope.Data {
        transform = x => x + 1
        other = Some(x => x + 2)
        value = 12
    }
    let second = Envelope.Data {
        other =
            let extra = 2
            Some(x => x + extra)
        value = 12
        transform = x => x + 1
    }
    assert read(first) == 15
    assert read(second) == 15
"#,
    )
    .expect("direct and nested contextual closures use later field constraints");
}

#[test]
fn unresolved_context_does_not_suppress_variance_defaults() {
    common::compile_and_run(
        r#"
package a

record Data<A> =
    other: Option<A>

enum Envelope<T> =
    Data(Data<T>)

function main(): Unit =
    let shorthand = Envelope.Data { other = None }
    let explicit = Envelope.Data(Data { other = None })
    match shorthand with
        case Data { other } => assert other.isNone
    match explicit with
        case Data { other } => assert other.isNone
"#,
    )
    .expect("shorthand preserves normal Option<Never> inference");
}

#[test]
fn calls_with_closures_supply_context_before_defaults() {
    common::compile_and_run(
        r#"
package a

record Data<A> =
    value: A
    transform: A => A
    other: Option<A>
enum Envelope<T> =
    Data(Data<T>)

function calculate(transform: Int32 => Int32): Int32 = transform(12)

function read<T>(envelope: Envelope<T>): T =
    match envelope with
        case Data { value; transform; other = None } => transform(value)
        case _ => panic "expected None"

function main(): Unit =
    let first = Envelope.Data {
        transform = x => x + 1
        other = None
        value = calculate(x => x + 2)
    }
    let second = Envelope.Data {
        value = calculate(x => x + 2)
        transform = x => x + 1
        other = None
    }
    assert read(first) == 15
    assert read(second) == 15
"#,
    )
    .expect("callee-provided closure context participates in field inference");
}

#[test]
fn outer_field_constraints_override_nested_defaults() {
    common::compile_and_run(
        r#"
package a

record Inner<A> =
    other: Option<A>
enum Nested<T> =
    Data(Inner<T>)
record Outer<A> =
    value: A
    nested: Nested<A>
enum Envelope<T> =
    Data(Outer<T>)

function read<T>(envelope: Envelope<T>): T =
    match envelope with
        case Data { value; nested = Data { other = None } } => value
        case _ => panic "expected None"

function main(): Unit =
    let first = Envelope.Data { nested = Nested.Data { other = None }; value = 12 }
    let second = Envelope.Data { value = "hello"; nested = Nested.Data { other = None } }
    assert read(first) == 12
    assert read(second) == "hello"
"#,
    )
    .expect("nested defaults do not determine an outer type prematurely");
}

#[test]
fn closure_results_infer_unknown_payload_parameters() {
    common::compile_and_run(
        r#"
package a

record Data<A> =
    make: Int32 => A
enum Envelope<T> =
    Data(Data<T>)

function read<T>(envelope: Envelope<T>): T =
    match envelope with
        case Data { make } => make(12)

function main(): Unit =
    let annotated = Envelope.Data { make = (x: Int32) => x + 1 }
    let contextual = Envelope.Data { make = x => x + 1 }
    let explicit = Envelope.Data(Data { make = (x: Int32) => x + 1 })
    assert read(annotated) == 13
    assert read(contextual) == 13
    assert read(explicit) == 13
"#,
    )
    .expect("known closure inputs are retained while its output is inferred");
}

#[test]
fn compound_defaults_wait_for_concrete_field_types() {
    common::compile_and_run(
        r#"
package a

record Data<A> =
    other: Option<A>
    value: A
enum Envelope<T> =
    Data(Data<T>)

function read<T>(envelope: Envelope<Option<T>>): T =
    match envelope with
        case Data { other = Some(None); value = Some(value) } => value
        case _ => panic "expected a value and an empty nested option"

function main(): Unit =
    let first = Envelope.Data { other = Some(None); value = Some(12) }
    let second = Envelope.Data { value = Some("hello"); other = Some(None) }
    assert read(first) == 12
    assert read(second) == "hello"
"#,
    )
    .expect("Option<Never> evidence stays weaker than a later Option<Int32>");
}

#[test]
fn contravariant_defaults_wait_for_concrete_field_types() {
    common::compile_and_run(
        r#"
package a

enum Consumer<in T> =
    Empty
record Inner<A> =
    consumer: Consumer<A>
enum Nested<T> =
    Data(Inner<T>)
record Outer<A> =
    value: A
    nested: Nested<A>
enum Envelope<T> =
    Data(Outer<T>)

function main(): Unit =
    let first = Envelope.Data { nested = Nested.Data { consumer = Consumer.Empty }; value = 12 }
    let second = Envelope.Data { value = 12; nested = Nested.Data { consumer = Consumer.Empty } }
    match first with
        case Data { value } => assert value + 1 == 13
    match second with
        case Data { value } => assert value + 1 == 13
"#,
    )
    .expect("contravariant Any defaults do not erase a later concrete field type");
}

#[test]
fn explicit_any_evidence_is_preserved_in_either_field_order() {
    common::compile_and_run(
        r#"
package a

record Data<T> =
    first: T
    second: T
enum Envelope<T> =
    Data(Data<T>)

function wrap(value: Any): Unit =
    let first = Envelope.Data { first = value; second = 1 }
    let second = Envelope.Data { second = 1; first = value }
    match first with
        case Data { first = x } => assert x is Int32
    match second with
        case Data { first = x } => assert x is Int32

function main(): Unit = wrap(12)
"#,
    )
    .expect("explicit Any values remain constraints rather than inferred defaults");
}

#[test]
fn composite_candidates_refine_independent_default_slots() {
    common::compile_and_run(
        r#"
package a

record Data<A> =
    other: Option<A>
    value: A
enum Envelope<T> =
    Data(Data<T>)

function main(): Unit =
    let first = Envelope.Data { other = Some(Ok(None)); value = Ok(Some(12)) }
    let second = Envelope.Data { value = Ok(Some(12)); other = Some(Ok(None)) }
    match first with
        case Data { value = Ok(Some(value)) } => assert value + 1 == 13
        case _ => assert false
    match second with
        case Data { value = Ok(Some(value)) } => assert value + 1 == 13
        case _ => assert false
"#,
    )
    .expect("a success type can refine while the unused error type remains Never");
}

#[test]
fn partial_closure_returns_preserve_known_type_structure() {
    common::compile_and_run(
        r#"
package a

record Inner<A> =
    value: A
record Data<A> =
    make: Unit => Option<Inner<A>>
enum Envelope<T> =
    Data(Data<T>)

record Nested<A> =
    make: Int32 => (String => A)
enum NestedEnvelope<T> =
    Data(Nested<T>)

function main(): Unit =
    let first = Envelope.Data { make = _ => Some { value = 1 } }
    match first with
        case Data { make } =>
            match make(()) with
                case Some { value } => assert value == 1
                case None => assert false
    let second = NestedEnvelope.Data { make = x => y => x + 1 }
    match second with
        case Data { make } =>
            let next = make(12)
            assert next("hello") == 13
"#,
    )
    .expect("known record identities and nested closure inputs survive partial return inference");
}

#[test]
fn mixed_pattern_forms_share_exhaustiveness() {
    common::compile_and_run(
        r#"
package a

record Flags =
    first: Bool
    second: Bool

enum State =
    Ready(Flags)
    Empty

function read(state: State): Int32 =
    match state with
        case Ready { first = true; second = true } => 1
        case State.Ready(Flags { first = true; second = false }) => 2
        case Ready { first = false } => 3
        case Empty => 4

function main(): Unit =
    assert read(State.Ready { first = true; second = false }) == 2
    assert read(State.Ready { first = false; second = true }) == 3
"#,
    )
    .expect("brace and positional patterns cover the same nested space");
}

#[test]
fn missing_nested_case_is_non_exhaustive() {
    let errors = common::compile_expecting_errors(
        r#"
package a
record Flag =
    enabled: Bool

enum State =
    Ready(Flag)

function read(state: State): Int32 =
    match state with
        case Ready { enabled = true } => 1

function main(): Unit = ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("non-exhaustive")),
        "{errors:?}"
    );
}

#[test]
fn nested_braces_only_unwrap_one_layer() {
    common::compile_and_run(
        r#"
package a
record Inner =
    count: Int32
record Outer =
    inner: Inner

enum State =
    Ready(Outer)

function main(): Unit =
    let state = State.Ready { inner = Inner { count = 5 } }
    let wrapped = Some(state)
    match wrapped with
        case Some(Ready { inner = Inner { count } }) => assert count == 5
        case None => assert false
"#,
    )
    .expect("nested bare record payload patterns");
}

#[test]
fn ordinary_record_names_keep_precedence_in_expressions() {
    common::compile_and_run(
        r#"
package a
record Some =
    count: Int32

enum State =
    Some(Some)

function main(): Unit =
    let detail = Some { count = 1 }
    assert detail.count == 1
    let state = State.Some { count = 2 }
    match state with
        case Some { count } => assert count == 2
"#,
    )
    .expect("record expressions and enum patterns resolve by their context");
}

#[test]
fn invalid_fields_are_rejected() {
    let declarations = r#"
package a
record Position =
    x: Int32
    y: Int32

enum Shape =
    At(Position)
"#;
    for (expression, diagnostic) in [
        ("Shape.At { x = 1 }", "missing field 'y'"),
        ("Shape.At { x = 1; y = 2; z = 3 }", "unknown field 'z'"),
        ("Shape.At { x = 1; x = 2; y = 3 }", "duplicate field 'x'"),
        ("Shape.At { x = true; y = 2 }", "type mismatch"),
    ] {
        let source = format!(
            "{declarations}\nfunction main(): Unit =\n    let value = {expression}\n    ()\n"
        );
        let errors = common::compile_expecting_errors(&source);
        assert!(
            errors.iter().any(|e| e.contains(diagnostic)),
            "{expression}: {errors:?}"
        );
    }
    for (pattern, diagnostic) in [
        ("At { z }", "no field 'z'"),
        ("At { x; x }", "duplicate field 'x'"),
        ("At { x = true }", "type mismatch"),
    ] {
        let source = format!(
            "{declarations}\nfunction read(value: Shape): Unit =\n    match value with\n        case {pattern} => ()\n\nfunction main(): Unit = ()\n"
        );
        let errors = common::compile_expecting_errors(&source);
        assert!(
            errors.iter().any(|e| e.contains(diagnostic)),
            "{pattern}: {errors:?}"
        );
    }
}

#[test]
fn braces_require_a_single_known_record_payload() {
    for payload in ["Int32", "Position, Position", "Wrapped", "Object", "T"] {
        let source = format!(
            r#"
package a
record Position =
    x: Int32
newtype Wrapped = Position
class Object(x: Int32)
enum Shape<T> =
    At({payload})

function read<T>(value: Shape<T>): Unit =
    match value with
        case At {{ x }} => ()

function main(): Unit = ()
"#
        );
        let errors = common::compile_expecting_errors(&source);
        assert!(
            errors.iter().any(|e| e.contains("record-style payload")),
            "{payload}: {errors:?}"
        );
    }
    let errors = common::compile_expecting_errors(
        r#"
package a
function main(): Unit =
    let value = Some { x = 1 }
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("known record type")),
        "{errors:?}"
    );
}

#[test]
fn phantom_record_parameters_require_context() {
    let errors = common::compile_expecting_errors(
        r#"
package a
record Data<T> =
    count: Int32
enum State<T> =
    Ready(Data<T>)

function main(): Unit =
    let value = State.Ready { count = 1 }
    ()
"#,
    );
    assert!(
        errors.iter().any(|e| e.contains("cannot infer")),
        "{errors:?}"
    );
}

#[test]
fn callback_constraints_respect_variance_and_field_order() {
    common::compile_and_run(
        r#"
package a

record Callbacks<T> =
    first: T => Unit
    second: T => Unit
record Data<T> =
    value: T
    consume: T => Unit
enum Envelope<T> =
    Callbacks(Callbacks<T>)
    Data(Data<T>)
function consumeInt(value: Int32): Unit = assert value == 12
function consumeAny(value: Any): Unit = ()
function main(): Unit =
    let first = Envelope.Callbacks { first = consumeInt; second = consumeAny }
    let second = Envelope.Callbacks { second = consumeAny; first = consumeInt }
    let closures = Envelope.Callbacks { first = (x: Int32) => (); second = (x: Any) => () }
    let forward = Envelope.Data { value = 12; consume = consumeAny }
    let reverse = Envelope.Data { consume = consumeAny; value = 12 }
    match first with
        case Callbacks { first; second } =>
            first(12)
            second(12)
        case Data {} => assert false
    match second with
        case Callbacks { first } => first(12)
        case Data {} => assert false
    match forward with
        case Data { value } => assert value + 1 == 13
        case Callbacks {} => assert false
    match reverse with
        case Data { value } => assert value + 1 == 13
        case Callbacks {} => assert false
"#,
    )
    .expect("callback inputs provide upper bounds independently of value lower bounds");
}

#[test]
fn provisional_callback_bounds_do_not_widen_producer_closures() {
    common::compile_and_run(
        r#"
package a
class Parent()
class Child() extends Parent()
record Data<A> =
    first: A => Unit
    make: Unit => A
    second: A => Unit
enum Envelope<T> = Data(Data<T>)
function consumeParent(value: Parent): Unit = ()
function consumeChild(value: Child): Unit = ()
function main(): Unit =
    let wrapped = Envelope.Data {
        first = consumeParent
        make = _ => Child()
        second = consumeChild
    }
    match wrapped with
        case Data { make; first; second } =>
            let child: Child = make(())
            first(child)
            second(child)
"#,
    )
    .expect("closure output evidence remains independent of provisional callback context");
}

#[test]
fn partial_producer_results_preserve_concrete_types_with_callback_bounds() {
    common::compile_and_run(
        r#"
package a
class Parent()
class Child() extends Parent()
record Data<A> =
    first: A => Unit
    make: Unit => A
    second: A => Unit
enum Envelope<T> = Data(Data<T>)
function consumeParent(value: Result<Parent, String>): Unit = ()
function consumeChild(value: Result<Child, String>): Unit = ()
function main(): Unit =
    let forward = Envelope.Data {
        first = consumeParent
        make = _ => Ok(Child())
        second = consumeChild
    }
    let reverse = Envelope.Data {
        second = consumeChild
        make = _ => Ok(Child())
        first = consumeParent
    }
    match forward with
        case Data { make; second } => second(make(()))
    match reverse with
        case Data { make; second } => second(make(()))
"#,
    )
    .expect("producer evidence retains Child while its Result error type defaults to Never");
}

#[test]
fn identity_closures_wait_for_all_callback_bounds() {
    common::compile_and_run(
        r#"
package a
class Parent()
class Child() extends Parent()
record Data<A> =
    first: A => Unit
    transform: A => A
    second: A => Unit
enum Envelope<T> = Data(Data<T>)
function consumeParent(value: Parent): Unit = ()
function consumeChild(value: Child): Unit = ()
function main(): Unit =
    let forward = Envelope.Data {
        first = consumeParent
        transform = x => x
        second = consumeChild
    }
    let reverse = Envelope.Data {
        second = consumeChild
        transform = x => x
        first = consumeParent
    }
    match forward with
        case Data { transform; second } => second(transform(Child()))
    match reverse with
        case Data { transform; second } => second(transform(Child()))
"#,
    )
    .expect("identity closures use all independently available callback constraints");
}
