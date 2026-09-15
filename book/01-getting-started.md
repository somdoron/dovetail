# Part 1: Getting Started

## 1.1 Introduction to Dovetail

### What is Dovetail?

Dovetail is a new high-level programming language designed for writing business-logic heavy applications. It compiles to WebAssembly (specifically WasmGC), prioritizing fast compilation and great tooling over raw runtime performance.

Dovetail is:

- **Expression-based** - Everything is an expression that returns a value
- **Strongly typed** - A powerful type system catches errors at compile time
- **Layout-sensitive** - Uses indentation instead of curly braces and semicolons
- **Functional-first** - Encourages immutability and pure functions, while supporting imperative and object-oriented styles
- **Garbage collected** - Memory management is handled automatically
- **Async-first** - Built-in async/await with elegant error propagation
- **No exceptions** - Errors are values using `Result` and `Option` types with railway-oriented programming

### Why Another Language?

Rust is a modern language for system-level programming, written from the ground up with modern concepts and a new ecosystem. It didn't inherit old baggage from VMs or C/C++.

But we don't have such a language for high-level programming.

**TypeScript** - A great language with modern concepts, but it's based on JavaScript with the JavaScript ecosystem and a lot of baggage:
- Both async and callbacks
- Exceptions
- Nulls
- Inconsistency across the ecosystem (typed/untyped libraries)
- No ad-hoc polymorphism (traits, typeclasses)

**Python** - Popular but problematic for large codebases:
- Exceptions
- Nulls
- Untyped by default
- Both async and sync APIs
- Inconsistency in the ecosystem (is it typed or not? are all dependencies typed?)
- No ad-hoc polymorphism

**Java** - Mature but carrying decades of baggage:
- No async/await
- Type erasure
- Nulls
- Exceptions
- No standard across tools (formatter, build tool)
- No tuples
- No ad-hoc polymorphism

**Kotlin** - Modern features on top of Java, but still inherits Java's baggage:
- JVM type erasure
- Exceptions
- Null safety is incomplete - the JVM isn't null-safe, so you can encounter nulls at runtime from Java interop
- Standard library split between Kotlin and Java APIs
- No ad-hoc polymorphism

**Scala** - Powerful type system but with significant drawbacks:
- Inherits Java/JVM baggage
- Slow compilation times
- High learning curve
- Null safety is incomplete - JVM interop can introduce nulls at runtime
- Has ad-hoc polymorphism (implicits/givens), but at the cost of compilation performance and complexity

**Go** - Simple and fast to compile, but a limited type system:
- No sum types (discriminated unions)
- Limited generics (added recently, still maturing)
- Nulls (nil pointers)
- No async/await and no way to extend the language with custom coroutines
- Interfaces provide basic ad-hoc polymorphism, but more limited than traits (no associated types, can't extend types you don't own)

**C#** - Modern features but carrying .NET legacy:
- Standard library supports multiple models for backward compatibility
- Nullable reference types are opt-in and incomplete
- Exceptions
- No ad-hoc polymorphism

**Rust** - Amazing but low-level:
- No garbage collection
- Not optimized for compile times
- Steep learning curve for business logic developers

**What is ecosystem baggage?**

When we say "ecosystem baggage," we mean:
- Legacy constructs in the language that exist for backward compatibility
- Standard libraries supporting multiple patterns (sync/async, exceptions/results, nullable/non-nullable)
- Half-baked null safety solutions where the runtime can still produce nulls even when the type system says otherwise
- Inconsistent tooling where the community is split across multiple formatters, build tools, and conventions

**We need a new language** - one with modern concepts and no ecosystem baggage. Dovetail is that language: garbage collected, fast to compile, with a clean type system, proper error handling, and consistent tooling from day one.

### Design Philosophy and Principles

Dovetail is built around several core principles:

**Fast Compilation**

Compilation should feel instant. Dovetail prioritizes fast feedback loops over maximum runtime optimization. The target competition is Python and Node.js - even unoptimized Dovetail code will be significantly faster than interpreted languages.

**Beautiful Code**

Dovetail is designed to produce readable code that flows like English:

- String interpolation without prefixes: `"Hello, $name!"`
- Named arguments for clarity: `createUser(name = "John", age = 30)`
- Layout-sensitive syntax eliminates visual noise
- No cryptic abbreviations - `function` not `fn`, `Comparable` not `Ord`, `Equatable` not `Eq`
- Keywords instead of operators where it improves readability (`and`, `or`, `not`)

**Strong Type System**

The type system helps eliminate bugs:

- No null values - use `Option<T>` instead
- Discriminated unions (enums) for modeling data
- Traits for shared behavior
- Result types for error handling - no exceptions

**Great Tooling**

Dovetail includes excellent tools from day one:

- Integrated package manager and build tool
- Language Server Protocol (LSP) support
- Built-in formatter (planned)
- REPL for exploration (planned)

### Target Platform (WasmGC)

Dovetail compiles to WebAssembly with Garbage Collection (WasmGC). This provides:

- **Portability** - Run anywhere WebAssembly runs (browsers, servers, edge)
- **Performance** - Near-native speed, faster than interpreted languages
- **Safety** - WebAssembly's sandboxed execution model
- **Interoperability** - Use WASI for system access

Dovetail uses traits for generic behavior and interfaces for contracts that can
also be stored and passed as values. See [Interfaces and Interface Types](06-type-system.md#611-interfaces-and-interface-types)
and [Traits and Implementations](08-traits.md).

### Comparison with Other Languages

| Feature | Dovetail | TypeScript | Python | Java | Kotlin | Go | C# | Rust |
|---------|--------|------------|--------|------|--------|-----|-----|------|
| No Nulls | Yes | No | No | No | Partial | No | Partial | Yes |
| No Exceptions | Yes | No | No | No | No | Yes | No | Yes |
| Consistent Async | Yes | No | No | No | Yes | No | Yes | Yes |
| Ad-hoc Polymorphism (Traits) | Yes | No | No | No | No | Partial | No | Yes |
| Sum Types | Yes | Yes | No | No | Yes | No | No | Yes |
| Layout-Sensitive | Yes | No | Yes | No | No | No | No | No |
| Garbage Collected | Yes | Yes | Yes | Yes | Yes | Yes | Yes | No |
| Fast Compilation | Yes | Yes | Yes | No | No | Yes | No | No |
| No Ecosystem Baggage | Yes | No | No | No | No | No | No | Yes |

---

## 1.2 Installation and Setup

### Prerequisites

Before installing Dovetail, you need:

- **Rust toolchain** - Install from [rustup.rs](https://rustup.rs/)
- **Git** - For cloning the repository

Verify Rust is installed:

```bash
rustc --version
cargo --version
```

### Installing from Source

1. **Fork and clone the repository:**

```bash
git clone https://github.com/somdoron/dovetail.git
cd dovetail
```

2. **Build the project:**

```bash
cargo build --release
```

3. **Install the Dovetail CLI:**

```bash
cargo install --path dovetail
```

This installs the `dovetail` command to your Cargo bin directory (usually `~/.cargo/bin/`).

### Verifying the Installation

Verify Dovetail is installed correctly:

```bash
dovetail --help
```

You should see output listing the available commands:

```
Dovetail project compiler

Usage: dovetail <COMMAND>

Commands:
  init   Initialize a new Dovetail workspace
  build  Build projects in a workspace
  check  Type check projects in a workspace
  run    Build and run projects in a workspace
  lsp    Start the Language Server Protocol server
  help   Print this message or the help of the given subcommand(s)
```

---

## 1.3 Editor Setup

### VS Code Extension

Dovetail provides a VS Code extension for the best development experience.

#### Installing the Extension

1. Open VS Code
2. Navigate to the `vscode-dovetail` directory in the Dovetail repository
3. Run:

```bash
npm install
npm run compile
```

4. Package the extension:

```bash
npx vsce package
```

5. Install the generated `.vsix` file in VS Code:
   - Open Command Palette (Cmd/Ctrl + Shift + P)
   - Select "Extensions: Install from VSIX..."
   - Choose the generated `.vsix` file

### Extension Features

The Dovetail VS Code extension provides:

- **Syntax highlighting** - Colorized Dovetail code
- **LSP support** - Real-time error diagnostics, autocomplete, go-to-definition
- **Hover documentation** - View type information on hover
- **Find references** - Find all usages of a symbol
- **Workspace symbols** - Quick navigation to types and functions

### Configuration Options

The extension can be configured in VS Code settings:

| Setting | Default | Description |
|---------|---------|-------------|
| `dovetail.serverPath` | `"dovetail"` | Path to the Dovetail executable |
| `dovetail.serverPort` | `9257` | TCP port for LSP when dovetail.serverConnection is tcp |
| `dovetail.trace.server` | `"off"` | Trace communication with LSP server |

---

## 1.4 Your First Dovetail Program

### Creating a Project with `dovetail init`

Create a new Dovetail project using the `init` command:

```bash
mkdir my-project
cd my-project
dovetail init hello
```

This creates:

- `Dovetail.toml` - The project manifest
- `hello/src/main.dove` - The main source file

For a library project, use the `--lib` flag:

```bash
dovetail init mylib --lib
```

### The `Dovetail.toml` Manifest File

The generated `Dovetail.toml` looks like this:

```toml
compiler-version = "0.1.0"

[[project]]
name = "hello"
root_package = "hello"
packages = ["."]
```

Key fields:

- `compiler-version` - The exact compiler version required by the workspace
- `[[project]]` - Project definition (can have multiple)
  - `name` - Project name
  - `root_package` - The root package name for this project
  - `packages` - Package directories relative to the project's `src/` directory

### Project Directory Structure

After running `dovetail init hello`, your directory looks like:

```
my-project/
├── Dovetail.toml
└── hello/
    └── src/
        └── main.dove
```

Convention:

- Each project has its own directory matching the project name
- Source files go in `src/`
- Test files go in `test/` (for integration tests)
- The directory structure under `src/` determines package names

### Writing "Hello, World!"

Open `hello/src/main.dove`:

```dovetail
package hello

function main() =
    println("Hello, World!")
```

Let's break this down:

- `package hello` - Declares this file belongs to the `hello` package
- `function main()` - The entry point for an application
- `=` followed by indented code - The function body
- `println(...)` - Prints text to the console

### Building and Running

**Build the project:**

```bash
dovetail build
```

This compiles your code and produces a `.wasm` file.

**Build and run:**

```bash
dovetail run
```

Output:

```
Hello, World!
```

**Type check without building:**

```bash
dovetail check
```

This verifies your code is correct without generating a WebAssembly file - useful for quick feedback during development.

---

## Summary

You now have Dovetail installed and know how to:

- Create a new project with `dovetail init`
- Understand the project structure and `Dovetail.toml`
- Write and run a simple program
- Use the VS Code extension for a better development experience

In the next part, we'll explore the Dovetail CLI commands in more detail.
