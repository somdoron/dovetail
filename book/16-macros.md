# Part 16: Macros

Dovetail has one form of metaprogramming: **derive macros**. A derive macro looks at a record, enum, or newtype declaration and generates an `implement` block for it — typically a trait implementation that would otherwise be tedious or error-prone to write by hand. Equality, hashing, JSON encoding and decoding, and similar structural operations are good fits.

This chapter covers the `@derive` keyword on the user side, the built-in derives that ship with the compiler and the standard library, and how to write your own derive macro in Rhai.

> **See also:** [Part 24: Prefixed String Literals](24-prefixed-literals.md) is the same bargain applied to syntax rather than declarations — the compiler lexes `sql"..."` and defines what `$` may do, and a library decides what the literal means by marking a name `@stringLiteral`.

---

## 16.1 Overview

The macros described here run at **compile time**. They generate Dovetail source code, the compiler type-checks the generated code along with everything else, and the rest of the pipeline proceeds as if you had written the implementation by hand. There is no runtime cost — and no way for a macro to do anything at runtime, either.

Dovetail deliberately keeps the surface small:

- The only form of macro available today is the **derive macro**, applied to a record, enum, or newtype.
- There are no expression-level macros, no token tricks, no syntax extensions. Macros do not introduce new keywords or operators.
- A derive macro's only job is to emit a single `implement` block.

This keeps generated code readable, predictable, and easy to reason about. If you can't tell what code a macro would produce just by knowing the type and the trait, the macro is probably trying to do too much.

---

## 16.2 The `@derive` Attribute

The `@derive` attribute goes directly above a record, enum, or newtype declaration. It names a single macro to apply.

```dovetail
@derive(Equatable)
record Point =
    x: Int32
    y: Int32
```

This generates an `implement Equatable for Point` block — a structural equality check that compares each field. The original declaration is unchanged; the derive sits beside it.

### Multiple Derives

To apply more than one derive, stack the attributes. Each `@derive` names exactly one macro:

```dovetail
@derive(Equatable)
@derive(JsonEncoder)
@derive(JsonDecoder)
record User =
    name: String
    age: Int32
```

Order does not matter — each derive sees the original declaration, not the output of the previous one. The compiler runs them all and adds each generated `implement` block to the package.

### Enums and Newtypes

Derives work the same way on enums and newtypes:

```dovetail
@derive(Equatable)
enum Shape =
    Circle(Float64)
    Rect { width: Float64, height: Float64 }
    Empty

@derive(Equatable)
newtype UserId = Int64
```

A single derive macro typically handles all three kinds; the macro inspects what it was given and emits code accordingly.

### Derives and Private Types

Generated implementations obey the same access rules as handwritten ones. `@derive(Equatable)` can compare a private record's fields or match a private enum's variants, since those operations remain public. A decoder that directly constructs either type is rejected outside its associated module.

Private newtypes also hide `.value` and unwrapping patterns. A derive that reads the wrapped value directly, including the standard `Equatable` derive, therefore cannot be used on a private newtype. Use an implementation that delegates to the associated module, or a derive that generates those module calls. For decoding, a useful approach is to decode a public snapshot and pass it to a module function that validates and constructs the private value.

### Generic Types

When the target type has type parameters, the macro adds the corresponding trait bounds for you. Deriving `Equatable` for a generic record:

```dovetail
@derive(Equatable)
record Box<T> =
    value: T
```

produces an `implement` block that requires `T: Equatable` — so `Box<Int32>` is equatable, but `Box<SomeNonEquatableType>` is rejected at the call site with a clear error.

### Fully Qualified Macro Names

If two macros share a name, or you want to be explicit, use a dotted path:

```dovetail
@derive(standard.json.JsonEncoder)
record Point =
    x: Int32
    y: Int32
```

The short form (`@derive(JsonEncoder)`) works whenever a macro of that name is reachable; the qualified form is unambiguous.

---

## 16.3 Built-in Derives

The compiler and standard library ship a small set of derives. These are the most common ones and are the only "magic" — everything else is just a derive that you (or some library) wrote.

| Derive | Package | Trait it implements |
|---|---|---|
| `Equatable` | `standard.prelude` | Structural equality (`equals`) |
| `JsonEncoder` | `standard.json` | Convert the type to JSON (`toJson`) |
| `JsonDecoder` | `standard.json` | Parse the type from JSON (`fromJson`) |

`Equatable` is available everywhere because the prelude is always in scope. `JsonEncoder` and `JsonDecoder` require a dependency on `standard.json`.

All three follow the structural rule: for a record, each field is encoded/compared in turn; for an enum, each variant is handled; for a newtype, the single inner value carries through.

```dovetail
@derive(JsonEncoder)
record Point =
    x: Int32
    y: Int32

function main(): Unit =
    let p = Point { x = 1, y = 2 }
    Console.println(p.toJson().stringify())   // {"x":1,"y":2}
```

---

## 16.4 Writing Custom Derives (Rhai)

A derive macro is a [Rhai](https://rhai.rs) script that returns Dovetail source code as a string. The compiler runs the script once per type that names the derive, parses the returned string back into a declaration, and adds it to the package.

### Registering a Macro

Macros live in your project alongside the source. A `[[project.macro]]` entry in `Dovetail.toml` tells the compiler the name, the trait it implements, and where to find the script:

```toml
[[project.macro]]
name = "Hashable"
package = "my.app"
kind = "derive"
trait = "my.app.Hashable"
script = "macros/Hashable.rhai"
```

Once registered, any package that depends on `my.app` can write `@derive(Hashable)` on a record or enum.

### The `input` Variable

A derive script receives a constant named `input` describing the type it is being applied to:

```rhai
input = #{
    kind: "record",          // or "enum" or "newtype"
    name: "Point",
    type_params: [],
    fields: [
        #{ name: "x", ty: "Int32" },
        #{ name: "y", ty: "Int32" },
    ],
    // For enums:
    // variants: [ #{ name: "...", kind: "none" | "tuple" | "record", types: [...], fields: [...] } ]
    // For newtypes:
    // inner: "Int32"
}
```

The script reads `input` and returns a string. That string must be a valid Dovetail `implement` block for the trait the macro is registered to.

### A Small Example: a `Tag` Derive

Suppose we want a derive that implements a tiny `Tag` trait, returning the type's own name as a string:

```dovetail
public trait Tag =
    function tag(self: Self): String
```

The Rhai script is short:

```rhai
fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}

let type_args = if input.type_params.is_empty() {
    ""
} else {
    "<" + join(input.type_params, ", ") + ">"
};

`implement Tag for ${input.name}${type_args} =
    public function tag(self: ${input.name}${type_args}): String = "${input.name}"
`
```

The script's last expression is the return value — a backtick template that interpolates `input.name` into the generated Dovetail source. Using it is then just:

```dovetail
@derive(Tag)
record Cat =
    age: Int32

function main(): Unit =
    let c = Cat { age = 3 }
    Console.println(c.tag())   // Cat
```

### Generating from Fields and Variants

A more realistic derive iterates over `input.fields` (for records) or `input.variants` (for enums). The structural-equality derive does both: for records it joins `self.f == other.f` for every field with `&&`; for enums it builds a `match (self, other) with ...` whose arms unpack each variant on both sides and compare component-by-component.

The pattern is always the same: read `input`, build up a Dovetail source string, return it.

### Conventions

A few practical points keep custom derives well-behaved:

- **Generate one `implement` block.** Derive macros are not for adding arbitrary declarations to the package.
- **Add trait bounds for every type parameter.** If your derive emits `T: MyTrait` bounds in the generated `where` clause, the type system rejects misuse at the call site with a useful message.
- **Keep the script readable.** A derive is a description of code a human could have written. If the script is hard to follow, the generated code will be too.
- **Test against records, enums (with all three payload shapes: none, tuple, record), and newtypes.** A derive that only handles one of these is a derive that quietly fails to apply elsewhere.

---

## Summary

- `@derive(Name)` above a record, enum, or newtype generates an `implement` block at compile time.
- Stack multiple `@derive` attributes to apply more than one; order does not matter.
- Generic targets get the right trait bounds added automatically.
- Built-in derives: `Equatable`, `JsonEncoder`, `JsonDecoder`.
- Custom derives are Rhai scripts that read an `input` description and return Dovetail source as a string; register them in `Dovetail.toml`.
