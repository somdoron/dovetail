//! Shared WASI-p3 host setup: the version the vendored WIT is pinned to, the
//! engine flags a p3 component needs, the tokio runtime that drives it, and the
//! `wasi:cli/run` call sequence.
//!
//! Everything here was previously copy-pasted between `runner`, `test_runner`,
//! and the integration-test harness, where the copies were already drifting.

use std::sync::OnceLock;

use wasmtime::component::{Component, Linker, ResourceTable, Val};
use wasmtime::{Config, Engine, Store, WasmBacktraceDetails};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

/// wasi p3 version suffix (matches the vendored WIT in `dovetail/wit`).
pub const P3_VERSION: &str = "0.3.0-rc-2026-03-15";

/// The vendored WASI p3 WIT (from wasmtime-wasi 45 `src/p3/wit/deps`), in the
/// order it is pushed into a `Resolve`.
///
/// The order is load-bearing, not cosmetic: it fixes the anonymous type indices
/// that end up in `[stream-read-N]`-style builtin names, and therefore the
/// numbering of the whole import table. Every consumer — the component encoder
/// and the import-table generator — reads this one list so a reordering cannot
/// silently apply to only one of them.
pub const WIT_FILES: &[(&str, &str)] = &[
    (
        "wasi-p3-clocks.wit",
        include_str!("../wit/wasi-p3-clocks.wit"),
    ),
    (
        "wasi-p3-filesystem.wit",
        include_str!("../wit/wasi-p3-filesystem.wit"),
    ),
    (
        "wasi-p3-sockets.wit",
        include_str!("../wit/wasi-p3-sockets.wit"),
    ),
    (
        "wasi-p3-random.wit",
        include_str!("../wit/wasi-p3-random.wit"),
    ),
    ("wasi-p3-cli.wit", include_str!("../wit/wasi-p3-cli.wit")),
];

/// Enable everything a Dovetail component needs: the p3 async component-model
/// features, WASMGC, and enough debug info for a readable backtrace.
///
/// The four async flags travel together — a component lifted with
/// `async-stackful` will not instantiate without all of them — which is why
/// they live in one place rather than being spelled out per call site.
pub fn configure_p3_engine(config: &mut Config) {
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);
    config.wasm_component_model_more_async_builtins(true);
    config.wasm_component_model_async_stackful(true);
    config.concurrency_support(true);
    config.wasm_gc(true);
    config.wasm_function_references(true);
    config.wasm_wide_arithmetic(true);
    config.debug_info(true);
    config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
}

/// A configured engine, for callers with nothing to add.
pub fn p3_engine() -> Result<Engine, String> {
    let mut config = Config::new();
    configure_p3_engine(&mut config);
    Engine::new(&config).map_err(|e| format!("failed to create engine: {e}"))
}

/// The process-wide tokio runtime components are driven on. Shared rather than
/// built per run: integration tests call this once per compiled program and a
/// fresh multi-threaded runtime each time means a full worker pool spun up and
/// torn down per test.
pub fn runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .map_err(|e| format!("failed to build async runtime: {e}"))
        })
        .as_ref()
        .map_err(|e| e.clone())
}

/// The store state every Dovetail host needs: a WASI context and its resource
/// table. Callers differ only in how they build the `WasiCtx`.
pub struct State {
    pub ctx: WasiCtx,
    pub table: ResourceTable,
}

impl State {
    /// Wrap a built `WasiCtx` with a fresh resource table.
    pub fn new(ctx: WasiCtx) -> Self {
        Self {
            ctx,
            table: ResourceTable::new(),
        }
    }
}

impl WasiView for State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.ctx,
            table: &mut self.table,
        }
    }
}

/// Why driving a component did not end in `ok`.
///
/// Three genuinely different things arrive on one channel, and a caller that
/// only ever renders them cannot tell them apart afterwards. `dovetail run` does
/// only render them — but a test asserting "this program traps" is asserting
/// something much narrower than "this returned Err", and collapsing the two
/// would let a panic quietly regress into a clean error exit with every trap
/// test still green.
pub enum RunFailure {
    /// The engine, the instantiation, or the export lookup failed: the guest
    /// never got to run.
    Setup(String),
    /// The guest was driven and the drive failed — a trap, or a host error
    /// raised while serving it.
    Guest(wasmtime::Error),
    /// The guest ran to completion and `run` returned `err`. Not a failure of
    /// the guest so much as the guest's own verdict on its work.
    ExitedWithError,
    /// `run` returned something that is not a `result` at all.
    UnexpectedResult(String),
}

impl RunFailure {
    /// Whether this is a genuine wasm trap.
    ///
    /// The variant alone is not enough: a host error raised while serving the
    /// guest also lands in `Guest`, so the trap has to be found in the error
    /// chain rather than assumed from the channel it arrived on. Same reasoning
    /// as `test_runner::is_epoch_interrupt`, which matches the trap rather than
    /// the rendered chain for the same reason.
    pub fn is_trap(&self) -> bool {
        match self {
            RunFailure::Guest(e) => e
                .chain()
                .any(|cause| cause.downcast_ref::<wasmtime::Trap>().is_some()),
            _ => false,
        }
    }

    /// The failure as a human-readable string. Guest errors go through
    /// `backtrace` so a trap arrives with the Dovetail-level stack rather than a
    /// bare wasm message.
    pub fn message(&self) -> String {
        match self {
            RunFailure::Setup(m) => m.clone(),
            RunFailure::Guest(e) => crate::backtrace::format_backtrace(e),
            RunFailure::ExitedWithError => "program exited with error".to_string(),
            RunFailure::UnexpectedResult(v) => format!("unexpected run result {v}"),
        }
    }
}

/// Instantiate a p3 component and drive its async `wasi:cli/run` export to
/// completion, returning the program's own success/failure.
///
/// Errors coming out of the guest are rendered through `backtrace` so a trap
/// arrives with the Dovetail-level stack rather than a bare wasm message.
pub fn run_cli_component<T: WasiView + Send + 'static>(
    component: &Component,
    linker: &Linker<T>,
    store: &mut Store<T>,
) -> Result<(), String> {
    run_cli_component_detailed(component, linker, store).map_err(|f| f.message())
}

/// `run_cli_component`, keeping the failure structured instead of rendering it.
pub fn run_cli_component_detailed<T: WasiView + Send + 'static>(
    component: &Component,
    linker: &Linker<T>,
    store: &mut Store<T>,
) -> Result<(), RunFailure> {
    let rt = runtime().map_err(RunFailure::Setup)?;
    rt.block_on(async {
        let instance = linker
            .instantiate_async(&mut *store, component)
            .await
            .map_err(|e| RunFailure::Setup(format!("failed to instantiate component: {e}")))?;

        let run_iface = instance
            .get_export_index(&mut *store, None, &format!("wasi:cli/run@{P3_VERSION}"))
            .ok_or_else(|| {
                RunFailure::Setup("component does not export wasi:cli/run".to_string())
            })?;
        let run_idx = instance
            .get_export_index(&mut *store, Some(&run_iface), "run")
            .ok_or_else(|| {
                RunFailure::Setup("wasi:cli/run export has no `run` function".to_string())
            })?;
        let run = instance
            .get_func(&mut *store, run_idx)
            .ok_or_else(|| RunFailure::Setup("`run` export is not a function".to_string()))?;

        let mut results = [Val::Bool(false)];
        store
            .run_concurrent(async |acc| run.call_concurrent(acc, &[], &mut results).await)
            .await
            .map_err(RunFailure::Guest)?
            .map_err(RunFailure::Guest)?;

        match &results[0] {
            Val::Result(Ok(_)) => Ok(()),
            Val::Result(Err(_)) => Err(RunFailure::ExitedWithError),
            other => Err(RunFailure::UnexpectedResult(format!("{other:?}"))),
        }
    })
}
