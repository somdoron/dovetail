use crate::common::span::{Span, Spanned};
use crate::common::types::{Fqn, PackagePath, Variance, Visibility};

/// Binary operators.
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,         // +
    Concat,      // ++
    TupleExtend, // ~
    Sub,         // -
    Mul,         // *
    Div,         // /
    Rem,         // %
    Eq,          // ==
    Ne,          // !=
    Lt,          // <
    Gt,          // >
    Le,          // <=
    Ge,          // >=
    BitAnd,      // &
    BitOr,       // |
    BitXor,      // ^
    Shl,         // <<
    Shr,         // >>
    LogicalAnd,  // &&
    LogicalOr,   // ||
}

impl std::fmt::Display for BinOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BinOp::Add => write!(f, "+"),
            BinOp::Concat => write!(f, "++"),
            BinOp::TupleExtend => write!(f, "~"),
            BinOp::Sub => write!(f, "-"),
            BinOp::Mul => write!(f, "*"),
            BinOp::Div => write!(f, "/"),
            BinOp::Rem => write!(f, "%"),
            BinOp::Eq => write!(f, "=="),
            BinOp::Ne => write!(f, "!="),
            BinOp::Lt => write!(f, "<"),
            BinOp::Gt => write!(f, ">"),
            BinOp::Le => write!(f, "<="),
            BinOp::Ge => write!(f, ">="),
            BinOp::BitAnd => write!(f, "&"),
            BinOp::BitOr => write!(f, "|"),
            BinOp::BitXor => write!(f, "^"),
            BinOp::Shl => write!(f, "<<"),
            BinOp::Shr => write!(f, ">>"),
            BinOp::LogicalAnd => write!(f, "&&"),
            BinOp::LogicalOr => write!(f, "||"),
        }
    }
}

/// Unary operators.
#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,    // - (numeric negation)
    Not,    // ! (boolean not)
    BitNot, // ~ (bitwise not)
}

impl std::fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnaryOp::Neg => write!(f, "-"),
            UnaryOp::Not => write!(f, "!"),
            UnaryOp::BitNot => write!(f, "~"),
        }
    }
}

/// Combined AST for a single package, potentially from multiple source files.
#[derive(serde::Serialize, Debug)]
pub struct PackageAst {
    pub package_path: PackagePath,
    pub files: Vec<SourceFile>,
}

/// A parsed source file.
#[derive(serde::Serialize, Debug)]
pub struct SourceFile {
    pub package: PackageDecl,
    pub imports: Vec<ImportDecl>,
    pub declarations: Vec<Declaration>,
}

/// A package declaration: `package a` or `package com.example.myapp`.
#[derive(serde::Serialize, Debug)]
pub struct PackageDecl {
    pub path: Vec<Spanned<String>>,
    pub span: Span,
}

/// An import declaration: `import a.utils.func` or `import a.utils as u`.
#[derive(serde::Serialize, Debug)]
pub struct ImportDecl {
    pub path: Vec<Spanned<String>>,
    pub alias: Option<Spanned<String>>,
    pub span: Span,
}

/// A top-level declaration.
#[derive(serde::Serialize, Debug)]
pub enum Declaration {
    Function(FunctionDecl),
    GlobalVar(GlobalVarDecl),
    Record(RecordDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    Extension(ExtensionDecl),
    Implement(ImplementDecl),
    Module(ModuleDecl),
    Newtype(NewtypeDecl),
    TypeAlias(TypeAliasDecl),
    Class(ClassDecl),
    Test(TestDecl),
}

/// An attribute on a test declaration: `@skip`, `@panics("msg")`, `@timeout(1000)`
#[derive(serde::Serialize, Debug)]
pub enum TestAttribute {
    Skip {
        reason: Option<Spanned<String>>,
        span: Span,
    },
    Panics {
        message: Option<Spanned<String>>,
        span: Span,
    },
    Timeout {
        millis: Spanned<String>,
        span: Span,
    },
}

/// A derive attribute on a record or enum declaration: `@derive(Equatable)`.
///
/// `macro_name` is the path to the macro as written at the call site
/// (e.g. `["Equatable"]` or `["json", "JsonCodec"]`). The macro phase
/// resolves this against the file's imports + the MacroRegistry.
#[derive(serde::Serialize, Debug, Clone)]
pub struct DeriveAttribute {
    pub macro_name: Vec<Spanned<String>>,
    pub span: Span,
}

/// A test declaration: `test "name" = body`
#[derive(serde::Serialize, Debug)]
pub struct TestDecl {
    pub attributes: Vec<TestAttribute>,
    pub name: Spanned<String>,
    pub body: Expr,
    pub span: Span,
}

/// A type alias declaration: `type Cents = Int32` or `type Maybe<T> = Option<T>`
#[derive(serde::Serialize, Debug)]
pub struct TypeAliasDecl {
    pub visibility: Visibility,
    /// Span of a `@stringLiteral` attribute, when the alias names a prefixed
    /// string literal (`@stringLiteral public type sql = SqlBuilder`).
    pub string_literal: Option<Span>,
    pub name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    pub where_clause: Vec<TraitConstraint>,
    pub type_expr: TypeExpr,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// A class declaration.
#[derive(serde::Serialize, Debug)]
pub struct ClassDecl {
    pub visibility: Visibility,
    /// Span of a `@stringLiteral` attribute, when this type is itself the
    /// prefix of a prefixed string literal. Usually the marker goes on a
    /// lowercase type alias instead; see website/content/book/24-prefixed-literals.md.
    pub string_literal: Option<Span>,
    pub is_final: bool,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub name: Spanned<String>,
    pub type_params: Vec<VariantTypeParam>,
    pub constructor_visibility: Visibility,
    pub params: Vec<ConstructorParam>,
    pub extends: Option<ClassExtends>,
    pub implements: Vec<TypeExpr>,
    pub where_clause: Vec<TraitConstraint>,
    pub body: Vec<ClassMember>,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// An extends clause: `extends ParentType(args...)`.
#[derive(serde::Serialize, Debug)]
pub struct ClassExtends {
    pub parent_type: TypeExpr,
    pub super_args: Vec<Expr>,
    pub span: Span,
}

/// A constructor parameter in a class declaration.
#[derive(serde::Serialize, Debug)]
pub struct ConstructorParam {
    pub visibility: Visibility,
    pub mutable: bool,
    pub name: Spanned<String>,
    pub type_annotation: TypeExpr,
    pub default_value: Option<Expr>,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// A member of a class body.
#[derive(serde::Serialize, Debug)]
pub enum ClassMember {
    LetBinding(ClassLetBinding),
    Method(FunctionDecl),
    Property(PropertyDecl),
    Expression(Expr),
}

/// A let binding inside a class body.
#[derive(serde::Serialize, Debug)]
pub struct ClassLetBinding {
    pub visibility: Visibility,
    pub is_static: bool,
    pub mutable: bool,
    pub name: Spanned<String>,
    pub type_annotation: Option<TypeExpr>,
    pub value: Expr,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// A newtype declaration: `newtype Cents = Int32` or `newtype Wrapper<out T> = T`
#[derive(serde::Serialize, Debug)]
pub struct NewtypeDecl {
    /// Compiler-owned opaque type declared with `type ... = intrinsic`.
    pub intrinsic: bool,
    pub visibility: Visibility,
    pub name: Spanned<String>,
    pub type_params: Vec<VariantTypeParam>,
    pub where_clause: Vec<TraitConstraint>,
    pub inner_private: bool,
    pub inner_type: TypeExpr,
    pub doc_comment: Option<String>,
    /// `@derive(...)` attributes attached at parse time. Consumed by the
    /// macro phase before Collect runs. Derives on newtypes typically
    /// forward the trait implementation to the inner type.
    pub attributes: Vec<DeriveAttribute>,
    pub span: Span,
}

/// A module declaration: `module Math = ...`
/// May have where clause (inherited from associated type): `module Map<K, V> = ...`
#[derive(serde::Serialize, Debug)]
pub struct ModuleDecl {
    pub name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    pub functions: Vec<FunctionDecl>,
    pub properties: Vec<PropertyDecl>,
    pub globals: Vec<GlobalVarDecl>,
    pub tests: Vec<TestDecl>,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// An extension declaration: `extension IntMath for Int32 = ...`
/// May have type parameters: `extension ArrayHelper<T> for Array<T> = ...`
/// May have where clause: `extension BoxHelper<T> for Box<T> where T: Display = ...`
#[derive(serde::Serialize, Debug)]
pub struct ExtensionDecl {
    pub name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    pub for_type: TypeExpr,
    pub where_clause: Vec<TraitConstraint>,
    pub methods: Vec<FunctionDecl>,
    pub properties: Vec<PropertyDecl>,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// A property declaration: `property name(self): Type = expression`
#[derive(serde::Serialize, Debug)]
pub struct PropertyDecl {
    pub visibility: Visibility,
    pub is_override: bool,
    pub is_final: bool,
    pub is_abstract: bool,
    pub name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    pub params: Vec<Param>,
    pub return_type: TypeExpr,
    pub body: Option<Expr>,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// A global variable declaration: `let x: Int32 = 42` or `let mutable x = 0`
#[derive(serde::Serialize, Debug)]
pub struct GlobalVarDecl {
    pub visibility: Visibility,
    pub name: Spanned<String>,
    pub mutable: bool,
    pub type_annotation: Option<TypeExpr>,
    pub value: Expr,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// A trait constraint in a where clause: `T: Display + Equatable` or `T: From<Int32>`
#[derive(serde::Serialize, Debug, Clone)]
pub struct TraitConstraint {
    pub type_param: Spanned<String>,
    pub trait_bounds: Vec<TypeBound>,
    pub span: Span,
}

/// A nominal constraint or the built-in class category.
#[derive(serde::Serialize, Debug, Clone)]
pub enum TypeBound {
    Named(NamedTraitBound),
    Class(Span),
}

impl TypeBound {
    pub fn span(&self) -> &Span {
        match self {
            Self::Named(ty) => &ty.span,
            Self::Class(span) => span,
        }
    }
}

/// A function declaration.
#[derive(serde::Serialize, Debug)]
pub struct FunctionDecl {
    pub visibility: Visibility,
    pub is_async: bool,
    pub is_override: bool,
    pub is_final: bool,
    pub is_abstract: bool,
    pub name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub where_clause: Vec<TraitConstraint>,
    pub body: Expr,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// A function parameter.
#[derive(serde::Serialize, Debug, Clone)]
pub struct Param {
    pub name: Spanned<String>,
    pub type_annotation: TypeExpr,
    pub span: Span,
}

/// The kind of a closure parameter.
#[derive(serde::Serialize, Debug, Clone)]
pub enum ClosureParamKind {
    /// A simple named parameter: `x` or `x: Int32`
    Name(Spanned<String>),
    /// A tuple destructuring pattern: `((a, b), c)`
    TuplePattern(Pattern),
}

/// A closure parameter (type annotation is optional).
#[derive(serde::Serialize, Debug, Clone)]
pub struct ClosureParam {
    pub kind: ClosureParamKind,
    pub type_annotation: Option<TypeExpr>,
    pub span: Span,
}

/// A type parameter with an optional variance annotation.
/// Used by `RecordDecl`, `EnumDecl`, and `ClassDecl`; other declarations use `Vec<Spanned<String>>`.
#[derive(serde::Serialize, Debug, Clone)]
pub struct VariantTypeParam {
    pub variance: Variance,
    pub name: Spanned<String>,
    pub span: Span,
}

/// A record type declaration.
#[derive(serde::Serialize, Debug)]
pub struct RecordDecl {
    pub visibility: Visibility,
    pub construction_private: bool,
    /// Span of a `@stringLiteral` attribute, when this type is itself the
    /// prefix of a prefixed string literal. Usually the marker goes on a
    /// lowercase type alias instead; see website/content/book/24-prefixed-literals.md.
    pub string_literal: Option<Span>,
    pub name: Spanned<String>,
    pub type_params: Vec<VariantTypeParam>,
    pub fields: Vec<RecordField>,
    pub where_clause: Vec<TraitConstraint>,
    pub doc_comment: Option<String>,
    /// `@derive(...)` attributes attached at parse time. Consumed by the
    /// macro phase before Collect runs.
    pub attributes: Vec<DeriveAttribute>,
    pub span: Span,
}

/// A field in a record type declaration.
#[derive(serde::Serialize, Debug)]
pub struct RecordField {
    pub name: Spanned<String>,
    pub type_annotation: TypeExpr,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// An enum type declaration.
#[derive(serde::Serialize, Debug)]
pub struct EnumDecl {
    pub visibility: Visibility,
    pub construction_private: bool,
    pub name: Spanned<String>,
    pub type_params: Vec<VariantTypeParam>,
    pub variants: Vec<EnumVariant>,
    pub where_clause: Vec<TraitConstraint>,
    pub doc_comment: Option<String>,
    /// `@derive(...)` attributes attached at parse time. Consumed by the
    /// macro phase before Collect runs.
    pub attributes: Vec<DeriveAttribute>,
    pub span: Span,
}

/// The payload form of an enum variant declaration.
#[derive(serde::Serialize, Debug)]
pub enum EnumVariantPayload {
    None,                     // no payload
    Tuple(Vec<TypeExpr>),     // Variant(Type1, Type2)
    Record(Vec<RecordField>), // Variant { field: Type }
}

/// A variant in an enum type declaration.
#[derive(serde::Serialize, Debug)]
pub struct EnumVariant {
    pub name: Spanned<String>,
    pub payload: EnumVariantPayload,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// A trait method signature (no body): `function equals(self, other: Self): Bool`
#[derive(serde::Serialize, Debug)]
pub struct TraitMethodSignature {
    pub name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub where_clause: Vec<TraitConstraint>,
    /// Default implementation body (trait-design-appendix §4). Implementors
    /// may omit a member whose trait declaration carries a body.
    pub body: Option<Expr>,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// An associated type declaration in a trait: `type Foo` or `type Foo<T>`
#[derive(serde::Serialize, Debug)]
pub struct AssociatedTypeDecl {
    pub name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// An associated type definition in an implement block: `type Foo = ConcreteType` or `type Foo<T> = Array<T>`
#[derive(serde::Serialize, Debug)]
pub struct AssociatedTypeDef {
    pub name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    pub type_expr: TypeExpr,
    pub span: Span,
}

/// A trait declaration: `trait Equatable = function equals(self, other: Self): Bool`
///
/// Also used for interface declarations (`interface Drawable = ...`): an
/// interface is a trait that passes the object-safety check and may appear in
/// type position. `is_interface` records which keyword introduced the decl.
#[derive(serde::Serialize, Debug)]
pub struct TraitDecl {
    pub visibility: Visibility,
    pub name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    /// Super traits/interfaces from the `extends` clause, in declaration
    /// order: `trait B extends A and C`.
    pub supers: Vec<NamedType>,
    pub methods: Vec<TraitMethodSignature>,
    pub properties: Vec<PropertyDecl>,
    pub associated_types: Vec<AssociatedTypeDecl>,
    pub is_interface: bool,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// An implement block: `implement Trait for Type = methods...`
#[derive(serde::Serialize, Debug)]
pub struct ImplementDecl {
    pub trait_name: Spanned<String>,
    pub type_params: Vec<Spanned<String>>,
    pub trait_type_args: Vec<TypeExpr>,
    pub for_type: TypeExpr,
    pub where_clause: Vec<TraitConstraint>,
    pub methods: Vec<FunctionDecl>,
    pub properties: Vec<PropertyDecl>,
    pub associated_types: Vec<AssociatedTypeDef>,
    pub doc_comment: Option<String>,
    pub span: Span,
}

/// A field initializer in a record construction expression.
#[derive(serde::Serialize, Debug, Clone)]
pub struct FieldInit {
    pub name: Spanned<String>,
    pub value: Box<Expr>,
    pub span: Span,
}

/// A field pattern in a record pattern match.
#[derive(serde::Serialize, Debug, Clone)]
pub struct FieldPattern {
    pub name: Spanned<String>,
    pub pattern: Option<Pattern>, // None = bare IDENT shorthand (bind field to same-name variable)
    pub span: Span,
}

/// A type expression in the AST (unresolved).
#[derive(serde::Serialize, Debug, Clone)]
pub enum TypeExpr {
    Named(NamedType),
    Tuple(Vec<TypeExpr>, Span),
    TupleExtend(Box<TypeExpr>, Box<TypeExpr>, Span),
    /// Intersection type: `Display and Equatable`
    Intersection(Vec<NamedType>),
    /// Function type: `Int32 => Bool` or `(Int32, String) => Bool`
    Function(Vec<TypeExpr>, Box<TypeExpr>, Span),
}

impl TypeExpr {
    pub fn span(&self) -> Span {
        match self {
            TypeExpr::Named(n) => n.span.clone(),
            TypeExpr::Tuple(_, span) | TypeExpr::TupleExtend(_, _, span) => span.clone(),
            TypeExpr::Intersection(types) => {
                let first = types.first().unwrap().span.clone();
                let last = types.last().unwrap().span.clone();
                first.merge(&last)
            }
            TypeExpr::Function(_, _, span) => span.clone(),
        }
    }
}

/// A named type reference like `Unit`, `com.example.MyType`, or `Box<Int32>`.
#[derive(serde::Serialize, Debug, Clone)]
pub struct NamedType {
    pub name: Spanned<String>,
    pub type_args: Vec<TypeExpr>,
    pub span: Span,
}

/// A trait application in a bound, with optional associated-type equalities.
#[derive(serde::Serialize, Debug, Clone)]
pub struct NamedTraitBound {
    pub name: Spanned<String>,
    pub type_args: Vec<TypeExpr>,
    pub associated_types: Vec<(Spanned<String>, TypeExpr)>,
    pub span: Span,
}

/// An expression in the AST (untyped).
#[derive(serde::Serialize, Debug, Clone)]
pub enum Expr {
    /// The unit literal `()`.
    UnitLiteral(Span),
    /// A boolean literal `true` or `false`.
    BoolLiteral(bool, Span),
    /// A string literal `"hello"`.
    StringLiteral(String, Span),
    /// A character literal `'A'`.
    CharLiteral(char, Span),
    /// Signed integer literals.
    Int8Literal(i8, Span),
    Int16Literal(i16, Span),
    Int32Literal(i32, Span),
    Int64Literal(i64, Span),
    /// Unsigned integer literals.
    Uint8Literal(u8, Span),
    Uint16Literal(u16, Span),
    Uint32Literal(u32, Span),
    Uint64Literal(u64, Span),
    Uint128Literal(u128, Span),
    /// Exact library numeric literal, including its suffix and optional sign.
    ExactNumberLiteral(String, Span),
    /// Floating-point literals.
    Float32Literal(f32, Span),
    Float64Literal(f64, Span),
    /// A binary operation: `a + b`, `x == y`, etc.
    BinaryOp {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// A unary operation: `-x`, `!b`, `~n`.
    UnaryOp {
        op: UnaryOp,
        operand: Box<Expr>,
        span: Span,
    },
    /// A block of expressions (from Begin ... End).
    Block(BlockExpr),
    /// `panic "message"`
    Panic {
        message: Box<Expr>,
        span: Span,
    },
    /// `assert condition` or `assert condition, "message"`
    Assert {
        condition: Box<Expr>,
        message: Option<Box<Expr>>,
        span: Span,
    },
    /// A let binding: `let x = 5` or `let mutable x: Int32 = 5`
    Let {
        name: Spanned<String>,
        mutable: bool,
        type_annotation: Option<TypeExpr>,
        value: Box<Expr>,
        span: Span,
    },
    /// A variable reference: `x`
    Identifier(String, Span),
    /// An assignment: `x = 5`
    Assignment {
        target: Box<Expr>,
        value: Box<Expr>,
        span: Span,
    },
    /// A named argument, valid only directly inside a call argument list.
    NamedArgument {
        name: Spanned<String>,
        value: Box<Expr>,
        span: Span,
    },
    /// A function call: `add(1, 2)` or `identity<Int32>(42)`
    FunctionCall {
        name: Spanned<String>,
        type_args: Vec<TypeExpr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// Field access: `obj.field`, `obj.prop<T>`, `Box<Int32>.count`, `Box<Int32>.zero<Bool>`.
    /// `object_type_params` are type args on the receiver (e.g., `<Int32>` in `Box<Int32>.count`).
    /// `field_type_params` are type args on the property (e.g., `<Bool>` in `obj.zero<Bool>`).
    FieldAccess {
        object: Box<Expr>,
        object_type_params: Vec<TypeExpr>,
        field: Spanned<String>,
        field_type_params: Vec<TypeExpr>,
        span: Span,
    },
    /// Method/qualified call: `obj.method(args)`, `Array<Int32>.empty()` — parsed as postfix
    MethodCall {
        receiver: Box<Expr>,
        method: Spanned<String>,
        receiver_type_args: Vec<TypeExpr>,
        type_args: Vec<TypeExpr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// An if-else expression: `if cond then a else b`
    If {
        condition: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Option<Box<Expr>>,
        span: Span,
    },
    /// A while loop: `while condition do body`
    While {
        condition: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    /// `break` — exits the innermost loop
    Break(Span),
    /// `continue` — skips to the next iteration of the innermost loop
    Continue(Span),
    /// A match expression: `match expr with case pat => body ...`
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    /// Record construction: `Point { x = 1; y = 2 }` or `Box<Int32> { value = 42 }`
    RecordCreate {
        type_name: Spanned<String>,
        type_args: Vec<TypeExpr>,
        fields: Vec<FieldInit>,
        span: Span,
    },
    /// Record with expression: `point with { x = 10 }`
    RecordWith {
        object: Box<Expr>,
        fields: Vec<FieldInit>,
        span: Span,
    },
    /// Enum variant record-style construction: `Shape.Point { x = 1, y = 2 }`
    EnumVariantRecordCreate {
        type_name: Spanned<String>,
        variant_name: Spanned<String>,
        fields: Vec<FieldInit>,
        span: Span,
    },
    /// Array literal: `[| 1, 2, 3 |]` or `[||]`
    ArrayLiteral {
        elements: Vec<Expr>,
        span: Span,
    },
    /// List literal: `[1, 2, 3]`, `[]`, or a cons chain `h :: t`.
    ///
    /// `tail` is `Some` only when the source used `::` and the right operand
    /// was not itself a list literal: `h :: t` is `{elements: [h], tail: t}`,
    /// while `a :: b :: []` flattens to `{elements: [a, b], tail: None}` — the
    /// same node `[a, b]` produces. Keeping both forms in one node is what lets
    /// them share an element-type join; desugaring `::` straight to
    /// `List.Cons` instead would bind the element type from the head alone and
    /// reject a widening tail (`dog :: animals`).
    ListLiteral {
        elements: Vec<Expr>,
        tail: Option<Box<Expr>>,
        span: Span,
    },
    /// Array index access: `arr[i]`
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// Bounded array/slice view: `arr[|start..end|]`.
    SliceIndex {
        object: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        inclusive: bool,
        span: Span,
    },
    /// Type test: `expr is Type` — returns Bool
    TypeTest {
        expr: Box<Expr>,
        target: TypeExpr,
        span: Span,
    },
    /// Type cast: `expr as Type` — returns T, traps on failure
    TypeCast {
        expr: Box<Expr>,
        target: TypeExpr,
        span: Span,
    },
    /// A tuple literal: `(1, true)`, `(10, 20, 30)`.
    TupleLiteral {
        elements: Vec<Expr>,
        span: Span,
    },
    /// A destructuring let binding: `let (x, y) = expr`
    LetDestructure {
        pattern: Pattern,
        type_annotation: Option<TypeExpr>,
        value: Box<Expr>,
        span: Span,
    },
    /// An intrinsic body: `= intrinsic`
    Intrinsic(Span),
    /// `try expr` — early return on failure (prefix form)
    Try {
        operand: Box<Expr>,
        span: Span,
    },
    /// `await expr` — extract inner value T from Awaitable<T>
    Await {
        operand: Box<Expr>,
        span: Span,
    },
    /// `use expr` — scoped resource acquisition. Captures rest of block as continuation.
    Use {
        operand: Box<Expr>,
        span: Span,
    },
    /// `expr.orReturn` — early return on failure (postfix form)
    OrReturn {
        operand: Box<Expr>,
        span: Span,
    },
    /// A deferred Awaitable computation: `async do body`.
    AsyncDo {
        body: Box<Expr>,
        span: Span,
    },
    /// A closure expression: `x => x + 1` or `(x: Int32, y: Int32) => x + y`
    Closure {
        is_async: bool,
        params: Vec<ClosureParam>,
        body: Box<Expr>,
        span: Span,
    },
    /// A for loop: `for pattern in iterable do body`
    For {
        pattern: Pattern,
        iterable: Box<Expr>,
        body: Box<Expr>,
        span: Span,
    },
    /// A prefixed string literal: `sql"SELECT ... $x ... $..xs"`.
    ///
    /// Lowered during inference to a builder-call chain on the type registered
    /// for `prefix` by `[[project.literal]]`. The compiler knows only the three
    /// part shapes; what they mean is the builder's business.
    PrefixedLiteral {
        prefix: Spanned<String>,
        parts: Vec<LiteralPart>,
        span: Span,
    },
    /// A type or module named by resolved FQN rather than by source name.
    ///
    /// Only ever produced by compiler lowering — there is no source syntax for
    /// it, because type names in the grammar are a single identifier. It exists
    /// so a lowering can name a type the user never imported.
    ResolvedTypeRef(Fqn, Span),
}

/// One part of a prefixed string literal, after sub-parsing.
#[derive(serde::Serialize, Debug, Clone)]
pub enum LiteralPart {
    /// Literal text between interpolations.
    Text(String, Span),
    /// `$ident` / `${expr}` — lowered to `builder.value(expr)`.
    Value(Expr, Span),
    /// `$..ident` / `$..{expr}` — lowered to `builder.spread(expr)`.
    Spread(Expr, Span),
}

impl LiteralPart {
    pub fn span(&self) -> &Span {
        match self {
            LiteralPart::Text(_, span)
            | LiteralPart::Value(_, span)
            | LiteralPart::Spread(_, span) => span,
        }
    }
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::UnitLiteral(span)
            | Expr::BoolLiteral(_, span)
            | Expr::StringLiteral(_, span)
            | Expr::CharLiteral(_, span)
            | Expr::Int8Literal(_, span)
            | Expr::Int16Literal(_, span)
            | Expr::Int32Literal(_, span)
            | Expr::Int64Literal(_, span)
            | Expr::Uint8Literal(_, span)
            | Expr::Uint16Literal(_, span)
            | Expr::Uint32Literal(_, span)
            | Expr::Uint64Literal(_, span)
            | Expr::Uint128Literal(_, span)
            | Expr::ExactNumberLiteral(_, span)
            | Expr::Float32Literal(_, span)
            | Expr::Float64Literal(_, span)
            | Expr::BinaryOp { span, .. }
            | Expr::UnaryOp { span, .. }
            | Expr::Panic { span, .. }
            | Expr::Assert { span, .. }
            | Expr::Let { span, .. }
            | Expr::Identifier(_, span)
            | Expr::Assignment { span, .. }
            | Expr::NamedArgument { span, .. }
            | Expr::FunctionCall { span, .. }
            | Expr::FieldAccess { span, .. }
            | Expr::MethodCall { span, .. }
            | Expr::If { span, .. }
            | Expr::While { span, .. }
            | Expr::Break(span)
            | Expr::Continue(span)
            | Expr::Match { span, .. }
            | Expr::RecordCreate { span, .. }
            | Expr::RecordWith { span, .. }
            | Expr::EnumVariantRecordCreate { span, .. }
            | Expr::ArrayLiteral { span, .. }
            | Expr::ListLiteral { span, .. }
            | Expr::TupleLiteral { span, .. }
            | Expr::LetDestructure { span, .. }
            | Expr::Index { span, .. }
            | Expr::SliceIndex { span, .. }
            | Expr::TypeTest { span, .. }
            | Expr::TypeCast { span, .. }
            | Expr::Intrinsic(span)
            | Expr::Try { span, .. }
            | Expr::Await { span, .. }
            | Expr::Use { span, .. }
            | Expr::OrReturn { span, .. }
            | Expr::Closure { span, .. }
            | Expr::AsyncDo { span, .. }
            | Expr::For { span, .. }
            | Expr::PrefixedLiteral { span, .. }
            | Expr::ResolvedTypeRef(_, span) => span.clone(),
            Expr::Block(block) => block.span.clone(),
        }
    }
}

/// A block expression containing a sequence of expressions.
#[derive(serde::Serialize, Debug, Clone)]
pub struct BlockExpr {
    pub expressions: Vec<Expr>,
    pub span: Span,
}

/// A pattern in a match expression.
#[derive(serde::Serialize, Debug, Clone)]
pub enum Pattern {
    /// The wildcard pattern `_`.
    Wildcard(Span),
    /// A literal pattern (int, float, bool, string).
    Literal(Box<Expr>, Span),
    /// A variable binding pattern.
    Variable(String, Span),
    /// A type-annotated pattern: `case b: Box<Int32> =>`.
    TypeAnnotated {
        binding: String,
        type_expr: TypeExpr,
        span: Span,
    },
    /// A record destructuring pattern: `Point { x, y = 0 }` or `Box<Int32> { value = v }`.
    Record {
        type_name: Spanned<String>,
        type_params: Vec<TypeExpr>,
        fields: Vec<FieldPattern>,
        span: Span,
    },
    /// A no-payload enum variant pattern: `Color.Red` or bare `None`.
    EnumVariant {
        type_name: Spanned<String>,
        variant_name: Spanned<String>,
        span: Span,
    },
    /// A tuple-style enum variant pattern: `Shape.Circle(r)` or bare `Some(x)`.
    EnumVariantTuple {
        type_name: Spanned<String>,
        variant_name: Spanned<String>,
        payload_patterns: Vec<Pattern>,
        span: Span,
    },
    /// A record-style enum variant pattern: `Shape.Point { x = px, y = py }`.
    EnumVariantRecord {
        type_name: Spanned<String>,
        variant_name: Spanned<String>,
        fields: Vec<FieldPattern>,
        span: Span,
    },
    /// A tuple destructuring pattern: `(x, y)`, `(a, _, c)`, `((a, b), c)`.
    Tuple(Vec<Pattern>, Span),
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Wildcard(span)
            | Pattern::Literal(_, span)
            | Pattern::Variable(_, span)
            | Pattern::TypeAnnotated { span, .. }
            | Pattern::Record { span, .. }
            | Pattern::EnumVariant { span, .. }
            | Pattern::EnumVariantTuple { span, .. }
            | Pattern::EnumVariantRecord { span, .. }
            | Pattern::Tuple(_, span) => span.clone(),
        }
    }
}

/// A single arm in a match expression.
#[derive(serde::Serialize, Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Box<Expr>>,
    pub body: Box<Expr>,
    pub span: Span,
}
