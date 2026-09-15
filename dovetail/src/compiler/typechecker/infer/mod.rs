mod async_arguments;
mod async_expressions;
mod async_loops;
mod classes;
mod closures;
mod enums;
mod exact_number;
mod expressions;
mod extensions;
mod function_expressions;
mod functions;
mod generic_enums;
mod generic_functions;
mod generic_modules;
mod generic_newtypes;
mod generic_records;
pub(crate) mod generics;
mod globals;
mod implements;
mod indexing;
mod match_expression;
mod method_contracts;
mod modules;
mod negation;
mod prefixed_literal;
mod record_expressions;
mod record_payloads;
mod records;
mod slices;
mod test_decls;
mod trait_defaults;
mod traits;
pub(crate) mod type_param_substitution;
pub(crate) mod types;
mod use_continuations;

use crate::common::diagnostics::Diagnostics;
use crate::common::span::FilePath;
use crate::common::types::{
    Fqn, MangledName, PackagePath, SymbolName, TypeParamName, VarName, Visibility,
};
use crate::parser::ast::{Declaration, SourceFile};

use crate::typechecker::imports::{ImportScope, PackageImportScopes};
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{
    ClassFieldDef, Type, TypeDef, TypeReference, TypedExpr, TypedExprKind, TypedExtensionBlock,
    TypedFunction, TypedGlobal, TypedImplementBlock, TypedModule, TypedTest,
};

/// Result of resolving a generic function call.
pub(super) enum ResolvedFunction {
    /// A regular function (or instantiated generic) — produces `FunctionCall`.
    Regular {
        mangled_name: MangledName,
        return_type: Type,
        /// Non-empty for generic function instantiations.
        type_args: Vec<Type>,
    },
    /// An intrinsic function — produces `IntrinsicCall`.
    Intrinsic {
        intrinsic: crate::typechecker::types::IntrinsicKind,
        return_type: Type,
    },
    /// A trait impl method — produces `ImplFunctionCall`.
    ImplMethod {
        resolved: crate::typechecker::types::ResolvedImplMethod,
        return_type: Type,
    },
    /// An extension method — produces `ExtFunctionCall`.
    ExtMethod {
        ext_fqn: Fqn,
        for_type: Type,
        method_name: SymbolName,
        type_args: Vec<Type>,
        return_type: Type,
    },
}
use std::collections::BTreeMap;

/// A variable binding in a scope.
#[derive(Clone)]
pub(super) struct VarBinding {
    ty: Type,
    mutable: bool,
}

/// A lexical scope containing variable bindings.
#[derive(Clone)]
struct Scope {
    bindings: BTreeMap<VarName, VarBinding>,
}

impl Scope {
    fn new() -> Self {
        Self {
            bindings: BTreeMap::new(),
        }
    }

    fn define(&mut self, name: VarName, binding: VarBinding) {
        self.bindings.insert(name, binding);
    }

    fn lookup(&self, name: &str) -> Option<&VarBinding> {
        self.bindings
            .iter()
            .find(|(k, _)| k.0 == name)
            .map(|(_, v)| v)
    }
}

/// Empty import scope used as a default when no scope is found for a file.
static EMPTY_IMPORT_SCOPE: ImportScope = ImportScope::EMPTY;

use crate::typechecker::types::TypedParam;

/// Build a human-readable display name for the WASM name section.
/// e.g., `a.add(Int32, Int32)` or `a.main` (no parens when no params).
pub(crate) fn make_display_name(base_name: &str, params: &[TypedParam]) -> String {
    if params.is_empty() {
        base_name.to_string()
    } else {
        let types: Vec<String> = params.iter().map(|p| p.ty.to_string()).collect();
        format!("{}({})", base_name, types.join(", "))
    }
}

/// Holds the state for the inference phase.
pub(super) struct Inference<'a> {
    package_path: PackagePath,
    current_file: FilePath,
    registry: &'a Registry,
    diagnostics: &'a mut Diagnostics,
    typed_functions: BTreeMap<MangledName, TypedFunction>,
    /// Default-body templates for trait members (see TypedModule::default_templates).
    default_templates: BTreeMap<MangledName, TypedFunction>,
    typed_globals: BTreeMap<MangledName, TypedGlobal>,
    typed_tests: Vec<TypedTest>,
    scopes: Vec<Scope>,
    loop_depth: u32,
    import_scope: &'a ImportScope,
    /// Class type defs built during inference (all fields, including non-public).
    /// Generic record/enum TypeDefs are now created by monomorphize from templates.
    class_type_defs: BTreeMap<MangledName, TypeDef>,
    /// Type parameters currently in scope (for monomorphized generic function bodies).
    /// Maps type parameter names to concrete types for type expression resolution.
    current_type_params: BTreeMap<TypeParamName, Type>,
    /// Counter for generating unique GenericParam IDs.
    type_param_counter: u32,
    /// Expected type hint from context (e.g. let binding annotation, function parameter type).
    /// Used to infer type args when they can't be determined from argument types alone.
    pub(super) expected_type: Option<Type>,
    /// Innermost source expression, restored after recursive inference.
    pub(super) current_expr_span: Option<crate::common::span::Span>,
    /// The return type of the enclosing function (for try/orReturn checking).
    pub(super) function_return_type: Option<Type>,
    /// When set, we are inside an async function body. Stores the Awaitable return type
    /// for same-Awaitable-type rule enforcement on await expressions.
    pub(super) async_return_type: Option<Type>,
    /// Await operand types collected while discovering an async expression's context.
    async_discovery: Option<Vec<Type>>,
    /// The enclosing block's expected wrapped-error type, if any. Set by the Block
    /// handler from its saved expected_type when the type looks async-shaped
    /// (`Async<_, E>`). Used by `use` expressions to determine the target error
    /// type for `From` auto-conversion when the block's expected type wraps an
    /// error (e.g. `let program: Async<Unit, AppError> = ...`).
    pub(super) block_wrapped_error: Option<Type>,
    /// When set, we are inferring inside a module or class. FQN symbols are qualified as "Container.member".
    pub(super) container_name: Option<String>,
    /// The enclosing module, excluding class and trait implementation bodies.
    pub(super) current_module_name: Option<String>,
    /// Unresolved method-level type param names during deferred closure re-inference.
    /// Used by closures and bare variant calls to distinguish these from outer-scope type params.
    pub(super) unresolved_method_type_params: Vec<TypeParamName>,
    /// When typechecking a class body (methods/properties), holds the class's fields
    /// so that `self.field` access works without inserting a temporary ClassTypeDef.
    pub(super) typechecking_class: Option<(MangledName, Vec<ClassFieldDef>)>,
    /// Resolved type name references with their source spans, for LSP navigation on type annotations.
    pub(super) type_references: Vec<TypeReference>,
    /// Typed implement blocks produced during inference.
    pub(super) implement_blocks: Vec<TypedImplementBlock>,
    /// Typed extension blocks produced during inference.
    pub(super) extension_blocks: Vec<TypedExtensionBlock>,
}

impl<'a> Inference<'a> {
    fn new(
        package_path: PackagePath,
        current_file: FilePath,
        registry: &'a Registry,
        import_scope: &'a ImportScope,
        diagnostics: &'a mut Diagnostics,
    ) -> Self {
        Self {
            package_path,
            current_file,
            registry,
            diagnostics,
            typed_functions: BTreeMap::new(),
            default_templates: BTreeMap::new(),
            typed_globals: BTreeMap::new(),
            typed_tests: Vec::new(),
            scopes: Vec::new(),
            loop_depth: 0,
            import_scope,
            class_type_defs: BTreeMap::new(),
            current_type_params: BTreeMap::new(),
            type_param_counter: 0,
            expected_type: None,
            current_expr_span: None,
            block_wrapped_error: None,
            function_return_type: None,
            async_return_type: None,
            async_discovery: None,
            container_name: None,
            current_module_name: None,
            unresolved_method_type_params: Vec::new(),
            typechecking_class: None,
            type_references: Vec::new(),
            implement_blocks: Vec::new(),
            extension_blocks: Vec::new(),
        }
    }

    /// Build a name → GenericParam map with fresh unique IDs for each type parameter.
    /// Used when entering a generic scope during inference.
    pub(super) fn type_param_map(
        &mut self,
        type_params: &[TypeParamName],
        trait_bounds: &crate::typechecker::types::TraitBounds,
    ) -> BTreeMap<String, Type> {
        type_params
            .iter()
            .map(|tp| {
                self.type_param_counter += 1;
                let bounds: Vec<crate::typechecker::types::TraitBound> =
                    trait_bounds.get(tp).cloned().unwrap_or_default();
                (
                    tp.0.clone(),
                    Type::GenericParam(tp.clone(), bounds, self.type_param_counter),
                )
            })
            .collect()
    }

    pub(super) fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    pub(super) fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Check whether a function/global signature is accessible from the current context.
    /// Uses the declaring package (from the containing module/type FQN) for Internal checks.
    pub(super) fn is_member_visible(
        &self,
        visibility: Visibility,
        member_package: &PackagePath,
        source_file: &FilePath,
    ) -> bool {
        match visibility {
            Visibility::Public | Visibility::Protected => true,
            Visibility::Internal => *member_package == self.package_path,
            Visibility::Private => *source_file == self.current_file,
        }
    }

    /// Check whether access to a private newtype's inner value is allowed.
    /// Returns `true` if access is allowed (either not private, or inside the associated module).
    /// Emits a diagnostic and returns `false` otherwise.
    pub(super) fn check_newtype_inner_access(
        &mut self,
        sig: &crate::typechecker::registry::NewtypeSignature,
        span: &crate::common::span::Span,
        operation: &str,
    ) -> bool {
        if sig.fqn.to_string() == "standard.prelude.ReadonlySlice" {
            self.diagnostics.error(
                span.clone(),
                format!("cannot {operation} the opaque ReadonlySlice representation"),
            );
            return false;
        }
        self.check_private_type_access(&sig.fqn, sig.inner_private, "newtype", span, operation)
    }

    /// Private operations belong exclusively to the associated module in its package.
    pub(super) fn check_private_type_access(
        &mut self,
        fqn: &Fqn,
        private: bool,
        kind: &str,
        span: &crate::common::span::Span,
        operation: &str,
    ) -> bool {
        if !private {
            return true;
        }
        let allowed = self.current_module_name.as_deref() == Some(fqn.symbol.0.as_str())
            && self.package_path == fqn.package;
        if !allowed {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "cannot {} private {} '{}' outside its associated module",
                    operation, kind, fqn.symbol,
                ),
            );
        }
        allowed
    }

    pub(super) fn define_variable(&mut self, name: VarName, ty: Type, mutable: bool) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.define(name, VarBinding { ty, mutable });
        }
    }

    pub(super) fn lookup_variable(&self, name: &str) -> Option<VarBinding> {
        for scope in self.scopes.iter().rev() {
            if let Some(binding) = scope.lookup(name) {
                return Some(binding.clone());
            }
        }
        None
    }

    /// For async functions, wrap the inferred body in an `AsyncBlock` node
    /// carrying the resolved `succeed` method from the Awaitable trait impl.
    /// For non-async functions, returns the body unchanged.
    pub(super) fn wrap_async_body(
        &mut self,
        body: TypedExpr,
        return_type: &Type,
        is_async: bool,
    ) -> TypedExpr {
        if !is_async {
            return body;
        }
        let awaitable_fqn = Fqn::from_dotted("standard.prelude.Awaitable").unwrap();
        let inner_type = match self.resolve_awaitable_value_type(return_type) {
            Some(ty) => ty,
            None => return body,
        };
        let (succeed_resolved, _) = match self.resolve_trait_impl_method_for_type(
            return_type,
            &awaitable_fqn,
            "succeed",
            &[&inner_type],
        ) {
            Some(result) => result,
            None => return body,
        };
        TypedExpr {
            kind: TypedExprKind::AsyncBlock {
                body: Box::new(body.clone()),
                succeed_method: succeed_resolved,
            },
            ty: body.ty.clone(),
            span: body.span.clone(),
        }
    }
}

pub(crate) fn class_trait_virtual_slot(
    registry: &Registry,
    receiver: &Type,
    trait_fqn: &Fqn,
    trait_parameters: &[Type],
    member: &SymbolName,
) -> Option<u32> {
    let mut diagnostics = Diagnostics::new();
    let inference = Inference::new(
        trait_fqn.package.clone(),
        FilePath::from("<specialization>"),
        registry,
        &EMPTY_IMPORT_SCOPE,
        &mut diagnostics,
    );
    inference.trait_virtual_slot(receiver, trait_fqn, trait_parameters, member)
}

/// Check resolved bounds before generating a concrete virtual-method body.
pub(crate) fn generic_bounds_satisfied(
    registry: &Registry,
    package: &PackagePath,
    bounds: &crate::typechecker::types::TraitBounds,
    parameters: &[TypeParamName],
    arguments: &[Type],
) -> bool {
    let mut diagnostics = Diagnostics::new();
    let inference = Inference::new(
        package.clone(),
        FilePath::from("<specialization>"),
        registry,
        &EMPTY_IMPORT_SCOPE,
        &mut diagnostics,
    );
    inference
        .unsatisfied_trait_bounds(bounds, parameters, arguments)
        .is_empty()
}

/// Complete operand-selected implementation parameters and validate their bounds.
/// All bound names are already resolved, so no source import scope is needed.
pub(crate) fn complete_impl_substitution(
    registry: &Registry,
    implementation: &crate::typechecker::registry::ImplBlockSignature,
    mut substitution: type_param_substitution::TypeParamSubstitution,
) -> Option<type_param_substitution::TypeParamSubstitution> {
    let mut diagnostics = Diagnostics::new();
    let inference = Inference::new(
        implementation.package.clone(),
        implementation.source_file.clone(),
        registry,
        &EMPTY_IMPORT_SCOPE,
        &mut diagnostics,
    );
    inference.infer_associated_bound_types(&implementation.trait_bounds, &mut substitution);
    let type_args = substitution.resolve_type_params(&implementation.type_params)?;
    inference
        .unsatisfied_trait_bounds(
            &implementation.trait_bounds,
            &implementation.type_params,
            &type_args,
        )
        .is_empty()
        .then_some(substitution)
}

/// Infer phase: type-check expression bodies and build the typed module.
pub fn infer(
    package_path: &PackagePath,
    files: &[&SourceFile],
    registry: &Registry,
    import_scopes: &PackageImportScopes,
    diagnostics: &mut Diagnostics,
) -> TypedModule {
    let normalization = crate::typechecker::associated_types::NormalizationScope::install(registry);
    let mut typed_functions = BTreeMap::new();
    let mut default_templates: BTreeMap<MangledName, TypedFunction> = BTreeMap::new();
    let mut typed_globals = BTreeMap::new();
    let mut typed_tests: Vec<TypedTest> = Vec::new();
    let mut class_type_defs: BTreeMap<MangledName, TypeDef> = BTreeMap::new();
    let mut type_references: Vec<TypeReference> = Vec::new();
    let mut implement_blocks: Vec<TypedImplementBlock> = Vec::new();
    let mut extension_blocks: Vec<TypedExtensionBlock> = Vec::new();

    for file in files {
        let file_path = file.package.span.file.clone();
        let import_scope = import_scopes.get(&file_path).unwrap_or(&EMPTY_IMPORT_SCOPE);
        let mut inference = Inference::new(
            package_path.clone(),
            file_path,
            registry,
            import_scope,
            diagnostics,
        );
        for decl in &file.declarations {
            match decl {
                Declaration::Function(func) => inference.infer_function(func),
                Declaration::GlobalVar(global) => inference.infer_global(global),
                Declaration::Record(_) => {}
                Declaration::Enum(_) => {}
                Declaration::Trait(trait_decl) => inference.infer_trait_defaults(trait_decl),
                Declaration::Extension(ext) => inference.infer_extension(ext),
                Declaration::Implement(impl_decl) => inference.infer_implement(impl_decl),
                Declaration::Module(module) => inference.infer_module(module),
                Declaration::Newtype(_) => {}
                Declaration::TypeAlias(_) => {}
                Declaration::Class(class) => inference.infer_class(class),
                Declaration::Test(test) => inference.infer_test(test),
            }
        }
        typed_functions.extend(inference.typed_functions);
        default_templates.extend(inference.default_templates);
        typed_globals.extend(inference.typed_globals);
        typed_tests.extend(inference.typed_tests);
        class_type_defs.extend(inference.class_type_defs);
        type_references.extend(inference.type_references);
        implement_blocks.extend(inference.implement_blocks);
        extension_blocks.extend(inference.extension_blocks);
    }

    // Populate type defs from registry for codegen
    let mut types = records::collect_record_type_defs(package_path, files, registry);
    // Merge in enum type defs
    types.extend(enums::collect_enum_type_defs(package_path, files, registry));
    // Merge in class type defs
    types.extend(class_type_defs);

    for message in normalization.errors() {
        if let Some(file) = files.first() {
            diagnostics.error(file.package.span.clone(), message);
        }
    }

    // Collect function signature types from the entire typed module
    TypedModule {
        main_function_fqn: None,
        functions: typed_functions,
        globals: typed_globals,
        types,
        tests: typed_tests,
        type_references,
        implement_blocks,
        extension_blocks,
        resources: std::collections::BTreeMap::new(),
        default_templates,
        function_templates: BTreeMap::new(),
        synthetic_interface_coercions: Vec::new(),
        direct_rebox_authorizations: Vec::new(),
    }
}
