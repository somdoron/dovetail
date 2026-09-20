use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, LazyLock};

use include_dir::{Dir, include_dir};

use crate::common::diagnostics::Diagnostics;
use crate::common::span::FilePath;
use crate::common::types::PackagePath;
use crate::compiler::codegen;
use crate::compiler::desugar;
use crate::compiler::layout::LayoutFilter;
use crate::compiler::lexer::Lexer;
use crate::compiler::lexer::attach_doc_comments;
use crate::compiler::macros;
use crate::compiler::monomorphize;
use crate::compiler::parser::Parser;
use crate::compiler::parser::ast::PackageAst;
use crate::compiler::typechecker;
use crate::compiler::typechecker::registry::Registry;
use crate::compiler::typechecker::types::TypedModule;
use crate::compiler::witgen;
use crate::manifest::{ResolvedProject, ResolvedWorkspace};

/// Reports completed projects, total projects, and the current project name.
pub type WorkspaceProgress = dyn Fn(usize, usize, &str) + Send + Sync;

/// Metadata about a test exported from the WASM component.
#[derive(Debug, Clone)]
pub struct TestExportInfo {
    pub index: usize,
    pub name: String,
    pub fqtn: String,
    pub package_path: String,
    pub source_file: String,
    /// None = no @skip; Some(None) = @skip; Some(Some(r)) = @skip("r")
    pub skip_reason: Option<Option<String>>,
    /// None = no @panics; Some(None) = @panics; Some(Some(m)) = @panics("m")
    pub expected_panic: Option<Option<String>>,
    /// None = no @timeout; Some(ms) = @timeout(ms)
    pub timeout_ms: Option<u64>,
}

/// The build mode controls which pipeline stages run and how tests are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    /// Normal build: resolve main, codegen, ignore tests.
    Build,
    /// Type-check only: no codegen, no main validation.
    Check,
    /// Type-check and retain body-free declarations for API queries.
    Query,
    /// Test mode: skip main resolution, convert tests to functions, codegen.
    Test,
}

/// Embedded prelude source directory (all .dove files).
static PRELUDE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/prelude/src");

/// Iterate over embedded prelude source files.
/// Returns `(relative_path, contents)` pairs, e.g. `("SourceLocation.dove", "package standard.prelude\n...")`.
pub fn prelude_sources() -> impl Iterator<Item = (&'static str, &'static str)> {
    PRELUDE_DIR.files().filter_map(|entry| {
        let path = entry.path().to_str()?;
        if !path.ends_with(".dove") {
            return None;
        }
        let contents = entry.contents_utf8()?;
        Some((path, contents))
    })
}

/// Cached prelude output (package path, registry, typed module), compiled once and reused.
pub(crate) static PRELUDE_OUTPUT: LazyLock<(PackagePath, Registry, TypedModule)> =
    LazyLock::new(|| {
        let (package_path, result) = compile_prelude();
        (package_path, result.registry, result.typed_module)
    });

/// Compile all prelude source files into a single package.
/// Returns the prelude's package path alongside the typechecker result.
fn compile_prelude() -> (PackagePath, typechecker::TypeCheckerResult) {
    let mut source_files = Vec::new();
    let mut package_path = None;

    for entry in PRELUDE_DIR.files() {
        let file_name = entry.path().to_str().unwrap();
        if !file_name.ends_with(".dove") {
            continue;
        }
        let source = entry.contents_utf8().unwrap_or_else(|| {
            panic!("prelude file {file_name} is not valid UTF-8");
        });

        let file: FilePath = Arc::from(format!("<prelude>/{file_name}").as_str());
        let mut lexer = Lexer::new(source, file);
        let raw_tokens = lexer.tokenize();
        assert!(
            lexer.diagnostics().is_empty(),
            "prelude lex errors in {file_name}: {:?}",
            lexer.diagnostics()
        );

        let raw_tokens = attach_doc_comments(raw_tokens);
        let mut filter = LayoutFilter::new(raw_tokens);
        let tokens = filter.filter();

        let mut parser = Parser::new(tokens);
        let sf = parser.parse_source_file();
        assert!(
            parser.diagnostics().is_empty(),
            "prelude parse errors in {file_name}: {:?}",
            parser.diagnostics()
        );

        // All prelude files must share the same package declaration
        let file_pkg = PackagePath(sf.package.path.iter().map(|s| s.value.clone()).collect());
        if let Some(ref expected) = package_path {
            assert!(
                &file_pkg == expected,
                "prelude file {file_name} has package '{file_pkg}', expected '{expected}'",
            );
        } else {
            package_path = Some(file_pkg);
        }

        source_files.push(sf);
    }

    assert!(
        !source_files.is_empty(),
        "prelude directory contains no .dove files"
    );

    let pkg_path = package_path.unwrap();
    let mut package_ast = PackageAst {
        package_path: pkg_path.clone(),
        files: source_files,
    };

    // Macro phase. The prelude is the package that declares `@derive(Equatable)`,
    // and the Equatable derive is a built-in (Rust) expander, so expansion can
    // run inside the prelude without bootstrap issues.
    let macro_registry = macros::builtin_registry();
    let mut macro_diagnostics = Diagnostics::new();
    macros::expand_package(&mut package_ast, &macro_registry, &mut macro_diagnostics);
    assert!(
        !macro_diagnostics.has_errors(),
        "prelude macro expansion errors: {:?}",
        macro_diagnostics.iter().collect::<Vec<_>>()
    );

    let base_registry = Registry::new();
    let mut result = typechecker::typecheck(&package_ast, &base_registry);
    assert!(
        !result.has_errors(),
        "prelude typecheck errors: {:?}",
        result.diagnostics.iter().collect::<Vec<_>>()
    );

    // Desugar + coerce/capture before the coerce pass
    desugar::desugar_all(&mut result.typed_module);
    desugar::coerce_and_capture(&mut result.typed_module);

    let merged_registry = base_registry.merge(&result.registry);
    super::coerce::elaborate_coercions(&mut result.typed_module, &merged_registry);
    (pkg_path, result)
}

/// Result of a full compilation pipeline run.
pub struct CompileResult {
    pub wasm: Option<Vec<u8>>,
    pub diagnostics: Diagnostics,
    pub test_exports: Vec<TestExportInfo>,
}

/// Run the full compilation pipeline on a source string.
/// Returns WASM bytes on success, or diagnostics on failure.
pub fn compile(source: &str, file_path: &str) -> CompileResult {
    let mut diagnostics = Diagnostics::new();
    let file: FilePath = Arc::from(file_path);

    // 1. Lex
    let mut lexer = Lexer::new(source, file);
    let raw_tokens = lexer.tokenize();
    diagnostics.extend(lexer.diagnostics());
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }

    // 2. Layout filter
    let raw_tokens = attach_doc_comments(raw_tokens);
    let mut filter = LayoutFilter::new(raw_tokens);
    let tokens = filter.filter();

    // 3. Parse
    let mut parser = Parser::new(tokens);
    let source_file = parser.parse_source_file();
    diagnostics.extend(parser.diagnostics());
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }

    // 4. Typecheck
    let package_path = PackagePath(
        source_file
            .package
            .path
            .iter()
            .map(|s| s.value.clone())
            .collect(),
    );
    let mut package_ast = PackageAst {
        package_path: package_path.clone(),
        files: vec![source_file],
    };

    // Macro phase (between Parse and Collect)
    let macro_registry = macros::builtin_registry();
    macros::expand_package(&mut package_ast, &macro_registry, &mut diagnostics);
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }

    let (_prelude_path, prelude_registry, prelude_module) = &*PRELUDE_OUTPUT;
    let base_registry = prelude_registry.clone();
    let mut tc_result = typechecker::typecheck(&package_ast, &base_registry);
    diagnostics.extend_from(&tc_result.diagnostics);
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }

    // Merge prelude typed module (needed for prelude function bodies like bytes())
    tc_result.typed_module.merge_from(prelude_module.clone());

    // Desugar (after merge, before monomorphize)
    desugar::desugar_all(&mut tc_result.typed_module);

    // 5. Resolve and validate main function
    let main_fqn = typechecker::rules::resolve_main_function(
        &tc_result.typed_module,
        None,
        &package_path,
        &mut diagnostics,
    );
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }
    tc_result.typed_module.main_function_fqn = main_fqn;
    if let Some(ref fqn) = tc_result.typed_module.main_function_fqn {
        typechecker::rules::validate_main_signature(&tc_result.typed_module, fqn, &mut diagnostics);
    }
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }

    // 6. Monomorphize
    let merged_registry = base_registry.merge(&tc_result.registry);
    let Some(mono_module) =
        prepare_codegen(tc_result.typed_module, &merged_registry, &mut diagnostics)
    else {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    };

    // 8. Codegen (single-file compile: no component dependencies)
    let empty_universe = witgen::WitImportUniverse::empty();
    let wasm = match codegen::generate_component(
        &mono_module,
        &merged_registry,
        &[],
        &empty_universe,
        &[],
    ) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            diagnostics.error(
                crate::common::span::Span::point(FilePath::from("<codegen>"), 0, 0),
                e.message,
            );
            None
        }
    };

    CompileResult {
        wasm,
        diagnostics,
        test_exports: vec![],
    }
}

/// Prepare all concrete functions, including implementations discovered by coercion.
fn prepare_codegen(
    module: TypedModule,
    registry: &Registry,
    diagnostics: &mut Diagnostics,
) -> Option<TypedModule> {
    let prepare = || -> Result<TypedModule, monomorphize::SpecializationError> {
        let mut module = monomorphize::monomorphize(module, registry)?;
        desugar::coerce_and_capture(&mut module);
        super::coerce::elaborate_coercions(&mut module, registry);
        monomorphize::ensure_vtable_functions(&mut module, registry)?;
        Ok(module)
    };
    match prepare() {
        Ok(module) => Some(module),
        Err(error) => {
            diagnostics.error(error.span, error.message);
            None
        }
    }
}

/// Run the pipeline up to typechecking (no codegen).
pub fn check(source: &str, file_path: &str) -> typechecker::TypeCheckerResult {
    let file: FilePath = Arc::from(file_path);
    let mut lexer = Lexer::new(source, file);
    let raw_tokens = lexer.tokenize();

    let raw_tokens = attach_doc_comments(raw_tokens);
    let mut filter = LayoutFilter::new(raw_tokens);
    let tokens = filter.filter();

    let mut parser = Parser::new(tokens);
    let source_file = parser.parse_source_file();

    // If there are lex/parse errors, return early with them
    let mut early_diagnostics = Diagnostics::new();
    early_diagnostics.extend(lexer.diagnostics());
    early_diagnostics.extend(parser.diagnostics());
    if early_diagnostics.has_errors() {
        return typechecker::TypeCheckerResult {
            registry: Registry::new(),
            typed_module: TypedModule::empty(),
            diagnostics: early_diagnostics,
        };
    }

    let package_path = PackagePath(
        source_file
            .package
            .path
            .iter()
            .map(|s| s.value.clone())
            .collect(),
    );
    let mut package_ast = PackageAst {
        package_path: package_path.clone(),
        files: vec![source_file],
    };

    // Macro phase (between Parse and Collect)
    let macro_registry = macros::builtin_registry();
    let mut macro_diags = Diagnostics::new();
    macros::expand_package(&mut package_ast, &macro_registry, &mut macro_diags);
    if macro_diags.has_errors() {
        return typechecker::TypeCheckerResult {
            registry: Registry::new(),
            typed_module: TypedModule::empty(),
            diagnostics: macro_diags,
        };
    }

    let (_prelude_path, prelude_registry, _prelude_module) = &*PRELUDE_OUTPUT;
    let base_registry = prelude_registry.clone();
    let mut result = typechecker::typecheck(&package_ast, &base_registry);
    // Surface any macro-phase warnings on the result.
    result.diagnostics.extend_from(&macro_diags);

    // Resolve and validate main function
    let main_fqn = typechecker::rules::resolve_main_function(
        &result.typed_module,
        None,
        &package_path,
        &mut result.diagnostics,
    );
    result.typed_module.main_function_fqn = main_fqn;
    if let Some(ref fqn) = result.typed_module.main_function_fqn {
        typechecker::rules::validate_main_signature(
            &result.typed_module,
            fqn,
            &mut result.diagnostics,
        );
    }

    result
}

/// Result of building an entire project.
pub struct ProjectResult {
    /// Present only in query mode; source declarations with resolved signatures.
    pub declarations: Vec<crate::query::DeclarationView>,
    /// Production declarations exported to dependents, excluding external test packages.
    dependency_module: Option<TypedModule>,
    pub typed_module: TypedModule,
    pub registry: Registry,
    /// Registry containing only this project's own public symbols (no dependency types).
    pub project_registry: Registry,
    /// Macro registry containing only this project's own `[[project.macro]]`
    /// entries (no built-ins, no dependency macros). The workspace builder
    /// accumulates these per project and re-derives the dependency macro
    /// registry for each dependent.
    pub project_macros: macros::MacroRegistry,
    pub diagnostics: Diagnostics,
    pub wasm: Option<Vec<u8>>,
    pub test_exports: Vec<TestExportInfo>,
}

/// Load each `[[project.macro]]` entry's script from disk and register it
/// in the given registry as a Rhai-backed derive. Returns the FQNs that
/// were loaded so callers can build a per-project registry for chaining
/// to dependent projects.
fn load_project_macros(
    project: &ResolvedProject,
    registry: &mut macros::MacroRegistry,
    diagnostics: &mut Diagnostics,
) {
    use crate::manifest::MacroKind;
    for m in &project.macros {
        match m.kind {
            MacroKind::Derive => match std::fs::read_to_string(&m.script_path) {
                Ok(source) => {
                    registry.register_rhai_derive(macros::MacroFqn::new(m.fqn.clone()), source);
                }
                Err(e) => {
                    diagnostics.error(
                        crate::common::span::Span::point(FilePath::from("<manifest>"), 0, 0),
                        format!(
                            "failed to read macro script `{}` for project `{}`: {}",
                            m.script_path.display(),
                            project.name.0,
                            e
                        ),
                    );
                }
            },
        }
    }
}

/// Build a single project: discover, parse, typecheck, and merge each package in order.
///
/// `dependency_registry` contains types/signatures from dependency projects.
/// `dependency_typed_module` contains typed functions from dependency projects (for self-contained WASM).
/// `dependency_macros` contains derive macros from transitive dependency projects;
/// the current project's own macros are added on top (taking precedence on FQN collisions).
/// `mode` controls codegen, main validation, and test handling.
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the compiler context parameters explicit at this call boundary."
)]
pub fn build_project(
    project: &ResolvedProject,
    workspace_root: &Path,
    dependency_registry: &Registry,
    dependency_typed_module: TypedModule,
    dependency_macros: &macros::MacroRegistry,
    mode: BuildMode,
    overlays: &HashMap<String, String>,
    resilient: bool,
) -> ProjectResult {
    let mut diagnostics = Diagnostics::new();
    let mut declarations = Vec::new();

    // Build the macro registry up front so every short-circuit return path
    // can include the project's own macros — useful when the workspace
    // builder needs to forward them to a dependent even after this project
    // fails to compile.
    //
    // Layering (last write wins on FQN collision):
    //   builtin (e.g. Equatable) ← transitive dependency macros ← this project's own macros.
    //
    // `project_macros` contains *only* this project's entries so the
    // workspace builder can chain it to dependents without re-merging
    // the prelude or other deps.
    let mut macro_registry = macros::builtin_registry();
    macro_registry.merge_from(dependency_macros);
    let mut project_macros = macros::MacroRegistry::new();
    load_project_macros(project, &mut project_macros, &mut diagnostics);
    macro_registry.merge_from(&project_macros);

    let (prelude_path, prelude_registry, prelude_module) = &*PRELUDE_OUTPUT;

    // Skip adding the prelude when the project itself IS the prelude,
    // or when the dependency registry already contains the prelude (avoids duplicate overloads).
    let is_prelude_project = project.packages.iter().any(|pkg| &pkg.path == prelude_path);
    let deps_have_prelude = dependency_registry.has_package(prelude_path);

    let mut accumulated_registry = if is_prelude_project || deps_have_prelude {
        dependency_registry.clone()
    } else {
        dependency_registry.merge(prelude_registry)
    };
    let mut accumulated_module = dependency_typed_module;
    if !is_prelude_project && !deps_have_prelude {
        accumulated_module.merge_from(prelude_module.clone());
    }

    // Track only this project's own public symbols (for downstream dependency merging)
    let mut project_registry = Registry::new();

    // Load declared binary resources from disk and register them under this
    // project's root package. They're added to *both* the accumulated registry
    // (so the typechecker can resolve `Resource.bytes(...)` calls from this
    // project's source files) and the project registry (so downstream
    // dependents inherit them — useful when a library re-exports an asset,
    // though the visibility check still scopes lookups to the declaring
    // project's root package).
    for resource_name in &project.resources {
        let resource_path = project.project_dir.join(resource_name);
        match std::fs::read(&resource_path) {
            Ok(bytes) => {
                accumulated_registry.register_resource(
                    project.root_package.clone(),
                    resource_name.clone(),
                    bytes.clone(),
                );
                project_registry.register_resource(
                    project.root_package.clone(),
                    resource_name.clone(),
                    bytes.clone(),
                );
                // Stash on the typed module too so codegen sees it after the
                // module is handed off (codegen only takes a `TypedModule`,
                // not a `Registry`).
                accumulated_module
                    .resources
                    .insert((project.root_package.clone(), resource_name.clone()), bytes);
            }
            Err(e) => {
                diagnostics.error(
                    crate::common::span::Span::point(FilePath::from("<manifest>"), 0, 0),
                    format!(
                        "failed to read resource `{}` for project `{}`: {}",
                        resource_name, project.name.0, e
                    ),
                );
            }
        }
    }

    // Component dependencies: build the WIT import universe from this project's
    // declared components, then inject the generated low-level bindings into
    // both registries + the typed module so this project's sources (and
    // downstream dependents) can `use` them. Must run before typechecking any
    // package. The universe + bindings + component bytes are kept for codegen
    // (table build + composition).
    let (mut wit_universe, wit_bindings, wit_component_bytes) =
        match witgen::injection::build_component_universe(&project.components) {
            Ok(Some((universe, bindings, bytes))) => (universe, bindings, bytes),
            Ok(None) => (witgen::WitImportUniverse::empty(), Vec::new(), Vec::new()),
            Err(msg) => {
                diagnostics.error(
                    crate::common::span::Span::point(FilePath::from("<manifest>"), 0, 0),
                    format!(
                        "component dependency error in project `{}`: {msg}",
                        project.name.0
                    ),
                );
                return ProjectResult {
                    declarations,
                    dependency_module: None,
                    typed_module: accumulated_module,
                    registry: accumulated_registry,
                    project_registry,
                    project_macros: project_macros.clone(),
                    diagnostics,
                    wasm: None,
                    test_exports: vec![],
                };
            }
        };
    if !wit_universe.is_empty()
        && let Err(msg) = witgen::injection::inject_wit_bindings(
            &wit_universe,
            &wit_bindings,
            &mut accumulated_registry,
            Some(&mut project_registry),
            &mut accumulated_module,
            Some((
                workspace_root,
                &project.generated_sources_dir(workspace_root),
            )),
        )
    {
        // Generated bindings must always compile — a failure is an internal
        // compiler error, surfaced rather than silently dropped.
        diagnostics.error(
            crate::common::span::Span::point(FilePath::from("<wit-bindings>"), 0, 0),
            format!(
                "internal error generating component bindings for project `{}`: {msg}",
                project.name.0
            ),
        );
        return ProjectResult {
            declarations,
            dependency_module: None,
            typed_module: accumulated_module,
            registry: accumulated_registry,
            project_registry,
            project_macros: project_macros.clone(),
            diagnostics,
            wasm: None,
            test_exports: vec![],
        };
    }

    if mode == BuildMode::Query {
        for (interface, binding) in wit_universe.interfaces.iter().zip(&wit_bindings) {
            if dependency_registry.has_package(&interface.dovetail_package) {
                continue;
            }
            let path = project
                .generated_sources_dir(workspace_root)
                .join(format!("{}.dove", interface.dovetail_package));
            let path = path
                .strip_prefix(workspace_root)
                .unwrap_or(&path)
                .to_string_lossy();
            let mut generated =
                crate::query::parse_declarations(&binding.source, &path, &mut diagnostics);
            crate::query::enrich(
                &mut generated,
                &accumulated_registry,
                &accumulated_module,
                !diagnostics.has_errors(),
            );
            for declaration in &mut generated {
                declaration.generated = true;
            }
            declarations.extend(generated);
        }
    }

    // Validate that no src package uses the reserved "test" prefix
    for package in &project.packages {
        if package.path.0.first().is_some_and(|s| s == "test") {
            diagnostics.error(
                crate::common::span::Span::point(FilePath::from("<manifest>"), 0, 0),
                format!(
                    "package '{}' uses reserved 'test' prefix; 'test' packages belong in the test/ directory",
                    package.path
                ),
            );
        }
    }
    if diagnostics.has_errors() && !resilient {
        return ProjectResult {
            declarations,
            dependency_module: None,
            typed_module: accumulated_module,
            registry: accumulated_registry,
            project_registry,
            project_macros: project_macros.clone(),
            diagnostics,
            wasm: None,
            test_exports: vec![],
        };
    }

    for package in &project.packages {
        // 1. Discover and parse source files
        let mut package_ast = match crate::discovery::discover_and_parse_package_with_overlays(
            &package.path,
            &package.source_dir,
            workspace_root,
            overlays,
            &mut diagnostics,
        ) {
            Some(ast) => ast,
            None => {
                if resilient {
                    continue;
                }
                return ProjectResult {
                    declarations,
                    dependency_module: None,
                    typed_module: accumulated_module,
                    registry: accumulated_registry,
                    project_registry,
                    project_macros: project_macros.clone(),
                    diagnostics,
                    wasm: None,
                    test_exports: vec![],
                };
            }
        };

        let mut package_declarations = if mode == BuildMode::Query {
            let mut views = crate::query::capture(&package_ast);
            views.extend(crate::query::recover_unparsed(
                &package_ast,
                &package.source_dir,
                workspace_root,
                &diagnostics,
            ));
            views
        } else {
            Vec::new()
        };

        // 2. Macro phase (between Parse and Collect)
        macros::expand_package(&mut package_ast, &macro_registry, &mut diagnostics);
        if diagnostics.has_errors() && !resilient {
            return ProjectResult {
                declarations,
                dependency_module: None,
                typed_module: accumulated_module,
                registry: accumulated_registry,
                project_registry,
                project_macros: project_macros.clone(),
                diagnostics,
                wasm: None,
                test_exports: vec![],
            };
        }

        // 3. Typecheck this package against accumulated registry
        let tc_result = typechecker::typecheck(&package_ast, &accumulated_registry);
        if mode == BuildMode::Query {
            crate::query::add_generated(&mut package_declarations, &package_ast);
            crate::query::enrich(
                &mut package_declarations,
                &tc_result.registry,
                &tc_result.typed_module,
                !diagnostics.has_errors() && !tc_result.diagnostics.has_errors(),
            );
            declarations.extend(package_declarations);
        }
        diagnostics.extend_from(&tc_result.diagnostics);

        // 3. Merge results for next packages (always merge in resilient mode)
        accumulated_registry = accumulated_registry.merge(&tc_result.registry);
        project_registry = project_registry.merge(&tc_result.registry);
        accumulated_module.merge_from(tc_result.typed_module);

        if diagnostics.has_errors() && !resilient {
            return ProjectResult {
                declarations,
                dependency_module: None,
                typed_module: accumulated_module,
                registry: accumulated_registry,
                project_registry,
                project_macros: project_macros.clone(),
                diagnostics,
                wasm: None,
                test_exports: vec![],
            };
        }
    }

    let dependency_module = (mode == BuildMode::Test).then(|| accumulated_module.clone());

    // Discover and compile test packages (Test mode only)
    if mode == BuildMode::Test {
        let test_packages = crate::discovery::discover_test_directories(&project.project_dir);

        // Grant test packages internal access to all src packages
        if !test_packages.is_empty() {
            for package in &project.packages {
                accumulated_registry.grant_test_internal_access(package.path.clone());
            }
        }

        for (pkg_path, source_dir) in &test_packages {
            let mut package_ast = match crate::discovery::discover_and_parse_package_with_overlays(
                pkg_path,
                source_dir,
                workspace_root,
                overlays,
                &mut diagnostics,
            ) {
                Some(ast) => ast,
                None => continue,
            };

            // Macro phase
            macros::expand_package(&mut package_ast, &macro_registry, &mut diagnostics);
            if diagnostics.has_errors() && !resilient {
                return ProjectResult {
                    declarations,
                    dependency_module: None,
                    typed_module: accumulated_module,
                    registry: accumulated_registry,
                    project_registry,
                    project_macros: project_macros.clone(),
                    diagnostics,
                    wasm: None,
                    test_exports: vec![],
                };
            }

            let tc_result = typechecker::typecheck(&package_ast, &accumulated_registry);
            diagnostics.extend_from(&tc_result.diagnostics);

            // Accumulate for later test packages and codegen
            accumulated_registry = accumulated_registry.merge(&tc_result.registry);
            accumulated_module.merge_from(tc_result.typed_module);

            if diagnostics.has_errors() && !resilient {
                return ProjectResult {
                    declarations,
                    dependency_module: None,
                    typed_module: accumulated_module,
                    registry: accumulated_registry,
                    project_registry,
                    project_macros: project_macros.clone(),
                    diagnostics,
                    wasm: None,
                    test_exports: vec![],
                };
            }
        }
    }

    // 4. Mode-dependent: main resolution or test conversion
    let emit_wasm = mode == BuildMode::Build || mode == BuildMode::Test;

    // Desugar all expressions before codegen pipeline
    if emit_wasm {
        desugar::desugar_all(&mut accumulated_module);
    }

    let test_exports = if mode == BuildMode::Test {
        accumulated_module
            .tests
            .retain(|t| !t.span.file.starts_with("<prelude>/"));
        convert_tests_to_functions(&mut accumulated_module)
    } else if mode == BuildMode::Build {
        // Clear tests in build mode — they don't participate in codegen
        accumulated_module.tests.clear();

        // Resolve and validate main function
        let main_fqn = typechecker::rules::resolve_main_function(
            &accumulated_module,
            project.main_function.as_ref(),
            &project.root_package,
            &mut diagnostics,
        );
        if diagnostics.has_errors() {
            return ProjectResult {
                declarations,
                dependency_module: None,
                typed_module: accumulated_module,
                registry: accumulated_registry,
                project_registry,
                project_macros: project_macros.clone(),
                diagnostics,
                wasm: None,
                test_exports: vec![],
            };
        }
        accumulated_module.main_function_fqn = main_fqn;
        if let Some(ref fqn) = accumulated_module.main_function_fqn {
            typechecker::rules::validate_main_signature(&accumulated_module, fqn, &mut diagnostics);
        }
        if diagnostics.has_errors() {
            return ProjectResult {
                declarations,
                dependency_module: None,
                typed_module: accumulated_module,
                registry: accumulated_registry,
                project_registry,
                project_macros: project_macros.clone(),
                diagnostics,
                wasm: None,
                test_exports: vec![],
            };
        }
        vec![]
    } else {
        // Check mode: no main validation, no codegen
        vec![]
    };

    // 5. Monomorphize + ByName/Capture + Variance casts + Codegen
    let wasm = if emit_wasm {
        prepare_codegen(
            accumulated_module.clone(),
            &accumulated_registry,
            &mut diagnostics,
        )
        .and_then(|mono_module| {
            // Map the (monomorphized) binding functions back to their WIT imports so
            // codegen can swap each stub body for a canonical-ABI import call.
            if !wit_universe.is_empty() {
                witgen::injection::build_wit_table(&mut wit_universe, &wit_bindings, &mono_module);
            }
            match codegen::generate_component(
                &mono_module,
                &accumulated_registry,
                &test_exports,
                &wit_universe,
                &wit_component_bytes,
            ) {
                Ok(bytes) => Some(bytes),
                Err(e) => {
                    diagnostics.error(
                        crate::common::span::Span::point(FilePath::from("<codegen>"), 0, 0),
                        e.message,
                    );
                    None
                }
            }
        })
    } else {
        None
    };

    ProjectResult {
        declarations,
        dependency_module,
        typed_module: accumulated_module,
        registry: accumulated_registry,
        project_registry,
        project_macros,
        diagnostics,
        wasm,
        test_exports,
    }
}

/// Result of building an entire workspace (or filtered subset).
pub struct WorkspaceResult {
    /// Per-project results, in topological order.
    pub project_results: Vec<(String, ProjectResult)>,
    /// Accumulated diagnostics from all projects.
    pub diagnostics: Diagnostics,
}

/// Build a workspace: iterate projects in topological order, passing accumulated
/// dependency registry and typed module to each project.
///
/// If `project_filter` is Some, only build that project and its transitive dependencies.
/// `mode` controls codegen, main validation, and test handling.
pub fn build_workspace(
    workspace: &ResolvedWorkspace,
    project_filter: Option<&str>,
    mode: BuildMode,
    overlays: &HashMap<String, String>,
    resilient: bool,
    on_progress: Option<&WorkspaceProgress>,
) -> WorkspaceResult {
    let mut all_diagnostics = Diagnostics::new();
    let mut project_results: Vec<(String, ProjectResult)> = Vec::new();

    let projects_to_build: Vec<&ResolvedProject> = match project_filter {
        Some(name) => filter_projects(workspace, name),
        None => workspace.projects.iter().collect(),
    };

    let total_projects = projects_to_build.len();
    for target in projects_to_build
        .iter()
        .filter(|p| workspace.is_local(p) && project_filter.is_none_or(|name| p.name.0 == name))
    {
        if let Err(error) = crate::manifest::validate_dependency_closure(workspace, &target.name.0)
        {
            all_diagnostics.error(
                crate::common::span::Span::point(Arc::from("Dovetail.toml"), 1, 1),
                error.to_string(),
            );
            return WorkspaceResult {
                project_results,
                diagnostics: all_diagnostics,
            };
        }
    }

    // Stores each completed project's own registry, typed module, and macro
    // registry (no transitive deps).
    let mut project_outputs: BTreeMap<String, (Registry, TypedModule, macros::MacroRegistry)> =
        BTreeMap::new();

    for (index, project) in projects_to_build.iter().enumerate() {
        if let Some(cb) = on_progress {
            cb(index, total_projects, &project.name.0);
        }
        // Collect all transitive dependency project names (deduplicated).
        let mut all_dep_names: Vec<String> = Vec::new();
        let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut queue: std::collections::VecDeque<String> = project
            .depends
            .iter()
            .filter_map(|d| workspace.project(&d.0).map(|p| p.identity()))
            .collect();
        while let Some(name) = queue.pop_front() {
            if !seen.insert(name.clone()) {
                continue;
            }
            all_dep_names.push(name.clone());
            if let Some(dep_proj) = workspace.project(&name) {
                for dd in &dep_proj.depends {
                    if let Some(dep) = workspace.project(&dd.0) {
                        queue.push_back(dep.identity());
                    }
                }
            }
        }

        // Merge each transitive dependency's own registry, typed module, and
        // macro registry exactly once. Strip dependency tests — they belong
        // to the dependency project, not to us.
        let mut dep_registry = Registry::new();
        let mut dep_module = TypedModule::empty();
        let mut dep_macros = macros::MacroRegistry::new();
        for dep_name in &all_dep_names {
            if let Some((reg, module, mreg)) = project_outputs.get(dep_name) {
                dep_registry = dep_registry.merge(reg);
                let mut dep = module.clone();
                dep.tests.clear();
                dep.functions.retain(|k, _| !k.0.starts_with("$test$"));
                dep_module.merge_from(dep);
                dep_macros.merge_from(mreg);
            } else if !resilient {
                panic!(
                    "internal error: dependency '{}' not built before '{}' \
                     — projects must be in topological order",
                    dep_name, project.name.0
                );
            }
        }

        // In Test mode every project gets Test mode so each produces its own
        // WASM component with its own test exports.  For Build/Check mode only
        // the final project (the target) gets the real mode; dependencies are
        // compiled in Check mode (no codegen needed).
        let project_mode = if mode == BuildMode::Query
            || workspace.is_local(project)
                && project_filter.is_none_or(|name| project.name.0 == name)
        {
            mode
        } else {
            BuildMode::Check
        };

        let mut result = build_project(
            project,
            &workspace.workspace_root,
            &dep_registry,
            dep_module,
            &dep_macros,
            project_mode,
            overlays,
            resilient,
        );

        all_diagnostics.extend_from(&result.diagnostics);

        // Export production declarations with the project's public registry and
        // macros. External test packages belong only to the owning project.
        project_outputs.insert(
            project.identity(),
            (
                result.project_registry.clone(),
                result
                    .dependency_module
                    .take()
                    .unwrap_or_else(|| result.typed_module.clone()),
                result.project_macros.clone(),
            ),
        );

        if result.diagnostics.has_errors() && !resilient {
            project_results.push((workspace.result_key(project), result));
            break;
        }

        project_results.push((workspace.result_key(project), result));
    }

    WorkspaceResult {
        project_results,
        diagnostics: all_diagnostics,
    }
}

/// Convert test declarations into regular functions for codegen.
/// Each test becomes a no-param, Unit-returning function inserted into `typed_module.functions`.
/// Returns metadata about each test for export from the WASM component.
fn convert_tests_to_functions(typed_module: &mut TypedModule) -> Vec<TestExportInfo> {
    use crate::common::types::Visibility;
    use crate::compiler::typechecker::types::TypedFunction;

    // Build a lookup map from mangled name to test metadata before draining
    let mut test_meta: BTreeMap<String, TestExportInfo> = BTreeMap::new();
    for (idx, test) in typed_module.tests.iter().enumerate() {
        test_meta.insert(
            test.mangled_name.0.clone(),
            TestExportInfo {
                index: idx, // temporary; re-assigned below
                name: test.name.clone(),
                fqtn: test.fqtn.clone(),
                package_path: test.package_path.to_string(),
                source_file: test.span.file.to_string(),
                skip_reason: test.skip_reason.clone(),
                expected_panic: test.expected_panic.clone(),
                timeout_ms: test.timeout_ms,
            },
        );
    }

    let tests: Vec<_> = std::mem::take(&mut typed_module.tests);
    for test in tests {
        let display_name = format!("test::{}", test.name);
        typed_module.functions.insert(
            test.mangled_name.clone(),
            TypedFunction {
                visibility: Visibility::Internal,
                name: test.mangled_name,
                type_params: vec![],
                params: vec![],
                return_type: test.return_type,
                body: test.body,
                span: test.span,
                vtable_self_type: None,
                is_async: false,
                source_name: test.name.clone(),
                display_name,
            },
        );
    }

    // Build test_exports by scanning functions map in BTreeMap order
    let mut test_exports = Vec::new();
    let mut index = 0;
    for key in typed_module.functions.keys() {
        if key.0.starts_with("$test$")
            && let Some(meta) = test_meta.get(&key.0)
        {
            let mut info = meta.clone();
            info.index = index;
            test_exports.push(info);
            index += 1;
        }
    }
    test_exports
}

/// Run the full compilation pipeline for test mode on a single source string.
/// Skips main function resolution; converts tests to functions for codegen.
/// Returns the compile result and test metadata (names).
pub fn compile_for_test(source: &str, file_path: &str) -> CompileResult {
    compile_for_test_with_derives(source, file_path, &[])
}

/// Like [`compile_for_test`] but also registers additional Rhai-defined
/// derive macros into the macro registry before the macro phase runs.
/// Each entry is `(fqn, script_source)` — e.g.
/// `("a.MyShow", "let parts = []; ... `implement ...` ")`.
///
/// Used by integration tests that exercise user-defined derive macros.
pub fn compile_for_test_with_derives(
    source: &str,
    file_path: &str,
    rhai_derives: &[(&str, &str)],
) -> CompileResult {
    let mut diagnostics = Diagnostics::new();
    let file: FilePath = Arc::from(file_path);

    // 1. Lex
    let mut lexer = Lexer::new(source, file);
    let raw_tokens = lexer.tokenize();
    diagnostics.extend(lexer.diagnostics());
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }

    // 2. Layout filter
    let raw_tokens = attach_doc_comments(raw_tokens);
    let mut filter = LayoutFilter::new(raw_tokens);
    let tokens = filter.filter();

    // 3. Parse
    let mut parser = Parser::new(tokens);
    let source_file = parser.parse_source_file();
    diagnostics.extend(parser.diagnostics());
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }

    // 4. Typecheck
    let package_path = PackagePath(
        source_file
            .package
            .path
            .iter()
            .map(|s| s.value.clone())
            .collect(),
    );
    let mut package_ast = PackageAst {
        package_path: package_path.clone(),
        files: vec![source_file],
    };

    // Macro phase (between Parse and Collect)
    let mut macro_registry = macros::builtin_registry();
    for (fqn, script) in rhai_derives {
        macro_registry.register_rhai_derive(macros::MacroFqn::new(*fqn), (*script).to_string());
    }
    macros::expand_package(&mut package_ast, &macro_registry, &mut diagnostics);
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }

    let (_prelude_path, prelude_registry, prelude_module) = &*PRELUDE_OUTPUT;
    let base_registry = prelude_registry.clone();
    let mut tc_result = typechecker::typecheck(&package_ast, &base_registry);
    diagnostics.extend_from(&tc_result.diagnostics);
    if diagnostics.has_errors() {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    }

    // Merge prelude typed module
    tc_result.typed_module.merge_from(prelude_module.clone());

    // Desugar (after merge, before monomorphize)
    desugar::desugar_all(&mut tc_result.typed_module);

    // Convert tests to functions (no main resolution), excluding prelude tests
    tc_result
        .typed_module
        .tests
        .retain(|t| !t.span.file.starts_with("<prelude>/"));
    let test_exports = convert_tests_to_functions(&mut tc_result.typed_module);

    // 5. Monomorphize + ByName/Capture + Variance casts + Codegen
    let merged_registry = base_registry.merge(&tc_result.registry);
    let Some(mono_module) =
        prepare_codegen(tc_result.typed_module, &merged_registry, &mut diagnostics)
    else {
        return CompileResult {
            wasm: None,
            diagnostics,
            test_exports: vec![],
        };
    };
    let empty_universe = witgen::WitImportUniverse::empty();
    let wasm = match codegen::generate_component(
        &mono_module,
        &merged_registry,
        &test_exports,
        &empty_universe,
        &[],
    ) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            diagnostics.error(
                crate::common::span::Span::point(FilePath::from("<codegen>"), 0, 0),
                e.message,
            );
            None
        }
    };

    CompileResult {
        wasm,
        diagnostics,
        test_exports,
    }
}

/// Filter workspace projects to include only the target project and its transitive dependencies.
/// Preserves topological order.
fn filter_projects<'a>(
    workspace: &'a ResolvedWorkspace,
    target_name: &str,
) -> Vec<&'a ResolvedProject> {
    let mut needed = BTreeSet::new();
    let mut queue = vec![target_name.to_string()];
    while let Some(key) = queue.pop() {
        if let Some(project) = workspace.project(&key)
            && needed.insert(project.identity())
        {
            queue.extend(project.depends.iter().map(|d| d.0.clone()));
        }
    }
    workspace
        .projects
        .iter()
        .filter(|p| needed.contains(&p.identity()))
        .collect()
}
