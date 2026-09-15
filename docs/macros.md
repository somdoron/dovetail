# Dovetail Macros

> **Status:** Design document. Not yet implemented. Tracked as item #36 on
> [Backlog.md](Backlog.md). This doc describes the intended design once the
> macro phase is added to the compiler.

Dovetail macros are **compile-time AST transformers** written in
[Rhai](https://rhai.rs/). A macro receives un-typed AST as input and returns
un-typed AST as output. The compiler then runs Collect / Inference / Rules
over the expanded AST as normal.

## Why Rhai?

- **Stable and mature.** Rhai 1.x has been the stable line since 2021; the
  embedding API and language semantics rarely break. The macro layer needs to
  be boring infrastructure that outlives whichever Rust version you're on.
- **Best Rust-host ergonomics in the ecosystem.** The `#[export_module]` proc
  macro turns a Rust module into a typed Rhai API in one block — critical for
  exposing a large AST-builder surface (~40+ constructors and pattern
  helpers).
- **Best-in-class sandboxing.** Per-script budgets for operations, call depth,
  expression depth, string/array/map sizes, allocations, and progress
  callbacks for cooperative cancellation. No I/O is exposed unless the host
  registers it explicitly.
- **Position on everything.** Every AST node and runtime error carries a
  `Position` (line, column) that maps cleanly onto Dovetail's `Span` for
  diagnostics.
- **Pure Rust.** No system dependencies, no JIT, no FFI surface.
- **No bootstrap problem.** Macros work before Dovetail is self-hosting.

**Trade-off accepted: dynamic typing.** A macro that mis-constructs an AST
node will fail at expansion time rather than at script-load. We mitigate this
by keeping the builder API small and consistent, generating clear runtime
errors with call-site spans, and shipping a `dovetail expand` subcommand so
authors can dry-run their macros against test inputs.

Statically-typed alternatives (Gluon, Mun) were considered and ruled out:
Gluon has not had a release since 2020, and Mun is designed for game scripting
with hot-reload, not as a compiler-embedded layer. Rune was a close runner-up
but is still pre-1.0 with occasional breaking changes.

## Pipeline Placement

```
┌──────────────────────────────────────────────────────────────────┐
│                      Dovetail Compiler                              │
│                                                                   │
│  1. Parse                                                         │
│  2. ═══════════════ MACRO PHASE ═══════════════                   │
│     • Find @derive annotations and #macro calls in the AST        │
│     • For each macro invocation:                                  │
│       - Load the Rhai script                                      │
│       - Pass: call-site AST node + package AST view               │
│       - Receive: AST nodes to splice in                           │
│     • Re-parse generated source spans if needed                   │
│  3. Collect    (runs once, on the post-expansion AST)             │
│  4. Inference                                                     │
│  5. Rules                                                         │
│  6. Codegen                                                       │
└──────────────────────────────────────────────────────────────────┘
```

**Macros never see the registry.** They run before Collect, so there is no
`TypeRegistry`, no `MangledName`, no type information. Macros operate purely
on syntax. This is the key simplification compared to earlier design drafts.

**What macros *can* see:**

- The AST node they're attached to (for `@derive`) or the AST of their
  arguments (for `#macro`).
- A read-only view of the **current package's** parsed AST — used to look up
  sibling declarations by name (e.g. a `#client<UserService>` macro can find
  the trait declaration named `UserService` in the same package).

**What macros *cannot* see:**

- Inferred types, trait implementations, method resolution.
- Mangled names or anything from later pipeline phases.
- Other packages' source — only the current package is in scope.

**Philosophy:** Generate the code and let the typechecker verify it. If
`@derive(JsonCodec) record User` emits `self.name.encode()` and `name`'s type
doesn't implement `JsonCodec`, the typechecker produces a normal error. The
macro does not need to validate this itself.

## Macro Types

| Type | Syntax | Use Case |
|------|--------|----------|
| **Derive** | `@derive(MacroName)` on a declaration | Generate trait implementations |
| **Function** | `#macroName<T>(args)` at an expression site | Generate arbitrary code |

---

## Derive Macros

Derive macros attach to a record, enum, or class declaration and generate a
sibling `implement` block.

### Usage

```dovetail
package myapp

import json.JsonCodec

@derive(JsonCodec)
public record User =
    name: String
    age: Int32
```

The macro generates:

```dovetail
implement JsonCodec for User =
    function encode(self): JsonValue = ...
    function decode(value: JsonValue): Result<User, JsonError> = ...
```

### Definition in Dovetail.toml

```toml
[[project]]
name = "json"
type = "library"
root-package = "json"

[[project.macro]]
name = "JsonCodec"              # FQN: json.JsonCodec
package = "json"                # Must match root-package prefix
kind = "derive"
trait = "json.JsonCodec"        # Trait being derived (for diagnostics)
script = "macros/json_codec.rhai"
```

### Rhai Implementation

```rhai
// macros/json_codec.rhai

fn derive(input) {
    let target = input.target;

    if target.kind != "record" {
        return macro::error(
            "@derive(JsonCodec) can only be applied to records",
            input.span
        );
    }

    // Build the body of encode.
    let encode_stmts = [];
    for field in target.fields {
        encode_stmts.push(
            ast::method_call(
                ast::ident("writer"),
                "field",
                [
                    ast::string(field.name),
                    ast::method_call(
                        ast::field(ast::ident("self"), field.name),
                        "encode",
                        []
                    )
                ]
            )
        );
    }
    encode_stmts.push(ast::method_call(ast::ident("writer"), "build", []));

    // Build the body of decode.
    let decode_stmts = [
        ast::let("obj", ast::try(
            ast::method_call(ast::ident("value"), "asObject", [])
        ))
    ];
    let field_names = [];
    for field in target.fields {
        field_names.push(field.name);
        decode_stmts.push(
            ast::let(
                field.name,
                ast::try(ast::method_call(
                    ast::ident("obj"),
                    "get",
                    [ast::string(field.name)]
                ))
            )
        );
    }
    decode_stmts.push(
        ast::call_ident("Ok", [ast::record_ctor(target.name, field_names)])
    );

    macro::emit(ast::implement("JsonCodec", target.name, [
        ast::function(
            "encode",
            [#{ name: "self", ty: ast::type_self() }],
            ast::type_ident("JsonValue"),
            ast::block(encode_stmts)
        ),
        ast::function(
            "decode",
            [#{ name: "value", ty: ast::type_ident("JsonValue") }],
            ast::type_apply("Result", [
                ast::type_ident(target.name),
                ast::type_ident("JsonError")
            ]),
            ast::block(decode_stmts)
        )
    ]))
}
```

The compiler loads the script, calls its top-level `derive(input)` function
with the call-site input, and splices the returned AST into the package.

---

## Function Macros

Function macros generate code at expression sites. They can accept type
arguments (resolved syntactically — note these are *names*, not type IDs)
and value arguments (un-typed AST nodes).

### Syntax

```
#macroName<TypeArg1, TypeArg2>(arg1, arg2)
          ^^^^^^^^^^^^^^^^^^^^ ^^^^^^^^^^^^
          Type names (Ast)     Value args (Ast)
```

Type arguments are written with `<>` to match Dovetail's generic syntax. They
are passed to the macro as type-reference AST nodes — the macro is
responsible for looking them up in the package AST view if it needs the
underlying declaration.

### Usage

```dovetail
package myapp

import grpc.client

public trait UserService =
    function getUser(id: UserId): Result<User, RpcError>
    function createUser(req: CreateRequest): Result<User, RpcError>

function main(): Unit =
    let userClient = #client<UserService>("https://api.example.com")
    let user = userClient.getUser(UserId(123)).require
    print(user.name)
```

### Definition in Dovetail.toml

```toml
[[project.macro]]
name = "client"
package = "grpc"
kind = "function"
script = "macros/client.rhai"
type-params = 1                 # Arity hint for the parser
value-params = 1
```

### Rhai Implementation

The function macro looks up `UserService` in the package AST view, walks its
methods, and emits a class definition plus an instantiation expression.

```rhai
// macros/client.rhai

fn expand(input) {
    if input.type_args.len() != 1 {
        return macro::error("expected one type argument", input.span);
    }
    let trait_name = input.type_args[0].name;

    let decl = macro::find_decl(input.package, trait_name);
    if decl == () {
        return macro::error(`${trait_name} not found in package`, input.span);
    }
    if decl.kind != "trait" {
        return macro::error(`${trait_name} is not a trait`, input.span);
    }

    // Build proxy methods.
    let methods = [];
    for m in decl.methods {
        let arg_refs = [];
        for p in m.params {
            arg_refs.push(ast::ident(p.name));
        }
        methods.push(
            ast::function(m.name, m.params, m.return_type,
                ast::method_call(
                    ast::field(ast::ident("self"), "channel"),
                    "call",
                    [
                        ast::string(trait_name),
                        ast::string(m.name),
                        ast::array(arg_refs)
                    ]
                ))
        );
    }

    let class_name = trait_name + "Client";
    let endpoint = input.value_args[0];

    // Emit the class declaration *and* the instantiation expression that
    // replaces the macro call site.
    macro::emit_many([
        ast::class(class_name, [trait_name],
            [ast::field_def("channel", ast::type_ident("GrpcChannel"))],
            methods),
        ast::method_call(ast::ident(class_name), "new", [endpoint])
    ])
}
```

---

## Macro Imports

Macros are imported by FQN, like types. A derive macro and the trait it
derives typically share a name, so a single `import` brings in both.

```dovetail
import json.JsonCodec    // Imports BOTH the trait AND the macro

@derive(JsonCodec)        // Resolves to the macro
public record User =
    name: String

implement JsonCodec for Custom =  // Resolves to the trait
    function encode(self): JsonValue = ...
```

Disambiguation is by syntactic position:

- `@derive(Name)` → look for a derive macro
- `#name<...>(...)` → look for a function macro
- `Name` in a type position or after `implement` → look for a type/trait

Aliases work as with types:

```dovetail
import json.JsonCodec as Json

@derive(Json)
public record User =
    name: String
```

## Macro Transitivity

Macros are transitive through the dependency graph. A package may use any
macro defined in any of its (transitive) dependencies, subject to the normal
import rules.

```
json   (defines json.JsonCodec macro)
  ↑
utils  (can use json.JsonCodec)
  ↑
app    (can also use json.JsonCodec)
```

---

## Rhai API Reference

The compiler exposes two namespaces to macro scripts: `ast` (AST builders and
pattern destructors) and `macro` (input shapes, error reporting, and package
introspection).

### `ast` — AST Builders

**Literals**

```rhai
ast::string(s)          // "hello"
ast::int(n)             // 42
ast::float(x)           // 3.14
ast::bool(b)            // true / false
ast::unit()             // ()
```

**Names and access**

```rhai
ast::ident(name)             // foo
ast::field(expr, name)       // expr.name
ast::index(arr, idx)         // arr[idx]
```

**Calls**

```rhai
ast::call(callee, args)              // callee(args...)
ast::call_ident(name, args)          // name(args...)
ast::method_call(recv, name, args)   // recv.name(args...)
```

**Operators and control flow**

```rhai
ast::binop(left, op, right)          // left op right
ast::unop(op, expr)                  // op expr
ast::if_(cond, then_expr, else_expr) // if cond then ... else ...
ast::match_(subject, arms)           // match subject with case ...
ast::block(stmts)                    // block expression
ast::try(expr)                       // ? / orReturn operator
```

**Bindings**

```rhai
ast::let(name, value)                // let name = value
ast::let_mut(name, value)            // let mutable name = value
ast::assign(target, value)           // target = value
```

**Types** (note Dovetail uses `<>` for generics, not `[]`)

```rhai
ast::type_ident(name)                // String
ast::type_apply(name, type_args)     // Result<T, E>
ast::type_self()                     // Self
```

**Constructors**

```rhai
ast::array(elements)                 // [a, b, c]
ast::record_ctor(name, field_names)  // User { name, age }
```

**Declarations**

```rhai
ast::function(name, params, return_type, body)
ast::property(name, return_type, body)
ast::field_def(name, ty)
ast::implement(trait_name, for_type, decls)
ast::implement_generic(trait_name, for_type, bounds, decls)
                                     // implement<T: Bound> Trait for Type<T>
                                     // bounds: [["T", ["JsonCodec"]], ...]
ast::class(name, implements_list, field_defs, decls)
```

**Parameters** are passed as object maps: `#{ name: "x", ty: ast::type_ident("Int32") }`.

### `macro` — Driver Interface

**Input shapes** (read-only, passed by the compiler)

For derive macros, `input` is a map with:

```
input.target   — the Decl the @derive is attached to
input.span     — call-site Span
input.package  — opaque PackageView handle
```

For function macros, `input` is a map with:

```
input.type_args   — array of TypeRef nodes (names, not resolved types)
input.value_args  — array of Expr nodes (un-typed)
input.span        — call-site Span
input.package     — opaque PackageView handle
```

**Inspecting AST nodes:** every node carries a `.kind` discriminator
(`"record"`, `"enum"`, `"class"`, `"trait"`, `"function"`, `"ident"`,
`"call"`, `"type_ref"`, …) plus kind-specific properties:

```
record.kind     == "record"
record.name     // String
record.fields   // array of #{ name, ty, span }
record.type_params // array of #{ name, bounds, span }

enum.kind       == "enum"
enum.name
enum.variants   // array of #{ name, fields, span }
enum.type_params

trait.kind      == "trait"
trait.name
trait.methods   // array of #{ name, params, return_type, span }
```

**Output**

```rhai
macro::emit(decl)             // emit one declaration
macro::emit_many(items)       // emit multiple decls/exprs
macro::emit_expr(expr)        // function macros: replace the call site with an expr
```

**Diagnostics**

```rhai
macro::error(message, span)   // emit error, abort this macro (returns Output)
macro::warning(message, span) // emit warning, continue
macro::debug(value)           // print to compiler stderr (for development)
```

**Package introspection**

```rhai
macro::find_decl(package, name)  // -> Decl or ()
macro::all_decls(package)        // -> array of Decl
```

The `package` handle only exposes the **current package's** parsed AST.
Dependency packages have already been fully compiled and are not visible
through this view.

---

## Handling Generics

When a generic declaration is decorated with `@derive`, the type parameters
are visible on the AST node. Macros generate bounded `implement` blocks
syntactically — the typechecker validates them later.

```dovetail
@derive(JsonCodec)
public record Box<T> =
    value: T
```

```rhai
// In the derive macro, inspect target.type_params and emit bounds.
let bounds = [];
for p in target.type_params {
    bounds.push([p.name, ["JsonCodec"]]);
}

macro::emit(ast::implement_generic("JsonCodec", target.name, bounds, [
    // ... methods
]))
```

Generated:

```dovetail
implement<T: JsonCodec> JsonCodec for Box<T> =
    function encode(self): JsonValue =
        JsonObject.new
            .field("value", self.value.encode)
            .build

    function decode(value: JsonValue): Result<Box<T>, JsonError> =
        let obj = value.asObject.require
        let v = T.decode(obj.get("value").require).require
        Ok(Box { value = v })
```

---

## Package Structure

```
my-lib/
├── Dovetail.toml              # Project config with [[project.macro]] entries
├── src/
│   └── lib.dove           # Library code (traits, types)
└── macros/
    ├── json_codec.rhai      # Rhai macro implementations
    └── client.rhai
```

---

## Debugging Macros

### Viewing Generated Code

```bash
# Show all macro expansions in a package
cargo run -- expand -p myapp

# Expand a single file
cargo run -- expand -p myapp src/models.dove

# Write expanded source to disk
cargo run -- expand -p myapp --output expanded/
```

### Debug Output

```rhai
fn derive(input) {
    macro::debug(`Processing: ${input.target.name}`);
    macro::debug(input.target);
    // ... rest of macro
}
```

`macro::debug` writes to the compiler's stderr stream and does not affect
expansion output.

### Error Messages

```
error[E0501]: macro expansion failed
  --> src/models.dove:5:1
   |
 5 | @derive(JsonCodec)
   | ^^^^^^^^^^^^^^^^^^ json.JsonCodec
   |
   = @derive(JsonCodec) can only be applied to records
```

Macro errors are reported with the **call-site span**, not the generated-code
span — users see the line they wrote, not the synthetic output.

---

## Sandboxing and Resource Limits

The macro driver registers Rhai engines with conservative defaults to prevent
malicious or accidentally runaway macros from hanging a build:

| Limit | Default | Rhai API |
|---|---|---|
| Max operations | 1,000,000 | `engine.set_max_operations` |
| Max call depth | 64 | `engine.set_max_call_levels` |
| Max expression depth | 64 | `engine.set_max_expr_depths` |
| Max string size | 16 KiB | `engine.set_max_string_size` |
| Max array size | 65,536 | `engine.set_max_array_size` |
| Max map size | 65,536 | `engine.set_max_map_size` |

Macros have no access to the filesystem, network, environment variables, or
process control — the host registers only the `ast::*` and `macro::*`
namespaces. Limits are configurable per-workspace in `Dovetail.toml` if a macro
legitimately needs more headroom.

---

## Complete Example

### Library: `json`

**`json/Dovetail.toml`**

```toml
[workspace]
name = "json-lib"

[[project]]
name = "json"
type = "library"
root-package = "json"

[[project.macro]]
name = "JsonCodec"
package = "json"
kind = "derive"
trait = "json.JsonCodec"
script = "macros/json_codec.rhai"
```

**`json/src/lib.dove`**

```dovetail
package json

public record JsonValue =
    // ...

public enum JsonError =
    ParseError(message: String)
    TypeError(expected: String, got: String)
    MissingField(name: String)

public trait JsonCodec =
    function encode(self): JsonValue
    function decode(value: JsonValue): Result<Self, JsonError>
```

### Application

**`myapp/Dovetail.toml`**

```toml
[[project]]
name = "app"
type = "application"
root-package = "myapp"
depends = ["json"]
```

**`myapp/src/main.dove`**

```dovetail
package myapp

import json.JsonCodec

@derive(JsonCodec)
public record User =
    name: String
    age: Int32

@derive(JsonCodec)
public record Post =
    title: String
    author: User

function main(): Unit =
    let user = User { name = "Alice", age = 30 }
    let json = user.encode
    print(json.toString)

    let decoded = User.decode(json).require
    print(decoded.name)
```

---

## Open Questions

These are intentionally unresolved and should be settled during
implementation:

1. **Cross-package introspection for function macros.** Currently a function
   macro can only `find_decl` within its own package. If real-world use shows
   demand for introspecting dependency-package declarations (e.g.
   `#client<some.lib.Trait>`), the dependency's already-collected registry
   could be exposed in a second hybrid view. Defer until needed.
2. **Quote/unquote syntax sugar.** The builder API is verbose. A future
   `quote { ... }` form could desugar to the same `ast::*` calls in a
   pre-processing pass. Not in scope for the initial cut.
3. **Recursive macro expansion.** Should generated code be re-scanned for
   further macro invocations? Initial answer: **no**, one pass only. Revisit
   if needed.
4. **Hygiene.** Generated identifiers currently share the call-site scope.
   We may need gensym-style fresh names for locals like `obj` in the
   `JsonCodec` decode body to avoid shadowing user variables.
5. **Caching.** Macro outputs should be cached as part of the package-level
   typechecker cache. The cache key needs to include the macro script's
   contents so that editing a `.rhai` file invalidates dependent packages.
