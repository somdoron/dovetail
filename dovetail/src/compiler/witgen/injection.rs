//! Pipeline-side WIT machinery: compiling generated bindings packages,
//! injecting them as virtual packages, building the function table, and
//! deriving the import universe from a project's component dependencies.
//! The build pipeline (`compiler/pipeline.rs`) only orchestrates these.

use std::sync::Arc;

use crate::common::span::FilePath;
use crate::common::types::PackagePath;
use crate::compiler::layout::LayoutFilter;
use crate::compiler::lexer::{Lexer, attach_doc_comments};
use crate::compiler::parser::Parser;
use crate::compiler::parser::ast::PackageAst;
use crate::compiler::typechecker;
use crate::compiler::typechecker::registry::Registry;
use crate::compiler::typechecker::types::TypedModule;

/// Compile one generated WIT-bindings package (virtual source) against the
/// given registry. Generated source failing any stage is an internal
/// compiler error — bindgen must always produce valid Dovetail code — so
/// failures come back as `Err(message)` for the caller to surface as an ICE.
fn compile_wit_bindings_package(
    source: &str,
    virtual_file: &str,
    base_registry: &Registry,
) -> Result<typechecker::TypeCheckerResult, String> {
    let file: FilePath = Arc::from(if virtual_file.starts_with(".dovetail/") {
        virtual_file.to_string()
    } else {
        format!("<wit>/{virtual_file}")
    });
    let mut lexer = Lexer::new(source, file);
    let raw_tokens = lexer.tokenize();
    if !lexer.diagnostics().is_empty() {
        return Err(format!(
            "generated WIT bindings for {virtual_file} failed to lex: {:?}\nsource:\n{source}",
            lexer.diagnostics()
        ));
    }

    let raw_tokens = attach_doc_comments(raw_tokens);
    let mut filter = LayoutFilter::new(raw_tokens);
    let tokens = filter.filter();

    let mut parser = Parser::new(tokens);
    let source_file = parser.parse_source_file();
    if !parser.diagnostics().is_empty() {
        return Err(format!(
            "generated WIT bindings for {virtual_file} failed to parse: {:?}\nsource:\n{source}",
            parser.diagnostics()
        ));
    }

    let package_path = PackagePath(
        source_file
            .package
            .path
            .iter()
            .map(|s| s.value.clone())
            .collect(),
    );
    let package_ast = PackageAst {
        package_path,
        files: vec![source_file],
    };

    let result = typechecker::typecheck(&package_ast, base_registry);
    if result.has_errors() {
        return Err(format!(
            "generated WIT bindings for {virtual_file} failed to typecheck: {:?}\nsource:\n{source}",
            result.diagnostics.iter().collect::<Vec<_>>()
        ));
    }
    Ok(result)
}

/// Compile the generated bindings for every interface whose Dovetail package
/// is not already provided (by a dependency project that injected it),
/// merging each into `registry` (and `project_registry` when given, so
/// dependents inherit the symbols) and `module`. Returns an ICE message on
/// failure. The function table is built separately by [`build_wit_table`]
/// against the final merged module.
pub(crate) fn inject_wit_bindings(
    universe: &super::WitImportUniverse,
    bindings: &[super::bindgen::GeneratedBindings],
    registry: &mut Registry,
    mut project_registry: Option<&mut Registry>,
    module: &mut TypedModule,
    source_directory: Option<(&std::path::Path, &std::path::Path)>,
) -> Result<(), String> {
    for (idx, interface) in universe.interfaces.iter().enumerate() {
        if registry.has_package(&interface.dovetail_package) {
            // A dependency project already injected this bindings package.
            continue;
        }
        let generated = &bindings[idx];
        let filename = format!("{}.dove", interface.dovetail_package.0.join("."));
        let virtual_file = if let Some((root, directory)) = source_directory {
            std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
            let path = directory.join(&filename);
            if std::fs::read_to_string(&path).ok().as_deref() != Some(&generated.source) {
                use std::io::Write;
                let mut file =
                    tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
                file.write_all(generated.source.as_bytes())
                    .map_err(|e| e.to_string())?;
                file.persist(&path).map_err(|e| e.to_string())?;
            }
            generated_source_path(root, &path)?
        } else {
            filename
        };
        let tc = compile_wit_bindings_package(&generated.source, &virtual_file, registry)?;
        *registry = registry.merge(&tc.registry);
        if let Some(ref mut pr) = project_registry {
            **pr = pr.merge(&tc.registry);
        }
        module.merge_from(tc.typed_module);
    }
    Ok(())
}

/// Span paths use forward slashes on every platform, including Windows.
fn generated_source_path(root: &std::path::Path, path: &std::path::Path) -> Result<String, String> {
    let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
    Ok(relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

/// Fill the universe's function table by locating generated binding
/// functions in the (merged) module: a binding function is identified by
/// its virtual span file (`<wit>/<package>.dove`) plus its `source_name`,
/// which together are unambiguous regardless of which project injected it.
pub(crate) fn build_wit_table(
    universe: &mut super::WitImportUniverse,
    bindings: &[super::bindgen::GeneratedBindings],
    module: &TypedModule,
) {
    universe.table.funcs.clear();
    for (idx, interface) in universe.interfaces.iter().enumerate() {
        let generated = &bindings[idx];
        let virtual_file = format!("<wit>/{}.dove", interface.dovetail_package.0.join("."));
        for (mangled, func) in &module.functions {
            let generated_file = func.span.file.as_ref();
            let disk_binding = generated_file.starts_with(".dovetail/generated/")
                && generated_file
                    .ends_with(&format!("/{}.dove", interface.dovetail_package.0.join(".")));
            if generated_file != virtual_file && !disk_binding {
                continue;
            }
            if let Some((_, wit_ref)) = generated
                .funcs
                .iter()
                .find(|(source_name, _)| source_name == &func.source_name)
            {
                universe
                    .table
                    .funcs
                    .insert(mangled.clone(), wit_ref.clone());
            }
        }
    }
}

/// Build the WIT universe for a project's component dependencies: load each
/// component's bytes (built-in registry or disk), derive its interface WIT
/// from the binary, and run bindgen. Returns `None` for an empty list.
#[allow(clippy::type_complexity)]
pub(crate) fn build_component_universe(
    components: &[crate::manifest::ResolvedComponent],
) -> Result<
    Option<(
        super::WitImportUniverse,
        Vec<super::bindgen::GeneratedBindings>,
        Vec<(String, Vec<u8>)>,
    )>,
    String,
> {
    if components.is_empty() {
        return Ok(None);
    }
    let mut decls = Vec::with_capacity(components.len());
    let mut component_bytes = Vec::with_capacity(components.len());
    for component in components {
        let crate::manifest::ComponentSource::Path(path) = &component.source;
        let bytes = std::fs::read(path)
            .map_err(|e| format!("failed to read component `{}`: {e}", path.display()))?;
        let interface = &component.interface;
        let decl = super::universe::decl_from_component(
            &bytes,
            &component.display_name,
            interface.as_deref(),
            component.dovetail_package.clone(),
        )
        .map_err(|e| e.message)?;
        decls.push(decl);
        component_bytes.push((component.display_name.clone(), bytes));
    }
    let (universe, bindings) = super::universe::build_universe(&decls).map_err(|e| e.message)?;
    Ok(Some((universe, bindings, component_bytes)))
}

#[cfg(test)]
mod wit_binding_tests {
    use super::super::WitFuncKind;
    use super::super::universe::{WitImportDecl, build_universe};
    use super::*;
    use crate::common::types::PackagePath;
    use crate::compiler::typechecker::types::TypedModule;

    const SQLITE_WIT: &str = include_str!("../../../../components/sqlite-shim/wit/sqlite-raw.wit");

    #[test]
    fn generated_binding_span_path_uses_portable_separators() {
        let root = std::path::Path::new("workspace");
        let path = root
            .join(".dovetail")
            .join("generated")
            .join("identity")
            .join("sqlite.raw.dove");
        assert_eq!(
            generated_source_path(root, &path).unwrap(),
            ".dovetail/generated/identity/sqlite.raw.dove"
        );
    }

    /// The generated bindings must typecheck and every generated function
    /// must be matched back to its WIT import via `source_name`.
    #[test]
    fn wit_import_table_is_fully_populated() {
        let decls = vec![WitImportDecl {
            filename: "sqlite-raw.wit".into(),
            wit_text: SQLITE_WIT.into(),
            interface: Some("raw".into()),
            dovetail_package: PackagePath(vec!["sqlite".into(), "raw".into()]),
        }];
        let (mut universe, bindings) = build_universe(&decls).expect("universe");
        let expected: usize = bindings.iter().map(|b| b.funcs.len()).sum();

        let (_path, prelude_registry, _module) = &*crate::compiler::PRELUDE_OUTPUT;
        let mut registry = prelude_registry.clone();
        let mut module = TypedModule::empty();
        // The generated async bindings reference `standard.wasi.{AsyncCall,
        // ComponentSubtask}` and `standard.io.Async`. In the real pipeline those
        // packages are compiled before any component-importing project; here only
        // the prelude is loaded, so stand up minimal stubs (in dependency order)
        // the bindings can resolve against.
        let stubs = [
            (
                "standard.wasi.dove",
                "package standard.wasi\n\n\
                 public newtype AsyncCall = Int64\n\n\
                 public record ComponentSubtask<out T, out E> =\n    \
                 start: () => AsyncCall\n    \
                 finish: (AsyncCall) => Result<T, E>\n",
            ),
            (
                "standard.io.types.dove",
                "package standard.io\n\n\
                 public sealed abstract class Async<out T, out E>()\n",
            ),
            (
                "standard.io.Async.dove",
                "module standard.io.Async<T, E>\n\n\
                 import standard.wasi.AsyncCall\n\
                 import standard.wasi.ComponentSubtask\n\n\
                 public function subtask(call: ComponentSubtask<T, E>): Async<T, E> = \
                 panic \"stub\"\n",
            ),
        ];
        for (file, src) in stubs {
            let tc = compile_wit_bindings_package(src, file, &registry)
                .unwrap_or_else(|e| panic!("compile stub {file}: {e}"));
            registry = registry.merge(&tc.registry);
            module.merge_from(tc.typed_module);
        }
        let workspace = tempfile::tempdir().unwrap();
        let directory = workspace.path().join(".dovetail/generated/test");
        inject_wit_bindings(
            &universe,
            &bindings,
            &mut registry,
            None,
            &mut module,
            Some((workspace.path(), &directory)),
        )
        .expect("inject");
        assert!(directory.join("sqlite.raw.dove").is_file());
        build_wit_table(&mut universe, &bindings, &module);

        assert_eq!(
            universe.table.funcs.len(),
            expected,
            "every generated binding function must map to a WIT import"
        );
        // Every function is async, so each projects to a start/finish pair.
        let starts = universe
            .table
            .funcs
            .values()
            .filter(|r| matches!(r.kind, WitFuncKind::AsyncStart(_)))
            .count();
        let finishes = universe
            .table
            .funcs
            .values()
            .filter(|r| matches!(r.kind, WitFuncKind::AsyncFinish(_)))
            .count();
        assert!(starts > 0, "expected async start bindings");
        assert_eq!(
            starts, finishes,
            "each async import needs a start and finish"
        );
    }
}
