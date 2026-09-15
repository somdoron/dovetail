mod class_rules;
mod coherence;
mod exhaustiveness;
mod generic_static_rules;
mod object_safety;
mod orphan_impl;
mod type_param_shadow;
mod unsafe_cast;
mod variance_position;
pub mod visitor;

use std::sync::Arc;

use crate::common::diagnostics::Diagnostics;
use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName, PackagePath};
use crate::parser::ast::SourceFile;

use crate::typechecker::registry::Registry;
use crate::typechecker::types::{Type, TypedModule};

pub use visitor::{ExprVisitor, TypedExprVisitor};

/// A modular rule that checks properties of a typed module.
pub trait Rule {
    fn check(&mut self, typed_module: &TypedModule, diagnostics: &mut Diagnostics);
}

/// Rules phase: verify that inferred types satisfy declared constraints.
pub fn check_rules(
    typed_module: &TypedModule,
    merged_registry: &Registry,
    package_registry: &Registry,
    package_path: &PackagePath,
    source_files: &[&SourceFile],
    diagnostics: &mut Diagnostics,
) {
    exhaustiveness::ExhaustivenessRule::new(&typed_module.types, merged_registry)
        .check(typed_module, diagnostics);
    type_param_shadow::check_type_param_rules(source_files, diagnostics);
    orphan_impl::check_orphan_rule(package_registry, package_path, diagnostics);
    coherence::check_coherence(merged_registry, package_path, diagnostics);
    object_safety::check_object_safety(package_registry, diagnostics);
    unsafe_cast::UnsafeCastRule::new().check(typed_module, diagnostics);
    variance_position::check_variance_positions(package_registry, merged_registry, diagnostics);
    class_rules::check_class_rules(package_registry, package_path, source_files, diagnostics);
    generic_static_rules::check_generic_static_rules(package_registry, diagnostics);
}

/// Resolve the entry point function after all packages are merged.
///
/// If `explicit_main` is provided (from Dovetail.toml), validates it exists and is
/// within the root_package subtree, then returns it.
///
/// Otherwise, scans for `main` functions in packages starting with `root_package`:
/// - 0 found → `None` (library)
/// - 1 found → use it
/// - Multiple found → error
pub fn resolve_main_function(
    typed_module: &TypedModule,
    explicit_main: Option<&Fqn>,
    root_package: &PackagePath,
    diagnostics: &mut Diagnostics,
) -> Option<Fqn> {
    // If an explicit main was specified in Dovetail.toml, always use it.
    if let Some(fqn) = explicit_main {
        if !fqn.package.starts_with(root_package) {
            diagnostics.error(
                Span::point(Arc::from("Dovetail.toml"), 1, 1),
                format!(
                    "specified main function '{}' is not within root package '{}' subtree",
                    fqn, root_package
                ),
            );
            return None;
        }
        let name = MangledName::for_function_no_params(fqn);
        if typed_module.functions.contains_key(&name) {
            return Some(fqn.clone());
        }
        diagnostics.error(
            Span::point(Arc::from("Dovetail.toml"), 1, 1),
            format!("specified main function '{}' not found", fqn),
        );
        return None;
    }

    // Auto-detect: find all zero-param "main" functions in root_package subtree.
    let main_candidates: Vec<&MangledName> = typed_module
        .functions
        .keys()
        .filter(|name| {
            // Zero-param main: mangled name is "pkg.main" (no $ suffix)
            name.0.ends_with(".main") && !name.0.contains('$')
        })
        .filter(|name| {
            // Must belong to a package starting with root_package
            match Fqn::from_dotted(&name.0) {
                Some(fqn) => fqn.package.starts_with(root_package),
                None => false,
            }
        })
        .collect();

    match main_candidates.len() {
        0 => None,
        1 => Fqn::from_dotted(&main_candidates[0].0),
        _ => {
            let names: Vec<&str> = main_candidates.iter().map(|n| n.0.as_str()).collect();
            diagnostics.error(
                Span::point(Arc::from("Dovetail.toml"), 1, 1),
                format!(
                    "multiple 'main' functions found: {}; specify which to use via 'main' in Dovetail.toml",
                    names.join(", ")
                ),
            );
            None
        }
    }
}

/// Validate that the resolved main function has no params and returns Unit.
pub fn validate_main_signature(
    typed_module: &TypedModule,
    main_fqn: &Fqn,
    diagnostics: &mut Diagnostics,
) {
    let main_name = MangledName::for_function_no_params(main_fqn);
    let Some(main_func) = typed_module.functions.get(&main_name) else {
        return;
    };

    if !main_func.params.is_empty() {
        diagnostics.error(
            main_func.span.clone(),
            "'main' function must have no parameters".to_string(),
        );
    }

    if main_func.return_type != Type::Unit && main_func.return_type != Type::Error {
        diagnostics.error(
            main_func.span.clone(),
            format!(
                "'main' function must return Unit, found '{}'",
                main_func.return_type
            ),
        );
    }
}
