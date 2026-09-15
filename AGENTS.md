# Repository guidance

The language is Dovetail, formerly called Domain. The user may still say “Domain” or “the Domain language” out of habit; when referring to this language, compiler, or tooling, interpret that as Dovetail. Use Dovetail in new prose and code references, with `.dove` source files, `Dovetail.toml`, and the `dovetail` executable. Ordinary uses of “domain,” including domain-driven design and network domains, retain their meaning.

Read and follow [CLAUDE.md](CLAUDE.md) for shared build commands, architecture, language style, testing, compiler-bug handling, and staging/commit policy. Keep that file as the shared detailed guidance rather than duplicating those policies here.

Use `cargo run --` for local compiler validation; an installed `dovetail` binary may be stale. Preserve existing working-tree changes. Do not stage or commit unless explicitly requested.
