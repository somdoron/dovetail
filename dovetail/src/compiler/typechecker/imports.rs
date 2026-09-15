use std::collections::BTreeMap;

use crate::common::diagnostics::Diagnostics;
use crate::common::span::{FilePath, Span};
use crate::common::types::{Fqn, PackagePath, SymbolName};
use crate::parser::ast::SourceFile;

use super::registry::{ExtensionBlockSignature, Registry};

/// What an import resolved to.
#[derive(Debug)]
pub enum ImportTarget {
    /// A specific symbol (function) in a package.
    Symbol(Fqn),
    /// An entire package (accessed via `alias.func()`).
    Package(PackagePath),
    /// A named extension (must be imported to use its methods).
    Extension(Fqn),
}

/// A fully resolved import with its local alias and source span.
#[derive(Debug)]
pub struct ResolvedImport {
    pub local_name: String,
    pub target: ImportTarget,
    pub span: Span,
}

/// Per-file import scope.
#[derive(Debug, Default)]
pub struct ImportScope {
    pub imports: Vec<ResolvedImport>,
    /// Imported extension blocks. Visibility filtering deferred to lookup time.
    pub extension_blocks: Vec<ExtensionBlockSignature>,
}

impl ImportScope {
    pub const EMPTY: ImportScope = ImportScope {
        imports: Vec::new(),
        extension_blocks: Vec::new(),
    };

    pub fn new() -> Self {
        Self {
            imports: Vec::new(),
            extension_blocks: Vec::new(),
        }
    }

    pub fn lookup(&self, name: &str) -> Option<&ResolvedImport> {
        self.imports.iter().find(|i| i.local_name == name)
    }
}

/// Per-file import scopes, keyed by file path.
pub type PackageImportScopes = BTreeMap<FilePath, ImportScope>;

/// Build import scopes for each file in the package.
///
/// For each import declaration, tries to resolve it as:
/// 1. A symbol import (last segment is a function name in the package formed by the preceding segments)
/// 2. A package import (entire path is a package)
///
/// Diagnostics are emitted for unresolvable imports and duplicate local names.
pub fn build_import_scopes(
    current_package: &PackagePath,
    files: &[&SourceFile],
    registry: &Registry,
    diagnostics: &mut Diagnostics,
) -> PackageImportScopes {
    let mut scopes = BTreeMap::new();

    for file in files {
        let file_path = file.package.span.file.clone();
        let mut scope = ImportScope::new();
        let mut local_names: BTreeMap<String, Span> = BTreeMap::new();

        for import in &file.imports {
            let local_name = import
                .alias
                .as_ref()
                .map(|a| a.value.clone())
                .unwrap_or_else(|| import.path.last().unwrap().value.clone());

            // Warn on duplicate local names and override the previous import
            if let Some(prev_span) = local_names.get(&local_name) {
                diagnostics.warning(
                    import.span.clone(),
                    format!(
                        "duplicate import name '{}' (previously imported at {}:{}); overriding",
                        local_name,
                        prev_span.file.as_ref(),
                        prev_span.line
                    ),
                );
                scope.imports.retain(|i| i.local_name != local_name);
            }

            // Try symbol import first: path[0..n-1] = package, path[n-1] = symbol
            if import.path.len() >= 2 {
                let pkg_segments: Vec<String> = import.path[..import.path.len() - 1]
                    .iter()
                    .map(|s| s.value.clone())
                    .collect();
                let symbol_name = &import.path.last().unwrap().value;
                let pkg_path = PackagePath(pkg_segments);
                let fqn = Fqn {
                    package: pkg_path,
                    symbol: SymbolName(symbol_name.clone()),
                };

                if registry
                    .lookup_function(&fqn, current_package, &file_path)
                    .is_some()
                    || registry
                        .lookup_generic_function(&fqn, current_package)
                        .is_some()
                    || registry
                        .lookup_type(&fqn, current_package, &file_path)
                        .is_some()
                    || registry
                        .lookup_type_alias(&fqn, current_package, &file_path)
                        .is_some()
                    || registry.lookup_trait(&fqn, current_package).is_some()
                    || registry.lookup_module(&fqn).is_some()
                    || registry
                        .lookup_global(&fqn, current_package, &file_path)
                        .is_some()
                {
                    local_names.insert(local_name.clone(), import.span.clone());
                    scope.imports.push(ResolvedImport {
                        local_name,
                        target: ImportTarget::Symbol(fqn),
                        span: import.span.clone(),
                    });
                    continue;
                }

                // Try extension block import (unified for generic and non-generic).
                // A named extension may have multiple blocks under one FQN targeting
                // different `for_type`s; a single import brings them all into scope.
                let ext_blocks = registry.lookup_extension_blocks_by_fqn(&fqn);
                if !ext_blocks.is_empty() {
                    for ext_block in ext_blocks {
                        scope.extension_blocks.push(ext_block.clone());
                    }
                    local_names.insert(local_name.clone(), import.span.clone());
                    scope.imports.push(ResolvedImport {
                        local_name,
                        target: ImportTarget::Extension(fqn),
                        span: import.span.clone(),
                    });
                    continue;
                }
            }

            // Try package import: entire path is a package
            let full_path = PackagePath(import.path.iter().map(|s| s.value.clone()).collect());
            if registry.has_package(&full_path) {
                local_names.insert(local_name.clone(), import.span.clone());
                scope.imports.push(ResolvedImport {
                    local_name,
                    target: ImportTarget::Package(full_path),
                    span: import.span.clone(),
                });
                continue;
            }

            // Neither symbol nor package found
            diagnostics.error(
                import.span.clone(),
                format!(
                    "cannot resolve import '{}': no matching symbol or package found",
                    import
                        .path
                        .iter()
                        .map(|s| s.value.as_str())
                        .collect::<Vec<_>>()
                        .join(".")
                ),
            );
        }

        scopes.insert(file_path, scope);
    }

    scopes
}
