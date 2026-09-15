//! P3 component-import table. Maps each generated WIT-binding function (by its
//! [`WitFuncRef`]) to a core-module import and a function index assigned right
//! after the static WASI table + `[task-return]` imports (see
//! `Codegen::runtime_func_base`).
//!
//! For a **sync** component import, the core import's module name is the WIT
//! interface's canonical id (e.g. `dovetail:sqlite-raw/raw@0.1.0`) and the field
//! name is the canonical-ABI name — exactly the `wit_parser` function key,
//! which already carries `[method]r.m` / `[static]r.m` / `[constructor]r`
//! prefixes — or `[resource-drop]r` for a resource drop. `wit-component`
//! resolves these against the imported interface when lifting the core module.

use std::collections::BTreeMap;

use wasm_encoder::ValType;
use wit_parser::abi::{AbiVariant, WasmType};
use wit_parser::{FunctionKind, Resolve, TypeDefKind};

use crate::compiler::witgen::{WitFuncKind, WitFuncRef, WitImportUniverse};

/// Core valtype a canonical-ABI flat slot lowers to.
pub(super) fn val_type(ty: &WasmType) -> ValType {
    match ty {
        WasmType::I32 | WasmType::Pointer | WasmType::Length => ValType::I32,
        WasmType::I64 | WasmType::PointerOrI64 => ValType::I64,
        WasmType::F32 => ValType::F32,
        WasmType::F64 => ValType::F64,
    }
}

fn is_async_kind(kind: &FunctionKind) -> bool {
    matches!(
        kind,
        FunctionKind::AsyncFreestanding
            | FunctionKind::AsyncMethod(_)
            | FunctionKind::AsyncStatic(_)
    )
}

/// Flat core valtypes of a function's params (no retptr folding).
fn flat_params(resolve: &Resolve, func: &wit_parser::Function) -> Vec<ValType> {
    let mut out = Vec::new();
    for param in &func.params {
        let mut storage = [WasmType::I32; 32];
        let mut flat = wit_parser::abi::FlatTypes::new(&mut storage);
        resolve.push_flat(&param.ty, &mut flat);
        out.extend(flat.to_vec().iter().map(val_type));
    }
    out
}

/// One imported core function: `(module, field)` plus its flat signature.
pub(super) struct WitImportEntry {
    pub module: String,
    pub field: String,
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

/// The ordered import list plus a lookup from a [`WitFuncRef`] to its function
/// index. Entry order is deterministic (interface order, then WIT function
/// order, then resource drops) and is the single source of truth for both the
/// import section and the marshaling call target.
#[derive(Default)]
pub(super) struct WitImportRegistry {
    pub entries: Vec<WitImportEntry>,
    /// `(interface_idx, kind_disc, name)` → position in `entries`.
    index_of: BTreeMap<(usize, u8, String), u32>,
    /// Filled during `emit_type_section`: func-type index per entry.
    pub type_indices: Vec<u32>,
    /// Function index of the first entry.
    base: u32,
}

fn key_of(r: &WitFuncRef) -> (usize, u8, String) {
    match &r.kind {
        WitFuncKind::Function(n) => (r.interface_idx, 0, n.clone()),
        WitFuncKind::ResourceDrop(n) => (r.interface_idx, 1, n.clone()),
        WitFuncKind::AsyncStart(n) => (r.interface_idx, 2, n.clone()),
        // `finish` lifts from memory; it wraps no import, so it never resolves
        // to an index.
        WitFuncKind::AsyncFinish(_) => (r.interface_idx, 3, String::new()),
    }
}

impl WitImportRegistry {
    /// Build the table from the universe. `base` is the function index the
    /// first import occupies.
    pub fn build(universe: &WitImportUniverse, base: u32) -> Self {
        let mut entries: Vec<WitImportEntry> = Vec::new();
        let mut index_of: BTreeMap<(usize, u8, String), u32> = BTreeMap::new();
        let resolve = &universe.resolve;

        for (iface_idx, imp) in universe.interfaces.iter().enumerate() {
            let iface = &resolve.interfaces[imp.interface_id];
            for (fname, func) in &iface.functions {
                if is_async_kind(&func.kind) {
                    // Async import: one `[async-lower]<fn>` core import taking
                    // the flat params plus (for non-unit results) a result
                    // buffer pointer, returning an i32 subtask status.
                    let mut params = flat_params(resolve, func);
                    if func.result.is_some() {
                        params.push(ValType::I32);
                    }
                    let pos = entries.len() as u32;
                    index_of.insert((iface_idx, 2, fname.clone()), pos);
                    entries.push(WitImportEntry {
                        module: imp.wit_name.clone(),
                        field: format!("[async-lower]{fname}"),
                        params,
                        results: vec![ValType::I32],
                    });
                    continue;
                }
                // For a GuestImport, `wasm_signature` already folds the
                // indirect-result pointer into `params` (and clears `results`)
                // when `retptr` is set, so both lists are used verbatim.
                let sig = resolve.wasm_signature(AbiVariant::GuestImport, func);
                let params: Vec<ValType> = sig.params.iter().map(val_type).collect();
                let results: Vec<ValType> = sig.results.iter().map(val_type).collect();
                let pos = entries.len() as u32;
                index_of.insert((iface_idx, 0, fname.clone()), pos);
                entries.push(WitImportEntry {
                    module: imp.wit_name.clone(),
                    field: fname.clone(),
                    params,
                    results,
                });
            }
            for (tname, tid) in &iface.types {
                if matches!(resolve.types[*tid].kind, TypeDefKind::Resource) {
                    let pos = entries.len() as u32;
                    index_of.insert((iface_idx, 1, tname.clone()), pos);
                    entries.push(WitImportEntry {
                        module: imp.wit_name.clone(),
                        field: format!("[resource-drop]{tname}"),
                        params: vec![ValType::I32],
                        results: Vec::new(),
                    });
                }
            }
        }

        WitImportRegistry {
            entries,
            index_of,
            type_indices: Vec::new(),
            base,
        }
    }

    /// Number of imported functions.
    pub fn len(&self) -> u32 {
        self.entries.len() as u32
    }

    /// Function index of the import wrapped by `r`, if any.
    pub fn func_index(&self, r: &WitFuncRef) -> Option<u32> {
        self.index_of.get(&key_of(r)).map(|&pos| self.base + pos)
    }
}
