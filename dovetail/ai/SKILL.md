---
name: dovetail
description: Write, test, configure, and review Dovetail language projects (.dove sources and Dovetail.toml). Use for Dovetail language, standard-library, runtime, image, and CI tasks; not unrelated uses of the word domain.
metadata:
  dovetail-version: "{{DOVETAIL_VERSION}}"
---

# Dovetail

This guidance is bundled with Dovetail **{{DOVETAIL_VERSION}}**.
Dovetail was formerly called Domain. Use `.dove`, `Dovetail.toml`, and `dovetail`.
Dovetail is in preview; compiler defects and behavior changes are possible.

## Check compatibility once per session

Run `dovetail --version` when first using this skill. If the task explicitly
uses a compiler checkout, run that checkout's `cargo run -- --version` instead
(use `--manifest-path` when working from a separate consumer workspace).
Compare the reported compiler version with the version above.

If they differ, tell the user to refresh this guidance by running
`dovetail ai install` with the intended compiler. That command refreshes remembered
installation targets. Do not reinstall automatically or update the compiler.
Continue unrelated work; verify version-sensitive syntax before relying on it.
If the compiler is unavailable, say compatibility could not be checked.

## Compiler errors during preview

For a suspected compiler defect (a crash, incorrect diagnostic, or wrong code
generation), first search the [GitHub issues](https://github.com/somdoron/dovetail/issues)
for the diagnostic and affected feature, including both open and closed issues.
Distinguish ordinary source errors from compiler bugs; fix valid diagnostics in
the application normally.

Check [releases](https://github.com/somdoron/dovetail/releases) for a newer compiler
version and whether it fixes the problem. Recommend a relevant update; do not
upgrade automatically. After an upgrade, remind the user to refresh the skill with
`dovetail ai install`.

If the defect remains unreported, prepare a minimal reproduction, compiler version,
command, and expected versus actual behavior, then ask the user for permission
before filing a GitHub issue. Show the proposed report first and exclude secrets
or private project content. If an existing issue matches, share its link and status
instead of opening a duplicate. If GitHub is unavailable, say those checks remain
unverified. Investigate defects rather than hiding them in application workarounds.

## Working method

1. Read the project's own instructions and `Dovetail.toml`. Identify the selected
   project, dependencies, existing conventions, and the user's requested scope.
2. Read only the task references below that matter. They cover workflow and common
   mistakes; the [book](references/book.md) owns detailed syntax and examples.
   Fetch relevant chapters on demand, with version and offline checks from that guide.
3. Inspect nearby source and query actual dependency declarations before inventing
   API calls (`dovetail query search`, `package`, and `definition`). In particular, import named extensions explicitly, even in their package.
4. For a new domain or substantial feature, suggest a small `types.dove` draft and
   discussion before behavior; follow [domain modeling](references/domain.md).
   Respect the requested scope: a modeling session can end with a compiling draft
   and open questions. Implement behavior once the relevant rules are understood,
   using valid types and explicit failure handling.
5. Format changed sources, check, and run relevant tests. Report what ran and what
   remains unverified. Use the compiler matching the project.
6. Run review profiles only if the user requests a review. Ordinary edits do not
   authorize automatic review delegation or a review/fix loop.

Use `dovetail` for consumer projects. During compiler development use `cargo run --`
from the compiler checkout, or `cargo run --manifest-path /path/to/Cargo.toml --`
from the consumer workspace. Dovetail resolves its manifest from the working directory.

## Essential rules

- Blocks are expressions; their final expression is the result. Use spaces,
  `function`, `let mutable`, `match ... with`, and `case ... =>`.
- Use camelCase values and source filenames; a type module may use `Order.dove`.
  Prefer descriptive names and `make` for new factory APIs. Call existing APIs by
  their actual names, including older factories named `new`.
- Use records/enums/newtypes to encode data and states; classes can own mutable state.
  Immutable record fields do not make referenced arrays or classes immutable.
- `Option` represents absence; `Result` uses `Ok` and `Error`. Expected failures
  belong in typed results, not `panic`, `assert`, or `.require`.
- `Async` and `Resource` describe deferred work. Constructing or discarding them
  does not execute work or acquire resources. Use `await`, `use`, and an entry runner.
- Ordinary intermediate values need no discard binding. Explicit `let _ = ...`
  acknowledges intentional discard of `Result`, `Async`, or `Resource`; it does
  not execute the value.
- A plain trait is a generic bound, not a runtime value type. Use an interface for
  runtime-selected implementations. Dependencies between packages must be acyclic.
- Formatter, compiler warnings, and proposed lint rules are different capabilities.
  There is no `dovetail lint` command yet. Default parameter values and a durable
  execution runtime are also not implemented.
- Do not translate familiar syntax or library APIs from another language by guesswork.
  Follow the preview compiler-error workflow above for suspected compiler defects.

## Load by task

| Task | Reference |
|---|---|
| Find detailed language documentation, match versions, or work offline | [Book access](references/book.md) |
| Find APIs, inspect signatures, explore dependencies or the prelude | [API discovery](references/api-discovery.md) |
| Expressions, literals, control flow, functions, named arguments, closures | [Syntax](references/syntax.md) |
| Records, enums, private construction, newtypes, modules, arrays/lists/slices, casts | [Types and collections](references/types.md) |
| Traits, interfaces, dispatch, classes, inheritance and identity | [Contracts and classes](references/contracts.md) |
| Type parameters, bounds, variance, associated types, tuple extension | [Generics](references/generics.md) |
| Option/Result, early return, async execution, fibers and cancellation | [Errors and async](references/effects.md) |
| Acquisition, cleanup, scopes, streams and byte transports | [Resources and streams](references/resources.md) |
| Package layout, imports, dependencies, lockfiles and library discovery | [Projects and libraries](references/projects.md) |
| Testing, formatter, diagnostics and planned linter | [Development tools](references/tooling.md) |
| Custom derives, components and prefixed string literals | [Language integrations](references/integrations.md) |
| Filesystem roots/paths, networking, environment and guest arguments | [WASI](references/wasi.md) |
| OCI configuration, build/push, runtimes and reproducibility | [Images](references/images.md) |
| GitHub Actions installation, checks and publishing | [CI](references/ci.md) |
| Collaborative types-first modeling, value objects, entities, aggregates and domain tests | [Domain modeling](references/domain.md) |
| Layers, transactions, ports, contracts and bounded contexts | [Application architecture](references/architecture.md) |

## Requested reviews

For an unqualified Dovetail review use the general profile. Include the DDD profile
when the reviewed changes affect domain modeling or architecture. Respect explicit
requests for only one profile.

- [General review](reviews/dovetail-reviewer.md): correctness, test gaps, and idioms.
- [DDD review](reviews/dovetail-ddd-reviewer.md): invariants and architectural boundaries.

If subagents are supported and permitted, delegate the requested scope and relevant
profile to an independent reviewer. Claude installations provide native agents named
`dovetail-reviewer` and `dovetail-ddd-reviewer`. Generic installations provide portable
instructions, not a claim of native agent registration. Without delegation, apply
profiles sequentially yourself and disclose that limitation. Do not recursively
spawn reviewers. Reviewers report findings and do not edit files.

The parent supplies the diff or files, requirements, and validation results. Reviewers
load only relevant language/design references and inspect enough surrounding code
to establish consequences. Do not assume they inherit the parent's context.
