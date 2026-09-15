//! Macro phase. Runs between Parse and Collect.
//!
//! Walks each file's `RecordDecl` / `EnumDecl` declarations for `@derive(...)`
//! attributes. For each attribute, looks the macro up in the [`MacroRegistry`]
//! and asks the registered [`DeriveExpander`] for a list of `Declaration`s
//! (typically a single `Declaration::Implement`). The generated declarations
//! are spliced into the same source file. The `attributes` vec is then
//! cleared on the target so Collect / Inference do not re-process it.
//!
//! The macro phase is purely syntactic — it does not look at the registry
//! built by Collect, does not infer types, and does not see mangled names.

use std::collections::HashMap;
use std::sync::Arc;

use crate::common::diagnostics::Diagnostics;
use crate::common::span::Span;
use crate::compiler::parser::ast::{
    Declaration, DeriveAttribute, EnumDecl, NewtypeDecl, PackageAst, RecordDecl, SourceFile,
};

pub mod rhai_expander;

/// The fully-qualified name of a macro, e.g. `standard.prelude.Equatable`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MacroFqn(pub String);

impl MacroFqn {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

/// What a derive macro sees: the decl it's attached to.
#[derive(Debug)]
pub enum DeriveTarget<'a> {
    Record(&'a RecordDecl),
    Enum(&'a EnumDecl),
    Newtype(&'a NewtypeDecl),
}

#[derive(Debug)]
pub struct DeriveInput<'a> {
    pub target: DeriveTarget<'a>,
    /// Span of the `@derive(...)` attribute. Used by the expander as the
    /// default span for any synthesized AST node so diagnostics point at
    /// the call site.
    pub call_site: Span,
}

/// A failure to expand a macro. Surfaces as a diagnostic on the call-site span.
#[derive(Debug)]
pub struct MacroError {
    pub message: String,
}

impl MacroError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// A derive macro: takes a record/enum AST node, returns one or more
/// declarations to splice into the same source file.
pub trait DeriveExpander: Send + Sync {
    fn expand(&self, input: DeriveInput<'_>) -> Result<Vec<Declaration>, MacroError>;
}

/// Holds all macros visible to the current compilation. Built up package by
/// package (transitive through dependencies) before each package's macro
/// phase runs.
#[derive(Default, Clone)]
pub struct MacroRegistry {
    derives: HashMap<MacroFqn, Arc<dyn DeriveExpander>>,
    /// Short names → FQN map for unqualified lookups. Multiple FQNs may
    /// share a short name; in that case the short name is ambiguous and
    /// resolution falls back to "report not-found" rather than picking one.
    short_names: HashMap<String, Vec<MacroFqn>>,
}

impl MacroRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_derive(&mut self, fqn: MacroFqn, expander: Arc<dyn DeriveExpander>) {
        let short_name = fqn
            .0
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_string();
        self.short_names
            .entry(short_name)
            .or_default()
            .push(fqn.clone());
        self.derives.insert(fqn, expander);
    }

    /// Convenience: register a [`rhai_expander::RhaiDeriveExpander`] by
    /// FQN, taking the script source directly.
    pub fn register_rhai_derive(&mut self, fqn: MacroFqn, script: impl Into<String>) {
        let expander = Arc::new(rhai_expander::RhaiDeriveExpander::new(
            fqn.0.clone(),
            script,
        ));
        self.register_derive(fqn, expander);
    }

    /// Merge another registry into this one. Entries from `other` take
    /// precedence on FQN collisions — callers should ensure precedence is
    /// the desired direction (the workspace builder feeds transitive deps
    /// first, then the current project, so the current project's macros win).
    pub fn merge_from(&mut self, other: &MacroRegistry) {
        for (fqn, expander) in &other.derives {
            self.register_derive(fqn.clone(), Arc::clone(expander));
        }
    }

    fn lookup(&self, fqn: &MacroFqn) -> Option<&Arc<dyn DeriveExpander>> {
        self.derives.get(fqn)
    }

    fn resolve(
        &self,
        path: &[String],
        imports: &[(Vec<String>, Option<String>)],
    ) -> ResolveResult {
        // 1. Fully-qualified: try as a direct FQN.
        if path.len() > 1 {
            let fqn = MacroFqn::new(path.join("."));
            if self.lookup(&fqn).is_some() {
                return ResolveResult::Found(fqn);
            }
            return ResolveResult::NotFound;
        }

        let name = &path[0];

        // 2. Aliased import: `import a.b.X as Y` and the user wrote `@derive(Y)`.
        for (import_path, alias) in imports {
            if alias.as_deref() == Some(name.as_str()) {
                let fqn = MacroFqn::new(import_path.join("."));
                if self.lookup(&fqn).is_some() {
                    return ResolveResult::Found(fqn);
                }
            }
        }

        // 3. Direct import: `import a.b.X` and the user wrote `@derive(X)`.
        for (import_path, alias) in imports {
            if alias.is_none()
                && import_path.last().map(|s| s.as_str()) == Some(name.as_str())
            {
                let fqn = MacroFqn::new(import_path.join("."));
                if self.lookup(&fqn).is_some() {
                    return ResolveResult::Found(fqn);
                }
            }
        }

        // 4. Unqualified prelude-like lookup by short name.
        if let Some(candidates) = self.short_names.get(name) {
            if candidates.len() == 1 {
                return ResolveResult::Found(candidates[0].clone());
            } else if candidates.len() > 1 {
                return ResolveResult::Ambiguous(candidates.clone());
            }
        }

        ResolveResult::NotFound
    }
}

#[derive(Debug)]
enum ResolveResult {
    Found(MacroFqn),
    Ambiguous(Vec<MacroFqn>),
    NotFound,
}

/// Run the macro phase on a single package.
///
/// Mutates `package_ast` in place: each `@derive(...)` attribute is replaced
/// with the spliced-in `implement` (or other) declaration(s) it generated,
/// and the attribute list on the source decl is emptied so downstream phases
/// don't see it again.
pub fn expand_package(
    package_ast: &mut PackageAst,
    registry: &MacroRegistry,
    diagnostics: &mut Diagnostics,
) {
    for file in &mut package_ast.files {
        expand_file(file, registry, diagnostics);
    }
}

fn expand_file(
    file: &mut SourceFile,
    registry: &MacroRegistry,
    diagnostics: &mut Diagnostics,
) {
    let imports: Vec<(Vec<String>, Option<String>)> = file
        .imports
        .iter()
        .map(|imp| {
            (
                imp.path.iter().map(|s| s.value.clone()).collect::<Vec<_>>(),
                imp.alias.as_ref().map(|a| a.value.clone()),
            )
        })
        .collect();

    let mut generated: Vec<Declaration> = Vec::new();

    for decl in &mut file.declarations {
        match decl {
            Declaration::Record(r) => {
                let attrs = std::mem::take(&mut r.attributes);
                expand_decl_attrs(
                    DeriveTarget::Record(r),
                    &attrs,
                    registry,
                    &imports,
                    diagnostics,
                    &mut generated,
                );
            }
            Declaration::Enum(e) => {
                let attrs = std::mem::take(&mut e.attributes);
                expand_decl_attrs(
                    DeriveTarget::Enum(e),
                    &attrs,
                    registry,
                    &imports,
                    diagnostics,
                    &mut generated,
                );
            }
            Declaration::Newtype(n) => {
                let attrs = std::mem::take(&mut n.attributes);
                expand_decl_attrs(
                    DeriveTarget::Newtype(n),
                    &attrs,
                    registry,
                    &imports,
                    diagnostics,
                    &mut generated,
                );
            }
            _ => {}
        }
    }

    file.declarations.extend(generated);
}

fn expand_decl_attrs(
    target: DeriveTarget<'_>,
    attrs: &[DeriveAttribute],
    registry: &MacroRegistry,
    imports: &[(Vec<String>, Option<String>)],
    diagnostics: &mut Diagnostics,
    generated: &mut Vec<Declaration>,
) {
    for attr in attrs {
        if attr.macro_name.is_empty() {
            // Parser already reported a syntax error.
            continue;
        }
        let path: Vec<String> = attr
            .macro_name
            .iter()
            .map(|s| s.value.clone())
            .collect();

        match registry.resolve(&path, imports) {
            ResolveResult::Found(fqn) => {
                let expander = registry
                    .lookup(&fqn)
                    .expect("resolve returned Found for a missing macro");
                let input = DeriveInput {
                    target: borrow_target(&target),
                    call_site: attr.span.clone(),
                };
                match expander.expand(input) {
                    Ok(decls) => generated.extend(decls),
                    Err(err) => {
                        diagnostics.error(
                            attr.span.clone(),
                            format!("@derive({}) failed: {}", path.join("."), err.message),
                        );
                    }
                }
            }
            ResolveResult::Ambiguous(candidates) => {
                let names: Vec<String> =
                    candidates.iter().map(|c| c.0.clone()).collect();
                diagnostics.error(
                    attr.span.clone(),
                    format!(
                        "@derive({}) is ambiguous; matches {}",
                        path.join("."),
                        names.join(", ")
                    ),
                );
            }
            ResolveResult::NotFound => {
                diagnostics.error(
                    attr.span.clone(),
                    format!("unknown derive macro '{}'", path.join(".")),
                );
            }
        }
    }
}

fn borrow_target<'a>(target: &DeriveTarget<'a>) -> DeriveTarget<'a> {
    match target {
        DeriveTarget::Record(r) => DeriveTarget::Record(*r),
        DeriveTarget::Enum(e) => DeriveTarget::Enum(*e),
        DeriveTarget::Newtype(n) => DeriveTarget::Newtype(*n),
    }
}

/// The Rhai source for `@derive(Equatable)`, embedded at compile time.
/// Lives at `dovetail/prelude/macros/Equatable.rhai`.
const EQUATABLE_RHAI_SCRIPT: &str =
    include_str!("../../../prelude/macros/Equatable.rhai");

/// Build a MacroRegistry pre-populated with the compiler's built-in derive macros.
///
/// Currently only `standard.prelude.Equatable` is registered. It is shipped
/// as a Rhai script (`dovetail/prelude/macros/Equatable.rhai`) rather than as Rust
/// code — this validates the script-driven framework on the prelude's own
/// most-used derive.
pub fn builtin_registry() -> MacroRegistry {
    let mut reg = MacroRegistry::new();
    reg.register_rhai_derive(
        MacroFqn::new("standard.prelude.Equatable"),
        EQUATABLE_RHAI_SCRIPT,
    );
    reg
}
