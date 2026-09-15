//! WIT interface → generated Dovetail source text (the low-level virtual
//! bindings package). Policy-free by design: resources become Int32 handle
//! newtypes with explicit `drop`, all functions get `panic` stub bodies that
//! codegen later replaces with canonical-ABI import calls.

use std::collections::BTreeSet;

use wit_parser::{FunctionKind, InterfaceId, Resolve, TypeDefKind, TypeOwner};

use super::mapping::{map_wit_type, DovetailTypeSketch};
use super::names;
use super::{BindgenError, WitFuncKind, WitFuncRef};

/// Universal stub body; typed as `Never`, so it is assignable to every
/// declared return type. Never emitted — codegen swaps it for the real body.
const STUB_BODY: &str = "panic \"wit-import stub\"";

pub struct GeneratedBindings {
    /// Complete Dovetail source for the virtual package (single file).
    pub source: String,
    /// `(source_name, ref)` for every generated binding function, where
    /// `source_name` matches `TypedFunction::source_name` after typechecking
    /// (e.g. `Database.open`).
    pub funcs: Vec<(String, WitFuncRef)>,
}

/// Generate the bindings source for one imported interface.
///
/// `dovetail_package` is the dotted Dovetail package path (e.g. `sqlite.raw`);
/// `interface_idx` is this interface's index in the universe (recorded in
/// each `WitFuncRef`).
pub fn generate_interface(
    resolve: &Resolve,
    interface_id: InterfaceId,
    interface_idx: usize,
    dovetail_package: &str,
) -> Result<GeneratedBindings, BindgenError> {
    let iface = &resolve.interfaces[interface_id];
    let iface_name = iface.name.as_deref().ok_or_else(|| BindgenError {
        message: "cannot import an anonymous (inline world) WIT interface".to_string(),
    })?;

    validate_self_contained(resolve, interface_id)?;

    let mut src = String::new();
    let mut funcs: Vec<(String, WitFuncRef)> = Vec::new();
    let mut declared_names: BTreeSet<String> = BTreeSet::new();

    src.push_str(&format!("package {dovetail_package}\n\n"));

    // Async imports project to a public `Async`-returning wrapper over an
    // internal `Start`/`Finish` pair, composed through a `ComponentSubtask`.
    let has_async = iface.functions.values().any(|f| {
        matches!(
            f.kind,
            FunctionKind::AsyncFreestanding
                | FunctionKind::AsyncMethod(_)
                | FunctionKind::AsyncStatic(_)
        )
    });
    if has_async {
        src.push_str("import standard.io.Async\n");
        src.push_str("import standard.wasi.AsyncCall\n");
        src.push_str("import standard.wasi.ComponentSubtask\n\n");
    }

    let mut declare = |name: &str| -> Result<(), BindgenError> {
        if !declared_names.insert(name.to_string()) {
            return Err(BindgenError {
                message: format!(
                    "interface `{iface_name}`: WIT names project to the duplicate \
                     Dovetail type name `{name}`"
                ),
            });
        }
        Ok(())
    };

    // --- Type declarations, in WIT declaration order -----------------------
    for (wit_name, type_id) in &iface.types {
        let def = &resolve.types[*type_id];
        let dovetail_name = names::pascal(wit_name);
        match &def.kind {
            TypeDefKind::Resource => {
                declare(&dovetail_name)?;
                src.push_str(&format!("public newtype {dovetail_name} = Int32\n\n"));
            }
            TypeDefKind::Record(record) => {
                declare(&dovetail_name)?;
                if record.fields.is_empty() {
                    return Err(BindgenError {
                        message: format!(
                            "interface `{iface_name}`: empty record `{wit_name}` is not supported"
                        ),
                    });
                }
                src.push_str(&format!("public record {dovetail_name} =\n"));
                for field in &record.fields {
                    let field_ty = map_wit_type(resolve, &field.ty)?;
                    src.push_str(&format!(
                        "    {}: {}\n",
                        names::safe_camel(&field.name),
                        field_ty.render()
                    ));
                }
                src.push('\n');
            }
            TypeDefKind::Enum(en) => {
                declare(&dovetail_name)?;
                src.push_str(&format!("public enum {dovetail_name} =\n"));
                for case in &en.cases {
                    src.push_str(&format!("    {}\n", names::pascal(&case.name)));
                }
                src.push('\n');
            }
            TypeDefKind::Variant(variant) => {
                declare(&dovetail_name)?;
                src.push_str(&format!("public enum {dovetail_name} =\n"));
                for case in &variant.cases {
                    match &case.ty {
                        Some(ty) => {
                            let payload = map_wit_type(resolve, ty)?;
                            src.push_str(&format!(
                                "    {}({})\n",
                                names::pascal(&case.name),
                                payload.render()
                            ));
                        }
                        None => src.push_str(&format!("    {}\n", names::pascal(&case.name))),
                    }
                }
                src.push('\n');
            }
            TypeDefKind::Flags(flags) => {
                declare(&dovetail_name)?;
                if flags.flags.len() > 32 {
                    return Err(BindgenError {
                        message: format!(
                            "interface `{iface_name}`: flags `{wit_name}` has more than 32 \
                             flags, which is not supported yet"
                        ),
                    });
                }
                src.push_str(&format!("public newtype {dovetail_name} = Uint32\n\n"));
                src.push_str(&format!("module {dovetail_name} =\n"));
                for (i, flag) in flags.flags.iter().enumerate() {
                    src.push_str(&format!(
                        "    public function {}(): {dovetail_name} = {dovetail_name}({}u32)\n",
                        names::safe_camel(&flag.name),
                        1u32 << i,
                    ));
                }
                src.push('\n');
            }
            // Aliases project through in the type mapping; no declaration.
            TypeDefKind::Type(_) => {}
            other => {
                // List/option/result/tuple/handle never appear as named
                // interface type entries with these kinds unless aliased
                // (handled above); async kinds are rejected by mapping.
                let _ = other;
                map_wit_type(resolve, &wit_parser::Type::Id(*type_id))?;
            }
        }
    }

    // --- Function modules ---------------------------------------------------
    // Resource functions group under the resource's module; freestanding
    // functions under a module named after the interface.
    struct ModuleDecl {
        name: String,
        lines: Vec<String>,
    }
    let mut modules: Vec<ModuleDecl> = Vec::new();
    let module_index = |modules: &mut Vec<ModuleDecl>, name: &str| -> usize {
        if let Some(i) = modules.iter().position(|m| m.name == name) {
            i
        } else {
            modules.push(ModuleDecl {
                name: name.to_string(),
                lines: Vec::new(),
            });
            modules.len() - 1
        }
    };

    let resource_module_name = |type_id: wit_parser::TypeId| -> Result<String, BindgenError> {
        let def = &resolve.types[type_id];
        let name = def.name.as_ref().ok_or_else(|| BindgenError {
            message: "resource type has no name".to_string(),
        })?;
        Ok(names::pascal(name))
    };

    for (wit_func_name, func) in &iface.functions {
        // v1 restriction: params must lower to direct flat args (≤16 slots).
        let sig = resolve.wasm_signature(wit_parser::abi::AbiVariant::GuestImport, func);
        if sig.indirect_params {
            return Err(BindgenError {
                message: format!(
                    "interface `{iface_name}`: function `{wit_func_name}` has too many                      flattened parameters (more than 16), which is not supported yet"
                ),
            });
        }
        // Async and sync function kinds project to identical binding shapes;
        // the async-ness is an ABI concern handled in codegen, not here.
        let (module_name, dovetail_func_name, skip_first_param) = match &func.kind {
            FunctionKind::Freestanding | FunctionKind::AsyncFreestanding => {
                let base = names::safe_camel(wit_func_name);
                (names::pascal(iface_name), base, false)
            }
            FunctionKind::Method(tid) | FunctionKind::AsyncMethod(tid) => {
                let base = wit_func_name
                    .rsplit_once('.')
                    .map(|(_, b)| b)
                    .unwrap_or(wit_func_name);
                (resource_module_name(*tid)?, names::safe_camel(base), true)
            }
            FunctionKind::Static(tid) | FunctionKind::AsyncStatic(tid) => {
                let base = wit_func_name
                    .rsplit_once('.')
                    .map(|(_, b)| b)
                    .unwrap_or(wit_func_name);
                (resource_module_name(*tid)?, names::safe_camel(base), false)
            }
            // House style: constructors are `make`.
            FunctionKind::Constructor(tid) => (resource_module_name(*tid)?, "make".into(), false),
        };

        let mut params: Vec<String> = Vec::new();
        // Just the parameter names, in order — for forwarding to `Start` from
        // the generated `Async` wrapper. Includes `self` for methods.
        let mut param_names: Vec<String> = Vec::new();
        for (i, param) in func.params.iter().enumerate() {
            let (param_name, param_ty) = (&param.name, &param.ty);
            if i == 0 && skip_first_param {
                params.push("self".to_string());
                param_names.push("self".to_string());
                continue;
            }
            let sketch = map_wit_type(resolve, param_ty)?;
            let camel = names::safe_camel(param_name);
            params.push(format!("{}: {}", camel, sketch.render()));
            param_names.push(camel);
        }

        // wit-parser 0.255: a function has at most one (anonymous) result.
        let return_sketch = match &func.result {
            Some(ty) => map_wit_type(resolve, ty)?,
            None => DovetailTypeSketch::Unit,
        };

        let is_async = matches!(
            func.kind,
            FunctionKind::AsyncFreestanding
                | FunctionKind::AsyncMethod(_)
                | FunctionKind::AsyncStatic(_)
        );

        let idx = module_index(&mut modules, &module_name);
        if is_async {
            // Internal ABI plumbing: `Start` posts the subtask (returning an
            // `AsyncCall`), `Finish` lifts its result once done. Hidden from the
            // package surface — callers use the public `Async` wrapper below.
            // Codegen still swaps these two stubs for the canonical-ABI calls;
            // `internal` keeps them in the module (and the WIT import table)
            // while removing them from the package's public API.
            modules[idx].lines.push(format!(
                "    internal function {dovetail_func_name}Start({}): AsyncCall = {STUB_BODY}",
                params.join(", ")
            ));
            funcs.push((
                format!("{module_name}.{dovetail_func_name}Start"),
                WitFuncRef {
                    interface_idx,
                    kind: WitFuncKind::AsyncStart(wit_func_name.clone()),
                },
            ));
            modules[idx].lines.push(format!(
                "    internal function {dovetail_func_name}Finish(call: AsyncCall): {} = {STUB_BODY}",
                return_sketch.render()
            ));
            funcs.push((
                format!("{module_name}.{dovetail_func_name}Finish"),
                WitFuncRef {
                    interface_idx,
                    kind: WitFuncKind::AsyncFinish(wit_func_name.clone()),
                },
            ));

            // Public wrapper: compose Start/Finish into an `Async` subtask — the
            // ergonomic form, so callers never touch the ABI pair. A WIT
            // `result<T, Err>` result projects to `Async<T, Err>` (Finish
            // already yields a `Result`); any other result projects to
            // `Async<T, Never>` (wrap the value in `Ok`).
            let (t_ty, e_ty, finish_expr) = match &return_sketch {
                DovetailTypeSketch::Result(ok, err) => (
                    ok.render(),
                    err.render(),
                    format!("{module_name}.{dovetail_func_name}Finish(call)"),
                ),
                other => (
                    other.render(),
                    "Never".to_string(),
                    format!("Ok({module_name}.{dovetail_func_name}Finish(call))"),
                ),
            };
            let start_call = if matches!(func.kind, FunctionKind::AsyncMethod(_)) {
                format!("self.{dovetail_func_name}Start({})", param_names[1..].join(", "))
            } else {
                format!("{module_name}.{dovetail_func_name}Start({})", param_names.join(", "))
            };
            modules[idx].lines.push(format!(
                "    public function {dovetail_func_name}({}): Async<{t_ty}, {e_ty}> =",
                params.join(", ")
            ));
            modules[idx].lines.push(format!(
                "        Async<{t_ty}, {e_ty}>.subtask(ComponentSubtask<{t_ty}, {e_ty}> {{"
            ));
            modules[idx]
                .lines
                .push(format!("            start = () => {start_call}"));
            modules[idx]
                .lines
                .push(format!("            finish = (call: AsyncCall) => {finish_expr}"));
            modules[idx].lines.push("        })".to_string());
        } else {
            modules[idx].lines.push(format!(
                "    public function {dovetail_func_name}({}): {} = {STUB_BODY}",
                params.join(", "),
                return_sketch.render()
            ));
            funcs.push((
                format!("{module_name}.{dovetail_func_name}"),
                WitFuncRef {
                    interface_idx,
                    kind: WitFuncKind::Function(wit_func_name.clone()),
                },
            ));
        }
    }

    // Explicit `drop` for every resource.
    for (wit_name, type_id) in &iface.types {
        if !matches!(resolve.types[*type_id].kind, TypeDefKind::Resource) {
            continue;
        }
        let module_name = names::pascal(wit_name);
        let idx = module_index(&mut modules, &module_name);
        modules[idx]
            .lines
            .push(format!("    public function drop(self): Unit = {STUB_BODY}"));
        funcs.push((
            format!("{module_name}.drop"),
            WitFuncRef {
                interface_idx,
                kind: WitFuncKind::ResourceDrop(wit_name.clone()),
            },
        ));
    }

    for module in &modules {
        if module.lines.is_empty() {
            continue;
        }
        src.push_str(&format!("module {} =\n", module.name));
        for line in &module.lines {
            src.push_str(line);
            src.push('\n');
        }
        src.push('\n');
    }

    Ok(GeneratedBindings { source: src, funcs })
}

/// v1 restriction: every named type referenced by the interface must be
/// declared in the interface itself — no `use` of foreign interfaces.
fn validate_self_contained(
    resolve: &Resolve,
    interface_id: InterfaceId,
) -> Result<(), BindgenError> {
    let iface = &resolve.interfaces[interface_id];
    for (wit_name, type_id) in &iface.types {
        let def = &resolve.types[*type_id];
        // A `use foreign.{t}` shows up as an alias whose target is owned by
        // another interface.
        if let TypeDefKind::Type(wit_parser::Type::Id(target)) = &def.kind {
            let target_def = &resolve.types[*target];
            match target_def.owner {
                TypeOwner::Interface(owner) if owner != interface_id => {
                    let owner_name = resolve.interfaces[owner]
                        .name
                        .clone()
                        .unwrap_or_else(|| "<anonymous>".to_string());
                    return Err(BindgenError {
                        message: format!(
                            "interface `{}`: type `{wit_name}` is used from foreign \
                             interface `{owner_name}` — imported interfaces must be \
                             self-contained (v1 restriction)",
                            iface.name.as_deref().unwrap_or("<anonymous>")
                        ),
                    });
                }
                _ => {}
            }
        }
    }
    Ok(())
}
