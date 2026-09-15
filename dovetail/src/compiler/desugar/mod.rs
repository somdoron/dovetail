mod awaitable;
pub mod desugar_await;
pub mod desugar_for;
pub mod desugar_try;
pub mod desugar_use;

use crate::typechecker::types::TypedModule;

/// Run syntactic desugar passes on a typed module.
/// Call after typecheck() and merge, before monomorphize.
/// Order: desugar_for → desugar_try → desugar_use → desugar_await.
///
/// `desugar_use` runs before `desugar_await` because a `use` expression may
/// contain `await` inside its continuation closure body; we want
/// `desugar_await` to find and process those awaits after `desugar_use` has
/// emitted the closure.
///
/// `coerce_byname` and `capture` run separately after monomorphize,
/// once all concrete functions have been instantiated.
pub fn desugar_all(module: &mut TypedModule) {
    desugar_for::desugar_for_expressions(module);
    desugar_try::desugar_try_expressions(module);
    desugar_use::desugar_use_expressions(module);
    desugar_await::desugar_await_expressions(module);
}

/// Run ByName coercion and capture analysis.
/// Call once after monomorphize, when all concrete functions are available.
/// Order: coerce_byname → capture (capture depends on coerced closures).
pub fn coerce_and_capture(module: &mut TypedModule) {
    super::coerce_byname::coerce_byname_args(module);
    super::capture::analyze_captures(module);
}
