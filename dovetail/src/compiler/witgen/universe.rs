//! Builds a [`WitImportUniverse`] from raw WIT texts. The manifest layer
//! supplies the texts (extracted from component binaries or read from disk)
//! plus the Dovetail package each interface's bindings should live in.

use crate::common::types::PackagePath;

use super::bindgen::{GeneratedBindings, generate_interface};
use super::{BindgenError, WitImportUniverse, WitImportedInterface};

/// One WIT interface to import, before resolution.
pub struct WitImportDecl {
    /// Virtual filename for diagnostics and the component encoder
    /// (e.g. `sqlite-raw.wit`).
    pub filename: String,
    /// The complete WIT package text.
    pub wit_text: String,
    /// Interface to import from the package. `None` = the package's sole
    /// interface (an error if it has more than one).
    pub interface: Option<String>,
    /// Dovetail package path for the generated bindings (e.g. `sqlite.raw`).
    pub dovetail_package: PackagePath,
}

/// Parse the WIT texts, select the interfaces, and run bindgen for each.
/// Returns the universe (with an empty table — the pipeline fills it after
/// typechecking the generated packages) plus the generated bindings in
/// interface order.
pub fn build_universe(
    decls: &[WitImportDecl],
) -> Result<(WitImportUniverse, Vec<GeneratedBindings>), BindgenError> {
    let mut universe = WitImportUniverse::empty();
    let mut all_bindings = Vec::new();

    // Distinct interfaces must not project into the same Dovetail package —
    // the injected packages would collide in the registry. Checked up front
    // (wit-parser also panics, rather than erroring, on a duplicate WIT
    // package push, which this guards against for the identical-decl case).
    for i in 0..decls.len() {
        for j in (i + 1)..decls.len() {
            if decls[i].dovetail_package == decls[j].dovetail_package {
                return Err(BindgenError {
                    message: format!(
                        "two imported WIT interfaces map to the same Dovetail package `{}`",
                        decls[i].dovetail_package.0.join(".")
                    ),
                });
            }
        }
    }

    // Duplicate WIT package names would panic inside wit-parser on push;
    // pre-parse each text in isolation to detect them with a clean error.
    let mut seen_wit_packages = std::collections::BTreeSet::new();
    for decl in decls {
        let mut scratch = wit_parser::Resolve::default();
        let pkg_id = scratch
            .push_str(&decl.filename, &decl.wit_text)
            .map_err(|e| BindgenError {
                message: format!("failed to parse WIT `{}`: {e:#}", decl.filename),
            })?;
        let name = scratch.packages[pkg_id].name.to_string();
        if !seen_wit_packages.insert(name.clone()) {
            return Err(BindgenError {
                message: format!("WIT package `{name}` is imported more than once"),
            });
        }
    }

    for (idx, decl) in decls.iter().enumerate() {
        let pkg_id = universe
            .resolve
            .push_str(&decl.filename, &decl.wit_text)
            .map_err(|e| BindgenError {
                message: format!("failed to parse WIT `{}`: {e:#}", decl.filename),
            })?;

        let pkg = &universe.resolve.packages[pkg_id];
        let interface_id = match &decl.interface {
            Some(name) => *pkg.interfaces.get(name).ok_or_else(|| BindgenError {
                message: format!(
                    "WIT package `{}` has no interface named `{name}`",
                    decl.filename
                ),
            })?,
            None => {
                if pkg.interfaces.len() != 1 {
                    return Err(BindgenError {
                        message: format!(
                            "WIT package `{}` declares {} interfaces; specify which one to \
                             import",
                            decl.filename,
                            pkg.interfaces.len()
                        ),
                    });
                }
                *pkg.interfaces.values().next().unwrap()
            }
        };

        let wit_name = universe
            .resolve
            .id_of(interface_id)
            .ok_or_else(|| BindgenError {
                message: format!(
                    "cannot derive canonical name for interface in `{}`",
                    decl.filename
                ),
            })?;

        let dovetail_package_str = decl.dovetail_package.0.join(".");
        let bindings =
            generate_interface(&universe.resolve, interface_id, idx, &dovetail_package_str)?;

        universe.interfaces.push(WitImportedInterface {
            interface_id,
            wit_name,
            dovetail_package: decl.dovetail_package.clone(),
        });
        universe
            .sources
            .push((decl.filename.clone(), decl.wit_text.clone()));
        all_bindings.push(bindings);
    }

    Ok((universe, all_bindings))
}

/// Derive a [`WitImportDecl`] from a component binary: decode its world,
/// validate that its imports are `wasi:*` only, locate the exported
/// interface, and print that interface's WIT package back to text.
pub fn decl_from_component(
    bytes: &[u8],
    component_name: &str,
    interface: Option<&str>,
    dovetail_package: PackagePath,
) -> Result<WitImportDecl, BindgenError> {
    let decoded = wit_component::decode(bytes).map_err(|e| BindgenError {
        message: format!("component `{component_name}`: failed to decode: {e:#}"),
    })?;
    let (resolve, world_id) = match decoded {
        wit_component::DecodedWasm::Component(resolve, world) => (resolve, world),
        _ => {
            return Err(BindgenError {
                message: format!("component `{component_name}` is not a WASM component"),
            });
        }
    };
    let world = &resolve.worlds[world_id];

    // Imports must be wasi:* only — anything else can't be satisfied after
    // composition (the runtime provides only WASI).
    for (key, _) in &world.imports {
        let name = match key {
            wit_parser::WorldKey::Interface(id) => {
                resolve.id_of(*id).unwrap_or_else(|| "<anonymous>".into())
            }
            wit_parser::WorldKey::Name(name) => name.clone(),
        };
        if !name.starts_with("wasi:") {
            return Err(BindgenError {
                message: format!(
                    "component `{component_name}` imports `{name}`, but only `wasi:*` \
                     imports are supported for component dependencies"
                ),
            });
        }
    }

    // Locate the exported interface.
    let mut exported: Vec<wit_parser::InterfaceId> = Vec::new();
    for item in world.exports.values() {
        if let wit_parser::WorldItem::Interface { id, .. } = item {
            exported.push(*id);
        }
    }
    let interface_id = match interface {
        Some(want) => *exported
            .iter()
            .find(|id| resolve.interfaces[**id].name.as_deref() == Some(want))
            .ok_or_else(|| BindgenError {
                message: format!(
                    "component `{component_name}` does not export an interface named `{want}`"
                ),
            })?,
        None => match exported.as_slice() {
            [only] => *only,
            [] => {
                return Err(BindgenError {
                    message: format!("component `{component_name}` exports no interfaces"),
                });
            }
            _ => {
                return Err(BindgenError {
                    message: format!(
                        "component `{component_name}` exports {} interfaces; specify which \
                         one to import",
                        exported.len()
                    ),
                });
            }
        },
    };
    let interface_name = resolve.interfaces[interface_id]
        .name
        .clone()
        .ok_or_else(|| BindgenError {
            message: format!(
                "component `{component_name}` exports an anonymous interface, which is \
                 not supported"
            ),
        })?;
    let owner_pkg = match resolve.interfaces[interface_id].package {
        Some(pkg) => pkg,
        None => {
            return Err(BindgenError {
                message: format!(
                    "component `{component_name}`: exported interface has no owning package"
                ),
            });
        }
    };

    // Print the owning package back to WIT text.
    let mut printer = wit_component::WitPrinter::default();
    printer
        .print(&resolve, owner_pkg, &[])
        .map_err(|e| BindgenError {
            message: format!(
                "component `{component_name}`: failed to print WIT for its interface: {e:#}"
            ),
        })?;
    let wit_text: String = printer.output.into();

    Ok(WitImportDecl {
        filename: format!("{component_name}.wit"),
        wit_text,
        interface: Some(interface_name),
        dovetail_package,
    })
}
