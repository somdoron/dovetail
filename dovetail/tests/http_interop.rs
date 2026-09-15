//! HTTP interop sanity test: exercises the Dovetail HTTP client and server
//! against a real Rust counterparty (raw TCP, hand-rolled HTTP/1.1) so we
//! catch any wire-level deviation that the Dovetail-vs-Dovetail tests in
//! `standard-io-http/test/` would silently agree on.
//!
//! Workflow per test:
//!   1. Build the workspace (via `build_workspace`), find the WASM for
//!      `standard-io-http-interop` (its `Main.dove` switches on the
//!      `MODE` env var).
//!   2. For "Dovetail client" — start a raw TCP server in a Rust thread
//!      and pass its bound port to the Dovetail WASM via `URL` env.
//!   3. For "Dovetail server" — bind a free port in Rust, release it,
//!      hand it to Dovetail via `PORT` env, run the WASM in a thread,
//!      then send a hand-rolled GET via `std::net::TcpStream`.
//!
//! The Dovetail main asserts on the response contents and panics on
//! mismatch, so a successful `run_component` (Ok return) implies the
//! Dovetail side accepted the wire bytes correctly.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at the `dovetail/` crate root; the workspace
    // root is one level up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("dovetail crate must live in a parent dir")
        .to_path_buf()
}

/// Build the workspace once and find the WASM bytes for the named project.
/// Panics with the manifest / build errors on failure.
fn build_interop_wasm(project: &str) -> Vec<u8> {
    let root = repo_root();
    let workspace = dovetail::manifest::load_manifest(&root)
        .unwrap_or_else(|errs| panic!("manifest load errors: {errs:?}"));

    let result = dovetail::build_workspace(
        &workspace,
        None,
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

/// Run a WASM component with the given env + network access.
fn run_with_env(
    wasm_bytes: &[u8],
    env_vars: Vec<(&str, &str)>,
) -> Result<(), dovetail::runner::RunError> {
    let env_permissions = dovetail::runner::EnvPermissions {
        variables: env_vars
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect(),
        args: vec![],
    };
    let net_permissions = dovetail::runner::NetPermissions {
        allow_network: true,
    };
    dovetail::runner::run_component(
        wasm_bytes,
        &dovetail::runner::FsPermissions::default(),
        &env_permissions,
        &net_permissions,
    )
}

// ============================================================================
// Test 1 — Dovetail client vs raw Rust HTTP/1.1 server
// ============================================================================

#[test]
fn dovetail_client_against_raw_rust_server() {
    let wasm = build_interop_wasm("standard-io-http-interop");

    // Start a one-shot Rust TCP server. It accepts one connection, reads
    // until "\r\n\r\n" (request headers complete), checks the request
    // looks like "GET /hello HTTP/1.1", then writes a canned response.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
    let port = listener.local_addr().expect("local_addr").port();

    let (tx, rx) = mpsc::channel::<String>();

    let server_thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = Vec::with_capacity(4096);
        let mut tmp = [0u8; 1024];
        // Read until we see \r\n\r\n (end of headers). The Dovetail client
        // sends GET with no body, so headers-end == message-end here.
        loop {
            let n = stream.read(&mut tmp).expect("read");
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&tmp[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let request = String::from_utf8_lossy(&buf).to_string();
        tx.send(request).expect("send request to test");

        // Canned 200 response.
        let body = b"hello from rust";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write headers");
        stream.write_all(body).expect("write body");
        stream.flush().expect("flush");
        // Closing the stream signals EOF to the client.
    });

    let url = format!("http://127.0.0.1:{port}/hello");
    if let Err(e) = run_with_env(&wasm, vec![("MODE", "client"), ("URL", &url)]) {
        panic!("Dovetail WASM client run failed: {}", e.message);
    }

    server_thread.join().expect("server thread join");
    let request = rx.recv().expect("server should have captured the request");

    // Validate the Dovetail client emitted recognisable HTTP/1.1.
    assert!(
        request.starts_with("GET /hello HTTP/1.1\r\n"),
        "unexpected request line; full request was:\n{request}"
    );
    assert!(
        request.to_ascii_lowercase().contains("host: 127.0.0.1"),
        "missing Host header; full request was:\n{request}"
    );
    assert!(
        request.to_ascii_lowercase().contains("connection: close"),
        "missing Connection: close; full request was:\n{request}"
    );
}

// ============================================================================
// Test 2 — Raw Rust client vs Dovetail HTTP server
// ============================================================================

#[test]
fn raw_rust_client_against_dovetail_server() {
    let wasm = build_interop_wasm("standard-io-http-interop");

    // Get an OS-assigned port, release the listener, hand the port to the
    // Dovetail server. Small TOCTOU window where another process could grab
    // the port; acceptable for a local test.
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind to pick port");
        listener.local_addr().expect("local_addr").port()
    };

    let server_wasm = wasm.clone();
    let port_str = port.to_string();
    thread::spawn(move || {
        // Server loops forever; this thread dies with the test process. Surface
        // any startup failure on stderr — otherwise a server that traps looks
        // identical to one that is merely slow to bind, and the only symptom is
        // the "failed to bind" timeout below.
        if let Err(e) = run_with_env(&server_wasm, vec![("MODE", "server"), ("PORT", &port_str)]) {
            eprintln!("Dovetail interop server exited with error: {}", e.message);
        }
    });

    // Poll the bind by retrying connect with a short timeout — the Dovetail
    // server takes a moment to start up.
    // Generous startup allowance: both tests in this binary run concurrently and
    // each compiles a full Dovetail workspace, so wasmtime's compile+instantiate of
    // the (large) interop component can take well over 5s under that contention.
    // This bounds a hang; it is not a latency assertion.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut stream = loop {
        match std::net::TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().expect("parse addr"),
            Duration::from_millis(250),
        ) {
            Ok(s) => break s,
            Err(_) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => panic!("Dovetail server failed to bind on port {port}: {e}"),
        }
    };

    // Hand-rolled HTTP/1.1 GET.
    let req = format!("GET /hello HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).expect("write request");
    stream.flush().expect("flush request");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set read timeout");

    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read response");
    let response_str = String::from_utf8_lossy(&response).to_string();

    assert!(
        response_str.starts_with("HTTP/1.1 200 OK"),
        "unexpected status line; full response was:\n{response_str}"
    );
    assert!(
        response_str
            .to_ascii_lowercase()
            .contains("content-length: 19"),
        "missing or wrong Content-Length; full response was:\n{response_str}"
    );
    assert!(
        response_str
            .to_ascii_lowercase()
            .contains("connection: close"),
        "missing Connection: close; full response was:\n{response_str}"
    );
    assert!(
        response_str.ends_with("hello from dovetail"),
        "body should be 'hello from dovetail'; full response was:\n{response_str}"
    );

    // Test for unmatched path → 404 via Route.choose's orNotFound.
    let mut stream2 =
        std::net::TcpStream::connect(format!("127.0.0.1:{port}")).expect("second connect");
    let req2 = format!(
        "GET /no/such/route HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    stream2
        .write_all(req2.as_bytes())
        .expect("write second request");
    stream2.flush().expect("flush");
    stream2
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("set timeout");
    let mut response2 = Vec::new();
    stream2
        .read_to_end(&mut response2)
        .expect("read second response");
    let response2_str = String::from_utf8_lossy(&response2).to_string();
    assert!(
        response2_str.starts_with("HTTP/1.1 404"),
        "unmatched path should yield 404; full response was:\n{response2_str}"
    );
}
