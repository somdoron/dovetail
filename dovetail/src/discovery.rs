use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::common::diagnostics::Diagnostics;
use crate::common::span::FilePath;
use crate::common::types::PackagePath;
use crate::layout::LayoutFilter;
use crate::lexer::Lexer;
use crate::lexer::attach_doc_comments;
use crate::parser::Parser;
use crate::parser::ast::{PackageAst, SourceFile};

/// Discover all `*.dove` files in a package's source directory,
/// parse them, and combine into a `PackageAst`.
///
/// Files are listed non-recursively (subdirectories are sub-packages).
/// Returns `None` if there are no source files or if any file has parse errors.
pub fn discover_and_parse_package(
    package_path: &PackagePath,
    source_dir: &Path,
    workspace_root: &Path,
    diagnostics: &mut Diagnostics,
) -> Option<PackageAst> {
    let file_paths = match list_dovetail_files(source_dir) {
        Ok(paths) => paths,
        Err(e) => {
            diagnostics.error(
                crate::common::span::Span::point(std::sync::Arc::from("<discovery>"), 1, 1),
                format!("could not read directory '{}': {}", source_dir.display(), e),
            );
            return None;
        }
    };

    if file_paths.is_empty() {
        diagnostics.error(
            crate::common::span::Span::point(std::sync::Arc::from("<discovery>"), 1, 1),
            format!(
                "package '{}' has no source files in '{}'",
                package_path,
                source_dir.display()
            ),
        );
        return None;
    }

    let mut files = Vec::new();

    for path in &file_paths {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                diagnostics.error(
                    crate::common::span::Span::point(std::sync::Arc::from("<discovery>"), 1, 1),
                    format!("could not read '{}': {}", path.display(), e),
                );
                continue;
            }
        };

        // Compute relative path from workspace root for FilePath
        let file_path: FilePath = path
            .strip_prefix(workspace_root)
            .unwrap_or(path)
            .to_string_lossy()
            .into();

        let (source_file, file_diags) = parse_file(&source, file_path);
        if file_diags.has_errors() {
            diagnostics.extend_from(&file_diags);
            continue;
        }
        diagnostics.extend_from(&file_diags);
        files.push(source_file);
    }

    Some(PackageAst {
        package_path: package_path.clone(),
        files,
    })
}

/// Like [`discover_and_parse_package`] but checks `overlays` first before reading from disk.
///
/// `overlays` maps workspace-relative paths (e.g., `src/a.dove`) to their in-memory content.
/// This lets the LSP server use unsaved editor buffers instead of stale disk content.
pub fn discover_and_parse_package_with_overlays(
    package_path: &PackagePath,
    source_dir: &Path,
    workspace_root: &Path,
    overlays: &HashMap<String, String>,
    diagnostics: &mut Diagnostics,
) -> Option<PackageAst> {
    let file_paths = match list_dovetail_files(source_dir) {
        Ok(paths) => paths,
        Err(e) => {
            diagnostics.error(
                crate::common::span::Span::point(std::sync::Arc::from("<discovery>"), 1, 1),
                format!("could not read directory '{}': {}", source_dir.display(), e),
            );
            return None;
        }
    };

    if file_paths.is_empty() {
        diagnostics.error(
            crate::common::span::Span::point(std::sync::Arc::from("<discovery>"), 1, 1),
            format!(
                "package '{}' has no source files in '{}'",
                package_path,
                source_dir.display()
            ),
        );
        return None;
    }

    let mut files = Vec::new();

    for path in &file_paths {
        // Compute relative path from workspace root for FilePath
        let relative: String = path
            .strip_prefix(workspace_root)
            .unwrap_or(path)
            .to_string_lossy()
            .into();

        // Check overlay first, then fall back to disk
        let source = if let Some(content) = overlays.get(&relative) {
            content.clone()
        } else {
            match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    diagnostics.error(
                        crate::common::span::Span::point(
                            std::sync::Arc::from("<discovery>"),
                            1,
                            1,
                        ),
                        format!("could not read '{}': {}", path.display(), e),
                    );
                    continue;
                }
            }
        };

        let file_path: FilePath = relative.into();
        let (source_file, file_diags) = parse_file(&source, file_path);
        if file_diags.has_errors() {
            diagnostics.extend_from(&file_diags);
            continue;
        }
        diagnostics.extend_from(&file_diags);
        files.push(source_file);
    }

    Some(PackageAst {
        package_path: package_path.clone(),
        files,
    })
}

/// List `*.dove` files in the given directory (non-recursive). Sorted for determinism.
fn list_dovetail_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, std::io::Error> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "dove" {
                    paths.push(path);
                }
            }
        }
    }
    paths.sort();
    Ok(paths)
}

/// Lex, layout-filter, and parse a single source string.
/// Public wrapper for use by the LSP server and other callers.
pub fn parse_source(source: &str, file_path: FilePath) -> (SourceFile, Diagnostics) {
    parse_file(source, file_path)
}

/// Lex, layout-filter, and parse a single source file.
fn parse_file(source: &str, file_path: FilePath) -> (SourceFile, Diagnostics) {
    let mut diagnostics = Diagnostics::new();

    let mut lexer = Lexer::new(source, file_path);
    let raw_tokens = lexer.tokenize();
    diagnostics.extend(lexer.diagnostics());

    let raw_tokens = attach_doc_comments(raw_tokens);
    let mut filter = LayoutFilter::new(raw_tokens);
    let tokens = filter.filter();

    let mut parser = Parser::new(tokens);
    let source_file = parser.parse_source_file();
    diagnostics.extend(parser.diagnostics());

    (source_file, diagnostics)
}

/// Discover test directories under `<project_dir>/test/`.
///
/// Walks recursively, finding directories that contain `.dove` files.
/// Returns `(PackagePath, PathBuf)` pairs sorted by package path.
/// - Root `test/` → `PackagePath(["test"])`
/// - Subdirectory `test/utils/` → `PackagePath(["test", "utils"])`
///
/// Returns an empty vec if `test/` doesn't exist.
pub fn discover_test_directories(project_dir: &Path) -> Vec<(PackagePath, PathBuf)> {
    let test_dir = project_dir.join("test");
    if !test_dir.is_dir() {
        return vec![];
    }

    let mut results = Vec::new();
    discover_test_dirs_recursive(&test_dir, &["test".to_string()], &mut results);
    results.sort_by(|(a, _), (b, _)| a.cmp(b));
    results
}

/// Recursively walk directories under test/, collecting those with .dove files.
fn discover_test_dirs_recursive(
    dir: &Path,
    path_segments: &[String],
    results: &mut Vec<(PackagePath, PathBuf)>,
) {
    // Check if this directory has any .dove files
    if let Ok(files) = list_dovetail_files(dir) {
        if !files.is_empty() {
            results.push((PackagePath(path_segments.to_vec()), dir.to_path_buf()));
        }
    }

    // Recurse into subdirectories
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let mut child_segments = path_segments.to_vec();
                child_segments.push(name.to_string());
                discover_test_dirs_recursive(&path, &child_segments, results);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_discover_two_files() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();

        fs::write(
            src.join("helper.dove"),
            "package a\n\npublic function helper(): Int32 = 42",
        )
        .unwrap();
        fs::write(
            src.join("main.dove"),
            "package a\n\nfunction main(): Unit = ()",
        )
        .unwrap();

        let mut diags = Diagnostics::new();
        let result = discover_and_parse_package(
            &PackagePath(vec!["a".to_string()]),
            &src,
            dir.path(),
            &mut diags,
        );

        assert!(
            !diags.has_errors(),
            "unexpected errors: {:?}",
            diags.iter().collect::<Vec<_>>()
        );
        let pkg = result.expect("should produce PackageAst");
        assert_eq!(pkg.files.len(), 2);
        assert_eq!(pkg.package_path, PackagePath(vec!["a".to_string()]));
    }

    #[test]
    fn test_discover_empty_directory() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();

        let mut diags = Diagnostics::new();
        let result = discover_and_parse_package(
            &PackagePath(vec!["a".to_string()]),
            &src,
            dir.path(),
            &mut diags,
        );

        assert!(result.is_none());
        assert!(diags.has_errors());
        let errors: Vec<_> = diags.iter().collect();
        assert!(errors[0].message.contains("no source files"));
    }

    #[test]
    fn test_discover_parse_error_skips_bad_files() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();

        // Invalid syntax
        fs::write(src.join("bad.dove"), "this is not valid Dovetail").unwrap();

        let mut diags = Diagnostics::new();
        let result = discover_and_parse_package(
            &PackagePath(vec!["a".to_string()]),
            &src,
            dir.path(),
            &mut diags,
        );

        // Should still return Some with empty files (bad file skipped)
        let pkg = result.expect("should produce PackageAst even with parse errors");
        assert_eq!(pkg.files.len(), 0);
        assert!(diags.has_errors());
    }
}
