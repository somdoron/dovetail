pub(crate) mod associated_types;
mod bound_projections;
pub(crate) mod class_trait_methods;
pub mod collect;
pub mod imports;
pub mod infer;
pub mod registry;
pub mod rules;
pub mod subtyping;
pub(crate) mod tuple_extension;
pub mod types;

use std::collections::BTreeMap;

use crate::common::diagnostics::Diagnostics;
use crate::common::span::Span;
use crate::common::types::PackagePath;
use crate::parser::ast::{Declaration, PackageAst, SourceFile};
use registry::Registry;
use types::TypedModule;

/// The result of type-checking a single package.
pub struct TypeCheckerResult {
    /// The registry containing this package's public types and function signatures.
    pub registry: Registry,
    /// The typed AST for this package.
    pub typed_module: TypedModule,
    /// Diagnostics accumulated during type-checking.
    pub diagnostics: Diagnostics,
}

impl TypeCheckerResult {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.has_errors()
    }
}

/// Run all three typechecker phases: Collect → Infer → Rules.
///
/// Does NOT run desugar passes — call `desugar_all()` separately
/// before monomorphize/codegen in build mode.
///
/// `registry` contains types and signatures from dependency packages.
/// The typechecker merges new declarations into a clone of this registry.
pub fn typecheck(package_ast: &PackageAst, registry: &Registry) -> TypeCheckerResult {
    let mut diagnostics = Diagnostics::new();

    // Verify package declarations match the expected package path
    let valid_files = verify_package_declarations(package_ast, &mut diagnostics);

    // Validate test name uniqueness within the package
    validate_test_names(&valid_files, &package_ast.package_path, &mut diagnostics);

    let package_registry = collect_with_projection_evidence(
        &package_ast.package_path,
        &valid_files,
        registry,
        &mut diagnostics,
    );

    // Merge dependency + package registries for inference (read-only)
    let merged_registry = registry.merge(&package_registry);

    // Build per-file import scopes
    let import_scopes = imports::build_import_scopes(
        &package_ast.package_path,
        &valid_files,
        &merged_registry,
        &mut diagnostics,
    );

    // Phase 2: Infer — type-check expressions, build typed module
    let typed_module = infer::infer(
        &package_ast.package_path,
        &valid_files,
        &merged_registry,
        &import_scopes,
        &mut diagnostics,
    );

    // Phase 3: Rules — check constraints
    rules::check_rules(
        &typed_module,
        &merged_registry,
        &package_registry,
        &package_ast.package_path,
        &valid_files,
        &mut diagnostics,
    );

    TypeCheckerResult {
        registry: package_registry,
        typed_module,
        diagnostics,
    }
}

fn collect_with_projection_evidence(
    package: &PackagePath,
    files: &[&SourceFile],
    dependencies: &Registry,
    diagnostics: &mut Diagnostics,
) -> Registry {
    let discovery = associated_types::NormalizationScope::collecting();
    let mut provisional_diagnostics = Diagnostics::new();
    let mut current = collect::collect(package, files, dependencies, &mut provisional_diagnostics);
    current.normalize_associated_types();
    let needs_evidence = discovery.encountered_concrete_projections();
    drop(discovery);
    if !needs_evidence {
        diagnostics.extend_from(&provisional_diagnostics);
        return current;
    }

    // A concrete alias can unlock an implementation target, which can unlock
    // another alias. Refresh the snapshot until those inputs stop changing.
    // An acyclic dependency chain cannot exceed the number of declarations;
    // the extra rounds allow discovery and a final equality comparison.
    let limit = files
        .iter()
        .map(|file| file.declarations.len())
        .sum::<usize>()
        + 2;
    let mut converged = false;
    for _ in 0..limit {
        let complete = dependencies.merge(&current);
        let normalization = associated_types::NormalizationScope::provisional(&complete);
        let mut ignored_diagnostics = Diagnostics::new();
        let mut next = collect::collect(package, files, dependencies, &mut ignored_diagnostics);
        next.normalize_associated_types();
        converged = current.same_projection_evidence(&next);
        current = next;
        drop(normalization);
        if converged {
            break;
        }
    }
    if !converged {
        if let Some(file) = files.first() {
            diagnostics.error(
                file.package.span.clone(),
                "associated-type collection did not converge",
            );
        }
    }

    // Only the stable, strict pass contributes user diagnostics. Provisional
    // failures remain symbolic, so missing providers and cycles are not erased.
    let complete = dependencies.merge(&current);
    let normalization = associated_types::NormalizationScope::install(&complete);
    let mut result = collect::collect(package, files, dependencies, diagnostics);
    result.normalize_associated_types();
    for message in normalization.errors() {
        if let Some(file) = files.first() {
            diagnostics.error(file.package.span.clone(), message);
        }
    }
    result
}

/// Validate that test names are unique within the package.
fn validate_test_names(
    files: &[&SourceFile],
    package_path: &PackagePath,
    diagnostics: &mut Diagnostics,
) {
    let mut seen: BTreeMap<&str, &Span> = BTreeMap::new();
    for file in files {
        for decl in &file.declarations {
            let tests: Vec<&crate::parser::ast::TestDecl> = match decl {
                Declaration::Test(test) => vec![test],
                Declaration::Module(m) => m.tests.iter().collect(),
                _ => vec![],
            };
            for test in tests {
                if let Some(prev_span) = seen.get(test.name.value.as_str()) {
                    diagnostics.error(
                        test.name.span.clone(),
                        format!(
                            "duplicate test name '{}' in package '{}' (first defined at {}:{})",
                            test.name.value, package_path, prev_span.file, prev_span.line,
                        ),
                    );
                } else {
                    seen.insert(&test.name.value, &test.name.span);
                }
            }
        }
    }
}

/// Verify that each file's package declaration matches the expected package path.
/// Returns references to files with matching declarations; mismatches get a diagnostic.
fn verify_package_declarations<'a>(
    package_ast: &'a PackageAst,
    diagnostics: &mut Diagnostics,
) -> Vec<&'a SourceFile> {
    let mut valid = Vec::new();
    for file in &package_ast.files {
        let file_path = PackagePath(file.package.path.iter().map(|s| s.value.clone()).collect());
        if file_path != package_ast.package_path {
            diagnostics.error(
                file.package.span.clone(),
                format!(
                    "file declares package '{}' but expected '{}'",
                    file_path, package_ast.package_path
                ),
            );
        } else {
            valid.push(file);
        }
    }
    valid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::span::FilePath;
    use crate::layout::LayoutFilter;
    use crate::lexer::Lexer;
    use crate::lexer::attach_doc_comments;
    use crate::parser::Parser;

    use crate::parser::ast::SourceFile;

    fn parse_source(source: &str, file_name: &str) -> SourceFile {
        let mut lexer = Lexer::new(source, FilePath::from(file_name));
        let tokens = lexer.tokenize();
        let tokens = attach_doc_comments(tokens);
        let mut filter = LayoutFilter::new(tokens);
        let filtered = filter.filter();
        let mut parser = Parser::new(filtered);
        let source_file = parser.parse_source_file();
        assert!(
            parser.diagnostics().is_empty(),
            "parse errors: {:?}",
            parser.diagnostics()
        );
        source_file
    }

    fn typecheck_source(source: &str) -> TypeCheckerResult {
        let source_file = parse_source(source, "test.dove");
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
        let base_registry = Registry::new();
        typecheck(&package_ast, &base_registry)
    }

    #[test]
    fn test_typecheck_minimal_program() {
        let result = typecheck_source("package a\n\nfunction main(): Unit = ()");
        assert!(
            !result.has_errors(),
            "unexpected errors: {:?}",
            result.diagnostics.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_typecheck_unknown_type() {
        let result = typecheck_source("package a\n\nfunction main(): Foo = ()");
        assert!(result.has_errors());
        let errors: Vec<_> = result.diagnostics.iter().collect();
        assert!(errors[0].message.contains("unknown type"));
    }

    #[test]
    fn test_collect_registers_public_function() {
        use crate::common::types::{Fqn, PackagePath, SymbolName};
        let result = typecheck_source("package a\n\npublic function main(): Unit = ()");
        let fqn = Fqn {
            package: PackagePath(vec!["a".to_string()]),
            symbol: SymbolName("main".to_string()),
        };
        let caller_package = PackagePath(vec!["a".to_string()]);
        let caller_file: crate::common::span::FilePath = "test.dove".into();
        let overloads = result
            .registry
            .lookup_function(&fqn, &caller_package, &caller_file);
        assert!(
            overloads.is_some(),
            "public function should be in returned registry"
        );
        assert_eq!(overloads.as_ref().unwrap().len(), 1);
        assert_eq!(
            overloads.as_ref().unwrap()[0].return_type,
            types::Type::Unit
        );
    }

    #[test]
    fn test_internal_function_filtered_by_caller_package() {
        use crate::common::types::{Fqn, PackagePath, SymbolName};
        let result = typecheck_source("package a\n\nfunction foo(): Unit = ()");
        let fqn = Fqn {
            package: PackagePath(vec!["a".to_string()]),
            symbol: SymbolName("foo".to_string()),
        };
        let caller_file: crate::common::span::FilePath = "test.dove".into();

        // From the same package, the internal function IS visible
        let same_package = PackagePath(vec!["a".to_string()]);
        let overloads = result
            .registry
            .lookup_function(&fqn, &same_package, &caller_file);
        assert!(
            overloads.is_some(),
            "internal function should be visible from the same package"
        );

        // From a different package, the internal function is NOT visible
        let other_package = PackagePath(vec!["b".to_string()]);
        let overloads = result
            .registry
            .lookup_function(&fqn, &other_package, &caller_file);
        assert!(
            overloads.is_none(),
            "internal function should not be visible from a different package"
        );
    }

    #[test]
    fn test_typed_module_has_function() {
        use crate::common::types::MangledName;
        let result = typecheck_source("package a\n\nfunction main(): Unit = ()");
        assert!(!result.has_errors());
        assert_eq!(result.typed_module.functions.len(), 1);
        let func = &result.typed_module.functions[&MangledName("a.main".to_string())];
        assert_eq!(func.return_type, types::Type::Unit);
        assert_eq!(func.body.ty, types::Type::Unit);
    }

    #[test]
    fn test_main_must_return_unit() {
        let result = typecheck_source("package a\n\nfunction main(): Bool = true");
        assert!(!result.has_errors(), "typecheck itself should not error");

        // Main validation happens post-merge via resolve + validate
        let mut diagnostics = crate::common::diagnostics::Diagnostics::new();
        let package_path = PackagePath(vec!["a".to_string()]);
        let main_fqn = rules::resolve_main_function(
            &result.typed_module,
            None,
            &package_path,
            &mut diagnostics,
        );
        assert!(main_fqn.is_some());
        rules::validate_main_signature(
            &result.typed_module,
            main_fqn.as_ref().unwrap(),
            &mut diagnostics,
        );
        assert!(diagnostics.has_errors());
        let errors: Vec<_> = diagnostics.iter().collect();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("must return Unit")),
            "expected 'must return Unit' error, got: {:?}",
            errors
        );
    }

    #[test]
    fn test_non_main_function_can_return_bool() {
        let result = typecheck_source("package a\n\nfunction foo(): Bool = true");
        assert!(
            !result.has_errors(),
            "unexpected errors: {:?}",
            result.diagnostics.iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_multi_file_package() {
        let file_a = parse_source(
            "package a\n\npublic function helper(): Int32 = 42",
            "helper.dove",
        );
        let file_b = parse_source(
            "package a\n\nfunction main(): Unit = assert helper() == 42",
            "main.dove",
        );
        let package_ast = PackageAst {
            package_path: PackagePath(vec!["a".to_string()]),
            files: vec![file_a, file_b],
        };
        let result = typecheck(&package_ast, &Registry::new());
        assert!(
            !result.has_errors(),
            "unexpected errors: {:?}",
            result.diagnostics.iter().collect::<Vec<_>>()
        );
        assert_eq!(result.typed_module.functions.len(), 2);
    }

    #[test]
    fn test_package_declaration_mismatch() {
        let file = parse_source("package b\n\nfunction foo(): Unit = ()", "foo.dove");
        let package_ast = PackageAst {
            package_path: PackagePath(vec!["a".to_string()]),
            files: vec![file],
        };
        let result = typecheck(&package_ast, &Registry::new());
        assert!(result.has_errors());
        let errors: Vec<_> = result.diagnostics.iter().collect();
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("declares package 'b' but expected 'a'")),
            "expected package mismatch error, got: {:?}",
            errors
        );
    }
}
