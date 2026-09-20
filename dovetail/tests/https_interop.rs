//! HTTPS interop test: exercises the Dovetail HTTPS client and server against a
//! REAL third-party TLS 1.3 stack (`openssl s_client` / `openssl s_server`) so
//! we catch any wire-level deviation that the Dovetail-vs-Dovetail in-memory test
//! (`standard-io-http/test/https_exchange_test.dove`, where both sides use
//! our own TLS engine) would silently agree on.
//!
//! Two directions:
//!   1. `openssl_client_against_dovetail_https_server` — Dovetail `serveTls` (cert+
//!      key loaded from PEM) terminated by a real `openssl s_client -tls1_3`.
//!   2. `dovetail_https_client_against_openssl_server` — Dovetail HTTPS client (with
//!      a custom trust anchor) against a real `openssl s_server -tls1_3 -www`.
//!
//! Both are gated on a runtime probe (`openssl_tls13_works`) and SKIP (not fail)
//! when the local `openssl` can't complete a TLS 1.3 handshake with itself — so
//! environments without a capable openssl don't red-fail. On a machine with a
//! working `openssl` (LibreSSL >= 3.4 / OpenSSL >= 1.1.1) they run for real.
//!
//! Like the plain `http_interop.rs` tests, these touch real loopback sockets and
//! a subprocess, so they can be timing-sensitive; they are best-effort interop
//! smoke tests, not deterministic unit tests.

#[path = "common/interop.rs"]
mod interop;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Run a WASM component with env vars, filesystem access, and network access.
///
/// We preopen the host root at `/` (`allow_root`) rather than just the temp dir:
/// the Dovetail fs library resolves an absolute path by walking its full segments
/// from the `/` preopen, so a deep `allow_paths` mount (at its own absolute path)
/// wouldn't let `/var/folders/.../cert.pem` resolve. This mirrors
/// `dovetail run --allow-root`.
fn run_tls_wasm(
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
    let fs_permissions = dovetail::runner::FsPermissions {
        allow_cwd: false,
        allow_root: true,
        allow_paths: vec![],
    };
    let net_permissions = dovetail::runner::NetPermissions {
        allow_network: true,
    };
    dovetail::runner::run_component(
        wasm_bytes,
        &fs_permissions,
        &env_permissions,
        &net_permissions,
    )
}

/// Pick an OS-assigned free loopback port, then release it. Small TOCTOU window
/// (same pattern as `http_interop.rs`); acceptable for a local interop test.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to pick port");
    listener.local_addr().expect("local_addr").port()
}

/// Block until something is accepting TCP on `port`, or the deadline passes.
fn wait_for_port(port: u16, deadline: Instant) -> bool {
    let addr = format!("127.0.0.1:{port}").parse().expect("parse addr");
    while Instant::now() < deadline {
        if std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

/// A unique temp dir for this test's PEM files, removed on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "dovetail_https_interop_{}_{}",
            tag,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Generate a self-signed P-256 certificate + SEC1 EC key (PEM) in `dir`. Our
/// TLS server signs with ECDSA-P256, which our key loader and `openssl` both
/// support. Returns (cert_path, key_path).
fn gen_p256_cert(dir: &Path) -> (PathBuf, PathBuf) {
    let key = dir.join("key.pem");
    let cert = dir.join("cert.pem");
    let status = Command::new("openssl")
        .args([
            "ecparam",
            "-name",
            "prime256v1",
            "-genkey",
            "-noout",
            "-out",
        ])
        .arg(&key)
        .status()
        .expect("run openssl ecparam");
    assert!(status.success(), "openssl ecparam failed");
    // `-addext subjectAltName=IP:127.0.0.1` is REQUIRED: the Dovetail HTTPS client
    // now performs RFC 6125 hostname verification, and an IP-literal serverName
    // (127.0.0.1) is matched ONLY against iPAddress SANs (no CN fallback). Without
    // this SAN the legitimate handshake would be (correctly) rejected.
    let status = Command::new("openssl")
        .args([
            "req",
            "-new",
            "-x509",
            "-days",
            "3000",
            "-subj",
            "/CN=127.0.0.1",
            "-addext",
            "subjectAltName=IP:127.0.0.1",
            "-key",
        ])
        .arg(&key)
        .arg("-out")
        .arg(&cert)
        .status()
        .expect("run openssl req");
    assert!(status.success(), "openssl req failed");
    (cert, key)
}

/// Runtime capability probe: can the local `openssl` complete a TLS 1.3
/// handshake with itself? If not (missing binary, LibreSSL without 1.3 server,
/// etc.), the interop tests SKIP rather than fail.
fn openssl_tls13_works() -> bool {
    if Command::new("openssl").arg("version").output().is_err() {
        return false;
    }
    let dir = TempDir::new("probe");
    let (cert, key) = gen_p256_cert(&dir.path);
    let port = free_port();
    let mut server = match Command::new("openssl")
        .args([
            "s_server",
            "-tls1_3",
            "-accept",
            &port.to_string(),
            "-www",
            "-cert",
        ])
        .arg(&cert)
        .arg("-key")
        .arg(&key)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return false,
    };
    let ok = wait_for_port(port, Instant::now() + Duration::from_secs(3)) && {
        // NOTE: no `-quiet` here — the probe greps for the "TLSv1.3" session
        // line that `-quiet` would suppress. (The real tests below keep
        // `-quiet` so stdout carries only application data.)
        let out = Command::new("openssl")
            .args([
                "s_client",
                "-tls1_3",
                "-connect",
                &format!("127.0.0.1:{port}"),
                "-CAfile",
            ])
            .arg(&cert)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output();
        match out {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stderr) + String::from_utf8_lossy(&o.stdout);
                text.contains("TLSv1.3")
            }
            Err(_) => false,
        }
    };
    let _ = server.kill();
    let _ = server.wait();
    ok
}

// ============================================================================
// Direction 1 — real openssl s_client vs Dovetail HTTPS server (serveTls)
// ============================================================================

#[test]
fn openssl_client_against_dovetail_https_server() {
    if !openssl_tls13_works() {
        eprintln!(
            "SKIP openssl_client_against_dovetail_https_server: local openssl can't do TLS 1.3"
        );
        return;
    }
    let wasm = interop::interop_wasm();
    let dir = TempDir::new("server");
    let (cert, key) = gen_p256_cert(&dir.path);
    let port = free_port();

    // Dovetail TLS server runs forever in a thread; dies with the test process.
    let server_wasm = wasm;
    let cert_str = cert.to_string_lossy().to_string();
    let key_str = key.to_string_lossy().to_string();
    let port_str = port.to_string();
    thread::spawn(move || {
        let _ = run_tls_wasm(
            server_wasm,
            vec![
                ("MODE", "server-tls"),
                ("PORT", &port_str),
                ("CERT", &cert_str),
                ("KEY", &key_str),
            ],
        );
    });

    // Generous startup allowance — wasmtime's compile+instantiate of the large
    // interop component can be slow while the other test instantiates it too.
    // This bounds a hang, it is not a latency assertion.
    assert!(
        wait_for_port(port, Instant::now() + Duration::from_secs(60)),
        "Dovetail TLS server never bound on port {port}"
    );

    // Real TLS 1.3 client: hand-rolled HTTP/1.1 GET over the encrypted channel.
    let mut child = Command::new("openssl")
        .args([
            "s_client",
            "-tls1_3",
            "-connect",
            &format!("127.0.0.1:{port}"),
            "-CAfile",
        ])
        .arg(&cert)
        .arg("-quiet")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn openssl s_client");

    {
        let mut stdin = child.stdin.take().expect("s_client stdin");
        stdin
            .write_all(b"GET /hello HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .expect("write request to s_client");
        stdin.flush().expect("flush s_client stdin");
        // Drop stdin so s_client sees EOF after our request.
    }

    // Watchdog: kill s_client if the exchange hangs.
    let mut stdout = child.stdout.take().expect("s_client stdout");
    let mut stderr = child.stderr.take().expect("s_client stderr");
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let errreader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    while !reader.is_finished() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    let out = reader.join().expect("join s_client reader");
    let err = errreader.join().expect("join s_client err reader");
    let response = String::from_utf8_lossy(&out).to_string();
    let diag = String::from_utf8_lossy(&err).to_string();

    assert!(
        response.contains("200"),
        "expected HTTP 200 from Dovetail TLS server;\nstdout:\n{response}\nstderr:\n{diag}"
    );
    assert!(
        response.contains("hello from dovetail"),
        "expected body 'hello from dovetail' from Dovetail TLS server; got:\n{response}"
    );
}

// ============================================================================
// Direction 2 — Dovetail HTTPS client vs real openssl s_server
// ============================================================================

#[test]
fn dovetail_https_client_against_openssl_server() {
    if !openssl_tls13_works() {
        eprintln!(
            "SKIP dovetail_https_client_against_openssl_server: local openssl can't do TLS 1.3"
        );
        return;
    }
    let wasm = interop::interop_wasm();
    let dir = TempDir::new("client");
    let (cert, key) = gen_p256_cert(&dir.path);
    let port = free_port();

    // Real TLS 1.3 server: `-www` answers each request with a 200 status page;
    // `-alpn http/1.1` matches what our client advertises.
    let mut server = Command::new("openssl")
        .args([
            "s_server",
            "-tls1_3",
            "-accept",
            &port.to_string(),
            "-www",
            "-alpn",
            "http/1.1",
            "-cert",
        ])
        .arg(&cert)
        .arg("-key")
        .arg(&key)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn openssl s_server");

    assert!(
        wait_for_port(port, Instant::now() + Duration::from_secs(5)),
        "openssl s_server never bound on port {port}"
    );

    // The Dovetail client asserts status 200 internally (panics on mismatch), so a
    // clean Ok from run_component means the HTTPS round-trip succeeded.
    let url = format!("https://127.0.0.1:{port}/");
    let ca_str = cert.to_string_lossy().to_string();
    let result = run_tls_wasm(
        wasm,
        vec![("MODE", "client-tls"), ("URL", &url), ("CA", &ca_str)],
    );

    let _ = server.kill();
    let _ = server.wait();

    if let Err(e) = result {
        panic!("Dovetail HTTPS client run failed: {}", e.message);
    }
}
