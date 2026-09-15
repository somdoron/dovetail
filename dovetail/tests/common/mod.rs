#![allow(dead_code)]

use std::sync::OnceLock;

use wasmtime::component::{Component, Linker};
use wasmtime::{Engine, Store};
use wasmtime_wasi::WasiCtxBuilder;

use dovetail::p3::{RunFailure, State};

/// The engine plus a linker with the full WASI p3 surface registered, built
/// once per test binary. Every invocation compiles a *different* component,
/// so an `InstancePre` cannot be shared — but engine creation and the WASI
/// host-function registration are component-independent and were being
/// repeated per test. Both `Engine` and `Linker<State>` are `Send + Sync`
/// (host functions are required to be), so a plain `OnceLock` suffices.
fn engine_and_linker() -> &'static (Engine, Linker<State>) {
    static CACHE: OnceLock<(Engine, Linker<State>)> = OnceLock::new();
    CACHE.get_or_init(|| {
        let engine = dovetail::p3::p3_engine().expect("failed to create wasmtime engine");
        let mut linker = Linker::<State>::new(&engine);
        wasmtime_wasi::p3::add_to_linker(&mut linker).expect("failed to add WASI to linker");
        (engine, linker)
    })
}

fn create_engine() -> Engine {
    engine_and_linker().0.clone()
}

/// Compile Dovetail source and run via the WASI CLI component model.
pub fn compile_and_run(source: &str) -> Result<(), String> {
    // 1. Compile
    let result = dovetail::compile(source, "test.dove");

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        return Err(format!("compilation failed: {}", errors.join("; ")));
    }

    let wasm_bytes = result.wasm.ok_or("compilation produced no WASM output")?;

    // 2. Run as WASI CLI component
    let engine = create_engine();
    // `{e:?}` (not `{e}`): a component-load failure is a codegen bug, and
    // wasmtime puts the actionable part ("invalid `array.new_default`: ...")
    // in the error's cause chain, which Display drops. Same rendering as
    // `test_runner.rs`, rather than a second hand-walked chain that formats
    // the same information differently.
    let component =
        Component::new(&engine, &wasm_bytes).map_err(|e| format!("component load error: {e:?}"))?;

    run_p3_component(&engine, &component).map_err(|f| f.message())
}

/// Instantiate a p3 component and drive its async `wasi:cli/run` export.
///
/// Same drive sequence as `dovetail run`, so it goes through the shared driver
/// rather than a second copy that can drift; only the WASI context differs
/// (tests want loopback networking and inherited stdio, nothing else).
fn run_p3_component(engine: &Engine, component: &Component) -> Result<(), RunFailure> {
    let linker = &engine_and_linker().1;

    let wasi_ctx = WasiCtxBuilder::new()
        .inherit_stdout()
        .inherit_stderr()
        .inherit_network()
        .allow_tcp(true)
        .allow_ip_name_lookup(true)
        .build();
    let mut store = Store::new(engine, State::new(wasi_ctx));
    dovetail::p3::run_cli_component_detailed(component, linker, &mut store)
}

/// Compile Dovetail source and run, expecting a WASM trap.
pub fn compile_and_expect_trap(source: &str) {
    let result = dovetail::compile(source, "test.dove");

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        panic!("compilation failed unexpectedly: {}", errors.join("; "));
    }

    let wasm_bytes = result.wasm.expect("compilation produced no WASM output");

    let engine = create_engine();
    let component = Component::new(&engine, &wasm_bytes).expect("component load error");

    // A TRAP, not merely an `Err`. A program that exits with a clean error
    // status, and a component that fails to instantiate, both come back as
    // `Err` too — so accepting any error would leave every one of these tests
    // green if `panic`/`assert` regressed into an ordinary failed exit, which is
    // the exact regression they exist to catch.
    match run_p3_component(&engine, &component) {
        Err(failure) if failure.is_trap() => (),
        Err(failure) => panic!(
            "expected WASM trap, but execution failed without trapping: {}",
            failure.message()
        ),
        Ok(()) => panic!("expected WASM trap, but execution succeeded"),
    }
}

/// Run the pipeline up to typechecking (no codegen) and assert no errors.
pub fn check_no_errors(source: &str) {
    let result = dovetail::check(source, "test.dove");
    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        panic!("expected no errors, got: {}", errors.join("; "));
    }
}

/// Compile Dovetail source, expect errors, and return them.
pub fn compile_expecting_errors(source: &str) -> Vec<String> {
    let result = dovetail::compile(source, "test.dove");
    result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

/// Compile Dovetail source in test mode and run each test individually.
/// Uses compile_for_test which skips main resolution and converts test decls to functions.
pub fn compile_and_run_tests(source: &str) -> Result<(), String> {
    let result = dovetail::compile_for_test(source, "test.dove");

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        return Err(format!("compilation failed: {}", errors.join("; ")));
    }

    let wasm_bytes = result.wasm.ok_or("compilation produced no WASM output")?;

    let run_result = dovetail::test_runner::run_tests(&wasm_bytes, &result.test_exports)
        .map_err(|e| format!("test runner error: {e}"))?;

    for tr in &run_result.results {
        if let dovetail::test_runner::TestStatus::Fail { message } = &tr.status {
            return Err(format!("test '{}' failed: {}", tr.name, message));
        }
    }

    Ok(())
}

/// Compile Dovetail source in test mode, expecting at least one test to fail.
pub fn compile_tests_and_expect_trap(source: &str) {
    let result = dovetail::compile_for_test(source, "test.dove");

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        panic!("compilation failed unexpectedly: {}", errors.join("; "));
    }

    let wasm_bytes = result.wasm.expect("compilation produced no WASM output");

    let run_result = dovetail::test_runner::run_tests(&wasm_bytes, &result.test_exports)
        .expect("test runner should not fail");

    let any_failed = run_result
        .results
        .iter()
        .any(|tr| matches!(&tr.status, dovetail::test_runner::TestStatus::Fail { .. }));
    assert!(
        any_failed,
        "expected at least one test to fail, but all passed"
    );
}

/// Compile Dovetail source in test mode and return error messages.
pub fn compile_tests_expecting_errors(source: &str) -> Vec<String> {
    let result = dovetail::compile_for_test(source, "test.dove");
    result
        .diagnostics
        .iter()
        .map(|d| d.message.clone())
        .collect()
}

/// Compile Dovetail source in test mode and return full test results.
/// Allows inspecting individual TestStatus values (pass, skip, fail).
pub fn compile_and_run_tests_with_results(
    source: &str,
) -> Result<dovetail::test_runner::TestRunResult, String> {
    let result = dovetail::compile_for_test(source, "test.dove");

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| d.message.clone())
            .collect();
        return Err(format!("compilation failed: {}", errors.join("; ")));
    }

    let wasm_bytes = result.wasm.ok_or("compilation produced no WASM output")?;

    dovetail::test_runner::run_tests(&wasm_bytes, &result.test_exports)
        .map_err(|e| format!("test runner error: {e}"))
}

/// Compile Dovetail source and return only warnings (no errors expected).
pub fn compile_and_get_warnings(source: &str) -> Vec<String> {
    use dovetail::common::diagnostics::Severity;
    let result = dovetail::compile(source, "test.dove");
    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.message.clone())
            .collect();
        panic!("expected no errors but got: {}", errors.join("; "));
    }
    result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| d.message.clone())
        .collect()
}

// ── Async test helpers ──────────────────────────────────────────────
// Injects a small Async enum + Awaitable/From impls into
// test source, so async/await tests don't need a real library dependency.

const TEST_ASYNC_PREAMBLE: &str = r#"
enum Async<out T, out E> =
    Succeed(T)
    Fail(E)
    Deferred(() => Async<T, E>)

module Async<T, E> =
    public function evaluate(self): Async<T, E> =
        match self with
        case Async.Deferred(body) => body().evaluate()
        case _ => self

    public function map<U>(self, f: (T) => U): Async<U, E> =
        match self with
        case Async.Succeed(x) => Async.Succeed(f(x))
        case Async.Deferred(body) => Async.Deferred(() => body().map(f))
        case Async.Fail(e) => Async.Fail(e)
    public function andThen<U>(self, f: (T) => Async<U, E>): Async<U, E> =
        match self with
        case Async.Succeed(x) => f(x)
        case Async.Deferred(body) => Async.Deferred(() => body().andThen(f))
        case Async.Fail(e) => Async.Fail(e)
    public function whileLoop(cond: () => Bool, body: () => Async<Unit, E>, trace: SourceLocation): Async<Unit, E> =
        if cond() then
            body().andThen<Unit>((_: Unit) => Async<T, E>.whileLoop(cond, body, trace))
        else
            Async<Unit, E>.Succeed(())

implement <T, E> Awaitable<T> for Async<T, E> =
    type Rebind<U> = Async<U, E>
    public function succeed(x: T): Async<T, E> = Async.Succeed(x)
    public function defer(body: ByName<Async<T, E>>, trace: SourceLocation): Async<T, E> =
        Async.Deferred(() => body.get)
    public function map<U>(self: Async<T, E>, f: (T) => U, trace: SourceLocation): Async<U, E> =
        match self with
        case Async.Succeed(x) => Async.Succeed(f(x))
        case Async.Deferred(body) => Async.Deferred(() => body().map(f))
        case Async.Fail(e) => Async.Fail(e)
    public function andThen<U>(self: Async<T, E>, f: (T) => Async<U, E>, trace: SourceLocation): Async<U, E> =
        match self with
        case Async.Succeed(x) => f(x)
        case Async.Deferred(body) => Async.Deferred(() => body().andThen(f))
        case Async.Fail(e) => Async.Fail(e)
    public function whileLoop(cond: () => Bool, body: () => Async<Unit, E>, trace: SourceLocation): Async<Unit, E> =
        Async<T, E>.whileLoop(cond, body, trace)

implement <V, E> From<Result<Never, E>> for Async<V, E> =
    public function from(value: Result<Never, E>): Async<V, E> =
        match value with
        case Error(e) => Async.Fail(e)
        case Ok(_) => panic "unreachable"
"#;

/// Inject the test Async preamble after the package declaration and any imports.
fn inject_test_async(source: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let mut insert_after = 0;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("package ") || trimmed.starts_with("import ") || trimmed.is_empty() {
            insert_after = i + 1;
        } else {
            break;
        }
    }
    let before = lines[..insert_after].join("\n");
    let after = lines[insert_after..].join("\n");
    format!("{before}\n{TEST_ASYNC_PREAMBLE}{after}\n")
}

/// Compile Dovetail source with a test Async type and run it.
pub fn compile_and_run_async(source: &str) -> Result<(), String> {
    compile_and_run(&inject_test_async(source))
}

/// Compile Dovetail source with a test Async type, expect errors.
pub fn compile_expecting_errors_async(source: &str) -> Vec<String> {
    compile_expecting_errors(&inject_test_async(source))
}
