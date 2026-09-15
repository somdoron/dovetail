//! WIT import support: shared types connecting the manifest layer (which
//! component dependencies a project declares), the bindgen (WIT → generated
//! Dovetail source for a virtual package), and codegen (canonical-ABI
//! lift/lower plus core-module imports and component-world construction).
//!
//! An empty [`WitImportUniverse`] means "no component dependencies" and must
//! leave every stage's output byte-identical to the pre-WIT compiler.

pub mod bindgen;
pub mod injection;
pub mod mapping;
pub mod names;
pub mod universe;

use std::collections::BTreeMap;

use crate::common::types::{MangledName, PackagePath};

/// Everything codegen needs to know about the WIT interfaces a project
/// imports. Built once per project by the pipeline (from manifest component
/// declarations) and threaded through `generate_component`.
pub struct WitImportUniverse {
    /// WIT packages of all imported interfaces, fully resolved.
    pub resolve: wit_parser::Resolve,
    /// Raw WIT texts as `(virtual filename, wit text)`, in declaration order,
    /// for re-pushing into the component encoder's own `Resolve`.
    pub sources: Vec<(String, String)>,
    /// Imported interfaces in manifest declaration order. Import indices and
    /// world items are derived from this order, so it must be deterministic.
    pub interfaces: Vec<WitImportedInterface>,
    /// Mapping from generated binding functions to the WIT imports they wrap.
    pub table: WitImportTable,
}

impl WitImportUniverse {
    /// A universe with no imported interfaces (today's behavior).
    pub fn empty() -> Self {
        WitImportUniverse {
            resolve: wit_parser::Resolve::default(),
            sources: Vec::new(),
            interfaces: Vec::new(),
            table: WitImportTable::default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.interfaces.is_empty()
    }
}

/// One imported WIT interface and the Dovetail package its bindings live in.
pub struct WitImportedInterface {
    /// Interface id within [`WitImportUniverse::resolve`].
    pub interface_id: wit_parser::InterfaceId,
    /// Canonical WIT name, e.g. `dovetail:sqlite-raw/raw@0.1.0`. Used as the
    /// core-module import module name.
    pub wit_name: String,
    /// Dovetail package the generated bindings are injected as, e.g. `sqlite.raw`.
    pub dovetail_package: PackagePath,
}

/// Maps each generated binding function (by its post-typecheck MangledName)
/// to the WIT import it wraps. Codegen swaps the function's stub body for a
/// generated canonical-ABI call to the import.
#[derive(Default)]
pub struct WitImportTable {
    pub funcs: BTreeMap<MangledName, WitFuncRef>,
}

/// Reference to a single WIT import within the universe.
#[derive(Clone, Debug)]
pub struct WitFuncRef {
    /// Index into [`WitImportUniverse::interfaces`].
    pub interface_idx: usize,
    pub kind: WitFuncKind,
}

#[derive(Clone, Debug)]
pub enum WitFuncKind {
    /// Key into `wit_parser::Interface::functions` — the WIT function name,
    /// which already carries `[method]x.y` / `[static]x.y` / `[constructor]x`
    /// prefixes for resource functions.
    Function(String),
    /// Synthesized `[resource-drop]<resource>` import for the resource with
    /// this WIT name (kebab-case).
    ResourceDrop(String),
    /// `start` half of an async import (WIT function name): lowers params,
    /// allocates the result buffer, calls the `[async-lower]<fn>` import, and
    /// packs the returned status + buffer pointer into an `AsyncCall`.
    AsyncStart(String),
    /// `finish` half of an async import (WIT function name): lifts the result
    /// from the buffer the `AsyncCall` points at.
    AsyncFinish(String),
}

/// Error produced by bindgen when a WIT construct cannot be projected into
/// Dovetail (v1 restrictions) or the WIT itself is malformed.
#[derive(Debug)]
pub struct BindgenError {
    pub message: String,
}

impl std::fmt::Display for BindgenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for BindgenError {}
