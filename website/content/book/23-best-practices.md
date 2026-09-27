# Part 23: Best Practices

## 23.1 Code Style

Run `dovetail fmt` and let it choose layout. Use camelCase identifiers, descriptive
names, and `.dove` filenames. A file containing one type's module can use that type's
name, such as `Order.dove`; other source filenames use camelCase. Name new factory
functions `make`, or `parse` when construction validates external input. Existing
libraries may use different names; call the API they actually expose.

Keep examples and application code explicit about their dependencies. Import named
extensions before using their methods, including within the same package.

## 23.2 Functional Patterns

Use records and enums for immutable domain data. Represent mutually exclusive states
with enum variants carrying the fields each state requires. Use private newtypes
when values must be validated before use. Private records reserve construction and
`with` updates for their associated module.

Record fields are immutable bindings, not a deep freeze: a record holding an array
or a mutable class still shares that object's state. Prefer lists or other immutable
contents when the model requires immutable snapshots.

Use classes for owned mutable state. Keep mutation behind a small API. A type's
shape and its behavior should make valid transitions easy to identify.

## 23.3 Error Handling Patterns

Use `Option` for expected absence, and `Result` for an operation that may fail. In
async code, keep failures in the `Async<T, E>` error channel. Handle an outcome,
return it, or compose it into more work. An exhaustive match forces new outcomes to
be considered when the model changes.

Warnings catch discarded `Result`, `Async`, and `Resource` expressions. Use
`let _ = expression` only when deliberately ignoring that value. It neither awaits
an async computation nor acquires a resource. Assigning to a named variable is not
proof that the variable will later be used; these warnings are not a general
unused-variable analysis.

Reserve `panic`, `assert`, and `.require` for bugs, tests, and established invariants.
A synchronous panic terminates execution; async code can represent a panic in its
failure cause and run scoped cleanup. Panics are not the normal input-validation API.

Use `use` for external resources. Keep acquisition and release bounded, and choose
`fork` versus `forkBackground` according to whether a child should finish normally
or be cancelled when its scope ends. Read [Resources](13-resources.md) and
[Async Programming](12-async.md) before building long-lived services.

## 23.4 Project Organization

Start with one project. Split packages or projects when they represent a real
boundary, and keep dependencies acyclic. Tests next to a rule make it easy to change
them together; integration tests belong in `test/`.

Keep I/O at the edges of business rules. Pass facts, clocks, and generated IDs as
values. Give persistence and transport adapters interfaces owned by their consumers.
For larger applications, see [Project Structure](18-project-structure.md) and
[The Domain Layer](19-ddd.md).

Check in the manifest and dependency lockfile. Format, check, and test in CI using
a matching compiler version. Start from [Tool Commands](02-tool-commands.md#24-ci-and-local-compiler-development)
for the command sequence.
