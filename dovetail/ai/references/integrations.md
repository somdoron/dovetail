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

- JSON uses `Json.Number(JsonNumber)`: construct integers with `fromInt32`/`fromInt64`
  or primitive `toJson()`, and use checked `toInt32`/`toInt64` conversions. Integer
  tokens outside Int64 fail at parse time; decimal/exponent forms are float nodes
  and cannot decode as integers, even `1.0`. Ordinary derived Int64 fields round
  trip exactly through text. Converting to Float64 may round.
- Migrate old `Json.Number(float)` construction to checked `JsonNumber.fromFloat64`.
  It rejects non-finite values; the infallible `Float64.toJson()` maps them to null,
  including derived fields. Do not promise arbitrary-precision JSON or preservation
  of numeric spelling; floats retain their category when serialized.
- YAML integer nodes preserve Int64 exactly and Int32 narrowing is checked.
  Decimal/exponent float nodes (including `1.0` and `1e3`) cannot decode as integers.
  YAML has no encoder; do not suggest a serialization round-trip API.
