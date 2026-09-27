# Integrations: boundary checks

Read [Macros](https://dovetaillang.org/book/macros.md), [Components](https://dovetaillang.org/book/components.md), [Prefixed literals](https://dovetaillang.org/book/prefixed-literals.md) and [Standard library](https://dovetaillang.org/book/stdlib.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

- Derives obey private construction and inspection. Decode public snapshots then
  validate them; do not grant a serializer privileges to bypass domain invariants.
- Custom derives produce a single implementation. Test supported shapes, preserve
  generic bounds, and reject unsupported input. There are no expression macros.
- Do not edit generated component bindings under `.dovetail/generated/`. Pin the
  library revision carrying the binary, wrap raw handles in Resource ownership,
  and translate errors. Canonical ABI values are copied, not shared mutable memory.
- Prefer the standard SQLite wrapper. Scope connections and await operations;
  a database rollback cannot undo external effects.
- A prefixed string is only as safe as its builder. Parameterized `sql` interpolation
  is different from ordinary string interpolation; inspect the actual API.
- Prefix builders determine allowed interpolation types. Use normal language
  composition for optional fragments, not invented conditional interpolation syntax.

- For YAML input, use project `standard-yaml` and `Yaml.parse` followed by
  `Type.fromYaml`; import `Yaml`, `YamlError`, and `YamlDecoder` for derives.
  Keep file IO separate and propagate both parse and decode errors. Derived
  records reject unknown fields; missing/null optional fields become `None`.
  Use the documented subset, not assumed support for aliases, tags, multiple
  documents, or encoding. Error paths and spans should survive custom decoding.

- JSON integer decoders reject fractions and overflow in stored Float64 values
  with typed errors. The current JSON tree still loses numeric text precision;
  do not promise exact Int64 wire round trips or use post-float checks to validate
  original numeric tokens. Derived integer fields inherit this limitation.
- YAML integer nodes preserve Int64 exactly and Int32 narrowing is checked.
  Decimal/exponent float nodes (including `1.0` and `1e3`) cannot decode as integers.
  YAML has no encoder; do not suggest a serialization round-trip API.
