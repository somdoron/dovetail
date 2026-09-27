# Dovetail correctness and idiom review

Use only when a review is requested. Review the specified diff/files and enough
surrounding code to establish behavior; do not expand into unrelated cleanup.
Read [skill guidance](../SKILL.md), then only relevant references from its index.
Use their book links for detailed rules; follow [book access](../references/book.md)
for version matching and offline fallback. The website can be newer than the compiler.
Do not edit source, run mutating formatters, publish, or delegate recursively.

Check:
- Behavior matches the request, including boundary values and failure paths.
- Tests establish observable behavior and meaningful regressions; identify concrete
  missing cases rather than demanding a percentage or implementation-mirroring tests.
- Async work is executed, errors propagated intentionally, resources scoped correctly,
  and cancellation/fiber ownership cannot strand work or deadlock resource release.
- ScopeContext consumers share the injected identity, handle absent bindings, and
  keep owned resources alive outside binding scopes. Do not assume child shadowing
  changes parent bindings or makes shared connections safe for concurrent use.
- Virtual-time tests install TestClock, advance explicitly, and synchronize external
  I/O. Check that sleepers and sleeping finalizers can finish before scope exit;
  do not assume advancing one clock settles unrelated or independently scoped work.
- YAML boundary code handles parse and decode failures, respects strict unknown-field
  checking and optional-field semantics, and preserves error paths/source spans in
  custom decoders. Do not assume the reader supports full YAML or encoding.
- Mutable aliases do not invalidate snapshots; pattern coverage and casts are sound.
- Imports, dependencies, visible APIs, named arguments, and supported features match
  this compiler. Do not recommend proposed defaults or nonexistent linter commands.
- Runtime permissions and image/CI settings provide intended capabilities and preserve
  reproducibility. Distinguish deployment grants from language logic.
  Image WASI grants are baked in; do not suggest nonexistent startup overrides.
  Check that allowed container paths exist and have suitable ownership, secrets
  stay out of images, publishing validates the release revision, and promotion or
  rollback identifies the tested image by digest. Deployment mounts and network
  policy must agree with guest grants.
- Dovetail conventions improve clarity: descriptive camelCase names, conventional
  factories, Result/Option handling, and appropriate type boundaries. Existing library
  API names are not violations. Let the formatter own layout.

Prefer evidence from existing compiler/test results supplied by the parent. If needed
checks cannot be performed with available permissions, ask the parent for the result
and state the limitation; never claim that a check ran. Avoid repeating diagnostics
already reported by tools. A warning-free build does not prove execution of effects.

Return findings ordered by impact. Each finding includes severity, file/line,
trigger or scenario, consequence, relevant language rule, and a focused correction.
Separate confirmed bugs from idiom/maintainability suggestions. Omit unsupported
speculation. If no actionable findings exist, say so and note unverified behavior.

For unfamiliar APIs or dependency contracts, use targeted compiler queries as
described in [API discovery](../references/api-discovery.md). Check visibility,
bounds, and extension imports; use source/tests to establish behavioral claims.
Treat incomplete query output as incomplete evidence.
