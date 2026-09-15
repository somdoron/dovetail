mod associated_normalization;

use std::collections::{BTreeMap, BTreeSet};

use crate::common::span::{FilePath, Span};
use crate::common::types::{
    Fqn, MangledName, PackagePath, SymbolName, TypeParamName, Variance,
    Visibility,
};
use crate::parser::ast::Expr;

use super::types::{TraitBounds, Type};

/// Check if a symbol with the given visibility is accessible from the caller's context.
pub fn is_accessible(
    visibility: Visibility,
    symbol_package: &PackagePath,
    caller_package: &PackagePath,
    symbol_file: &FilePath,
    caller_file: &FilePath,
) -> bool {
    match visibility {
        Visibility::Public | Visibility::Protected => true,
        Visibility::Internal => *symbol_package == *caller_package,
        Visibility::Private => *symbol_file == *caller_file,
    }
}

/// Package-only variant for lookups that don't have file info.
/// Treats Private like Internal (same-package accessible) since file info is unavailable.
fn is_package_accessible(
    visibility: Visibility,
    symbol_package: &PackagePath,
    caller_package: &PackagePath,
) -> bool {
    match visibility {
        Visibility::Public | Visibility::Protected => true,
        Visibility::Internal | Visibility::Private => *symbol_package == *caller_package,
    }
}

/// A function signature: parameter types, return type, visibility, and mangled name.
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub visibility: Visibility,
    pub mangled_name: MangledName,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    /// The file where the function was declared.
    pub source_file: FilePath,
    /// Whether this function has an intrinsic body (`= intrinsic`).
    pub is_intrinsic: bool,
    /// Whether this is a property (accessed without parens via field-access syntax).
    pub is_property: bool,
    /// Whether this method is marked `final` (class methods only).
    pub is_final_method: bool,
    /// Whether this method is marked `abstract` (class methods only).
    pub is_abstract_method: bool,
}

impl FunctionSignature {
    /// Check whether the given argument types match this signature's parameters.
    /// Uses the provided `is_assignable` predicate for type compatibility checks.
    pub fn matches_args(
        &self,
        arg_types: &[&Type],
        is_assignable: impl Fn(&Type, &Type) -> bool,
    ) -> bool {
        Self::params_match_args(&self.params, arg_types, is_assignable)
    }

    /// Check whether the given argument types match a param list.
    pub fn params_match_args(
        params: &[(String, Type)],
        arg_types: &[&Type],
        is_assignable: impl Fn(&Type, &Type) -> bool,
    ) -> bool {
        params.len() == arg_types.len()
            && params
                .iter()
                .zip(arg_types.iter())
                .all(|((_, param_ty), arg_ty)| is_assignable(param_ty, arg_ty))
    }
}

/// A global variable signature: type, visibility, mutability, and mangled name.
#[derive(Debug, Clone)]
pub struct GlobalSignature {
    pub visibility: Visibility,
    pub mangled_name: MangledName,
    pub ty: Type,
    pub mutable: bool,
    /// The file where the global was declared.
    pub source_file: FilePath,
}

/// Information about a record type: field names, types, and visibility.
#[derive(Debug, Clone)]
pub struct RecordTypeSignature {
    pub visibility: Visibility,
    pub construction_private: bool,
    pub fqn: Fqn,
    /// Whether this declaration carries `@stringLiteral`, making its name the
    /// prefix of a prefixed string literal (`sql"..."`).
    pub is_string_literal: bool,
    pub type_params: Vec<TypeParamName>,
    /// Variance per type parameter (parallel to `type_params`).
    pub type_param_variances: Vec<Variance>,
    pub fields: Vec<(String, Type)>,
    /// Trait bounds from where clause on generic records.
    pub trait_bounds: TraitBounds,
    /// The file where the record was declared.
    pub source_file: FilePath,
    /// The span of the record declaration.
    pub span: Span,
}

/// The payload form of an enum variant in the registry.
#[derive(Debug, Clone)]
pub enum VariantPayload {
    None,
    Tuple(Vec<Type>),
    Record(Vec<(String, Type)>),
}

/// Information about an enum type: variants with payload types, and visibility.
#[derive(Debug, Clone)]
pub struct EnumTypeSignature {
    pub visibility: Visibility,
    pub construction_private: bool,
    pub fqn: Fqn,
    pub type_params: Vec<TypeParamName>,
    /// Variance per type parameter (parallel to `type_params`).
    pub type_param_variances: Vec<Variance>,
    pub variants: Vec<(String, VariantPayload)>,
    /// Trait bounds from where clause on generic enums.
    pub trait_bounds: TraitBounds,
    pub source_file: FilePath,
    /// The span of the enum declaration.
    pub span: Span,
}

/// Information about a type alias: expanded type and visibility.
#[derive(Debug, Clone)]
pub struct TypeAliasSignature {
    pub visibility: Visibility,
    pub fqn: Fqn,
    /// Whether this declaration carries `@stringLiteral`, making its name the
    /// prefix of a prefixed string literal (`sql"..."`).
    pub is_string_literal: bool,
    pub type_params: Vec<TypeParamName>,
    pub trait_bounds: TraitBounds,
    pub expanded_type: Type,
    pub source_file: FilePath,
}

/// Information about a newtype: inner type and visibility.
#[derive(Debug, Clone)]
pub struct NewtypeSignature {
    pub visibility: Visibility,
    pub fqn: Fqn,
    pub inner_private: bool,
    pub inner_type: Type,
    /// Type parameters for generic newtypes (empty for non-generic newtypes).
    pub type_params: Vec<TypeParamName>,
    /// Variance per type parameter (parallel to `type_params`).
    pub type_param_variances: Vec<Variance>,
    /// Trait bounds from where clause (for generic newtypes).
    pub trait_bounds: TraitBounds,
    pub source_file: FilePath,
    /// The span of the newtype declaration.
    pub span: Span,
}

/// Information about a class type.
#[derive(Debug, Clone)]
pub struct ClassTypeSignature {
    pub visibility: Visibility,
    pub is_final: bool,
    /// Whether this declaration carries `@stringLiteral`, making its name the
    /// prefix of a prefixed string literal (`sql"..."`).
    pub is_string_literal: bool,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub fqn: Fqn,
    pub parent_class: Option<Fqn>,
    pub constructor_visibility: Visibility,
    /// All fields in physical order: constructor params first, then let bindings.
    pub fields: Vec<ClassFieldInfo>,
    /// Instance methods (have `self`): name → overloads.
    pub instance_methods: BTreeMap<SymbolName, Vec<FunctionSignature>>,
    /// Static methods (no `self`): name → overloads.
    pub static_methods: BTreeMap<SymbolName, Vec<FunctionSignature>>,
    /// Generic instance methods (methods with their own type params): name → defs.
    pub generic_instance_methods: BTreeMap<SymbolName, Vec<GenericClassMethodDef>>,
    /// Generic static methods (methods with their own type params): name → defs.
    pub generic_static_methods: BTreeMap<SymbolName, Vec<GenericClassMethodDef>>,
    /// Generic static globals (let static bindings on generic classes): name → def.
    pub generic_static_globals: BTreeMap<SymbolName, GenericClassStaticGlobalDef>,
    pub source_file: FilePath,
    pub span: Span,
    /// Own constructor parameters with name, type, visibility, and mutability.
    pub constructor_params: Vec<ConstructorParam>,
    /// Type parameters for generic classes (empty for non-generic classes).
    pub type_params: Vec<TypeParamName>,
    /// Variance per type parameter (parallel to `type_params`).
    pub type_param_variances: Vec<Variance>,
    /// Trait bounds from where clause (for generic classes).
    pub trait_bounds: TraitBounds,
    /// Class body members (let bindings and expressions) for generic classes.
    /// Stored in declaration order for deferred inference during instantiation.
    pub body_members: Vec<ClassBodyMemberDef>,
    /// Pre-resolved parent type (with TypeParameter placeholders) for generic classes with extends.
    pub parent_type_expr: Option<Type>,
    /// Extends args AST (for generic classes with extends — re-inferred during instantiation).
    pub extends_args_ast: Vec<Expr>,
    /// Traits implemented by this class: (trait_fqn, trait_type_args).
    pub trait_impls: Vec<(Fqn, Vec<Type>)>,
    /// Members the class omitted and that a trait's DEFAULT body supplies:
    /// member name → the trait that supplies it for legacy single-member
    /// default collection. Overloaded defaults are matched by their signatures
    /// and source applications before registration.
    pub default_supplied_members: BTreeMap<SymbolName, Fqn>,
}

/// A body member definition inside a generic class. Preserves declaration order
/// for deferred inference during instantiation.
#[derive(Debug, Clone)]
pub enum ClassBodyMemberDef {
    LetBinding(ClassLetBindingDef),
    StaticLetBinding(ClassLetBindingDef),
    Expression(Expr),
}

/// A let binding definition inside a generic class. Stores AST body for deferred inference.
#[derive(Debug, Clone)]
pub struct ClassLetBindingDef {
    pub name: String,
    /// Pre-resolved type (with TypeParameter placeholders) from collect phase.
    pub resolved_type: Option<Type>,
    pub body: Expr,
    pub visibility: Visibility,
    pub mutable: bool,
}

/// Signature of a method on a generic class. Bodies are inferred as template
/// TypedFunctions during `infer_generic_class_declaration`; this struct holds
/// only the signature and metadata needed for overload resolution and rules.
#[derive(Debug, Clone)]
pub struct GenericClassMethodDef {
    pub visibility: Visibility,
    /// Class-level type params (empty for non-generic classes).
    pub class_type_params: Vec<TypeParamName>,
    /// Method's own type params.
    pub method_type_params: Vec<TypeParamName>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    /// Merged bounds (class + method).
    pub trait_bounds: TraitBounds,
    pub is_final_method: bool,
    pub is_abstract_method: bool,
    /// Whether this is a property (accessed without parens via field-access syntax).
    pub is_property: bool,
    /// Whether this method was declared with the `async` modifier.
    pub is_async: bool,
}

/// A static global (let static) defined inside a generic class.
/// Body is stored as AST for deferred inference at instantiation time.
#[derive(Debug, Clone)]
pub struct GenericClassStaticGlobalDef {
    pub visibility: Visibility,
    pub type_params: Vec<TypeParamName>,
    pub ty: Option<Type>,
    pub mutable: bool,
    pub body: Expr,
    pub source_file: FilePath,
    pub package: PackagePath,
}

/// A constructor parameter definition.
#[derive(Debug, Clone)]
pub struct ConstructorParam {
    pub name: String,
    pub ty: Type,
    pub visibility: Visibility,
    pub mutable: bool,
}

/// A field in a class (constructor parameter or let binding).
#[derive(Debug, Clone)]
pub struct ClassFieldInfo {
    pub name: String,
    pub ty: Type,
    pub visibility: Visibility,
    pub mutable: bool,
}


/// A resolved trait method signature (types are resolved, not AST TypeExpr).
#[derive(Debug, Clone)]
pub struct TraitMethodSig {
    pub name: String,
    pub type_params: Vec<TypeParamName>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub trait_bounds: TraitBounds,
    pub span: Span,
    /// `None` = declared on this trait. `Some((A, args))` = inherited via
    /// `extends`: the ultimately-declaring trait and its type args as seen
    /// from this trait (substituted through the super chain; `Self` stays
    /// `Self`). Member types in `params`/`return_type` are likewise
    /// substituted; vtable slot identity must come from the origin trait's
    /// *raw* signature, never from this substituted copy.
    pub origin: Option<(Fqn, Vec<Type>)>,
    /// `Some(T)` when the member has a default body, declared by trait `T`
    /// (this trait, or — through `extends` — the nearest overriding trait in
    /// the chain). The default template is keyed
    /// `MangledName::for_trait_default(T, member)`.
    pub default_source: Option<Fqn>,
}

/// A resolved trait property signature.
#[derive(Debug, Clone)]
pub struct TraitPropertySig {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub span: Span,
    /// See `TraitMethodSig::origin`.
    pub origin: Option<(Fqn, Vec<Type>)>,
    /// See `TraitMethodSig::default_source`.
    pub default_source: Option<Fqn>,
}

/// A signature for an associated type declared in a trait.
#[derive(Debug, Clone)]
pub struct AssociatedTypeSig {
    pub name: String,
    pub span: Span,
    pub type_params: Vec<TypeParamName>,
    /// See `TraitMethodSig::origin`.
    pub origin: Option<(Fqn, Vec<Type>)>,
}

/// One super reference from a trait's `extends` clause.
#[derive(Debug, Clone)]
pub struct TraitSuperRef {
    pub fqn: Fqn,
    /// The super's type args in terms of the extender's type params.
    pub type_args: Vec<Type>,
    /// The super's spot in the local `extends` clause (for diagnostics on
    /// inherited members, whose own spans live in other files).
    pub span: Span,
}

/// A trait definition registered in the registry.
#[derive(Debug, Clone)]
pub struct TraitSignature {
    pub visibility: Visibility,
    pub fqn: Fqn,
    pub type_params: Vec<TypeParamName>,
    /// Direct supers from the `extends` clause, declaration order.
    pub supers: Vec<TraitSuperRef>,
    /// Transitive super closure, substituted into this trait's type params,
    /// deduped by (fqn, args). Populated by trait flattening (Pass 1b).
    pub super_closure: Vec<(Fqn, Vec<Type>)>,
    /// Flattened member sets: own members plus all supers' members
    /// (substituted), each tagged with its `origin`.
    pub methods: Vec<TraitMethodSig>,
    /// Dispatch identities captured before concrete alias normalization.
    /// Parallel to methods; empty while inheritance is being collected.
    pub(crate) method_dispatch_names: Vec<SymbolName>,
    pub properties: Vec<TraitPropertySig>,
    pub associated_types: Vec<AssociatedTypeSig>,
    /// Declared with the `interface` keyword: object-safety is checked at the
    /// declaration and the name may appear in type position (interface object).
    pub is_interface: bool,
    pub source_file: FilePath,
    pub span: Span,
}

impl TraitSignature {
    pub(crate) fn method_dispatch_name(&self, method: &TraitMethodSig) -> SymbolName {
        if self.method_dispatch_names.len() == self.methods.len() {
            let index = self.methods.iter().position(|candidate| std::ptr::eq(candidate, method))
                .or_else(|| self.methods.iter().position(|candidate|
                    candidate.name == method.name && candidate.origin == method.origin
                        && same_method_parameters(candidate, method)));
            if let Some(index) = index { return self.method_dispatch_names[index].clone(); }
        }
        let mut signatures: Vec<&TraitMethodSig> = Vec::new();
        for candidate in self.methods.iter().filter(|candidate| candidate.name == method.name) {
            if !signatures.iter().any(|previous| same_method_parameters(previous, candidate)) {
                signatures.push(candidate);
            }
        }
        if signatures.len() == 1 { return SymbolName(method.name.clone()); }
        let index = signatures.iter().position(|candidate| same_method_parameters(candidate, method)).unwrap();
        SymbolName(format!("{}$overload{}", method.name, index))
    }
}

/// Instantiate enclosing parameters without capturing them in method binders.
/// The hidden method names cannot collide with a source-level trait parameter.
pub(crate) fn instantiate_trait_method(
    method: &TraitMethodSig,
    enclosing: &BTreeMap<TypeParamName, Type>,
) -> TraitMethodSig {
    let parameters: Vec<_> = method.type_params.iter().enumerate()
        .map(|(index, _)| TypeParamName(format!("$traitMethod${index}"))).collect();
    let mut substitution = enclosing.clone();
    substitution.extend(method.type_params.iter().cloned().zip(parameters.iter()
        .map(|name| Type::TypeVariable(name.clone(), vec![]))));
    let mut instantiated = method.clone();
    instantiated.params = method.params.iter().map(|(name, ty)| (
        name.clone(), super::collect::substitute_trait_type_params(ty, &substitution),
    )).collect();
    instantiated.return_type = super::collect::substitute_trait_type_params(
        &method.return_type, &substitution,
    );
    instantiated.trait_bounds = super::collect::rename_method_bounds(
        &method.trait_bounds, &method.type_params, &parameters, enclosing,
    );
    instantiated.type_params = parameters;
    instantiated
}

/// Compare method-local parameters independently of their source spelling.
/// Trait parameters retain their identity; only the method's binders are renamed.
pub(crate) fn canonical_method_signature(method: &TraitMethodSig) -> (Vec<(String, Type)>, Type) {
    let substitution: BTreeMap<_, _> = method.type_params.iter().enumerate()
        .map(|(index, name)| (name.clone(), Type::TypeVariable(
            TypeParamName(format!("$traitMethod${index}")), vec![],
        )))
        .collect();
    let parameters = method.params.iter().map(|(name, ty)| (
        name.clone(), super::collect::substitute_trait_type_params(ty, &substitution),
    )).collect();
    let result = super::collect::substitute_trait_type_params(&method.return_type, &substitution);
    (parameters, result)
}

pub(crate) fn same_method_parameters(left: &TraitMethodSig, right: &TraitMethodSig) -> bool {
    if left.type_params.len() != right.type_params.len() || left.params.len() != right.params.len() {
        return false;
    }
    let (left, _) = canonical_method_signature(left);
    let (right, _) = canonical_method_signature(right);
    left.iter().zip(&right).all(|((left_name, left), (right_name, right))| {
        (left_name == "self") == (right_name == "self") && super::subtyping::identical(left, right)
    })
}

/// Implement block signature — all methods stored with raw AST bodies.
/// Lives in `Registry.implement_blocks`. Inference type-checks the bodies to produce `TypedImplementBlock`.
#[derive(Debug, Clone)]
pub struct ImplBlockSignature {
    pub trait_fqn: Fqn,
    pub type_fqn: Fqn,
    pub for_type: Type,
    pub type_params: Vec<TypeParamName>,
    pub trait_type_args: Vec<Type>,
    pub trait_bounds: TraitBounds,
    pub methods: Vec<ImplMethodSignature>,
    pub properties: Vec<ImplMethodSignature>,
    pub associated_type_defs: BTreeMap<String, (Vec<TypeParamName>, Type)>,
    pub span: Span,
    pub source_file: FilePath,
    pub package: PackagePath,
}

/// Method or property signature within an implement block.
#[derive(Debug, Clone)]
pub struct ImplMethodSignature {
    pub name: SymbolName,
    pub dispatch_name: SymbolName,
    pub visibility: Visibility,
    pub method_type_params: Vec<TypeParamName>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub span: Span,
    pub is_async: bool,
    pub is_intrinsic: bool,
    pub is_property: bool,
    pub trait_bounds: TraitBounds,
    /// Synthesized from the trait member's default body (the impl block omits
    /// the member): the function is materialized from the default template at
    /// monomorphize, under the standard impl-member name.
    pub is_default: bool,
}

/// Extension block signature — unified for both generic and non-generic named extensions.
/// Mirrors `ImplBlockSignature`. Lives in `Registry.extension_blocks`.
#[derive(Debug, Clone)]
pub struct ExtensionBlockSignature {
    pub ext_fqn: Fqn,
    pub for_type: Type,
    pub type_params: Vec<TypeParamName>,
    pub trait_bounds: TraitBounds,
    pub methods: Vec<ExtMethodSignature>,
    pub properties: Vec<ExtMethodSignature>,
    pub span: Span,
    pub source_file: FilePath,
    pub package: PackagePath,
}

/// Method or property signature within an extension block. Lightweight — no AST body.
#[derive(Debug, Clone)]
pub struct ExtMethodSignature {
    pub name: SymbolName,
    pub visibility: Visibility,
    pub method_type_params: Vec<TypeParamName>,
    pub trait_bounds: TraitBounds,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub is_property: bool,
    pub is_intrinsic: bool,
    pub is_async: bool,
    pub span: Span,
}

/// A generic function definition: stores type parameters, parameter types (may contain TypeParameter),
/// return type, and the full AST body for specialization.
#[derive(Debug, Clone)]
pub struct GenericFunctionDef {
    pub visibility: Visibility,
    pub type_params: Vec<TypeParamName>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: Expr,
    pub span: Span,
    /// Container name (module or class) used to set `container_name` during
    /// body inference so bare name resolution and private field access work.
    pub container_name: Option<String>,
    /// Trait bounds from where clause.
    pub trait_bounds: TraitBounds,
    /// Whether this function was declared with the `async` modifier.
    pub is_async: bool,
    /// Whether the body is `intrinsic` (resolved specially in inference).
    pub is_intrinsic: bool,
}

/// A generic function or property defined inside a generic module.
/// Body is stored as AST for deferred inference at instantiation time.
#[derive(Debug, Clone)]
pub struct GenericModuleMemberDef {
    pub visibility: Visibility,
    pub type_params: Vec<TypeParamName>,
    pub for_type: Type,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: Expr,
    pub is_intrinsic: bool,
    pub is_property: bool,
    pub source_file: FilePath,
    pub package: PackagePath,
    /// Method's own type parameters (from FunctionDecl), separate from module-level type_params.
    pub method_type_params: Vec<TypeParamName>,
    /// Whether this method was declared with the `async` modifier.
    pub is_async: bool,
    /// Bounds from this member's own `where` clause, resolved over the module's
    /// type params AND the member's own — because a member of a generic module
    /// may constrain either (`function run(self): Unit where E: Display` in
    /// `module Async<T, E>` constrains the module's).
    ///
    /// Separate from the module's own bounds, which apply to every member: these
    /// are the ones the call site must check in addition, and dropping them is
    /// not a missing diagnostic but a miscompile — the call type-checks, and the
    /// unsatisfied bound surfaces as `ImplFunctionCall not resolved by
    /// monomorphize` when codegen looks for an impl that was never required.
    pub trait_bounds: TraitBounds,
}

/// A global variable defined inside a generic module.
/// Body is stored as AST for deferred inference at instantiation time.
#[derive(Debug, Clone)]
pub struct GenericModuleGlobalDef {
    pub visibility: Visibility,
    pub type_params: Vec<TypeParamName>,
    pub ty: Type,
    pub mutable: bool,
    pub body: Expr,
    pub source_file: FilePath,
    pub package: PackagePath,
}

/// Keyed collection of generic module members by name.
#[derive(Debug, Clone, Default)]
pub struct GenericModuleMembers(BTreeMap<SymbolName, Vec<GenericModuleMemberDef>>);

impl GenericModuleMembers {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    pub fn add(&mut self, member_name: SymbolName, def: GenericModuleMemberDef) {
        self.0.entry(member_name).or_default().push(def);
    }

    pub fn lookup(&self, member_name: &SymbolName) -> Option<&[GenericModuleMemberDef]> {
        self.0.get(member_name).map(|v| v.as_slice())
    }

    pub fn lookup_visible(
        &self,
        member_name: &SymbolName,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Vec<&GenericModuleMemberDef> {
        self.0
            .get(member_name)
            .map(|defs| {
                defs.iter()
                    .filter(|def| {
                        is_accessible(
                            def.visibility,
                            &def.package,
                            caller_package,
                            &def.source_file,
                            caller_file,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate over all member definitions (flattened across member names).
    pub fn iter_defs(&self) -> impl Iterator<Item = &GenericModuleMemberDef> {
        self.0.values().flat_map(|defs| defs.iter())
    }

    pub fn merge_from(&mut self, other: &GenericModuleMembers) {
        for (name, defs) in &other.0 {
            self.0
                .entry(name.clone())
                .or_default()
                .extend(defs.iter().cloned());
        }
    }
}

/// Information about a standalone module declaration.
/// Module members are stored directly here for resolution, and also registered
/// in the main Registry with module-qualified FQN symbols (e.g. `"Math.double"`).
#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub fqn: Fqn,
    /// Functions declared in this module: bare member name → overloads.
    pub functions: BTreeMap<SymbolName, Vec<FunctionSignature>>,
    /// Globals declared in this module: bare member name → signature.
    pub globals: BTreeMap<SymbolName, GlobalSignature>,
    /// Generic globals from a generic module: bare member name → def.
    pub generic_globals: BTreeMap<SymbolName, GenericModuleGlobalDef>,
    /// Generic members (functions/properties) from a generic module.
    pub generic_members: GenericModuleMembers,
    /// Variance per type parameter (parallel to the generic type's type_params).
    pub type_param_variances: Vec<Variance>,
    /// Trait bounds from the underlying generic type.
    pub trait_bounds: TraitBounds,
    /// The file where the module was declared.
    pub source_file: FilePath,
}

/// Stores all known packages, types, function signatures, global variables, and record types.
/// Functions are stored as a list of overloads per FQN (same name, different param types).
#[derive(Debug, Clone)]
pub struct Registry {
    packages: BTreeSet<PackagePath>,
    functions: BTreeMap<Fqn, Vec<FunctionSignature>>,
    generic_functions: BTreeMap<Fqn, Vec<GenericFunctionDef>>,
    types: BTreeMap<Fqn, Type>,
    globals: BTreeMap<Fqn, GlobalSignature>,
    record_types: BTreeMap<Fqn, RecordTypeSignature>,
    enum_types: BTreeMap<Fqn, EnumTypeSignature>,
    /// Collected extension blocks — unified for generic and non-generic named extensions.
    extension_blocks: Vec<ExtensionBlockSignature>,
    /// Trait definitions: trait_fqn → TraitSignature
    traits: BTreeMap<Fqn, TraitSignature>,
    /// Standalone modules: module_fqn → ModuleInfo
    modules: BTreeMap<Fqn, ModuleInfo>,
    /// Newtype declarations: newtype_fqn → NewtypeSignature
    newtype_types: BTreeMap<Fqn, NewtypeSignature>,
    /// Type alias declarations: alias_fqn → TypeAliasSignature
    type_alias_types: BTreeMap<Fqn, TypeAliasSignature>,
    /// Class declarations: class_fqn → ClassTypeSignature
    class_types: BTreeMap<Fqn, ClassTypeSignature>,
    /// Packages whose internal symbols are accessible from any caller (used for test/ packages).
    /// When a test package is compiled, this is populated with all src package paths so that
    /// the test package can access internal symbols from those packages.
    test_internal_access: BTreeSet<PackagePath>,
    /// Doc comments for top-level declarations, keyed by FQN.
    doc_comments: BTreeMap<Fqn, String>,
    /// Doc comments for sub-items (fields, variants, trait methods, class members),
    /// keyed by (parent_fqn, member_name).
    sub_doc_comments: BTreeMap<(Fqn, String), String>,
    /// Collected implement blocks — unified storage for all trait implementations.
    implement_blocks: Vec<ImplBlockSignature>,
    /// Binary resources declared by each package's project in `Dovetail.toml`.
    /// Keyed by `(declaring_package_root, resource_name)`; value is the raw
    /// bytes. `declaring_package_root` is the project's root package — only
    /// source files inside the same project can reference these resources
    /// (visibility enforced by the typechecker).
    package_resources: BTreeMap<(PackagePath, String), Vec<u8>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            packages: BTreeSet::new(),
            functions: BTreeMap::new(),
            generic_functions: BTreeMap::new(),
            types: BTreeMap::new(),
            globals: BTreeMap::new(),
            record_types: BTreeMap::new(),
            enum_types: BTreeMap::new(),
            extension_blocks: Vec::new(),
            traits: BTreeMap::new(),
            modules: BTreeMap::new(),
            newtype_types: BTreeMap::new(),
            type_alias_types: BTreeMap::new(),
            class_types: BTreeMap::new(),
            test_internal_access: BTreeSet::new(),
            doc_comments: BTreeMap::new(),
            sub_doc_comments: BTreeMap::new(),
            implement_blocks: Vec::new(),
            package_resources: BTreeMap::new(),
        }
    }

    /// Register a binary resource. `project_root` is the project's root
    /// package — used as the visibility scope (any file whose package is
    /// inside this root may reference the resource by name).
    pub fn register_resource(&mut self, project_root: PackagePath, name: String, bytes: Vec<u8>) {
        self.package_resources.insert((project_root, name), bytes);
    }

    /// Look up a resource visible to a caller in `caller_package`. The
    /// resource is visible if its declaring project root is a prefix of
    /// `caller_package` (i.e. the caller is inside the project that
    /// registered it). Returns the bytes and the declaring root on hit.
    pub fn lookup_resource(
        &self,
        caller_package: &PackagePath,
        name: &str,
    ) -> Option<(&PackagePath, &[u8])> {
        for ((root, res_name), bytes) in &self.package_resources {
            if res_name == name && caller_package.starts_with(root) {
                return Some((root, bytes.as_slice()));
            }
        }
        None
    }

    /// All registered resources. Used by codegen to allocate passive data
    /// segments for every embedded blob.
    pub fn all_resources(&self) -> impl Iterator<Item = (&PackagePath, &str, &[u8])> {
        self.package_resources
            .iter()
            .map(|((root, name), bytes)| (root, name.as_str(), bytes.as_slice()))
    }

    /// Grant internal access from any caller to the given package's internal symbols.
    /// Used when compiling test/ packages that need to see src/ package internals.
    pub fn grant_test_internal_access(&mut self, package: PackagePath) {
        self.test_internal_access.insert(package);
    }

    pub fn register_doc_comment(&mut self, fqn: Fqn, doc: String) {
        self.doc_comments.insert(fqn, doc);
    }

    pub fn register_sub_doc_comment(&mut self, parent_fqn: Fqn, member_name: String, doc: String) {
        self.sub_doc_comments.insert((parent_fqn, member_name), doc);
    }

    pub fn lookup_doc_comment(&self, fqn: &Fqn) -> Option<&str> {
        self.doc_comments.get(fqn).map(|s| s.as_str())
    }

    pub fn lookup_sub_doc_comment(&self, parent_fqn: &Fqn, member_name: &str) -> Option<&str> {
        self.sub_doc_comments
            .get(&(parent_fqn.clone(), member_name.to_string()))
            .map(|s| s.as_str())
    }

    /// Check if a symbol is visible from the caller's package, accounting for
    /// test internal access grants. Used by Registry-level lookup methods.
    fn is_visible(
        &self,
        visibility: Visibility,
        symbol_package: &PackagePath,
        caller_package: &PackagePath,
    ) -> bool {
        if is_package_accessible(visibility, symbol_package, caller_package) {
            return true;
        }
        // Test packages can access internal (but not private) symbols from granted packages
        matches!(visibility, Visibility::Internal | Visibility::Private)
            && self.test_internal_access.contains(symbol_package)
    }

    /// Check if a symbol is visible accounting for file-level private and
    /// test internal access grants.
    fn is_visible_with_file(
        &self,
        visibility: Visibility,
        symbol_package: &PackagePath,
        caller_package: &PackagePath,
        symbol_file: &FilePath,
        caller_file: &FilePath,
    ) -> bool {
        if is_accessible(visibility, symbol_package, caller_package, symbol_file, caller_file) {
            return true;
        }
        // Test packages can access internal (but not private) symbols from granted packages
        visibility == Visibility::Internal
            && self.test_internal_access.contains(symbol_package)
    }

    pub fn register_type(&mut self, fqn: Fqn, ty: Type) {
        self.types.insert(fqn, ty);
    }

    /// Register a function. Returns `false` if an overload with the same param types already exists.
    pub fn register_function(&mut self, fqn: Fqn, sig: FunctionSignature) -> bool {
        let overloads = self.functions.entry(fqn).or_default();
        let duplicate = overloads
            .iter()
            .any(|existing| existing.mangled_name == sig.mangled_name);
        if duplicate {
            return false;
        }
        overloads.push(sig);
        true
    }

    /// Look up a type by name (searches across all packages for now).
    // TODO: Remove once we have imports — types should be resolved via import scope, not global name search.
    pub fn lookup_type_by_name(&self, name: &str) -> Option<&Type> {
        self.types.iter().find_map(
            |(fqn, ty)| {
                if fqn.symbol.0 == name { Some(ty) } else { None }
            },
        )
    }

    /// Look up a type by FQN (no visibility check — used in collect phase).
    pub fn lookup_type_by_fqn(&self, fqn: &Fqn) -> Option<&Type> {
        self.types.get(fqn)
    }

    /// Look up a generic record definition by FQN.
    /// Records not visible from the caller's package are hidden.
    pub fn lookup_generic_record_by_fqn(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
    ) -> Option<&RecordTypeSignature> {
        self.record_types.get(fqn).filter(|info| {
            !info.type_params.is_empty()
                && self.is_visible(info.visibility, &fqn.package, caller_package)
        })
    }

    /// Look up a generic enum definition by FQN.
    /// Enums not visible from the caller's package are hidden.
    pub fn lookup_generic_enum_by_fqn(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
    ) -> Option<&EnumTypeSignature> {
        self.enum_types.get(fqn).filter(|info| {
            !info.type_params.is_empty()
                && self.is_visible(info.visibility, &fqn.package, caller_package)
        })
    }

    pub fn register_package(&mut self, package: PackagePath) {
        self.packages.insert(package);
    }

    pub fn has_package(&self, package: &PackagePath) -> bool {
        self.packages.contains(package)
    }

    /// Look up all overloads for a function by FQN.
    /// Functions not visible from the caller's package/file are filtered out.
    pub fn lookup_function(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<Vec<FunctionSignature>> {
        self.functions.get(fqn).and_then(|sigs| {
            let filtered: Vec<_> = sigs
                .iter()
                .filter(|sig| {
                    self.is_visible_with_file(
                        sig.visibility,
                        &fqn.package,
                        caller_package,
                        &sig.source_file,
                        caller_file,
                    )
                })
                .cloned()
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(filtered)
            }
        })
    }

    /// Look up all overloads for a function by symbol name (without package path).
    pub fn lookup_function_by_symbol(&self, name: &str) -> Option<(&Fqn, &[FunctionSignature])> {
        self.functions
            .iter()
            .find(|(fqn, _)| fqn.symbol.0 == name)
            .map(|(fqn, sigs)| (fqn, sigs.as_slice()))
    }

    /// Look up function overloads by bare name scoped to a specific package.
    /// Functions not visible from the caller's package/file are filtered out.
    pub fn lookup_function_in_package(
        &self,
        package: &PackagePath,
        name: &str,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<Vec<FunctionSignature>> {
        let fqn = Fqn {
            package: package.clone(),
            symbol: SymbolName(name.to_string()),
        };
        self.lookup_function(&fqn, caller_package, caller_file)
    }

    /// For error suggestions: find a public function by bare name across all packages.
    pub fn suggest_import_for_function(&self, name: &str) -> Option<&Fqn> {
        self.functions.iter().find_map(|(fqn, sigs)| {
            if fqn.symbol.0 == name && sigs.iter().any(|s| s.visibility == Visibility::Public) {
                Some(fqn)
            } else {
                None
            }
        })
    }

    /// Register a generic function definition.
    pub fn register_generic_function(&mut self, fqn: Fqn, def: GenericFunctionDef) {
        self.generic_functions.entry(fqn).or_default().push(def);
    }

    /// Look up generic function definitions by FQN.
    pub fn lookup_generic_function(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
    ) -> Option<Vec<GenericFunctionDef>> {
        self.generic_functions.get(fqn).and_then(|defs| {
            let filtered: Vec<_> = defs
                .iter()
                .filter(|def| self.is_visible(def.visibility, &fqn.package, caller_package))
                .cloned()
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some(filtered)
            }
        })
    }

    /// Look up generic function definitions by bare name in a specific package.
    pub fn lookup_generic_function_in_package(
        &self,
        package: &PackagePath,
        name: &str,
        caller_package: &PackagePath,
    ) -> Option<Vec<GenericFunctionDef>> {
        let fqn = Fqn {
            package: package.clone(),
            symbol: SymbolName(name.to_string()),
        };
        self.lookup_generic_function(&fqn, caller_package)
    }

    /// Look up a record type by bare name across all packages (generic records only).
    /// Used in the collect phase where FQNs are not yet resolved.
    pub fn lookup_generic_record_by_name(
        &self,
        name: &str,
        caller_package: &PackagePath,
    ) -> Option<(&Fqn, &RecordTypeSignature)> {
        self.record_types.iter().find(|(fqn, info)| {
            fqn.symbol.0 == name
                && !info.type_params.is_empty()
                && self.is_visible(info.visibility, &fqn.package, caller_package)
        })
    }

    /// Register a global variable. Returns `false` if a global with the same FQN already exists.
    pub fn register_global(&mut self, fqn: Fqn, sig: GlobalSignature) -> bool {
        if self.globals.contains_key(&fqn) {
            return false;
        }
        self.globals.insert(fqn, sig);
        true
    }

    /// Look up a global variable by FQN.
    /// Globals not visible from the caller's package/file are hidden.
    pub fn lookup_global(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<&GlobalSignature> {
        self.globals.get(fqn).filter(|sig| {
            self.is_visible_with_file(
                sig.visibility,
                &fqn.package,
                caller_package,
                &sig.source_file,
                caller_file,
            )
        })
    }

    /// Look up a global variable by bare name scoped to a specific package.
    /// Globals not visible from the caller's package/file are hidden.
    pub fn lookup_global_in_package(
        &self,
        package: &PackagePath,
        name: &str,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<&GlobalSignature> {
        let fqn = Fqn {
            package: package.clone(),
            symbol: SymbolName(name.to_string()),
        };
        self.lookup_global(&fqn, caller_package, caller_file)
    }

    /// For error suggestions: find a public global by bare name across all packages.
    pub fn suggest_import_for_global(&self, name: &str) -> Option<&Fqn> {
        self.globals.iter().find_map(|(fqn, sig)| {
            if fqn.symbol.0 == name && sig.visibility == Visibility::Public {
                Some(fqn)
            } else {
                None
            }
        })
    }

    /// Register a record type.
    pub fn register_record_type(&mut self, fqn: Fqn, info: RecordTypeSignature) {
        self.record_types.insert(fqn, info);
    }

    /// Look up a record type by FQN.
    /// Records not visible from the caller's package/file are hidden.
    pub fn lookup_record_type(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<&RecordTypeSignature> {
        self.record_types.get(fqn).filter(|info| {
            self.is_visible_with_file(
                info.visibility,
                &fqn.package,
                caller_package,
                &info.source_file,
                caller_file,
            )
        })
    }

    /// Look up a record type in a specific package.
    /// Records not visible from the caller's package/file are hidden.
    pub fn lookup_record_type_in_package(
        &self,
        package: &PackagePath,
        name: &str,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<&RecordTypeSignature> {
        let fqn = Fqn {
            package: package.clone(),
            symbol: SymbolName(name.to_string()),
        };
        self.lookup_record_type(&fqn, caller_package, caller_file)
    }

    /// For error suggestions: find a public record type by bare name across all packages.
    pub fn suggest_import_for_record(&self, name: &str) -> Option<&Fqn> {
        self.record_types.iter().find_map(|(fqn, info)| {
            if fqn.symbol.0 == name && info.visibility == Visibility::Public {
                Some(fqn)
            } else {
                None
            }
        })
    }

    /// Register an enum type.
    pub fn register_enum_type(&mut self, fqn: Fqn, info: EnumTypeSignature) {
        self.enum_types.insert(fqn, info);
    }

    /// Look up an enum type by FQN.
    /// Enums not visible from the caller's package/file are hidden.
    pub fn lookup_enum_type(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<&EnumTypeSignature> {
        self.enum_types.get(fqn).filter(|info| {
            self.is_visible_with_file(
                info.visibility,
                &fqn.package,
                caller_package,
                &info.source_file,
                caller_file,
            )
        })
    }

    /// Look up an enum type in a specific package.
    /// Enums not visible from the caller's package/file are hidden.
    pub fn lookup_enum_type_in_package(
        &self,
        package: &PackagePath,
        name: &str,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<&EnumTypeSignature> {
        let fqn = Fqn {
            package: package.clone(),
            symbol: SymbolName(name.to_string()),
        };
        self.lookup_enum_type(&fqn, caller_package, caller_file)
    }

    /// Register a newtype.
    pub fn register_newtype_type(&mut self, fqn: Fqn, sig: NewtypeSignature) {
        self.newtype_types.insert(fqn, sig);
    }

    /// Look up a newtype by FQN.
    /// Newtypes not visible from the caller's package/file are hidden.
    pub fn lookup_newtype_type(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<&NewtypeSignature> {
        self.newtype_types.get(fqn).filter(|sig| {
            self.is_visible_with_file(
                sig.visibility,
                &fqn.package,
                caller_package,
                &sig.source_file,
                caller_file,
            )
        })
    }

    /// Look up a generic newtype definition by FQN (package-level visibility check).
    /// Used during collect phase where file info is not available.
    pub fn lookup_generic_newtype_by_fqn(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
    ) -> Option<&NewtypeSignature> {
        self.newtype_types.get(fqn).filter(|sig| {
            !sig.type_params.is_empty()
                && self.is_visible(sig.visibility, &fqn.package, caller_package)
        })
    }

    /// Register a type alias.
    pub fn register_type_alias(&mut self, fqn: Fqn, sig: TypeAliasSignature) {
        self.type_alias_types.insert(fqn, sig);
    }

    /// Look up a type alias by FQN.
    /// Type aliases not visible from the caller's package/file are hidden.
    pub fn lookup_type_alias(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<&TypeAliasSignature> {
        self.type_alias_types.get(fqn).filter(|sig| {
            self.is_visible_with_file(
                sig.visibility,
                &fqn.package,
                caller_package,
                &sig.source_file,
                caller_file,
            )
        })
    }

    /// Look up a generic type alias by FQN (only returns aliases with type params).
    pub fn lookup_generic_type_alias_by_fqn(&self, fqn: &Fqn) -> Option<&TypeAliasSignature> {
        self.type_alias_types
            .get(fqn)
            .filter(|sig| !sig.type_params.is_empty())
    }

    /// Register a class type.
    pub fn register_class_type(&mut self, fqn: Fqn, sig: ClassTypeSignature) {
        self.class_types.insert(fqn, sig);
    }

    /// Add an instance method signature to an already-registered class (used
    /// for defaulted trait members the class omits — the function body is
    /// materialized from the default template at monomorphize).
    pub fn add_class_instance_method(
        &mut self,
        class_fqn: &Fqn,
        name: SymbolName,
        sig: FunctionSignature,
    ) {
        if let Some(class_sig) = self.class_types.get_mut(class_fqn) {
            class_sig.instance_methods.entry(name).or_default().push(sig);
        }
    }

    pub fn add_class_trait_impl(&mut self, class_fqn: &Fqn, trait_fqn: Fqn, trait_type_args: Vec<Type>) {
        if let Some(sig) = self.class_types.get_mut(class_fqn) {
            sig.trait_impls.push((trait_fqn, trait_type_args));
        }
    }

    /// Look up a class type by FQN.
    /// Classes not visible from the caller's package are hidden.
    pub fn lookup_class_type(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
    ) -> Option<&ClassTypeSignature> {
        self.class_types.get(fqn).filter(|sig| {
            self.is_visible(sig.visibility, &fqn.package, caller_package)
        })
    }

    /// Check if `child` is a subtype of `parent` (walks the parent_class chain).
    pub fn class_is_subtype(&self, child: &Fqn, parent: &Fqn) -> bool {
        let mut current = child.clone();
        loop {
            if &current == parent {
                return true;
            }
            match self.class_types.get(&current).and_then(|sig| sig.parent_class.as_ref()) {
                Some(parent_fqn) => current = parent_fqn.clone(),
                None => return false,
            }
        }
    }

    /// Look up a type by FQN.
    /// Types not visible from the caller's package/file are hidden.
    pub fn lookup_type(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
        caller_file: &FilePath,
    ) -> Option<&Type> {
        let ty = self.types.get(fqn)?;
        // If this is a record type, check visibility
        if let Some(info) = self.record_types.get(fqn) {
            if !self.is_visible_with_file(
                info.visibility,
                &fqn.package,
                caller_package,
                &info.source_file,
                caller_file,
            ) {
                return None;
            }
        }
        // If this is an enum type, check visibility
        if let Some(info) = self.enum_types.get(fqn) {
            if !self.is_visible_with_file(
                info.visibility,
                &fqn.package,
                caller_package,
                &info.source_file,
                caller_file,
            ) {
                return None;
            }
        }
        // If this is a newtype, check visibility
        if let Some(sig) = self.newtype_types.get(fqn) {
            if !self.is_visible_with_file(
                sig.visibility,
                &fqn.package,
                caller_package,
                &sig.source_file,
                caller_file,
            ) {
                return None;
            }
        }
        // If this is a type alias, check visibility
        if let Some(sig) = self.type_alias_types.get(fqn) {
            if !self.is_visible_with_file(
                sig.visibility,
                &fqn.package,
                caller_package,
                &sig.source_file,
                caller_file,
            ) {
                return None;
            }
        }
        // If this is a class type, check visibility
        if let Some(sig) = self.class_types.get(fqn) {
            if !self.is_visible(sig.visibility, &fqn.package, caller_package) {
                return None;
            }
        }
        Some(ty)
    }

    /// Register a collected implement block.
    pub fn register_implement_block(&mut self, block: ImplBlockSignature) {
        self.implement_blocks.push(block);
    }

    /// Translate a selected inherited declaration into its provider's member symbol.
    pub(crate) fn route_trait_method(
        &self, provider: &Fqn, provider_parameters: &[Type], requested: &Fqn,
        requested_parameters: &[Type], member: &SymbolName,
    ) -> SymbolName {
        use super::infer::generics::apply_substitution;
        use super::infer::type_param_substitution::TypeParamSubstitution;
        let Some(source) = self.get_trait(requested) else { return member.clone() };
        let Some(source_method) = source.methods.iter().find(|method| source.method_dispatch_name(method) == *member) else { return member.clone() };
        let Some(target) = self.get_trait(provider) else { return member.clone() };
        let identity = |signature: &TraitSignature, method: &TraitMethodSig, arguments: &[Type]| {
            let substitution = TypeParamSubstitution::from_pairs(&signature.type_params, arguments);
            match &method.origin {
                Some((origin, parameters)) => (origin.clone(), parameters.iter().map(|ty| apply_substitution(&substitution, ty)).collect::<Vec<_>>()),
                None => (signature.fqn.clone(), arguments.to_vec()),
            }
        };
        let (origin, parameters) = identity(source, source_method, requested_parameters);
        target.methods.iter().find(|method| {
            let (candidate, arguments) = identity(target, method, provider_parameters);
            method.name == source_method.name && candidate == origin && arguments.len() == parameters.len()
                && arguments.iter().zip(&parameters).all(|(a, b)| super::subtyping::identical(a, b))
        }).map(|method| target.method_dispatch_name(method)).unwrap_or_else(|| member.clone())
    }

    /// Find implement blocks for a given type and method name.
    pub fn find_impl_method(
        &self,
        type_fqn: &Fqn,
        method_name: &SymbolName,
    ) -> Vec<(&ImplBlockSignature, &ImplMethodSignature)> {
        self.implement_blocks
            .iter()
            .filter(|block| impl_receiver_family_matches(&block.type_fqn, type_fqn))
            .flat_map(|block| {
                block
                    .methods
                    .iter()
                    .chain(block.properties.iter())
                    .filter(|m| m.name == *method_name)
                    .map(move |m| (block, m))
            })
            .collect()
    }

    /// Find the implement block for a (trait, type) pair.
    pub fn find_impl_block(
        &self,
        trait_fqn: &Fqn,
        type_fqn: &Fqn,
    ) -> Option<&ImplBlockSignature> {
        self.implement_blocks
            .iter()
            .find(|b| b.trait_fqn == *trait_fqn && impl_receiver_family_matches(&b.type_fqn, type_fqn))
    }

    /// Get all implement blocks for a given type.
    pub fn find_impl_blocks_for_type(&self, type_fqn: &Fqn) -> Vec<&ImplBlockSignature> {
        self.implement_blocks
            .iter()
            .filter(|b| impl_receiver_family_matches(&b.type_fqn, type_fqn))
            .collect()
    }

    /// Register a collected extension block.
    pub fn register_extension_block(&mut self, block: ExtensionBlockSignature) {
        self.extension_blocks.push(block);
    }

    /// Check if any extension block exists under this FQN. A single named extension may
    /// have multiple blocks targeting different `for_type`s; this returns true if at
    /// least one such block exists. Used by imports to branch on "is this FQN an
    /// extension or a regular symbol?".
    pub fn has_extension_block(&self, ext_fqn: &Fqn) -> bool {
        self.extension_blocks.iter().any(|b| b.ext_fqn == *ext_fqn)
    }

    /// Check if an extension block exists for a specific `(ext_fqn, for_type)` pair.
    /// Used by the collector to detect true duplicates while still allowing the same
    /// extension name to target multiple types.
    pub fn has_extension_block_for_type(&self, ext_fqn: &Fqn, for_type: &Type) -> bool {
        self.extension_blocks
            .iter()
            .any(|b| b.ext_fqn == *ext_fqn && b.for_type == *for_type)
    }

    /// Look up all extension blocks under this FQN. A named extension may have multiple
    /// blocks (one per `for_type`); returning them all lets the import scope bring every
    /// block under a given FQN into scope with a single `import` line.
    pub fn lookup_extension_blocks_by_fqn(&self, ext_fqn: &Fqn) -> Vec<&ExtensionBlockSignature> {
        self.extension_blocks
            .iter()
            .filter(|b| b.ext_fqn == *ext_fqn)
            .collect()
    }

    /// Get all extension blocks.
    pub fn all_extension_blocks(&self) -> &[ExtensionBlockSignature] {
        &self.extension_blocks
    }

    /// Get all implement blocks.
    pub fn all_implement_blocks(&self) -> &[ImplBlockSignature] {
        &self.implement_blocks
    }

    /// Register a trait definition. Returns `false` if a trait with the same FQN already exists.
    /// Pre-register a trait name so that forward references resolve.
    /// The full signature is filled in later by `register_trait`.
    pub fn pre_register_trait(&mut self, fqn: Fqn, type_params: Vec<TypeParamName>, is_interface: bool) {
        self.traits.entry(fqn.clone()).or_insert_with(|| {
            let span = Span::point("".into(), 1, 1);
            TraitSignature {
                visibility: Visibility::Internal,
                fqn,
                type_params,
                supers: vec![],
                super_closure: vec![],
                methods: vec![],
                method_dispatch_names: vec![],
                properties: vec![],
                associated_types: vec![],
                is_interface,
                source_file: "".into(),
                span,
            }
        });
    }

    pub fn register_trait(&mut self, fqn: Fqn, sig: TraitSignature) -> bool {
        // Allow overwriting a pre-registered placeholder (empty methods/properties/associated_types),
        // but reject true duplicates (already has methods, properties, or associated types).
        if let Some(existing) = self.traits.get(&fqn) {
            if !existing.methods.is_empty()
                || !existing.properties.is_empty()
                || !existing.associated_types.is_empty()
                // A marker trait with only supers is a real registration too.
                || !existing.supers.is_empty()
            {
                return false;
            }
        }
        self.traits.insert(fqn, sig);
        true
    }

    /// Replace a trait signature in place (used by trait flattening, which
    /// rewrites local traits with their flattened member sets).
    pub fn replace_trait(&mut self, fqn: Fqn, sig: TraitSignature) {
        self.traits.insert(fqn, sig);
    }

    /// Look up a trait by FQN.
    /// Traits not visible from the caller's package are hidden.
    pub fn lookup_trait(
        &self,
        fqn: &Fqn,
        caller_package: &PackagePath,
    ) -> Option<&TraitSignature> {
        self.traits.get(fqn).filter(|sig| {
            self.is_visible(sig.visibility, &fqn.package, caller_package)
        })
    }

    /// Look up a trait by bare name across all packages.
    pub fn lookup_trait_by_name(
        &self,
        name: &str,
        caller_package: &PackagePath,
    ) -> Option<(&Fqn, &TraitSignature)> {
        self.traits.iter().find(|(fqn, sig)| {
            fqn.symbol.0 == name
                && self.is_visible(sig.visibility, &fqn.package, caller_package)
        })
    }

    /// The declaring-trait raw signature backing a (possibly inherited)
    /// method. Vtable slot identity — member name, erased param/return types
    /// — must come from the origin trait's *raw* member, never the flattened
    /// substituted copy, so that every trait sharing an inherited member
    /// agrees on the slot shape (which is what makes super-upcasts static).
    pub fn origin_method_raw<'a>(
        &'a self,
        owner_sig: &'a TraitSignature,
        m: &'a TraitMethodSig,
    ) -> (&'a TraitSignature, &'a TraitMethodSig) {
        if let Some((origin_fqn, _)) = &m.origin {
            if let Some(origin_sig) = self.traits.get(origin_fqn) {
                if let Some(raw) = origin_sig.methods.iter().find(|om| {
                    om.origin.is_none()
                        && om.name == m.name
                        && om.params.len() == m.params.len()
                }) {
                    return (origin_sig, raw);
                }
            }
        }
        (owner_sig, m)
    }

    /// As `origin_method_raw`, for properties.
    pub fn origin_property_raw<'a>(
        &'a self,
        owner_sig: &'a TraitSignature,
        p: &'a TraitPropertySig,
    ) -> (&'a TraitSignature, &'a TraitPropertySig) {
        if let Some((origin_fqn, _)) = &p.origin {
            if let Some(origin_sig) = self.traits.get(origin_fqn) {
                if let Some(raw) = origin_sig
                    .properties
                    .iter()
                    .find(|op| op.origin.is_none() && op.name == p.name)
                {
                    return (origin_sig, raw);
                }
            }
        }
        (owner_sig, p)
    }

    /// If `target` is in the (transitive) super closure of `trait_fqn`
    /// applied at `trait_args`, return the target's substituted type args.
    pub fn super_closure_args(
        &self,
        trait_fqn: &Fqn,
        trait_args: &[Type],
        target: &Fqn,
    ) -> Option<Vec<Type>> {
        let sig = self.traits.get(trait_fqn)?;
        let (_, closure_args) = sig.super_closure.iter().find(|(f, _)| f == target)?;
        if sig.type_params.is_empty() || trait_args.is_empty() {
            return Some(closure_args.clone());
        }
        let sub: std::collections::BTreeMap<TypeParamName, Type> = sig
            .type_params
            .iter()
            .cloned()
            .zip(trait_args.iter().cloned())
            .collect();
        Some(
            closure_args
                .iter()
                .map(|t| crate::typechecker::collect::substitute_trait_type_params(t, &sub))
                .collect(),
        )
    }

    /// All impl blocks that provide `trait_fqn` for `type_fqn`: direct impls
    /// first, then impls of traits whose super closure contains `trait_fqn`
    /// (with the closure-substituted target args attached as `via`). Order is
    /// deterministic (registration order within each group).
    pub fn find_providing_impl_blocks(
        &self,
        trait_fqn: &Fqn,
        type_fqn: &Fqn,
    ) -> Vec<(&ImplBlockSignature, Option<(Fqn, Vec<Type>)>)> {
        let mut out: Vec<(&ImplBlockSignature, Option<(Fqn, Vec<Type>)>)> = self
            .implement_blocks
            .iter()
            .filter(|b| b.trait_fqn == *trait_fqn && impl_receiver_family_matches(&b.type_fqn, type_fqn))
            .map(|b| (b, None))
            .collect();
        for b in &self.implement_blocks {
            if !impl_receiver_family_matches(&b.type_fqn, type_fqn) || b.trait_fqn == *trait_fqn {
                continue;
            }
            if let Some(args) = self.super_closure_args(&b.trait_fqn, &b.trait_type_args, trait_fqn) {
                out.push((b, Some((b.trait_fqn.clone(), args))));
            }
        }
        out
    }

    /// Check if a (trait, type) implementation pair exists (any type args).
    pub fn has_trait_impl(&self, trait_fqn: &Fqn, type_fqn: &Fqn) -> bool {
        self.implement_blocks.iter().any(|b| b.trait_fqn == *trait_fqn && impl_receiver_family_matches(&b.type_fqn, type_fqn))
            || self.class_types.get(type_fqn).is_some_and(|sig|
                sig.trait_impls.iter().any(|(t, _)| t == trait_fqn))
    }

    /// Check whether an exact duplicate implement block exists: same trait,
    /// same full for-type (so `Tr for List<Int32>` and `Tr for List<String>`
    /// are distinct), same trait type args. Non-generic blocks only — overlap
    /// among generic blocks is the coherence rule's job. The class arm keeps
    /// the base-FQN check: a class implements a trait for its whole shape.
    pub fn has_exact_trait_impl(
        &self,
        trait_fqn: &Fqn,
        for_type: &Type,
        trait_type_args: &[Type],
    ) -> bool {
        self.implement_blocks.iter().any(|b| {
            b.type_params.is_empty()
                && b.trait_fqn == *trait_fqn
                && b.for_type == *for_type
                && b.trait_type_args == trait_type_args
        }) || for_type.try_to_fqn().is_some_and(|type_fqn| {
            self.class_types.get(&type_fqn).is_some_and(|sig| {
                sig.trait_impls
                    .iter()
                    .any(|(t, args)| t == trait_fqn && args == trait_type_args)
            })
        })
    }

    /// Check if a (trait, type) implementation pair exists with specific trait type args.
    pub fn has_trait_impl_with_args(
        &self,
        trait_fqn: &Fqn,
        type_fqn: &Fqn,
        trait_type_args: &[Type],
    ) -> bool {
        self.implement_blocks.iter().any(|b| {
            b.trait_fqn == *trait_fqn
                && impl_receiver_family_matches(&b.type_fqn, type_fqn)
                && b.trait_type_args == trait_type_args
        })
            || self.class_types.get(type_fqn).is_some_and(|sig|
                sig.trait_impls.iter().any(|(t, args)| t == trait_fqn && args == trait_type_args))
    }

    /// Check whether a GENERIC implementation of a trait exists for a type —
    /// an impl block or class `implements` whose trait application still
    /// carries type parameters (`implement <T> Conv<T> for Box<T>`,
    /// `class Holder<T> implements Conv<T>`). Such an impl can match any
    /// requested application, so args-equality gates must not treat its
    /// absence from an exact-args lookup as "not implemented".
    pub fn has_generic_trait_impl(&self, trait_fqn: &Fqn, type_fqn: &Fqn) -> bool {
        self.implement_blocks.iter().any(|b| {
            b.trait_fqn == *trait_fqn
                && impl_receiver_family_matches(&b.type_fqn, type_fqn)
                && b.trait_type_args.iter().any(|t| t.contains_type_parameter())
        }) || self.class_types.get(type_fqn).is_some_and(|sig| {
            sig.trait_impls.iter().any(|(t, args)| {
                t == trait_fqn && args.iter().any(|a| a.contains_type_parameter())
            })
        })
    }

    /// Look up all implement blocks for a given (trait, type) pair.
    pub fn find_impl_blocks(
        &self,
        trait_fqn: &Fqn,
        type_fqn: &Fqn,
    ) -> Vec<&ImplBlockSignature> {
        self.implement_blocks
            .iter()
            .filter(|b| b.trait_fqn == *trait_fqn && impl_receiver_family_matches(&b.type_fqn, type_fqn))
            .collect()
    }

    /// Iterate over all registered traits.
    pub fn traits(&self) -> impl Iterator<Item = &TraitSignature> {
        self.traits.values()
    }

    /// Iterate over generic record types (those with type params).
    pub fn generic_record_types(&self) -> impl Iterator<Item = &RecordTypeSignature> {
        self.record_types
            .values()
            .filter(|sig| !sig.type_params.is_empty())
    }

    /// Iterate over generic enum types (those with type params).
    pub fn generic_enum_types(&self) -> impl Iterator<Item = &EnumTypeSignature> {
        self.enum_types
            .values()
            .filter(|sig| !sig.type_params.is_empty())
    }

    /// Iterate over generic newtype types (those with type params).
    pub fn generic_newtype_types(&self) -> impl Iterator<Item = &NewtypeSignature> {
        self.newtype_types
            .values()
            .filter(|sig| !sig.type_params.is_empty())
    }

    /// Iterate over generic class types (those with type params).
    pub fn generic_class_types(&self) -> impl Iterator<Item = &ClassTypeSignature> {
        self.class_types
            .values()
            .filter(|sig| !sig.type_params.is_empty())
    }

    /// Iterate over all registered generic function definitions (flattened across FQNs).
    pub fn generic_function_defs(&self) -> impl Iterator<Item = (&Fqn, &GenericFunctionDef)> {
        self.generic_functions
            .iter()
            .flat_map(|(fqn, defs)| defs.iter().map(move |def| (fqn, def)))
    }

    /// Iterate over all generic extension method definitions (from collected extension blocks).
    pub fn all_generic_extension_method_defs(
        &self,
    ) -> impl Iterator<Item = (&ExtensionBlockSignature, &ExtMethodSignature)> {
        self.extension_blocks
            .iter()
            .filter(|b| !b.type_params.is_empty())
            .flat_map(|b| {
                b.methods.iter().chain(b.properties.iter()).map(move |m| (b, m))
            })
    }

    /// Iterate over all generic module member definitions across all modules.
    pub fn all_generic_module_member_defs(&self) -> impl Iterator<Item = &GenericModuleMemberDef> {
        self.modules
            .values()
            .flat_map(|info| info.generic_members.iter_defs())
    }

    /// Register a standalone module. Returns `false` if a module with the same FQN already exists.
    pub fn register_module(&mut self, fqn: Fqn, info: ModuleInfo) -> bool {
        if self.modules.contains_key(&fqn) {
            return false;
        }
        self.modules.insert(fqn, info);
        true
    }

    /// Look up a module by FQN.
    pub fn lookup_module(&self, fqn: &Fqn) -> Option<&ModuleInfo> {
        self.modules.get(fqn)
    }

    /// Look up a module by bare name in a specific package.
    pub fn lookup_module_in_package(
        &self,
        package: &PackagePath,
        name: &str,
    ) -> Option<&ModuleInfo> {
        let fqn = Fqn {
            package: package.clone(),
            symbol: SymbolName(name.to_string()),
        };
        self.modules.get(&fqn)
    }

    /// Check if a module exists in a specific package.
    pub fn has_module_in_package(&self, package: &PackagePath, name: &str) -> bool {
        let fqn = Fqn {
            package: package.clone(),
            symbol: SymbolName(name.to_string()),
        };
        self.modules.contains_key(&fqn)
    }

    // ── Visibility-free accessors (used by LSP) ────────────────────

    /// Direct lookup of a record type by FQN, skipping visibility checks.
    pub fn get_record_type(&self, fqn: &Fqn) -> Option<&RecordTypeSignature> {
        self.record_types.get(fqn)
    }

    /// Direct lookup of an enum type by FQN, skipping visibility checks.
    pub fn get_enum_type(&self, fqn: &Fqn) -> Option<&EnumTypeSignature> {
        self.enum_types.get(fqn)
    }

    /// Direct lookup of a class type by FQN, skipping visibility checks.
    pub fn get_class_type(&self, fqn: &Fqn) -> Option<&ClassTypeSignature> {
        self.class_types.get(fqn)
    }

    /// Record that a trait's DEFAULT body supplies a member the class omitted.
    pub fn note_default_supplied_member(
        &mut self,
        class_fqn: &Fqn,
        member: SymbolName,
        default_source: Fqn,
    ) {
        if let Some(sig) = self.class_types.get_mut(class_fqn) {
            sig.default_supplied_members.insert(member, default_source);
        }
    }

    /// Direct lookup of a trait by FQN, skipping visibility checks.
    pub fn get_trait(&self, fqn: &Fqn) -> Option<&TraitSignature> {
        self.traits.get(fqn)
    }

    /// Direct lookup of a newtype by FQN, skipping visibility checks.
    pub fn get_newtype_type(&self, fqn: &Fqn) -> Option<&NewtypeSignature> {
        self.newtype_types.get(fqn)
    }

    /// Direct lookup of a type alias by FQN, skipping visibility checks.
    pub fn get_type_alias(&self, fqn: &Fqn) -> Option<&TypeAliasSignature> {
        self.type_alias_types.get(fqn)
    }

    /// Direct lookup of function overloads by FQN, skipping visibility checks.
    pub fn get_functions(&self, fqn: &Fqn) -> Option<&[FunctionSignature]> {
        self.functions.get(fqn).map(|v| v.as_slice())
    }

    /// Direct lookup of a global by FQN, skipping visibility checks.
    pub fn get_global(&self, fqn: &Fqn) -> Option<&GlobalSignature> {
        self.globals.get(fqn)
    }

    /// Iterate over all record types.
    pub fn all_record_types(&self) -> impl Iterator<Item = (&Fqn, &RecordTypeSignature)> {
        self.record_types.iter()
    }

    /// Iterate over all enum types.
    pub fn all_enum_types(&self) -> impl Iterator<Item = (&Fqn, &EnumTypeSignature)> {
        self.enum_types.iter()
    }

    /// Iterate over all class types.
    pub fn all_class_types(&self) -> impl Iterator<Item = (&Fqn, &ClassTypeSignature)> {
        self.class_types.iter()
    }

    /// Iterate over all traits (with FQN keys).
    pub fn all_traits(&self) -> impl Iterator<Item = (&Fqn, &TraitSignature)> {
        self.traits.iter()
    }

    /// Iterate over all modules.
    pub fn all_modules(&self) -> impl Iterator<Item = (&Fqn, &ModuleInfo)> {
        self.modules.iter()
    }

    /// Iterate over all newtype types.
    pub fn all_newtype_types(&self) -> impl Iterator<Item = (&Fqn, &NewtypeSignature)> {
        self.newtype_types.iter()
    }

    /// Iterate over all type aliases.
    pub fn all_type_aliases(&self) -> impl Iterator<Item = (&Fqn, &TypeAliasSignature)> {
        self.type_alias_types.iter()
    }

    /// Iterate over all functions (with FQN keys).
    pub fn all_functions(&self) -> impl Iterator<Item = (&Fqn, &[FunctionSignature])> {
        self.functions.iter().map(|(fqn, sigs)| (fqn, sigs.as_slice()))
    }

    /// Iterate over all globals (with FQN keys).
    pub fn all_globals(&self) -> impl Iterator<Item = (&Fqn, &GlobalSignature)> {
        self.globals.iter()
    }

    /// Merge two registries into a new one (e.g. dependency + package for inference).
    pub fn merge(&self, other: &Registry) -> Registry {
        let mut merged = self.clone();
        merged.packages.extend(other.packages.iter().cloned());
        for (fqn, sigs) in &other.functions {
            merged
                .functions
                .entry(fqn.clone())
                .or_default()
                .extend(sigs.iter().cloned());
        }
        for (fqn, defs) in &other.generic_functions {
            merged
                .generic_functions
                .entry(fqn.clone())
                .or_default()
                .extend(defs.iter().cloned());
        }
        merged
            .types
            .extend(other.types.iter().map(|(k, v)| (k.clone(), v.clone())));
        merged
            .globals
            .extend(other.globals.iter().map(|(k, v)| (k.clone(), v.clone())));
        merged.record_types.extend(
            other
                .record_types
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        merged
            .enum_types
            .extend(other.enum_types.iter().map(|(k, v)| (k.clone(), v.clone())));
        merged.extension_blocks.extend(other.extension_blocks.iter().cloned());
        merged
            .traits
            .extend(other.traits.iter().map(|(k, v)| (k.clone(), v.clone())));
        for (fqn, info) in &other.modules {
            if let Some(existing) = merged.modules.get_mut(fqn) {
                // Merge generic_members from the other registry
                existing.generic_members.merge_from(&info.generic_members);
                // Merge functions
                for (name, sigs) in &info.functions {
                    existing
                        .functions
                        .entry(name.clone())
                        .or_default()
                        .extend(sigs.iter().cloned());
                }
                // Merge globals
                existing
                    .globals
                    .extend(info.globals.iter().map(|(k, v)| (k.clone(), v.clone())));
                // Merge generic globals
                existing.generic_globals.extend(
                    info.generic_globals
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone())),
                );
            } else {
                merged.modules.insert(fqn.clone(), info.clone());
            }
        }
        merged.newtype_types.extend(
            other
                .newtype_types
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        merged.type_alias_types.extend(
            other
                .type_alias_types
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        merged.class_types.extend(
            other
                .class_types
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        merged
            .test_internal_access
            .extend(other.test_internal_access.iter().cloned());
        merged.doc_comments.extend(
            other
                .doc_comments
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        merged.sub_doc_comments.extend(
            other
                .sub_doc_comments
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        merged
            .implement_blocks
            .extend(other.implement_blocks.iter().cloned());
        merged.package_resources.extend(
            other
                .package_resources
                .iter()
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        merged
    }
}

/// Recursive tuple heads share a candidate family with every arity >= 3.
/// Exact shape matching and bound checking still decide applicability.
pub(crate) fn impl_receiver_family_matches(pattern: &Fqn, actual: &Fqn) -> bool {
    if pattern == actual { return true; }
    pattern.package.to_string() == "standard.prelude"
        && actual.package.0.is_empty()
        && pattern.symbol.0 == "TupleExtend"
        && actual.symbol.0.strip_prefix("Tuple").and_then(|n| n.parse::<usize>().ok()).is_some_and(|n| n >= 3)
}
