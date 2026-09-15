mod common;

#[test]
fn test_debug_sections_present() {
    // Compile a simple program and verify DWARF custom sections exist in the WASM
    let result = dovetail::compile(
        r#"
package a

function main(): Unit = ()
"#,
        "test.dove",
    );
    assert!(!result.diagnostics.has_errors());
    let wasm_bytes = result.wasm.expect("should produce WASM");

    // The component wraps a core module; parse the component to find custom sections
    // We check the core module directly via generate_core_module-style approach,
    // but since we have the component bytes, look for .debug_ sections in them
    let mut found_debug_info = false;
    let mut found_debug_abbrev = false;
    let mut found_debug_line = false;

    for payload in wasmparser::Parser::new(0).parse_all(&wasm_bytes) {
        let payload = payload.expect("valid payload");
        if let wasmparser::Payload::CustomSection(reader) = payload {
            match reader.name() {
                ".debug_info" => found_debug_info = true,
                ".debug_abbrev" => found_debug_abbrev = true,
                ".debug_line" => found_debug_line = true,
                _ => {}
            }
        }
    }

    assert!(
        found_debug_info,
        "expected .debug_info custom section in WASM"
    );
    assert!(
        found_debug_abbrev,
        "expected .debug_abbrev custom section in WASM"
    );
    assert!(
        found_debug_line,
        "expected .debug_line custom section in WASM"
    );
}

#[test]
fn test_trap_backtrace_contains_source_location() {
    // Compile a program that panics and verify the error contains source location info
    let err = common::compile_and_run(
        r#"
package a

function main(): Unit = panic "test error"
"#,
    )
    .expect_err("should trap");

    // The error message should contain the source file name
    assert!(
        err.contains("test.dove"),
        "expected source file in backtrace, got: {err}"
    );
}

#[test]
fn test_assert_backtrace_contains_source_location() {
    let err = common::compile_and_run(
        r#"
package a

function main(): Unit = assert false
"#,
    )
    .expect_err("should trap");

    assert!(
        err.contains("test.dove"),
        "expected source file in backtrace, got: {err}"
    );
}

#[test]
fn test_backtrace_filters_internal_frames() {
    let err = common::compile_and_run(
        r#"
package a

function main(): Unit = panic "test"
"#,
    )
    .expect_err("should trap");

    // Should NOT contain internal frame names
    assert!(
        !err.contains("panic_with_message"),
        "should filter panic_with_message: {err}"
    );
    assert!(!err.contains("!run"), "should filter run frame: {err}");
    // Should contain user function
    assert!(
        err.contains("a.main"),
        "should contain user function: {err}"
    );
}

#[test]
fn test_backtrace_format_structure() {
    let err = common::compile_and_run(
        r#"
package a

function fail(): Unit = panic "boom"

function main(): Unit = fail()
"#,
    )
    .expect_err("should trap");

    // Should start with "Backtrace:"
    assert!(
        err.contains("Backtrace:"),
        "should contain Backtrace header: {err}"
    );
    // Should contain both user frames
    assert!(err.contains("a.fail"), "should contain a.fail: {err}");
    assert!(err.contains("a.main"), "should contain a.main: {err}");
}
