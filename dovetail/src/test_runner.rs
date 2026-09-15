use std::time::Instant;

use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};

use crate::TestExportInfo;
use crate::p3::State;

/// Result of running a single test.
pub struct TestResult {
    pub fqtn: String,
    pub name: String,
    pub source_file: String,
    pub status: TestStatus,
}

/// Status of a test run.
pub enum TestStatus {
    Pass,
    Skip { reason: Option<String> },
    Fail { message: String },
}

/// Result of running all tests.
pub struct TestRunResult {
    pub results: Vec<TestResult>,
    pub total_duration: std::time::Duration,
}

/// Run tests in a compiled WASM component, one test per instantiation.
///
/// Each test is exported as `test-N` from the component. We create a fresh
/// store per test for isolation, so a trap in one test doesn't prevent others
/// from running.
pub fn run_tests(
    wasm_bytes: &[u8],
    test_exports: &[TestExportInfo],
) -> Result<TestRunResult, String> {
    let start = Instant::now();

    let mut config = Config::new();
    crate::p3::configure_p3_engine(&mut config);
    // Only the test runner needs this: `@timeout` is enforced by bumping the
    // epoch from a watchdog thread.
    config.epoch_interruption(true);
    let engine = Engine::new(&config).map_err(|e| format!("failed to create engine: {e}"))?;

    let component =
        // `{e:?}` (not `{e}`): a component-load failure is a codegen bug, and
        // wasmtime puts the actionable part ("invalid `array.new_default`: ...")
        // in the error's cause chain, which Display drops.
        Component::new(&engine, wasm_bytes).map_err(|e| format!("failed to load component: {e:?}"))?;

    let mut linker = Linker::<State>::new(&engine);
    wasmtime_wasi::p3::add_to_linker(&mut linker)
        .map_err(|e| format!("failed to add WASI to linker: {e}"))?;
    // A composed component may embed a p2-based dependency (e.g. the sqlite
    // shim does its disk I/O through synchronous wasi:filesystem/wasi:io), so
    // satisfy the p2 imports too. Harmless when no such dependency is present.
    wasmtime_wasi::p2::add_to_linker_async(&mut linker)
        .map_err(|e| format!("failed to add WASI p2 to linker: {e}"))?;

    // Resolve imports once; each test then only pays for instantiation, not
    // for re-matching the whole WASI import surface against the linker.
    let instance_pre = linker
        .instantiate_pre(&component)
        .map_err(|e| format!("failed to pre-instantiate component: {e}"))?;

    let runtime = crate::p3::runtime()?;

    let mut results = Vec::with_capacity(test_exports.len());

    for test_info in test_exports {
        // Handle @skip: don't instantiate or call the test
        if let Some(ref skip) = test_info.skip_reason {
            results.push(TestResult {
                fqtn: test_info.fqtn.clone(),
                name: test_info.name.clone(),
                source_file: test_info.source_file.clone(),
                status: TestStatus::Skip {
                    reason: skip.clone(),
                },
            });
            continue;
        }

        // Create a temp directory for filesystem tests, preopened as "."
        let temp_dir = tempfile::tempdir()
            .map_err(|e| format!("failed to create temp dir: {e}"))?;
        let mut builder = WasiCtxBuilder::new();
        builder
            .inherit_stdio()
            .inherit_network()
            .allow_tcp(true)
            .allow_udp(true)
            .allow_ip_name_lookup(true);
        builder.preopened_dir(temp_dir.path(), ".", DirPerms::all(), FilePerms::all())
            .map_err(|e| format!("failed to preopen temp dir: {e}"))?;
        let mut store = Store::new(&engine, State::new(builder.build()));

        // Handle @timeout: set epoch deadline and spawn timer thread
        let timer_handle = if let Some(ms) = test_info.timeout_ms {
            store.set_epoch_deadline(1);
            let engine_clone = engine.clone();
            let duration = std::time::Duration::from_millis(ms);
            // The timer waits on a channel rather than sleeping: a test that
            // finishes early drops the sender, which wakes the thread at once.
            // A plain `sleep` + flag made every `@timeout(N)` test cost N
            // milliseconds of wall time even when it passed in microseconds,
            // and detaching instead would let a late wake-up kill the *next*
            // test.
            let (cancel_tx, cancel_rx) = std::sync::mpsc::channel::<()>();
            let handle = std::thread::spawn(move || {
                if let Err(std::sync::mpsc::RecvTimeoutError::Timeout) =
                    cancel_rx.recv_timeout(duration)
                {
                    engine_clone.increment_epoch();
                }
            });
            Some((handle, cancel_tx))
        } else {
            // No timeout: set a high (but not max — wasmtime adds to it internally
            // and `u64::MAX` overflows) epoch deadline so epoch interruption
            // never fires for tests without an explicit @timeout.
            store.set_epoch_deadline(u64::MAX / 2);
            None
        };

        let export_name = format!("test-n{}", test_info.index);
        let test_future = async {
            let instance = instance_pre
                .instantiate_async(&mut store)
                .await
                .map_err(|e| {
                    wasmtime::Error::msg(format!(
                        "failed to instantiate component for test '{}': {e}",
                        test_info.name
                    ))
                })?;

            let func = instance.get_func(&mut store, &export_name).ok_or_else(|| {
                wasmtime::Error::msg(format!(
                    "test export '{}' not found in component for test '{}'",
                    export_name, test_info.name
                ))
            })?;

            store
                .run_concurrent(async |acc| func.call_concurrent(acc, &[], &mut []).await)
                .await?
        };
        // Two timeout mechanisms, because each covers what the other cannot:
        //   - the epoch trap (above) interrupts wasm that is *executing* — a
        //     pure-wasm spin never yields to the host, so only an epoch bump
        //     can stop it;
        //   - `tokio::time::timeout` fires while the guest is *suspended* in a
        //     host-side wait (waitable-set wait of an async-stackful lift) —
        //     no wasm executes there, the epoch trap never triggers, and
        //     without the host-side deadline `block_on` simply never returns.
        // On host-side expiry we synthesize the same `Trap::Interrupt` the
        // epoch path produces, so both funnel into the one "test timed out
        // after Nms" reporting path below. The timed-out future is dropped,
        // which cancels the in-flight call; the store, instance, and temp dir
        // are all per-test and dropped at the end of this iteration, so an
        // abandoned mid-await instance cannot poison later tests (the shared
        // engine/component/instance_pre are immutable).
        let call_result = match test_info.timeout_ms {
            Some(ms) => {
                let duration = std::time::Duration::from_millis(ms);
                // The `timeout` future must be *built* inside the runtime —
                // its internal `Sleep` grabs the reactor at construction, and
                // `block_on`'s argument is evaluated outside any runtime
                // context ("there is no reactor running" panic otherwise).
                match runtime
                    .block_on(async { tokio::time::timeout(duration, test_future).await })
                {
                    Ok(result) => result,
                    Err(_elapsed) => Err(wasmtime::Error::new(wasmtime::Trap::Interrupt)),
                }
            }
            None => runtime.block_on(test_future),
        };

        // Wake the timer thread so it exits without firing the epoch.
        if let Some((handle, cancel_tx)) = timer_handle {
            drop(cancel_tx);
            let _ = handle.join();
        }

        let status = match (&call_result, &test_info.expected_panic) {
            // No @panics: normal pass/fail
            (Ok(()), None) => {
                TestStatus::Pass
            }
            (Err(e), None) => {
                // Check if this was a timeout
                if let Some(ms) = test_info.timeout_ms {
                    if is_epoch_interrupt(e) {
                        TestStatus::Fail {
                            message: format!("test timed out after {ms}ms"),
                        }
                    } else {
                        TestStatus::Fail {
                            message: crate::backtrace::format_backtrace(e),
                        }
                    }
                } else {
                    TestStatus::Fail {
                        message: crate::backtrace::format_backtrace(e),
                    }
                }
            }
            // @panics: test passed (no trap) → fail
            (Ok(()), Some(_)) => {
                TestStatus::Fail {
                    message: "expected panic but test passed".to_string(),
                }
            }
            // @panics: test trapped
            (Err(e), Some(expected_msg)) => {
                // Check if this was actually a timeout, not a panic
                if test_info.timeout_ms.is_some() && is_epoch_interrupt(e) {
                    TestStatus::Fail {
                        message: format!("test timed out after {}ms", test_info.timeout_ms.unwrap()),
                    }
                } else {
                    match expected_msg {
                        // @panics (no message): any trap is a pass
                        None => TestStatus::Pass,
                        // @panics("msg"): check if full error chain contains the message
                        Some(msg) => {
                            let err_str = format_error_chain(e);
                            if err_str.contains(msg.as_str()) {
                                TestStatus::Pass
                            } else {
                                TestStatus::Fail {
                                    message: format!(
                                        "expected panic containing '{}' but got: {}",
                                        msg, err_str
                                    ),
                                }
                            }
                        }
                    }
                }
            }
        };

        results.push(TestResult {
            fqtn: test_info.fqtn.clone(),
            name: test_info.name.clone(),
            source_file: test_info.source_file.clone(),
            status,
        });
    }

    let total_duration = start.elapsed();

    Ok(TestRunResult {
        results,
        total_duration,
    })
}

/// Check if a wasmtime error is an epoch interruption (timeout).
fn is_epoch_interrupt(err: &wasmtime::Error) -> bool {
    // Match the trap itself, not the rendered error chain: that chain carries the
    // wasm backtrace, so a plain assertion failure inside (say) the runtime's
    // `interruptFiber` was being reported as "test timed out".
    err.chain()
        .any(|e| matches!(e.downcast_ref::<wasmtime::Trap>(), Some(wasmtime::Trap::Interrupt)))
}

/// Format the full error chain into a single string for message matching.
fn format_error_chain(err: &wasmtime::Error) -> String {
    // anyhow's "{:?}" debug format includes the full chain
    format!("{err:?}")
}
