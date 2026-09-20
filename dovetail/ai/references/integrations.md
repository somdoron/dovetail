# Derives, components, and prefixed literals

## Derives

Stack one `@derive(Name)` per attribute above a record, enum, or newtype.
Equatable is available from the prelude; JSON derives require standard-json.
Generated implementations are type-checked and respect private construction and
inspection. A decoder must not bypass a validated constructor; decode a public
snapshot then validate. Deriving Equatable on a private newtype cannot read its
hidden wrapped value; delegate through its module instead.

Custom derives are Rhai scripts returning one Dovetail `implement` block. Register:

```toml
[[project.macro]]
name = "Tag"
package = "myapp"
kind = "derive"
trait = "myapp.Tag"
script = "macros/Tag.rhai"
```

The constant `input` contains `kind` (record/enum/newtype), `name`, `type_params`,
record `fields` (name/ty), enum `variants` (name, kind, types/fields), or newtype
`inner`. Generate generic parameters and required bounds explicitly. Test supported
shapes and reject unsupported ones clearly. No expression/token/syntax macros exist.

## Components

```toml
[[project.component]]
path = "artifacts/library.wasm"
package = "library.raw"
```

Paths are relative to the owning project and must reference an actual component.
Select an exported interface with `interface` if needed. Dependencies bring declared
components transitively. Generated WIT bindings live under `.dovetail/generated/`;
do not edit them. Records/variants/options/results project to corresponding types,
lists to arrays, flags to Uint32 newtypes, and resources to explicit handles/drop.
The boundary copies values through the canonical ABI; it is not shared mutable
memory. Wrap low-level handles in Resource APIs and translate raw errors.

Prefer standard-sqlite's wrapper over raw generated calls. `use Connection.open(...)`
scopes a connection; await execute/query operations. Parameterize queries. Transactions
commit on success and roll back on failure, but cannot roll back external effects.
Pin the owning library revision carrying its component binary.

## Prefixed literals

Import `standard.sqlite.sql` for `sql"SELECT * FROM users WHERE id = $id"`.
The library binds interpolations as parameters. Ordinary interpolated strings do
not gain that protection. `$ident`, `${expression}`, and `$..sequence` are supported;
spread is exclusive to prefixed literals. The prefix must touch the quote. Triple
quotes preserve indentation and drop one leading newline. Escape backslashes normally.

The compiler lowers through public builder operations:
`empty()`, `.literal(text)`, `.value(value)`, `.spread(values)`, `.build()`.
The builder's bounds decide accepted interpolation types. Declare a prefix:

```dovetail
@stringLiteral
public type sql = SqlBuilder
```

Use a non-generic builder; make value/spread generic as needed. Mutable accumulators
avoid repeatedly copying the whole prefix. Imports and import aliases select/rename
the prefix; no global prefix registry exists. `test` cannot be a prefix. Compose
optional query fragments with ordinary expressions/library operations; there is no
conditional interpolation syntax. A prefix's escaping/binding safety comes from
its actual builder, not from the presence of a prefix alone.
