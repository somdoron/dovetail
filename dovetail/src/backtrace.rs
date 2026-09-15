use wasmtime::WasmBacktrace;

/// Internal frame prefixes to filter out of user-facing backtraces.
const INTERNAL_FRAMES: &[&str] = &[
    "run",
    "run_post",
    "realloc",
    "initialize",
    "string_eq",
    "string_concat",
    "string_cmp",
    "char_to_string",
    "string_from_bytes",
    "string_get_char",
    "debug_print",
    "panic_with_message",
];

/// The trap reason, without wasmtime's raw frame dump.
///
/// `{err}` renders as "error while executing at wasm backtrace:" followed by
/// every raw frame whenever a `WasmBacktrace` is attached — which would leak the
/// internal frames `format_backtrace` exists to filter. The actual trap message
/// ("wasm trap: …") is the deepest entry of the error's source chain.
fn trap_reason(err: &wasmtime::Error) -> String {
    let mut reason = None;
    let mut source = std::error::Error::source(err.as_ref() as &dyn std::error::Error);
    while let Some(e) = source {
        reason = Some(e.to_string());
        source = e.source();
    }
    reason.unwrap_or_else(|| "error while executing".to_string())
}

/// Extract a `WasmBacktrace` from a wasmtime error and format a clean,
/// user-friendly backtrace that filters out internal/runtime frames.
pub fn format_backtrace(err: &wasmtime::Error) -> String {
    let Some(bt) = err.downcast_ref::<WasmBacktrace>() else {
        return format!("{err}");
    };

    let mut lines = Vec::new();
    for frame in bt.frames() {
        let name = match frame.func_name() {
            Some(n) => n,
            None => continue,
        };

        if INTERNAL_FRAMES.contains(&name) {
            continue;
        }

        let location = frame.symbols().first().and_then(|sym| {
            let file = sym.file()?;
            let line = sym.line()?;
            let col = sym.column()?;
            Some(format!("{file}:{line}:{col}"))
        });

        match location {
            Some(loc) => lines.push(format!("  {name} ({loc})")),
            None => lines.push(format!("  {name}")),
        }
    }

    if lines.is_empty() {
        return format!("{err}");
    }

    let mut result = format!("{}\nBacktrace:\n", trap_reason(err));
    result.push_str(&lines.join("\n"));
    result
}
