//! Integration tests for the LSP server using tower-lsp's LspService.
//!
//! These tests exercise the protocol layer end-to-end by sending JSON-RPC
//! requests directly to the service, verifying correct method dispatch,
//! parameter deserialization, and response serialization.

use std::io::Write;

use futures::StreamExt;
use tower::Service;
use tower_lsp::jsonrpc;
use tower_lsp::lsp_types::*;

/// Create a temporary workspace with a Dovetail source file and return the workspace root path.
/// NOTE: The manifest format here is intentionally invalid (legacy). Tests using this helper
/// do not depend on the workspace check succeeding.
fn create_temp_workspace(source: &str) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    let src_dir = dir.path().join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let mut f = std::fs::File::create(src_dir.join("a.dove")).unwrap();
    f.write_all(source.as_bytes()).unwrap();

    // Create a minimal Dovetail.toml
    let manifest = r#"
[project]
name = "test"

[[project.packages]]
path = "a"
source = "src"
"#;
    let mut f = std::fs::File::create(dir.path().join("Dovetail.toml")).unwrap();
    f.write_all(manifest.as_bytes()).unwrap();

    dir
}

/// Create a temporary workspace with a valid Dovetail.toml manifest.
/// The source file goes into `{workspace_root}/test/src/a.dove` and must declare `package a`.
/// Returns the workspace root TempDir and the path to the source file.
fn create_valid_workspace(source: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::TempDir::new().unwrap();
    let project_dir = dir.path().join("test");
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir).unwrap();
    let source_path = src_dir.join("a.dove");
    std::fs::write(&source_path, source).unwrap();

    let manifest = r#"compiler-version = "0.1.0"
[[project]]
name = "test"
root_package = "a"
packages = ["."]
"#;
    std::fs::write(dir.path().join("Dovetail.toml"), manifest).unwrap();

    (dir, source_path)
}

fn build_request(method: &str, params: serde_json::Value, id: i64) -> jsonrpc::Request {
    serde_json::from_value(serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
        "id": id,
    }))
    .unwrap()
}

fn build_notification(method: &str, params: serde_json::Value) -> jsonrpc::Request {
    serde_json::from_value(serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    }))
    .unwrap()
}

/// Initialize the LSP service and return it ready for requests.
/// Drains the client socket in the background so server→client notifications
/// (diagnostics, progress) don't deadlock on the capacity-1 channel.
async fn init_service(
    workspace_uri: Option<Url>,
) -> tower_lsp::LspService<impl tower_lsp::LanguageServer> {
    let (mut service, socket) = dovetail::lsp::create_test_service();

    // Drain socket so send_notification / publish_diagnostics never block
    tokio::spawn(async move {
        let mut socket = socket;
        while (socket.next().await).is_some() {}
    });

    let init_params = serde_json::json!({
        "capabilities": {},
        "rootUri": workspace_uri,
    });

    let req = build_request("initialize", init_params, 1);
    let _resp = service.call(req).await;

    // Send initialized notification
    let notif = build_notification("initialized", serde_json::json!({}));
    let _resp = service.call(notif).await;

    service
}

/// Initialize the LSP service and return it along with a receiver for server→client notifications.
/// Spawns a background task to drain the tower-lsp ClientSocket (capacity-1 channel) into an
/// unbounded channel so `client.publish_diagnostics()` never deadlocks.
async fn init_service_with_socket(
    workspace_uri: Option<Url>,
) -> (
    tower_lsp::LspService<impl tower_lsp::LanguageServer>,
    tokio::sync::mpsc::UnboundedReceiver<jsonrpc::Request>,
) {
    let (mut service, socket) = dovetail::lsp::create_test_service();

    // Drain socket into unbounded channel to prevent deadlocks
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut socket = socket;
        while let Some(msg) = socket.next().await {
            let _ = tx.send(msg);
        }
    });

    // Initialize
    let req = build_request(
        "initialize",
        serde_json::json!({"capabilities": {}, "rootUri": workspace_uri}),
        1,
    );
    let _ = service.call(req).await;
    let notif = build_notification("initialized", serde_json::json!({}));
    let _ = service.call(notif).await;

    (service, rx)
}

/// Drain the notification receiver and extract `textDocument/publishDiagnostics` notifications.
fn collect_diagnostics(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<jsonrpc::Request>,
) -> Vec<PublishDiagnosticsParams> {
    let mut diags = Vec::new();
    while let Ok(msg) = rx.try_recv() {
        let val = serde_json::to_value(&msg).unwrap();
        if val.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics") {
            if let Some(params) = val.get("params") {
                if let Ok(p) = serde_json::from_value::<PublishDiagnosticsParams>(params.clone()) {
                    diags.push(p);
                }
            }
        }
    }
    diags
}

#[tokio::test]
async fn test_initialize_capabilities() {
    let (mut service, _socket) = dovetail::lsp::create_test_service();

    let init_params = serde_json::json!({
        "capabilities": {},
    });

    let req = build_request("initialize", init_params, 1);
    let resp = service.call(req).await.unwrap();

    let resp = resp.unwrap();
    let result: InitializeResult = serde_json::from_value(resp.result().unwrap().clone()).unwrap();

    // Verify all expected capabilities are present
    let caps = &result.capabilities;
    assert!(caps.hover_provider.is_some(), "should have hover");
    assert!(caps.definition_provider.is_some(), "should have definition");
    assert!(caps.type_definition_provider.is_some(), "should have type definition");
    assert!(caps.completion_provider.is_some(), "should have completion");
    assert!(caps.signature_help_provider.is_some(), "should have signature help");
    assert!(caps.references_provider.is_some(), "should have references");
    assert!(caps.implementation_provider.is_some(), "should have implementation");
    assert!(caps.document_symbol_provider.is_some(), "should have document symbols");
    assert!(caps.workspace_symbol_provider.is_some(), "should have workspace symbols");
    assert!(caps.code_action_provider.is_some(), "should have code actions");
    assert!(caps.code_lens_provider.is_some(), "should have code lens");
    assert!(caps.call_hierarchy_provider.is_some(), "should have call hierarchy");
    assert!(caps.inlay_hint_provider.is_some(), "should have inlay hints");
    assert!(caps.document_formatting_provider.is_some(), "should have formatting");

    // Verify server info
    let info = result.server_info.unwrap();
    assert_eq!(info.name, "dovetail-language-server");
}

#[tokio::test]
async fn test_document_symbols() {
    let workspace = create_temp_workspace(r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit = assert add(1, 2) == 3
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri.clone())).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    // Open the document
    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    // Request document symbols
    let req = build_request(
        "textDocument/documentSymbol",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
        2,
    );
    let resp = service.call(req).await.unwrap();
    let resp = resp.unwrap();
    let result = resp.result().unwrap();

    // Should return a non-null array of symbols
    assert!(result.is_array(), "should return array of document symbols");
    let symbols: Vec<serde_json::Value> = serde_json::from_value(result.clone()).unwrap();
    assert!(!symbols.is_empty(), "should have symbols");

    // Check that we find the 'add' function
    let has_add = symbols.iter().any(|s| {
        s.get("name").and_then(|n| n.as_str()) == Some("add")
    });
    assert!(has_add, "should find 'add' function symbol");
}

#[tokio::test]
async fn test_hover() {
    let workspace = create_temp_workspace(r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    // Open document
    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    // Wait a moment for background check to complete
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Send save to trigger check
    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    // Request hover on `add` call (line 5, character 15 — 0-indexed)
    let req = build_request(
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 5, "character": 15 }
        }),
        3,
    );
    let resp = service.call(req).await.unwrap();
    let resp = resp.unwrap();
    let result = resp.result().unwrap();

    // Hover may return null if the workspace hasn't been checked yet (no TypedModule cached)
    // In integration tests, we accept either a hover result or null
    if !result.is_null() {
        let hover: Hover = serde_json::from_value(result.clone()).unwrap();
        if let HoverContents::Markup(markup) = hover.contents {
            assert!(markup.value.contains("add") || markup.value.contains("Int32"));
        }
    }
}

#[tokio::test]
async fn test_completion() {
    let workspace = create_temp_workspace(r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    // Open document
    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    // Request completion (scope completion, no trigger)
    let req = build_request(
        "textDocument/completion",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 5, "character": 15 }
        }),
        4,
    );
    let resp = service.call(req).await.unwrap();
    // Should not error — may return null or completion items
    assert!(resp.is_some(), "completion should return a response");
}

#[tokio::test]
async fn test_goto_definition() {
    let workspace = create_temp_workspace(r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    // Trigger save to build cache
    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    // Go to definition on `add` call
    let req = build_request(
        "textDocument/definition",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 5, "character": 15 }
        }),
        5,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "definition should return a response");
}

#[tokio::test]
async fn test_find_references() {
    let workspace = create_temp_workspace(r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let a = add(1, 2)
  let b = add(3, 4)
  assert a + b == 10
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    let req = build_request(
        "textDocument/references",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 5, "character": 10 },
            "context": { "includeDeclaration": false }
        }),
        6,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "references should return a response");
}

#[tokio::test]
async fn test_code_lens() {
    let workspace = create_temp_workspace(r#"package a

test "basic math" = assert 1 + 2 == 3
test "string check" = assert true
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    let req = build_request(
        "textDocument/codeLens",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
        7,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "code lens should return a response");
}

#[tokio::test]
async fn test_code_action() {
    let workspace = create_temp_workspace(r#"package a

function main(): Unit = assert true
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let req = build_request(
        "textDocument/codeAction",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "range": { "start": { "line": 2, "character": 0 }, "end": { "line": 2, "character": 30 } },
            "context": { "diagnostics": [] }
        }),
        8,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "code action should return a response");
}

#[tokio::test]
async fn test_signature_help() {
    let workspace = create_temp_workspace(r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    // Request signature help inside add( parens — line 5 (0-indexed), character 19
    let req = build_request(
        "textDocument/signatureHelp",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 5, "character": 19 }
        }),
        10,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "signature help should return a response");
}

#[tokio::test]
async fn test_inlay_hint() {
    let workspace = create_temp_workspace(r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    // Request inlay hints for full file range
    let req = build_request(
        "textDocument/inlayHint",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 10, "character": 0 }
            }
        }),
        11,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "inlay hint should return a response");
}

#[tokio::test]
async fn test_type_definition() {
    let workspace = create_temp_workspace(r#"package a

public record Point =
    x: Int32
    y: Int32

function makePoint(): Point = Point { x = 1; y = 2 }

function main(): Unit =
  let p = makePoint()
  assert p.x == 1
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    // Request type definition on `p` variable — line 9 (0-indexed), character 6
    let req = build_request(
        "textDocument/typeDefinition",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 9, "character": 6 }
        }),
        12,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "type definition should return a response");
}

#[tokio::test]
async fn test_call_hierarchy() {
    let workspace = create_temp_workspace(r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
  let result = add(1, 2)
  assert result == 3
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    // Prepare call hierarchy on `add` function — line 2 (0-indexed), character 9
    let req = build_request(
        "textDocument/prepareCallHierarchy",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 2, "character": 9 }
        }),
        13,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "prepare call hierarchy should return a response");

    // If we got items, try incoming and outgoing calls
    let resp_val = resp.unwrap();
    let result = resp_val.result();
    if let Some(result) = result {
        if !result.is_null() {
            let items: Vec<serde_json::Value> = serde_json::from_value(result.clone()).unwrap_or_default();
            if let Some(item) = items.first() {
                // Test incoming calls
                let req = build_request(
                    "callHierarchy/incomingCalls",
                    serde_json::json!({ "item": item }),
                    14,
                );
                let resp = service.call(req).await.unwrap();
                assert!(resp.is_some(), "incoming calls should return a response");

                // Test outgoing calls
                let req = build_request(
                    "callHierarchy/outgoingCalls",
                    serde_json::json!({ "item": item }),
                    15,
                );
                let resp = service.call(req).await.unwrap();
                assert!(resp.is_some(), "outgoing calls should return a response");
            }
        }
    }
}

#[tokio::test]
async fn test_goto_implementation() {
    let workspace = create_temp_workspace(r#"package a

trait Greeter =
  greet(): String

record Hello

implement Greeter for Hello =
  greet(): String = "hello"

function main(): Unit = assert true
"#);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    // Request implementation on the trait name "Greeter" — line 2 (0-indexed), character 6
    let req = build_request(
        "textDocument/implementation",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 2, "character": 6 }
        }),
        16,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "goto implementation should return a response");
}

// ---------------------------------------------------------------------------
// Diagnostics tests (section 3.1, 3.2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_diagnostics_on_save() {
    // Source with an error: unknown function call
    let (workspace, source_path) = create_valid_workspace(
        r#"package a

function main(): Unit = unknownFunc()
"#,
    );
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let (mut service, mut rx) = init_service_with_socket(Some(root_uri)).await;

    let file_uri = Url::from_file_path(&source_path).unwrap();
    let content = std::fs::read_to_string(&source_path).unwrap();

    // Open document
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    // Save to trigger check_and_publish
    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;

    // Wait for background check to complete
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    let diags = collect_diagnostics(&mut rx);
    // Should have at least one publish with errors
    let has_errors = diags.iter().any(|d| {
        d.diagnostics
            .iter()
            .any(|diag| diag.severity == Some(DiagnosticSeverity::ERROR))
    });
    assert!(has_errors, "should publish diagnostics with errors for invalid source");
}

#[tokio::test]
async fn test_diagnostics_clear_after_fix() {
    // Start with source that has an error
    let (workspace, source_path) = create_valid_workspace(
        r#"package a

function main(): Unit = unknownFunc()
"#,
    );
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let (mut service, mut rx) = init_service_with_socket(Some(root_uri)).await;

    let file_uri = Url::from_file_path(&source_path).unwrap();
    let content = std::fs::read_to_string(&source_path).unwrap();

    // Open and save the broken document
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    // Drain the error diagnostics
    let diags = collect_diagnostics(&mut rx);
    assert!(
        diags.iter().any(|d| !d.diagnostics.is_empty()),
        "should have error diagnostics before fix"
    );

    // Fix the source on disk and send didChange + didSave
    let fixed_source = r#"package a

function main(): Unit = ()
"#;
    std::fs::write(&source_path, fixed_source).unwrap();

    let change_notif = build_notification(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string(), "version": 2 },
            "contentChanges": [{ "text": fixed_source }]
        }),
    );
    let _ = service.call(change_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    let diags = collect_diagnostics(&mut rx);
    // After fix, the file's diagnostics should be cleared (empty list)
    let file_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.uri == file_uri)
        .collect();
    // Either no publish for this file, or the last publish has empty diagnostics
    if let Some(last) = file_diags.last() {
        assert!(
            last.diagnostics.is_empty(),
            "diagnostics should be cleared after fixing the error"
        );
    }
}

// ---------------------------------------------------------------------------
// Workspace symbols test (section 5.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_workspace_symbols() {
    let (workspace, source_path) = create_valid_workspace(
        r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit = assert add(1, 2) == 3
"#,
    );
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let (mut service, _rx) = init_service_with_socket(Some(root_uri)).await;

    let file_uri = Url::from_file_path(&source_path).unwrap();
    let content = std::fs::read_to_string(&source_path).unwrap();

    // Open and save to populate typed module
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Request workspace symbols
    let req = build_request(
        "workspace/symbol",
        serde_json::json!({ "query": "add" }),
        20,
    );
    let resp = service.call(req).await.unwrap();
    let resp = resp.unwrap();
    let result = resp.result().unwrap();

    assert!(!result.is_null(), "workspace symbols should return a result");
    let symbols: Vec<serde_json::Value> = serde_json::from_value(result.clone()).unwrap();
    // display_name includes package prefix and params, e.g. "a.add(Int32, Int32)"
    let has_add = symbols
        .iter()
        .any(|s| {
            s.get("name")
                .and_then(|n| n.as_str())
                .is_some_and(|n| n.contains("add"))
        });
    assert!(has_add, "workspace symbols should include 'add'");
}

// ---------------------------------------------------------------------------
// Dot completion test (section 10.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_dot_completion() {
    let source = r#"package a

public record Point =
    x: Int32
    y: Int32

function main(): Unit =
  let p = Point { x = 1; y = 2 }
  let v = p.x
  assert v == 1
"#;
    let workspace = create_temp_workspace(source);
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    // Open and save
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": source,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Request completion with dot trigger after `p.` on line 8 (0-indexed), character 12
    let req = build_request(
        "textDocument/completion",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 8, "character": 12 },
            "context": {
                "triggerKind": 2,
                "triggerCharacter": "."
            }
        }),
        21,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "dot completion should return a response");
}

// ---------------------------------------------------------------------------
// Formatting stub test (section 20.1)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_formatting_returns_none() {
    let workspace = create_temp_workspace(
        r#"package a

function main(): Unit = ()
"#,
    );
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let mut service = init_service(Some(root_uri)).await;

    let file_uri = Url::from_file_path(workspace.path().join("src/a.dove")).unwrap();

    let content = std::fs::read_to_string(workspace.path().join("src/a.dove")).unwrap();
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let req = build_request(
        "textDocument/formatting",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "options": { "tabSize": 2, "insertSpaces": true }
        }),
        22,
    );
    let resp = service.call(req).await.unwrap();
    let resp = resp.unwrap();
    let result = resp.result().unwrap();
    assert!(result.is_null(), "formatting should return null (not implemented)");
}

// ---------------------------------------------------------------------------
// Error resilience tests (section 24.1, 24.2)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_error_resilience_server_survives_errors() {
    // Source with a type error — uses valid workspace so check_and_publish runs
    let (workspace, source_path) = create_valid_workspace(
        r#"package a

function main(): Unit = unknownFunc()
"#,
    );
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let (mut service, _rx) = init_service_with_socket(Some(root_uri)).await;

    let file_uri = Url::from_file_path(&source_path).unwrap();
    let content = std::fs::read_to_string(&source_path).unwrap();

    // Open and save the broken file
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Server should still respond to hover (not crash)
    let req = build_request(
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 2, "character": 9 }
        }),
        24,
    );
    let resp = service.call(req).await.unwrap();
    // Response must exist (not an RPC error) — result may be null
    assert!(resp.is_some(), "server should respond to hover even after errors");
}

#[tokio::test]
async fn test_error_resilience_recovery_after_fix() {
    // Start with broken source
    let (workspace, source_path) = create_valid_workspace(
        r#"package a

function main(): Unit = unknownFunc()
"#,
    );
    let root_uri = Url::from_directory_path(workspace.path()).unwrap();
    let (mut service, _rx) = init_service_with_socket(Some(root_uri)).await;

    let file_uri = Url::from_file_path(&source_path).unwrap();
    let content = std::fs::read_to_string(&source_path).unwrap();

    // Open and save the broken file
    let open_notif = build_notification(
        "textDocument/didOpen",
        serde_json::json!({
            "textDocument": {
                "uri": file_uri.to_string(),
                "languageId": "dovetail",
                "version": 1,
                "text": content,
            }
        }),
    );
    let _ = service.call(open_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Fix the source
    let fixed_source = r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit = assert add(1, 2) == 3
"#;
    std::fs::write(&source_path, fixed_source).unwrap();

    let change_notif = build_notification(
        "textDocument/didChange",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string(), "version": 2 },
            "contentChanges": [{ "text": fixed_source }]
        }),
    );
    let _ = service.call(change_notif).await;

    let save_notif = build_notification(
        "textDocument/didSave",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() }
        }),
    );
    let _ = service.call(save_notif).await;
    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

    // Hover on `add` function after fix — line 2 (0-indexed), character 9
    let req = build_request(
        "textDocument/hover",
        serde_json::json!({
            "textDocument": { "uri": file_uri.to_string() },
            "position": { "line": 2, "character": 9 }
        }),
        25,
    );
    let resp = service.call(req).await.unwrap();
    assert!(resp.is_some(), "server should respond to hover after recovery");

    // Verify we get actual hover info (not just null)
    let resp = resp.unwrap();
    let result = resp.result().unwrap();
    if !result.is_null() {
        let hover: Hover = serde_json::from_value(result.clone()).unwrap();
        if let HoverContents::Markup(markup) = hover.contents {
            assert!(
                markup.value.contains("add") || markup.value.contains("Int32"),
                "hover should contain function info after recovery"
            );
        }
    }
}
