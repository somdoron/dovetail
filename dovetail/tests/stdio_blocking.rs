mod common;

use std::io;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use wasmtime::Store;
use wasmtime::component::{Component, Linker};
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::cli::{IsTerminal, StdinStream, StdoutStream};

use dovetail::p3::State;

/// Run `source` on its own thread and fail if it has not finished in 60s.
///
/// The failure this guards is a hang, not a trap: the runtime's blocking stdio
/// path spins forever when a write does not complete in one go, and a plain
/// `#[test]` would take the whole `cargo test` run down with it. The hung
/// thread is abandoned — the process exits when the harness does.
fn run_with_timeout(source: &'static str, what: &str) {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(common::compile_and_run(source));
        })
        .expect("spawn");
    match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(result) => result.unwrap_or_else(|e| panic!("{what}: {e}")),
        Err(_) => panic!("{what}: timed out — the write never completed"),
    }
}

/// `debug` writes through the compiler-owned blocking stdio path, which is the
/// only writer available where there is no fiber runtime to hand the write to
/// (panic reporting, `debug` from a pure function).
///
/// Anything past the host's stdout buffer comes back BLOCKED, and completing it
/// means waiting for the completion event and re-posting the remainder. Getting
/// that wrong is invisible in a small print and a hang in a large one.
#[test]
fn debug_output_larger_than_the_host_stdout_buffer() {
    run_with_timeout(
        r#"
package a

function main(): Unit =
    // 256 KiB, far past any host buffer, so the write completes in many
    // chunks with a blocking wait between them.
    debug("abcdefgh".repeat(32768))
"#,
        "large debug",
    );
}

/// The same write, interleaved so the loop has to survive re-entering the
/// blocking path repeatedly rather than only on a fresh stream.
#[test]
fn repeated_large_debug_output() {
    run_with_timeout(
        r#"
package a

function main(): Unit =
    let chunk = "0123456789abcdef".repeat(4096)
    let mutable i = 0
    while i < 4 do
        debug(chunk)
        i = i + 1
"#,
        "repeated large debug",
    );
}

// ── Backpressured stdout ────────────────────────────────────────────────────
//
// The two tests above run against inherited stdio, where the host consumes every
// write synchronously (`std::io::stdout().write` on a blocking fd), so
// `stream.write` always answers COMPLETED and the BLOCKED arm of
// `emit_flush_write` is never reached. It is reachable on any host whose stdout
// can exert backpressure, and that arm is where the interesting failures live:
// the wait is satisfied only if the writable end is joined to the private flush
// waitable-set, and it can only be *our* completion if nothing else writes on
// that end. The stdout below makes every host-side write park once, so the arm
// runs — many times over, for a payload this size.

/// Bytes the guest has written, in order.
type Written = Arc<Mutex<Vec<u8>>>;

/// A stdout that accepts at most 4 KiB per call and parks once before each,
/// so a guest write of any real size takes many BLOCKED/resume rounds.
#[derive(Clone)]
struct BackpressuredStdout(Written);

impl IsTerminal for BackpressuredStdout {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for BackpressuredStdout {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(ParkOnceWriter {
            written: self.0.clone(),
            parked: false,
        })
    }
}

struct ParkOnceWriter {
    written: Written,
    parked: bool,
}

impl AsyncWrite for ParkOnceWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if !self.parked {
            // Park, then wake ourselves from a timer task: `Poll::Pending` here
            // is what turns the guest's `stream.write` into BLOCKED.
            self.parked = true;
            let waker = cx.waker().clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1)).await;
                waker.wake();
            });
            return Poll::Pending;
        }
        self.parked = false;
        let n = buf.len().min(4096);
        self.written
            .lock()
            .expect("written")
            .extend_from_slice(&buf[..n]);
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Compile and run `source` against a stdout that parks on every write,
/// returning everything the guest managed to write.
fn run_with_backpressured_stdout(source: &'static str, what: &'static str) -> Vec<u8> {
    let written: Written = Arc::new(Mutex::new(Vec::new()));
    let stdout = BackpressuredStdout(written.clone());
    run_with_stdout(source, stdout, &written).unwrap_or_else(|e| panic!("{what}: {e}"))
}

/// A stream that accepts everything, for reading back what the guest wrote.
#[derive(Clone)]
struct CapturingStream(Written);

impl IsTerminal for CapturingStream {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for CapturingStream {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(CapturingWriter(self.0.clone()))
    }
}

struct CapturingWriter(Written);

impl AsyncWrite for CapturingWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.0.lock().expect("written").extend_from_slice(buf);
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Run `source` on its own thread against `stdout`, returning what it wrote —
/// or the run's own error, which the caller may be asserting on.
fn run_with_stdout<S>(source: &'static str, stdout: S, written: &Written) -> Result<Vec<u8>, String>
where
    S: StdoutStream + 'static,
{
    run_with_streams(source, stdout, None, None, written)
}

fn run_with_streams<S>(
    source: &'static str,
    stdout: S,
    stderr: Option<CapturingStream>,
    stdin: Option<TruncatedStdin>,
    written: &Written,
) -> Result<Vec<u8>, String>
where
    S: StdoutStream + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(compile_and_run_with_stdout(source, stdout, stderr, stdin));
        })
        .expect("spawn");
    let outcome = match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(result) => result,
        Err(_) => panic!("timed out — a blocked write never completed"),
    };
    let bytes = written.lock().expect("written").clone();
    outcome.map(|()| bytes)
}

fn compile_and_run_with_stdout<S>(
    source: &str,
    stdout: S,
    stderr: Option<CapturingStream>,
    stdin: Option<TruncatedStdin>,
) -> Result<(), String>
where
    S: StdoutStream + 'static,
{
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

    let engine = dovetail::p3::p3_engine()?;
    let component =
        Component::new(&engine, &wasm_bytes).map_err(|e| format!("component load error: {e:?}"))?;

    let mut linker = Linker::<State>::new(&engine);
    wasmtime_wasi::p3::add_to_linker(&mut linker)
        .map_err(|e| format!("failed to add WASI to linker: {e}"))?;

    let mut builder = WasiCtxBuilder::new();
    builder.stdout(stdout);
    if let Some(stderr) = stderr {
        builder.stderr(stderr);
    }
    if let Some(stdin) = stdin {
        builder.stdin(stdin);
    }
    let mut store = Store::new(&engine, State::new(builder.build()));

    dovetail::p3::run_cli_component(&component, &linker, &mut store)
}

/// A `debug` whose every chunk parks the host must still deliver every byte in
/// order. This is the only coverage the BLOCKED arm of `emit_flush_write` has:
/// removing the `waitable.join` that arms the flush set, or letting anything
/// else post on that stream end, turns this into a hang rather than wrong bytes.
#[test]
fn debug_completes_against_a_stdout_that_blocks() {
    // 64 KiB in 4 KiB host bites = 16 parked rounds, and `debug` appends "\n".
    let written = run_with_backpressured_stdout(
        r#"
package a

function main(): Unit =
    debug("abcdefgh".repeat(8192))
"#,
        "blocked debug",
    );
    let expected: Vec<u8> = "abcdefgh".repeat(8192).into_bytes();
    assert_eq!(
        written.len(),
        expected.len() + 1,
        "wrote a different number of bytes"
    );
    assert_eq!(
        &written[..expected.len()],
        &expected[..],
        "bytes came back reordered or corrupted"
    );
    assert_eq!(written[expected.len()], b'\n');
}

/// Several blocked writes in a row: each has to leave the stream end idle and
/// the flush set empty for the next one, or the second write picks up the
/// first's completion event and miscounts its progress.
#[test]
fn repeated_debug_completes_against_a_stdout_that_blocks() {
    let written = run_with_backpressured_stdout(
        r#"
package a

function main(): Unit =
    let mutable i = 0
    while i < 3 do
        debug("0123456789".repeat(1024))
        i = i + 1
"#,
        "repeated blocked debug",
    );
    let one: String = "0123456789".repeat(1024) + "\n";
    let expected = one.repeat(3).into_bytes();
    assert_eq!(
        written, expected,
        "three blocked writes did not concatenate cleanly"
    );
}

// ── A stdout that goes away mid-write ───────────────────────────────────────
//
// `dovetail run app | head`: once head exits the pipe breaks, and the host turns
// the write error into a DROPPED completion. The blocking path used to execute
// `unreachable` on any non-COMPLETED status, so a closed pipe killed the
// component — and on the panic path it replaced the panic report with a bare
// stdio trap. It is best-effort now: stop writing, return, let the caller decide.

/// A stdout that takes `limit` bytes and then reports a broken pipe, which
/// wasmtime turns into `StreamResult::Dropped` for the guest.
#[derive(Clone)]
struct BrokenPipeStdout {
    written: Written,
    limit: usize,
}

impl IsTerminal for BrokenPipeStdout {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdoutStream for BrokenPipeStdout {
    fn async_stream(&self) -> Box<dyn AsyncWrite + Send + Sync> {
        Box::new(BreakAfterWriter {
            written: self.written.clone(),
            remaining: self.limit,
        })
    }
}

struct BreakAfterWriter {
    written: Written,
    remaining: usize,
}

impl AsyncWrite for BreakAfterWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.remaining == 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "reader is gone",
            )));
        }
        let n = buf.len().min(self.remaining);
        self.written
            .lock()
            .expect("written")
            .extend_from_slice(&buf[..n]);
        self.remaining -= n;
        Poll::Ready(Ok(n))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// The write that outruns the pipe, and every write after it, must return
/// normally — the program's own outcome is what the run reports.
#[test]
fn debug_survives_a_stdout_that_breaks_mid_write() {
    let written: Written = Arc::new(Mutex::new(Vec::new()));
    let stdout = BrokenPipeStdout {
        written: written.clone(),
        limit: 4096,
    };
    let result = run_with_stdout(
        r#"
package a

function main(): Unit =
    // The first debug outruns the 4 KiB the reader accepts; the second finds
    // the stream already dropped. Neither may take the component down.
    debug("abcdefgh".repeat(2048))
    debug("after the pipe broke")
    assert 1 + 1 == 2
"#,
        stdout,
        &written,
    );
    let bytes = result.expect("a broken stdout pipe must not fail the run");
    // Best-effort: what the reader took before it left, and nothing after.
    assert_eq!(
        bytes.len(),
        4096,
        "the reader's accepted prefix should still arrive"
    );
    assert_eq!(&bytes[..8], b"abcdefgh");
}

/// The program's own failure still gets reported when stdout is gone: the panic
/// text must reach stderr rather than being pre-empted by a trap from a `debug`
/// nobody can read. `limit: 0` breaks the pipe on the very first write.
#[test]
fn a_panic_survives_a_broken_stdout_pipe() {
    let stdout_bytes: Written = Arc::new(Mutex::new(Vec::new()));
    let stderr_bytes: Written = Arc::new(Mutex::new(Vec::new()));
    let stdout = BrokenPipeStdout {
        written: stdout_bytes.clone(),
        limit: 0,
    };
    let result = run_with_streams(
        r#"
package a

function main(): Unit =
    debug("this never reaches anyone")
    panic "the program's own failure"
"#,
        stdout,
        Some(CapturingStream(stderr_bytes.clone())),
        None,
        &stdout_bytes,
    );
    result.expect_err("the panic must still fail the run");
    let reported = String::from_utf8_lossy(&stderr_bytes.lock().expect("stderr")).to_string();
    assert!(
        reported.contains("the program's own failure"),
        "the panic report was lost behind a broken stdout: {reported:?}"
    );
    assert!(stdout_bytes.lock().expect("stdout").is_empty());
}

// ── A stdin that fails part-way through ─────────────────────────────────────
//
// The host drops the stdin stream both at end-of-input and when a read fails
// (a failing redirected file, a reset pipe) — the difference is only in the
// stream's result future. `Console`'s stdin wrapper hardcoded `Ok(())` there,
// so a mid-read failure latched as a clean EOF and the program processed a
// truncated input as a complete one, exiting successfully.

/// What a `TruncatedStdin` does once its bytes are gone.
#[derive(Clone, Copy)]
enum ThenWhat {
    /// Report a broken pipe — the host turns it into a resolved result future.
    Fail,
    /// Never produce anything again, and never wake. A reader that parks here
    /// parks for good, which is the point when the test is about whether a
    /// reader parks at all.
    Stall,
}

/// A stdin that hands over its bytes and then does `ThenWhat`.
#[derive(Clone)]
struct TruncatedStdin(&'static [u8], ThenWhat);

impl IsTerminal for TruncatedStdin {
    fn is_terminal(&self) -> bool {
        false
    }
}

impl StdinStream for TruncatedStdin {
    fn async_stream(&self) -> Box<dyn AsyncRead + Send + Sync> {
        Box::new(FailAfterReader {
            remaining: self.0,
            then: self.1,
        })
    }
}

struct FailAfterReader {
    remaining: &'static [u8],
    then: ThenWhat,
}

impl AsyncRead for FailAfterReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.remaining.is_empty() {
            return match self.then {
                ThenWhat::Fail => Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "writer is gone",
                ))),
                ThenWhat::Stall => Poll::Pending,
            };
        }
        let n = self.remaining.len().min(buf.remaining());
        buf.put_slice(&self.remaining[..n]);
        self.remaining = &self.remaining[n..];
        Poll::Ready(Ok(()))
    }
}

/// stdin failing mid-stream must fail the program, not look like end-of-input.
///
/// Driven through the `standard-io-interop` fixture: `Console` lives in the
/// workspace, not in the prelude the single-file harness compiles against, and
/// the in-process test runner inherits the real stdin and cannot be fed.
#[test]
fn a_stdin_that_fails_mid_stream_is_not_end_of_input() {
    let wasm = build_interop_wasm("standard-io-interop");
    let stdout_bytes: Written = Arc::new(Mutex::new(Vec::new()));
    let stderr_bytes: Written = Arc::new(Mutex::new(Vec::new()));
    let result = run_interop_with_timeout(
        wasm,
        CapturingStream(stdout_bytes.clone()),
        CapturingStream(stderr_bytes.clone()),
        "echo-lines",
        TruncatedStdin(b"hello\nworld", ThenWhat::Fail),
    );
    result.expect_err("a truncated stdin must fail the program, not end it cleanly");
    let out = String::from_utf8_lossy(&stdout_bytes.lock().expect("stdout")).to_string();
    let err = String::from_utf8_lossy(&stderr_bytes.lock().expect("stderr")).to_string();
    // The complete line before the failure is still the program's to process.
    assert!(
        out.contains("line: hello"),
        "the complete line should still be delivered: {out:?}"
    );
    assert!(
        !out.contains("line: world"),
        "the truncated tail is not a line and must not be reported as one: {out:?}"
    );
    assert!(
        err.contains("broken pipe"),
        "the transport failure should be reported, not swallowed as EOF: {err:?}"
    );
}

/// Build the workspace and return the WASM for `project`. Same shape as the
/// helper in `http_interop.rs`.
fn build_interop_wasm(project: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("dovetail crate must live in a parent dir")
        .to_path_buf();
    let workspace = dovetail::manifest::load_manifest(&root)
        .unwrap_or_else(|errs| panic!("manifest load errors: {errs:?}"));
    // The project filter matters: in Build mode only the LAST project in
    // topological order gets codegen, so ask for this one by name.
    let result = dovetail::build_workspace(
        &workspace,
        Some(project),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
        None,
    );
    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| format!("{}: {}", d.span.file, d.message))
            .collect();
        panic!("build_workspace failed:\n{}", errors.join("\n"));
    }
    let (_, project_result) = result
        .project_results
        .iter()
        .find(|(name, _)| name == project)
        .unwrap_or_else(|| panic!("project '{project}' not in build results"));
    project_result
        .wasm
        .clone()
        .unwrap_or_else(|| panic!("project '{project}' has no WASM output"))
}

/// Run the fixture on its own thread, abandoning it if it hangs — which is
/// exactly the failure some of these tests are looking for.
fn run_interop_with_timeout(
    wasm: Vec<u8>,
    stdout: CapturingStream,
    stderr: CapturingStream,
    mode: &'static str,
    stdin: TruncatedStdin,
) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let _ = tx.send(run_interop_component(&wasm, stdout, stderr, mode, stdin));
        })
        .expect("spawn");
    match rx.recv_timeout(Duration::from_secs(60)) {
        Ok(result) => result,
        Err(_) => panic!("timed out — a reader never returned"),
    }
}

fn run_interop_component(
    wasm: &[u8],
    stdout: CapturingStream,
    stderr: CapturingStream,
    mode: &str,
    stdin: TruncatedStdin,
) -> Result<(), String> {
    let engine = dovetail::p3::p3_engine()?;
    let component =
        Component::new(&engine, wasm).map_err(|e| format!("component load error: {e:?}"))?;
    let mut linker = Linker::<State>::new(&engine);
    wasmtime_wasi::p3::add_to_linker(&mut linker)
        .map_err(|e| format!("failed to add WASI to linker: {e}"))?;

    let mut builder = WasiCtxBuilder::new();
    builder.env("MODE", mode);
    builder.stdout(stdout);
    builder.stderr(stderr);
    builder.stdin(stdin);
    let mut store = Store::new(&engine, State::new(builder.build()));
    dovetail::p3::run_cli_component(&component, &linker, &mut store)
}

/// Two fibers reading lines from one stdin, with a single chunk carrying both
/// lines and nothing following it. `readLine` used to check its buffer, then
/// queue for the stream lock, and post a fresh host read the moment it was
/// granted — so the second reader waited for input that had already arrived and
/// would never arrive again.
#[test]
fn a_second_reader_takes_the_line_already_buffered() {
    let wasm = build_interop_wasm("standard-io-interop");
    let stdout_bytes: Written = Arc::new(Mutex::new(Vec::new()));
    let stderr_bytes: Written = Arc::new(Mutex::new(Vec::new()));
    run_interop_with_timeout(
        wasm,
        CapturingStream(stdout_bytes.clone()),
        CapturingStream(stderr_bytes.clone()),
        "two-readers",
        // One chunk, two lines, then silence forever.
        TruncatedStdin(b"alpha\nbeta\n", ThenWhat::Stall),
    )
    .expect("both readers should be satisfied by the one chunk");
    let out = String::from_utf8_lossy(&stdout_bytes.lock().expect("stdout")).to_string();
    assert!(
        out.contains("first: alpha"),
        "first reader got the wrong line: {out:?}"
    );
    assert!(
        out.contains("second: beta"),
        "second reader got the wrong line: {out:?}"
    );
}
