# The Dovetail Programming Language

## Table of Contents

---

### [Part 1: Getting Started](01-getting-started.md)

- 1.1 Introduction to Dovetail
  - What is Dovetail?
  - Why another language?
  - Design philosophy and principles
  - Target platform (WasmGC)
  - Comparison with other languages
- 1.2 Installation and Setup
  - Prerequisites (Rust toolchain, cargo)
  - Installing with Cargo
  - Installing from source
  - Verifying the installation
- 1.3 Editor Setup
  - VS Code Extension
  - Extension features (syntax highlighting, LSP support)
  - Configuration options
- 1.4 Your First Dovetail Program
  - Creating a project with `dovetail init`
  - The `Dovetail.toml` manifest file
  - Project directory structure
  - Writing "Hello, World!"
  - Building and running

---

### [Part 2: Dovetail Tool Commands](02-tool-commands.md)

- 2.1 The Dovetail CLI
- 2.2 Project Structure
- 2.3 Dependency Management
- 2.4 CI and Local Compiler Development

---

### [Part 3: Language Basics](03-language-basics.md)

- 3.1 Layout-Sensitive Syntax
- 3.2 Comments and Documentation
- 3.3 Variables and Bindings
- 3.4 Primitive Types
- 3.5 Operators
- 3.6 String Interpolation

---

### [Part 4: Control Flow](04-control-flow.md)

- 4.1 If Expressions
- 4.2 Match Expressions
- 4.3 Loops

---

### [Part 5: Functions](05-functions.md)

- 5.1 Function Definitions
- 5.2 Positional Arguments and Configuration
- 5.3 Closures and Lambdas
- 5.4 First-Class Function References
- 5.5 Method References
- 5.6 Higher-Order Functions

---

### [Part 6: Type System](06-type-system.md)

- 6.1 Type Inference
- 6.2 Records
  - Private construction and `with` updates
- 6.3 Enums (Discriminated Unions)
  - Option Type
  - Result Type
  - Private construction with public pattern matching
- 6.4 Tuples
- 6.5 Newtypes
  - Private construction and inspection
- 6.6 Type Aliases
- 6.7 Lists, Arrays, and Slices
  - Lists and List Literals
  - Accessing and Matching Lists
  - Transforming and Iterating Lists
  - Choosing Between Array and List
  - Shared Array Views and Slicing Syntax
  - Slice Bounds and Constructors
  - Copying, Comparing, and Iterating
  - Slices and Strings
- 6.8 Modules
- 6.9 Extension Methods
- 6.10 The Any Type and Type Casting
  - The Any Type
  - Type Testing with `is`
  - Type Casting with `as`
  - Working with Generic Types and Arrays
- 6.11 Interfaces and Interface Types
  - Declaring and Implementing Interfaces
  - Generic Interfaces and Bounds
  - Intersections and Upcasts
  - Inheritance, Defaults, and `Self`
  - Interface Restrictions and `Any`
  - Choosing a Contract

---

### [Part 7: Generics](07-generics.md)

- 7.1 Generic Functions
- 7.2 Generic Types
- 7.3 Constraints

---

### [Part 8: Traits and Implementations](08-traits.md)

- 8.1 Defining Traits
- 8.2 Implementing Traits
  - Implementations and private types
- 8.3 Core Traits
  - Identity Equality and Hashing
- 8.4 The Orphan Rule
- 8.5 Trait and Interface Inheritance
- 8.6 Default Methods and Properties
- 8.7 Method Resolution and Explicit Disambiguation

---

### [Part 9: Classes and Object-Oriented Programming](09-classes.md)

- 9.1 Class Definitions
- 9.2 Methods
- 9.3 Class Body Fields
- 9.4 Private Constructors
- 9.5 Mutable Fields
- 9.6 Inheritance
- 9.7 Abstract Classes
- 9.8 Sealed Abstract Classes
- 9.9 Generic Classes
- 9.10 Implementing Traits for Classes
- 9.11 Extension Properties
- 9.12 Extension Methods for Classes
- 9.13 Visibility Modifiers
- 9.14 Classes vs Records
- 9.15 Class Identity

---

### [Part 10: Error Handling](10-error-handling.md)

- 10.1 No Exceptions
- 10.2 Option Type
- 10.3 Result Type
- 10.4 Railway-Oriented Programming
- 10.5 Panics and Assertions

---

### [Part 11: Packages and Modules](11-packages.md)

- 11.1 Package Declaration
- 11.2 Imports
- 11.3 Visibility
  - Type visibility and private construction
- 11.4 No Circular Dependencies
- 11.5 GitHub Dependencies

---

### [Part 12: Async Programming](12-async.md)

- 12.1 The Async Type
- 12.2 Async Functions
- 12.3 The `await` Operator
- 12.4 Running Async Code
- 12.5 Parallel Execution

---

### [Part 13: Resource Management](13-resources.md)

- 13.1 The `use` Keyword
- 13.2 LIFO Release Order
- 13.3 Release on Failure
- 13.4 Composing Resources
- 13.5 Error Conversion at the Use Site

---

### [Part 14: Streams](14-streams.md)

- 14.1 Descriptions and Consumption
- 14.2 Creating a Source
- 14.3 Chunks and Transformations
- 14.4 Concatenation and Flat Mapping
- 14.5 Repeating Effects and Time
- 14.6 Errors and Resource Lifetimes
- 14.7 Merging Concurrent Sources
- 14.8 Running and Folding
- 14.9 Byte Transports

---

### [Part 15: Testing](15-testing.md)

- 15.1 Unit Tests
- 15.2 Test Organization
- 15.3 Test Attributes
- 15.4 Running Tests
- 15.5 Testing Best Practices

---

### [Part 16: Macros](16-macros.md)

- 16.1 Overview
- 16.2 The `@derive` Attribute
  - Derives and private types
- 16.3 Built-in Derives
- 16.4 Writing Custom Derives (Rhai)

---

### [Part 17: Components](17-components.md)

- 17.1 Declaring a Component Dependency
- 17.2 What Gets Generated
- 17.3 Using SQLite
- 17.4 Building and Running
- 17.5 Limits (v1)

---

### [Part 18: Project Structure and Onion Architecture](18-project-structure.md)

- 18.1 The Dependency Rule
- 18.2 Three Projects per Bounded Context
  - `primitives` — the value vocabulary
  - `contract` — the public wire surface (data and service contracts)
  - `app` — the context itself
- 18.3 Three Packages Inside `app`
  - The domain package
  - The application package
  - The infrastructure package
- 18.4 What the Compiler Enforces
- 18.5 Variant: the Domain Layer as Its Own Project
- 18.6 Ports and Adapters
  - Repositories
  - The anti-corruption layer
- 18.7 The Composition Root
- 18.8 The Shared Kernel
- 18.9 Talking to Other Bounded Contexts
  - Service contracts, client stubs, and the ACL
  - Integration events
- 18.10 Testing the Layers
- 18.11 Rules at a Glance

---

### [Part 19: The Domain Layer](19-ddd.md)

- 19.1 Start With the Types
- 19.2 Make Illegal States Unrepresentable
  - Rules of thumb
  - Errors are types too
  - The compiler as reviewer
  - How far to take it
- 19.3 Value Objects
- 19.4 Entities
  - Make the transition boundary enforceable
- 19.5 Two Styles: Values or Objects
- 19.6 Aggregates
- 19.7 Domain Events
- 19.8 Domain Services
  - Pass the fact, not the port
- 19.9 Factories and Reconstitution
- 19.10 The Anemic Domain Model
- 19.11 Testing the Domain
- 19.12 A Change, End to End

---

### [Part 20: The Application Layer](20-application-layer.md)

*(outline)*

- 20.1 What a Use Case Owns
  - The "one change, one transaction, one aggregate" rules
  - Consequences: notification or obligation
  - The sandwich: IO at the edges
  - When the sandwich does not fit: the layered cake
- 20.2 Commands, Queries, and Results
  - Errors from the domain
- 20.3 The Three Triggers
  - Requests
  - Event handlers
  - Jobs
- 20.4 Repositories
- 20.5 Publishing Integration Events
- 20.6 Sagas, Compensation, and Durable Execution
  - Two shapes: backward and forward recovery
  - The problem durable execution solves
  - Dovetail has no durable execution runtime today
- 20.7 Cross-Cutting Concerns
- 20.8 Testing the Application Layer
- 20.9 Worked Example

---

### [Part 21: Strategic Design and Context Mapping](21-context-mapping.md)

*(outline)*

- 21.1 Subdomains: Core, Supporting, Generic
- 21.2 Ubiquitous Language and Bounded Contexts
- 21.3 Context Relationship Patterns
- 21.4 Direct Contract Use vs Anticorruption Layer
- 21.5 Integration Events
- 21.6 Drawing and Maintaining the Context Map
- 21.7 Splitting and Merging Contexts

---

### [Part 22: Standard Library Overview](22-stdlib.md)

- 22.1 Core Types
- 22.2 Collections
- 22.3 Text Processing
- 22.4 Math
- 22.5 I/O
- 22.6 Networking
- 22.7 Time
- 22.8 JSON
- 22.9 Randomness and Crypto
- 22.10 Streams and SQLite

---

### [Part 23: Best Practices](23-best-practices.md)

- 23.1 Code Style
- 23.2 Functional Patterns
- 23.3 Error Handling Patterns
- 23.4 Project Organization

---

### [Part 24: Prefixed String Literals](24-prefixed-literals.md)

- 24.1 Why not just build a string?
- 24.2 Syntax
- 24.3 What a literal lowers to
- 24.4 Bounds come from the builder, not the compiler
- 24.5 Writing a builder
- 24.6 Declaring a prefix
- 24.7 Composition instead of in-string control flow

---

### [Part 25: Tuple Extension (Advanced)](25-tuple-extension.md)

- 25.1 Extending a Tuple
- 25.2 Extension in Generic Types
- 25.3 Tuple Shape and Accessors
- 25.4 Pair and Recursive Implementations
- 25.5 Parser Composition
- 25.6 Enclosing Parameter Bounds

---

### [Part 26: Advanced Generics](26-advanced-generics.md)

- 26.1 Generic Extensions
- 26.2 Class Constraints
- 26.3 Bounds on Enclosing Parameters
- 26.4 Trait and Override Contracts
- 26.5 Variance and Method Bounds
  - Plain Parameters, `out`, and `in`
  - Conditional Bounds on Virtual Methods
- 26.6 Pattern Matching on Generic Types
- 26.7 Generic Trait Defaults
- 26.8 Associated Outputs
- 26.9 Generic Associated Types
  - Generic Resource Helpers
- 26.10 Inherited Overloads

---

### [Part 27: Production Deployment and CI](27-production-deployment.md)

- 27.1 Prepare a Reproducible Release
- 27.2 Image Permissions and Deployment Configuration
- 27.3 Configure CI Checks
- 27.4 Build and Publish in CI
- 27.5 Run and Verify the Image
- 27.6 Operate and Roll Back

---

### Appendix

- [A. Grammar Reference](../../../grammar.md)
- [B. Operator Precedence Table](../../../grammar.md#operator-precedence-table)
- [C. Reserved Keywords](../../../grammar.md#keywords)
- [D. Standard Library Quick Reference](22-stdlib.md)

[How this book is validated](validation.md)
