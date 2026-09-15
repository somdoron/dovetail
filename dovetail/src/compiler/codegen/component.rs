use wit_component::{ComponentEncoder, StringEncoding, embed_component_metadata};
use wit_parser::Resolve;

use super::CodeGenError;
use crate::compiler::witgen::WitImportUniverse;

/// The vendored WASI p3 WIT and the version suffix it is pinned to. Defined in
/// `crate::p3` so the import-table generator reads the same list in the same
/// order — the order fixes the table's numbering.
pub(super) use crate::p3::{P3_VERSION, WIT_FILES};

fn p3_resolve() -> Result<Resolve, CodeGenError> {
    let mut resolve = Resolve::default();
    for (name, contents) in WIT_FILES {
        resolve.push_str(name, contents).map_err(|e| CodeGenError {
            message: format!(
                "Component generation failed: could not parse {name}.\n\
                 This is an internal compiler error. Details: {e}"
            ),
        })?;
    }
    Ok(resolve)
}

/// Wrap core module bytes into a WASI p3 CLI component (async command world),
/// declaring any WIT component imports the universe carries. `num_tests` is
/// `Some(n)` for a test component (one async-lifted `test-nN` export per test).
pub(super) fn encode_p3_with_imports(
    core_bytes: Vec<u8>,
    num_tests: Option<usize>,
    wit_imports: &WitImportUniverse,
) -> Result<Vec<u8>, CodeGenError> {
    encode_p3(core_bytes, num_tests, wit_imports)
}

fn encode_p3(
    mut core_bytes: Vec<u8>,
    num_tests: Option<usize>,
    wit_imports: &WitImportUniverse,
) -> Result<Vec<u8>, CodeGenError> {
    let mut resolve = p3_resolve()?;

    // Push the WIT of every imported component interface so the synthesized
    // world can `import` it and the core imports resolve.
    for (filename, wit_text) in &wit_imports.sources {
        resolve
            .push_str(filename, wit_text)
            .map_err(|e| CodeGenError {
                message: format!(
                    "Component generation failed: could not parse imported WIT `{filename}`.\n\
                     Details: {e}"
                ),
            })?;
    }

    let world_id = if num_tests.is_none() && wit_imports.is_empty() {
        let cli_pkg = resolve
            .package_names
            .get(&wit_parser::PackageName {
                namespace: "wasi".to_string(),
                name: "cli".to_string(),
                version: Some(P3_VERSION.parse().unwrap()),
            })
            .copied()
            .ok_or_else(|| CodeGenError {
                message: format!(
                    "Component generation failed: wasi:cli@{P3_VERSION} package not found.\n\
                     This is an internal compiler error."
                ),
            })?;
        resolve
            .select_world(&[cli_pkg], Some("command"))
            .map_err(|e| CodeGenError {
                message: format!(
                    "Component generation failed: could not select p3 'command' world.\n\
                     This is an internal compiler error. Details: {e}"
                ),
            })?
    } else {
        // A synthesized world: the CLI command world, plus (when present) one
        // async-lifted `test-nN` export per test, plus one `import <interface>;`
        // per imported WIT component.
        let test_lines: String = match num_tests {
            Some(n) => (0..n)
                .map(|i| format!("  export test-n{i}: async func();"))
                .collect::<Vec<_>>()
                .join("\n"),
            None => String::new(),
        };
        let import_lines: String = wit_imports
            .interfaces
            .iter()
            .map(|i| format!("  import {};\n", i.wit_name))
            .collect();
        let test_world = format!(
            "package dovetail:tests;\n\
             \n\
             world test-world {{\n\
             \x20 include wasi:cli/command@{P3_VERSION};\n\
             {import_lines}{test_lines}\n\
             }}\n"
        );
        let pkg = resolve
            .push_str("dovetail-tests.wit", &test_world)
            .map_err(|e| CodeGenError {
                message: format!(
                    "Component generation failed: could not parse dynamic test/import \
                     world.\nThis is an internal compiler error. Details: {e}"
                ),
            })?;
        resolve
            .select_world(&[pkg], Some("test-world"))
            .map_err(|e| CodeGenError {
                message: format!(
                    "Component generation failed: could not select test/import world.\n\
                     This is an internal compiler error. Details: {e}"
                ),
            })?
    };

    embed_component_metadata(&mut core_bytes, &resolve, world_id, StringEncoding::UTF8).map_err(
        |e| CodeGenError {
            message: format!(
                "Component generation failed: could not embed p3 component metadata.\n\
                 This may indicate a mismatch between the generated Wasm and WASI p3 interface \
                 expectations.\nDetails: {e}"
            ),
        },
    )?;

    ComponentEncoder::default()
        .module(&core_bytes)
        .map_err(|e| CodeGenError {
            message: format!(
                "Component generation failed: could not initialize component encoder with core \
                 module.\nThe generated Wasm module may be invalid. Details: {e}"
            ),
        })?
        .encode()
        .map_err(|e| CodeGenError {
            message: format!(
                "Component generation failed: could not encode p3 component.\n\
                 This may indicate missing or incorrect WASI imports/exports. Details: {e:#}"
            ),
        })
}

/// Plug component dependency binaries into `program` (the just-encoded Dovetail
/// component, which imports each dependency's interface). `plugs` is
/// `(display_name, component_bytes)`; wac matches each plug's exports to the
/// program's imports by interface, leaving only WASI imports on the result.
pub(super) fn compose_components(
    program: Vec<u8>,
    plugs: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, CodeGenError> {
    use wac_graph::types::Package;
    use wac_graph::{CompositionGraph, EncodeOptions, plug};

    let mut graph = CompositionGraph::new();

    let program_pkg = Package::from_bytes("dovetail:program", None, program, graph.types_mut())
        .map_err(|e| CodeGenError {
            message: format!(
                "Component composition failed: could not load the Dovetail component.\n\
                 Details: {e:#}"
            ),
        })?;
    let program_id = graph
        .register_package(program_pkg)
        .map_err(|e| CodeGenError {
            message: format!("Component composition failed: could not register program: {e:#}"),
        })?;

    let mut plug_ids = Vec::with_capacity(plugs.len());
    for (i, (name, bytes)) in plugs.iter().enumerate() {
        let pkg = Package::from_bytes(
            &format!("dovetail:plug{i}"),
            None,
            bytes.clone(),
            graph.types_mut(),
        )
        .map_err(|e| CodeGenError {
            message: format!(
                "Component composition failed: could not load component `{name}`.\nDetails: {e:#}"
            ),
        })?;
        let id = graph.register_package(pkg).map_err(|e| CodeGenError {
            message: format!(
                "Component composition failed: could not register component `{name}`: {e:#}"
            ),
        })?;
        plug_ids.push(id);
    }

    plug(&mut graph, plug_ids, program_id).map_err(|e| CodeGenError {
        message: format!(
            "Component composition failed: could not plug component dependencies into the \
             Dovetail component. Their exported interfaces may not match the imports.\nDetails: {e:#}"
        ),
    })?;

    graph
        .encode(EncodeOptions::default())
        .map_err(|e| CodeGenError {
            message: format!(
                "Component composition failed: could not encode the composed component.\n\
                 Details: {e:#}"
            ),
        })
}
