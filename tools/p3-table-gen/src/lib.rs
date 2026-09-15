//! Generate `dovetail/src/compiler/codegen/p3_imports.rs` from the vendored WASI
//! p3 WIT in `dovetail/wit`.
//!
//! The compiler declares its WASI imports by index into one flat table, so the
//! table has to agree with the WIT on three things at once: which canonical
//! builtins each `stream`/`future` payload needs, the core signature of every
//! one of them, and the order the whole lot appears in. Hand-maintaining that
//! is how you get an off-by-one that shows up as a trap in unrelated code, so
//! it is derived instead.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run -p p3-table-gen > dovetail/src/compiler/codegen/p3_imports.rs
//! ```
//!
//! Regenerating should be a no-op unless `dovetail/wit` changed. If the diff is
//! not empty, every renumbered constant is a real change in the emitted module.

use std::collections::HashSet;
use std::fmt::Write as _;

use wit_parser::abi::{AbiVariant, WasmType};
use wit_parser::{Function, FunctionKind, Resolve, Type, TypeDefKind, TypeId, WorldItem, WorldKey};

/// The WIT and the version come from the compiler itself — the same list, in
/// the same order, that `codegen::component::p3_resolve` pushes into its
/// `Resolve`. That order fixes the anonymous type indices in `[stream-read-N]`
/// names, so a table generated from a different order would be wrong in a way
/// that only shows up as a trap somewhere unrelated.
use dovetail::p3::{P3_VERSION, WIT_FILES};

/// One row of the emitted table.
struct Row {
    /// Import module (`$root` or `wasi:ns/iface@version`).
    module: String,
    /// Import field name, canonical-ABI spelling.
    name: String,
    /// Human description for the doc comment.
    description: String,
    params: Vec<&'static str>,
    results: Vec<&'static str>,
}

/// The concurrency builtins the guest calls directly rather than through any
/// one interface. Hand-written because they belong to no WIT function.
fn root_rows() -> Vec<Row> {
    let root = |name: &str,
                description: &str,
                params: Vec<&'static str>,
                results: Vec<&'static str>| Row {
        module: "$root".to_string(),
        name: name.to_string(),
        description: description.to_string(),
        params,
        results,
    };
    vec![
        root(
            "[waitable-set-new]",
            "waitable-set.new",
            vec![],
            vec!["I32"],
        ),
        root(
            "[waitable-set-wait]",
            "waitable-set.wait(set, retptr) -> event",
            vec!["I32", "I32"],
            vec!["I32"],
        ),
        root(
            "[waitable-set-drop]",
            "waitable-set.drop",
            vec!["I32"],
            vec![],
        ),
        root(
            "[waitable-join]",
            "waitable.join(waitable, set)",
            vec!["I32", "I32"],
            vec![],
        ),
        root("[subtask-drop]", "subtask.drop", vec!["I32"], vec![]),
        root(
            "[subtask-cancel]",
            "subtask.cancel -> status",
            vec!["I32"],
            vec!["I32"],
        ),
    ]
}

/// The full text of `p3_imports.rs`, derived from the vendored WIT.
pub fn generate() -> String {
    let mut resolve = Resolve::default();
    for (name, contents) in WIT_FILES {
        resolve
            .push_str(name, contents)
            .unwrap_or_else(|e| panic!("could not parse {name}: {e}"));
    }

    let cli_pkg = resolve
        .package_names
        .get(&wit_parser::PackageName {
            namespace: "wasi".to_string(),
            name: "cli".to_string(),
            version: Some(P3_VERSION.parse().unwrap()),
        })
        .copied()
        .expect("wasi:cli package not found");
    let world_id = resolve
        .select_world(&[cli_pkg], Some("command"))
        .expect("could not select the p3 `command` world");

    let mut rows = root_rows();
    for (key, item) in &resolve.worlds[world_id].imports {
        let WorldItem::Interface { id, .. } = item else {
            continue;
        };
        let module = match key {
            WorldKey::Name(name) => name.clone(),
            WorldKey::Interface(id) => resolve.id_of(*id).expect("interface has no id"),
        };
        let interface = &resolve.interfaces[*id];
        for func in interface.functions.values() {
            rows.extend(function_rows(&resolve, &module, func));
        }
        // Resource drops trail the functions of the interface that owns them.
        for type_id in interface.types.values() {
            let def = &resolve.types[*type_id];
            if !matches!(def.kind, TypeDefKind::Resource) {
                continue;
            }
            let name = def.name.clone().expect("resource without a name");
            rows.push(Row {
                module: module.clone(),
                name: format!("[resource-drop]{name}"),
                description: format!("drop resource {name}"),
                params: vec!["I32"],
                results: vec![],
            });
        }
    }

    // Append new builtins without renumbering the established import table.
    rows.push(Row {
        module: "$root".to_string(),
        name: "[waitable-set-poll]".to_string(),
        description: "waitable-set.poll(set, retptr) -> event, or zero".to_string(),
        params: vec!["I32", "I32"],
        results: vec!["I32"],
    });
    rows.push(Row {
        module: "$root".to_string(),
        name: "[thread-yield]".to_string(),
        description: "thread.yield -> cancellation status".to_string(),
        params: vec![],
        results: vec!["I32"],
    });
    render(&rows)
}

/// The import for `func` itself, followed by the canonical builtins every
/// `stream`/`future` in its signature brings with it.
fn function_rows(resolve: &Resolve, module: &str, func: &Function) -> Vec<Row> {
    let is_async = matches!(
        func.kind,
        FunctionKind::AsyncFreestanding
            | FunctionKind::AsyncMethod(_)
            | FunctionKind::AsyncStatic(_)
    );
    let variant = if is_async {
        AbiVariant::GuestImportAsync
    } else {
        AbiVariant::GuestImport
    };
    let signature = resolve.wasm_signature(variant, func);

    let base_name = if is_async {
        format!("[async-lower]{}", func.name)
    } else {
        func.name.clone()
    };
    let base_description = if is_async {
        format!("{} (async)", func.name)
    } else {
        func.name.clone()
    };
    let mut rows = vec![Row {
        module: module.to_string(),
        name: base_name,
        description: base_description,
        params: signature.params.iter().copied().map(val_type).collect(),
        results: signature.results.iter().copied().map(val_type).collect(),
    }];

    // Payload indices count streams and futures together, params before
    // results, in declaration order — the same numbering the canonical ABI uses
    // in `[stream-read-N]` and friends.
    for (index, (ty, from_params)) in payload_types(resolve, func).into_iter().enumerate() {
        rows.extend(payload_rows(
            resolve,
            module,
            func,
            &ty,
            index as u32,
            from_params,
        ));
    }
    rows
}

/// Every `stream`/`future` reachable from the signature, params first, each
/// paired with whether the guest is the one that creates it.
fn payload_types(resolve: &Resolve, func: &Function) -> Vec<(TypeId, bool)> {
    let mut found = Vec::new();
    let mut seen = HashSet::new();
    for param in &func.params {
        collect_payloads(resolve, param.ty, true, &mut found, &mut seen);
    }
    if let Some(result) = func.result {
        collect_payloads(resolve, result, false, &mut found, &mut seen);
    }
    found
}

fn collect_payloads(
    resolve: &Resolve,
    ty: Type,
    from_params: bool,
    found: &mut Vec<(TypeId, bool)>,
    seen: &mut HashSet<TypeId>,
) {
    let Type::Id(id) = ty else { return };
    if !seen.insert(id) {
        return;
    }
    match &resolve.types[id].kind {
        TypeDefKind::Stream(_) | TypeDefKind::Future(_) => found.push((id, from_params)),
        TypeDefKind::Tuple(tuple) => {
            for ty in &tuple.types {
                collect_payloads(resolve, *ty, from_params, found, seen);
            }
        }
        TypeDefKind::Option(inner) => collect_payloads(resolve, *inner, from_params, found, seen),
        TypeDefKind::Result(result) => {
            if let Some(ok) = result.ok {
                collect_payloads(resolve, ok, from_params, found, seen);
            }
            if let Some(err) = result.err {
                collect_payloads(resolve, err, from_params, found, seen);
            }
        }
        TypeDefKind::Type(inner) => collect_payloads(resolve, *inner, from_params, found, seen),
        _ => {}
    }
}

/// The builtins for one payload.
///
/// Which end the guest owns decides the set: a stream it hands to the host is
/// created guest-side and written to, one the host hands back is only read.
fn payload_rows(
    resolve: &Resolve,
    module: &str,
    func: &Function,
    payload: &TypeId,
    index: u32,
    guest_writes: bool,
) -> Vec<Row> {
    let kind = &resolve.types[*payload].kind;
    let (word, inner) = match kind {
        TypeDefKind::Stream(inner) => ("stream", *inner),
        TypeDefKind::Future(inner) => ("future", *inner),
        _ => unreachable!("payload is neither stream nor future"),
    };
    let description = format!(
        "{} : {word}<{}>",
        func.name,
        inner.map(|ty| type_label(resolve, ty)).unwrap_or_default()
    );

    let mut builtins: Vec<(String, Vec<&'static str>, Vec<&'static str>)> = Vec::new();
    if guest_writes {
        builtins.push((format!("[{word}-new-{index}]"), vec![], vec!["I64"]));
        builtins.push((
            format!("[{word}-cancel-write-{index}]"),
            vec!["I32"],
            vec!["I32"],
        ));
        builtins.push((
            format!("[{word}-drop-writable-{index}]"),
            vec!["I32"],
            vec![],
        ));
        builtins.push((
            format!("[{word}-drop-readable-{index}]"),
            vec!["I32"],
            vec![],
        ));
        builtins.push((
            format!("[async-lower][{word}-write-{index}]"),
            if word == "stream" {
                vec!["I32", "I32", "I32"]
            } else {
                vec!["I32", "I32"]
            },
            vec!["I32"],
        ));
    } else {
        builtins.push((
            format!("[{word}-cancel-read-{index}]"),
            vec!["I32"],
            vec!["I32"],
        ));
        builtins.push((
            format!("[{word}-drop-readable-{index}]"),
            vec!["I32"],
            vec![],
        ));
        builtins.push((
            format!("[async-lower][{word}-read-{index}]"),
            if word == "stream" {
                vec!["I32", "I32", "I32"]
            } else {
                vec!["I32", "I32"]
            },
            vec!["I32"],
        ));
    }

    builtins
        .into_iter()
        .map(|(name, params, results)| Row {
            module: module.to_string(),
            // The payload builtin wraps the function's own import name, so a
            // method keeps its `[method]resource.name` spelling inside.
            name: format!("{name}{}", func.name),
            description: description.clone(),
            params,
            results,
        })
        .collect()
}

/// How a payload type is spelled in the doc comment: its WIT name when it has
/// one, otherwise the structure, which is all an anonymous `result`/`handle`
/// has to identify it by.
fn type_label(resolve: &Resolve, ty: Type) -> String {
    match ty {
        Type::Id(id) => match &resolve.types[id].name {
            Some(name) => name.clone(),
            None => format!("{:?}", resolve.types[id].kind),
        },
        other => format!("{other:?}").to_lowercase(),
    }
}

fn val_type(ty: WasmType) -> &'static str {
    match ty {
        WasmType::I32 | WasmType::Pointer | WasmType::Length => "I32",
        WasmType::I64 | WasmType::PointerOrI64 => "I64",
        WasmType::F32 => "F32",
        WasmType::F64 => "F64",
    }
}

/// `wasi:cli/stdin@0.3.0-…` → `CLI_STDIN`; `$root` → `ROOT`.
fn module_tag(module: &str) -> String {
    if module == "$root" {
        return "ROOT".to_string();
    }
    let without_version = module.split('@').next().unwrap_or(module);
    let (namespace, interface) = without_version
        .split_once('/')
        .expect("interface id has no namespace");
    let _ = namespace;
    let package = without_version
        .split(':')
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .expect("interface id has no package");
    format!("{package}_{interface}")
        .to_uppercase()
        .replace('-', "_")
}

/// `[async-lower][stream-read-0][method]descriptor.read-via-stream`
/// → `ASYNC_STREAM_READ_0_METHOD_DESCRIPTOR_READ_VIA_STREAM`.
fn name_tag(name: &str) -> String {
    let mut out = String::new();
    let mut rest = name;
    while let Some(open) = rest.strip_prefix('[') {
        let (tag, after) = open.split_once(']').expect("unterminated ABI tag");
        match tag {
            "async-lower" => out.push_str("async-"),
            "resource-drop" => out.push_str("drop-"),
            other => {
                out.push_str(other);
                out.push('-');
            }
        }
        rest = after;
    }
    if rest.is_empty() {
        // A bare builtin such as `[waitable-set-new]` is all tag and no name.
        out.pop();
    }
    out.push_str(rest);
    out.replace(['.', '-'], "_").to_uppercase()
}

fn render(rows: &[Row]) -> String {
    let mut out = String::new();
    out.push_str("#![allow(dead_code)]\n\n");
    out.push_str("//! GENERATED by tools/p3-table-gen — the p3 WASI import table.\n");
    out.push_str("//! Derived from dovetail/wit/wasi-p3-*.wit via wit-parser wasm_signature.\n");
    out.push_str(
        "//! Regenerate with `cargo run -p p3-table-gen > \
                  dovetail/src/compiler/codegen/p3_imports.rs`\n",
    );
    out.push_str("//! if the WIT changes; do not hand-edit sigs.\n\n");
    out.push_str("use wasm_encoder::ValType;\n\n");
    out.push_str("pub struct P3Import {\n");
    out.push_str("    pub module: &'static str,\n");
    out.push_str("    pub name: &'static str,\n");
    out.push_str("    pub params: &'static [ValType],\n");
    out.push_str("    pub results: &'static [ValType],\n");
    out.push_str("}\n\n");

    for (index, row) in rows.iter().enumerate() {
        let _ = writeln!(out, "/// {} — {}", row.description, row.module);
        let _ = writeln!(
            out,
            "pub const FUNC_P3_{}_{}: u32 = {index};",
            module_tag(&row.module),
            name_tag(&row.name)
        );
    }

    let _ = writeln!(out, "\npub const NUM_P3_IMPORTS: u32 = {};\n", rows.len());
    out.push_str("pub const P3_IMPORTS: &[P3Import] = &[\n");
    for row in rows {
        let _ = writeln!(out, "    // {}", row.description);
        let _ = writeln!(
            out,
            "    P3Import {{ module: \"{}\", name: \"{}\", params: &[{}], results: &[{}] }},",
            row.module,
            row.name,
            row.params
                .iter()
                .map(|t| format!("ValType::{t}"))
                .collect::<Vec<_>>()
                .join(", "),
            row.results
                .iter()
                .map(|t| format!("ValType::{t}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    out.push_str("];\n\n");
    out
}
