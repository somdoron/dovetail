# Part 24: Prefixed String Literals

A **prefixed string literal** is a string with an identifier attached to its opening quote:

```dovetail
let query = sql"SELECT * FROM users WHERE id = $id"
```

The prefix selects a *handler*: a library type that decides what the literal means. The compiler owns the syntax — how the literal is lexed, and what `$` may do inside it — and nothing else. It has no idea what SQL is.

This is the same bargain as derive macros ([Part 16](16-macros.md)): the compiler provides a small structural hook, and libraries provide the meaning.

---

## 24.1 Why not just build a string?

Because a string that is *interpolated* and a string that is *parameterized* are different things, and only one of them is safe:

```dovetail
// An ordinary string: the name is spliced into the text.
let bad = "SELECT * FROM users WHERE name = '$name'"

// A prefixed literal: the name is bound as a parameter.
let good = sql"SELECT * FROM users WHERE name = $name"
```

The second renders to `SELECT * FROM users WHERE name = ?` with `name` carried alongside as a bound parameter. There is no syntax for splicing a value into the text, so SQL injection is not expressible — not "discouraged", not "caught by a lint".

The same shape suits any language-inside-a-language: a regex whose interpolations are escaped, a shell command whose interpolations are quoted, a query whose interpolations are bound.

---

## 24.2 Syntax

The prefix must touch the quote — `sql"..."`, never `sql "..."`. With a space, you get an ordinary identifier followed by an ordinary string.

Both quote forms work:

```dovetail
let one = sql"SELECT 1"

let many = sql"""
    SELECT u.id, u.name
    FROM users u
    WHERE u.total > $minTotal
"""
```

As with ordinary `"""` strings, one leading newline after the opener is dropped and the rest of the text is kept verbatim — there is no indentation stripping.

### Interpolation

Three forms are available inside a prefixed literal:

| Form | Meaning |
|------|---------|
| `$ident` | interpolate a bare identifier |
| `${ expr }` | interpolate any expression |
| `$..expr` | *spread*: interpolate a sequence of values |

`$ident` binds a bare name only. Anything with a field access, a call, or an operator needs the braced form:

```dovetail
let q = sql"WHERE name = ${user.name} AND age > ${minAge + 1}"
```

`$..` exists only in prefixed literals — an ordinary string has no builder to hand a sequence to, and using it there is an error that says so.

Escape sequences work exactly as in ordinary strings, so `\$` is a literal dollar sign and `\n` is a newline. That does mean a backslash-heavy literal (a regex, say) needs its backslashes doubled.

---

## 24.3 What a literal lowers to

The compiler rewrites the literal into a chain of ordinary method calls on the type the prefix names:

```dovetail
sql"WHERE id = $id AND tag IN ($..tags)"
```

becomes

```dovetail
SqlBuilder.empty()
    .literal("WHERE id = ")
    .value(id)
    .literal(" AND tag IN (")
    .spread(tags)
    .literal(")")
    .build()
```

Those five method names are the entire contract. Text between interpolations becomes `literal`, a `$` becomes `value`, a `$..` becomes `spread`, and `build` produces the final value.

The chain is then type-checked like any hand-written code, which is what makes the next section work.

---

## 24.4 Bounds come from the builder, not the compiler

The builder's own signatures decide what may be interpolated:

```dovetail
public function value<T>(self, item: T): SqlBuilder where T: ToSqlValue =
    ...
```

Because `value` is what the lowering calls, `T: ToSqlValue` is enforced at every interpolation site — and the compiler enforces it without knowing that `ToSqlValue` exists. Interpolating a type that doesn't implement it is an ordinary trait-bound failure, reported at the interpolation itself:

```
error: type 'Widget' does not implement trait 'ToSqlValue' required by constraint on 'T'
   |
   |     let q = sql"WHERE thing = $widget"
   |                               ^
```

Swap the bound in the builder and you have changed what the literal accepts. No compiler change is involved.

---

## 24.5 Writing a builder

A builder is an ordinary type with five methods. The `@stringLiteral` declaration associates a prefix with its builder.

```dovetail
public record Frag =
    text: String
    values: Array<String>

public record Builder =
    text: StringBuilder
    values: ArrayList<String>

module Builder =
    public function empty(): Builder =
        Builder { text = StringBuilder.new(); values = ArrayList<String>.new() }

    public function literal(self, chunk: String): Builder =
        self.text.append(chunk)
        self

    public function value<T>(self, item: T): Builder where T: Display =
        self.text.append("?")
        self.values.add(item.format())
        self

    public function spread<T>(self, items: Array<T>): Builder where T: Display =
        ...

    public function build(self): Frag =
        Frag { text = self.text.toString(); values = self.values.toArray() }
```

Two things are worth getting right:

- **Keep the builder type non-generic; make only `value` and `spread` generic.** This keeps the builder state independent of each interpolated value's type, while allowing each value method to express its own trait bounds.
- **Mutate, don't copy.** Returning `self` after appending to a `StringBuilder` makes an N-part literal cost O(total bytes). Rebuilding an immutable record at each step makes it O(N²).

All five methods must be `public`.

---

## 24.6 Declaring a prefix

A prefix is declared with `@stringLiteral`, which takes no argument — the prefix *is* the declared name:

```dovetail
@stringLiteral
public type sql = SqlBuilder
```

The marker usually goes on a lowercase **type alias**, so the builder keeps its descriptive name while the prefix reads the way it should at the use site. It may also go directly on a `record` or `class` whose own name is the prefix.

### The prefix is an ordinary name

This is the whole design. `sql"..."` is available exactly where `sql` is in scope:

```dovetail
import standard.sqlite.sql

let query = sql"SELECT * FROM users WHERE id = $id"
```

Three things fall out of that, none of which needed inventing:

**Collisions are impossible.** Two libraries may both export a `sql`. A file imports the one it wants; there is no global claim to arbitrate, no precedence rule, and no manifest override.

**Aliasing renames the literal.** `import` already renames, so:

```dovetail
import standard.sqlite.sql
import com.pg.sql as pg

let a = sql"SELECT 1"
let b = pg"SELECT 1"
```

**It is self-documenting.** A reader sees the import at the top of the file and knows exactly which library `sql"..."` comes from — something a build-config entry in a different project could never tell them.

A name that is in scope but was not marked `@stringLiteral` is not a prefix, and says so. A prefix that is not in scope points at the import that would fix it.

> `test` cannot be a prefix: `test "name" = ...` already declares a test, and a space-less `test"name"` must keep meaning that.

---

## 24.7 Composition instead of in-string control flow

There is deliberately no conditional form inside a literal. Since the built value is an ordinary value, conditional and dynamic construction is ordinary code — and the library, not the language, decides what "compose" means:

```dovetail
let query =
    sql"SELECT * FROM users"
        ++ Query.whereAndSome([
            nameFilter.map<Query>((name: String) => sql"name = $name"),
            statusFilter.map<Query>((status: Status) => sql"status = $status")
        ])
```

Inside the lambda, `name` is a plain `String` bound by an ordinary parameter. An in-string conditional would have needed the compiler to rebind an interpolated name to its unwrapped type — a typing rule specific to one library, in the compiler, forever.

Composition gets the same result with nothing new in the language, and gets *more*: `whereAndSome` drops the absent filters and emits no `WHERE` at all when they are all absent, so there is no `WHERE 1=1` placeholder to write. None of that is the literal's business — `Query`, `whereAndSome`, and the `++` operator (via the prelude's `Concat` trait) are all ordinary library code that the compiler knows nothing about.

---

## 24.8 Summary

| | |
|---|---|
| `prefix"..."` / `prefix"""..."""` | A prefixed literal; the prefix must touch the quote |
| `$ident`, `${ expr }` | Interpolate a value → `builder.value(expr)` |
| `$..expr` | Spread a sequence → `builder.spread(expr)` |
| Literal text | → `builder.literal("...")` |
| Builder contract | `empty`, `literal`, `value`, `spread`, `build`, all `public` |
| Declaration | `@stringLiteral` on a type alias, record, or class; the prefix is the declared name |
| Scope | Wherever the name is imported; `import pkg.sql as pg` renames the literal to `pg"..."` |

See [docs/SQL.md](../docs/SQL.md) for the first client of this mechanism, `standard-sqlite`'s `sql"..."`.
