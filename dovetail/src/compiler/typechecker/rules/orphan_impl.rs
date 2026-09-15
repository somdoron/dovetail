use crate::common::diagnostics::Diagnostics;
use crate::common::types::PackagePath;
use crate::typechecker::registry::Registry;

/// Enforce the orphan rule: an `implement` block is allowed only if the current
/// package owns the trait or owns the type.
pub(super) fn check_orphan_rule(
    package_registry: &Registry,
    package_path: &PackagePath,
    diagnostics: &mut Diagnostics,
) {
    for block in package_registry.all_implement_blocks() {
        let trait_is_local = block.trait_fqn.package == *package_path;
        let type_is_local = block.type_fqn.package == *package_path;

        if !trait_is_local && !type_is_local {
            diagnostics.error(
                block.span.clone(),
                format!(
                    "cannot implement foreign trait '{}' for foreign type '{}' — \
                     the implementation must be in the package that defines the trait or the type",
                    block.trait_fqn.symbol.0, block.type_fqn.symbol.0
                ),
            );
        }
    }
}
