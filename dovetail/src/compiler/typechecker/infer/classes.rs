use crate::common::span::{Span, Spanned};
use crate::common::types::{
    Fqn, MangledName, SymbolName, TypeParamName, VarName, Variance, Visibility,
};
use crate::parser::ast::{ClassDecl, ClassMember, FunctionDecl, PropertyDecl};

use crate::typechecker::types::{
    ClassFieldDef, ClassTypeDef, Type, TypeDef, TypedExpr, TypedExprKind, TypedFunction,
    TypedGlobal, TypedParam, VtableSlot,
};

/// Build the FQN for a class method: `package` from the class, `symbol` is `ClassName.methodName`.
/// Matches the mangling used by `MangledName::for_function(&method_fqn, &param_types)` so codegen
/// can derive the concrete function key as
/// `MangledName::for_function(impl_fqn, param_types).with_type_args(type_args)`.
fn class_method_fqn(class_fqn: &Fqn, method_name: &str) -> Fqn {
    Fqn {
        package: class_fqn.package.clone(),
        symbol: SymbolName(format!("{}.{}", class_fqn.symbol, method_name)),
    }
}

/// The `extends Parent<binding>` type binding: maps the parent's type params to
/// the args the child binds them to (in the child's type-param space). Empty
/// when the parent is non-generic. Used to re-express inherited vtable slots.
fn parent_binding(
    parent_sig: &ClassTypeSignature,
    parent_type_expr: Option<&Type>,
) -> std::collections::BTreeMap<TypeParamName, Type> {
    let mut binding = std::collections::BTreeMap::new();
    if parent_sig.type_params.is_empty() {
        return binding;
    }
    if let Some(Type::GenericClass { type_args, .. }) = parent_type_expr {
        for (tp, (_, t)) in parent_sig.type_params.iter().zip(type_args.iter()) {
            binding.insert(tp.clone(), t.clone());
        }
    }
    binding
}

use crate::typechecker::registry::{ClassTypeSignature, FunctionSignature, GenericClassMethodDef};

use super::generic_functions::MethodKind;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;
use super::{Inference, ResolvedFunction};
use crate::monomorphize::substitute::apply_type_substitution;

impl Inference<'_> {
    pub(super) fn trait_virtual_slot(
        &self,
        receiver: &Type,
        trait_fqn: &Fqn,
        trait_parameters: &[Type],
        member: &SymbolName,
    ) -> Option<u32> {
        let class_fqn = receiver.try_to_fqn()?;
        let class = self.registry.get_class_type(&class_fqn)?;
        let signature = self.registry.get_trait(trait_fqn)?;
        let method = signature
            .methods
            .iter()
            .find(|method| signature.method_dispatch_name(method) == *member);
        let property = signature
            .properties
            .iter()
            .find(|property| property.name == member.0);
        let (name, parameters, is_property) = match (method, property) {
            (Some(method), _) if method.type_params.is_empty() => {
                (&method.name, &method.params, false)
            }
            (_, Some(property)) => (&property.name, &property.params, true),
            _ => return None,
        };
        if !parameters.first().is_some_and(|(name, _)| name == "self") {
            return None;
        }
        let trait_substitution =
            TypeParamSubstitution::from_pairs(&signature.type_params, trait_parameters)
                .with_self_type(receiver.clone());
        let expected: Vec<_> = parameters
            .iter()
            .skip(1)
            .map(|(_, ty)| apply_substitution(&trait_substitution, ty))
            .collect();
        let class_parameters = match receiver {
            Type::GenericClass { type_args, .. } => {
                type_args.iter().map(|(_, ty)| ty.clone()).collect()
            }
            _ => Vec::new(),
        };
        let class_substitution =
            TypeParamSubstitution::from_pairs(&class.type_params, &class_parameters);
        self.compute_vtable_methods(&class_fqn, class)
            .iter()
            .position(|slot| {
                slot.is_property == is_property
                    && slot.method_name.0 == *name
                    && slot.param_types.len() == expected.len() + 1
                    && slot
                        .param_types
                        .iter()
                        .skip(1)
                        .zip(&expected)
                        .all(|(actual, expected)| {
                            crate::typechecker::subtyping::identical(
                                &apply_substitution(&class_substitution, actual),
                                expected,
                            )
                        })
            })
            .map(|index| index as u32)
    }

    /// Infer types for a class declaration: constructor body, let bindings, methods.
    /// For generic classes, typecheck method bodies with TypeParameter types in scope
    /// and register a class template TypeDef. Concrete ClassTypeDefs are created later
    /// during instantiation (see `infer_generic_class`).
    pub(super) fn infer_class(&mut self, class: &ClassDecl) {
        if !class.type_params.is_empty() {
            return self.infer_generic_class_declaration(class);
        }
        let class_name = class.name.value.clone();
        let prev_container = self.container_name.take();
        self.container_name = Some(class_name.clone());

        let class_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(class_name.clone()),
        };
        let class_sig = match self
            .registry
            .lookup_class_type(&class_fqn, &self.package_path)
        {
            Some(sig) => sig.clone(),
            None => {
                self.container_name = prev_container;
                return;
            }
        };

        self.check_inherited_class_trait_contracts(&class_sig);

        // 1. Push scope and define constructor params as variables
        self.push_scope();

        let mut constructor_typed_params = Vec::new();
        for param in &class.params {
            let ty = self.resolve_type_expr(&param.type_annotation);
            self.define_variable(VarName(param.name.value.clone()), ty.clone(), param.mutable);
            constructor_typed_params.push(TypedParam {
                name: param.name.value.clone(),
                ty: ty.clone(),
                span: param.name.span.clone(),
            });
        }

        // 2. If extends: infer extends args in current (child) scope
        let (typed_extends_args, resolved_parent_ty) = if let Some(ref ext) = class.extends {
            let parent_ty = self.resolve_type_expr(&ext.parent_type);
            match &parent_ty {
                Type::Class(_, _) | Type::GenericClass { .. } => {}
                _ if parent_ty.is_error() => {}
                _ => {
                    self.diagnostics.error(
                        ext.span.clone(),
                        format!("extends clause expected a class type, found '{parent_ty}'"),
                    );
                }
            }
            let args: Vec<TypedExpr> = ext.super_args.iter().map(|a| self.infer_expr(a)).collect();
            (Some(args), Some(parent_ty))
        } else {
            (None, None)
        };

        // 3. Infer own class body (let bindings + expressions) → own_body_stmts
        let mut own_body_stmts: Vec<TypedExpr> = Vec::new();
        let mut let_binding_info: Vec<(String, Type, Visibility, bool)> = Vec::new();

        for member in &class.body {
            match member {
                ClassMember::LetBinding(lb) if lb.is_static => {
                    // Static let bindings: infer as globals, not instance fields.
                    let global_fqn = Fqn {
                        package: self.package_path.clone(),
                        symbol: SymbolName(format!("{}.{}", class_name, lb.name.value)),
                    };
                    let mangled_name = MangledName::for_global(&global_fqn);

                    // Infer in a fresh scope (no self, no constructor params)
                    self.push_scope();
                    let typed_init = self.infer_expr(&lb.value);
                    self.pop_scope();

                    let ty = if let Some(ref type_ann) = lb.type_annotation {
                        let annotated = self.resolve_type_expr(type_ann);
                        self.check_assignable(lb.value.span(), &annotated, &typed_init.ty);
                        annotated
                    } else {
                        typed_init.ty.clone()
                    };

                    self.typed_globals.insert(
                        mangled_name.clone(),
                        TypedGlobal {
                            visibility: lb.visibility,
                            name: mangled_name,
                            mutable: lb.mutable,
                            ty,
                            initializer: typed_init,
                            span: lb.span.clone(),
                            type_params: vec![],
                        },
                    );
                }
                ClassMember::LetBinding(lb) => {
                    let annotated_ty = lb
                        .type_annotation
                        .as_ref()
                        .map(|ta| self.resolve_type_expr(ta));

                    let prev_expected = self.expected_type.take();
                    self.expected_type = annotated_ty.clone();
                    let value = self.infer_expr(&lb.value);
                    self.expected_type = prev_expected;

                    let ty = if let Some(expected) = annotated_ty {
                        self.check_assignable(lb.value.span(), &expected, &value.ty);
                        expected
                    } else {
                        value.ty.clone()
                    };

                    self.define_variable(VarName(lb.name.value.clone()), ty.clone(), lb.mutable);

                    own_body_stmts.push(TypedExpr {
                        kind: TypedExprKind::Let {
                            name: VarName(lb.name.value.clone()),
                            mutable: lb.mutable,
                            boxed: false,
                            var_ty: ty.clone(),
                            value: Box::new(value),
                        },
                        ty: Type::Unit,
                        span: lb.span.clone(),
                    });
                    let_binding_info.push((lb.name.value.clone(), ty, lb.visibility, lb.mutable));
                }
                ClassMember::Expression(expr) => {
                    let typed = self.infer_expr(expr);
                    own_body_stmts.push(typed);
                }
                ClassMember::Method(_) | ClassMember::Property(_) => {
                    // Methods and properties are inferred separately below
                }
            }
        }

        // 4. Build initializer_fields: [(param_name, param_ty)...] + [(let_name, let_ty)...]
        let mut initializer_fields: Vec<(String, Type)> = Vec::new();
        for param in &constructor_typed_params {
            initializer_fields.push((param.name.clone(), param.ty.clone()));
        }
        for (name, ty, _, _) in &let_binding_info {
            initializer_fields.push((name.clone(), ty.clone()));
        }

        // 5. Build initializer: individual statements (not wrapped in Block
        //    so that let-binding locals stay in scope for initializer_fields)
        let initializer = own_body_stmts;

        // 6. Build full fields list for ClassTypeDef:
        //    parent fields + own constructor params + own let bindings
        let class_mangled = MangledName::for_type(&class_fqn);
        let parent_mangled_name = match &resolved_parent_ty {
            Some(Type::Class(_, mn))
            | Some(Type::GenericClass {
                mangled_name: mn, ..
            }) => Some(mn.clone()),
            _ => class_sig.parent_class.as_ref().map(MangledName::for_type),
        };

        let mut class_fields: Vec<ClassFieldDef> = Vec::new();

        // Parent fields first (from parent's ClassTypeDef). We copy them VERBATIM —
        // including any TypeVar/GenericParam left from the parent's canonical erased layout.
        // This ensures the child's WASM struct matches the parent's WASM struct for subtype
        // validation (anyref-at-parent-position stays anyref-at-child-position). The typechecker
        // substitutes type-args at field-access sites via `try_resolve_class_field_from_registry`,
        // which walks the parent chain using `parent_type_expr`.
        if let Some(ref parent_mn) = parent_mangled_name {
            if let Some(TypeDef::Class(parent_cls)) = self.class_type_defs.get(parent_mn) {
                for f in &parent_cls.fields {
                    class_fields.push(f.clone());
                }
            } else if let Some(parent_fqn) = &class_sig.parent_class {
                // Dependency classes may only exist in the registry during inference.
                // Preserve their canonical (erased) field layout, just as above.
                self.collect_canonical_parent_fields(parent_fqn, &mut class_fields);
            }
        }

        // Child constructor params
        for param in &constructor_typed_params {
            let (vis, mutable) = class
                .params
                .iter()
                .find(|p| p.name.value == param.name)
                .map(|p| (p.visibility, p.mutable))
                .unwrap_or((Visibility::Internal, false));
            class_fields.push(ClassFieldDef {
                name: param.name.clone(),
                ty: param.ty.clone(),
                visibility: vis,
                mutable,
                declared_by: class_fqn.clone(),
            });
        }

        // Child let bindings
        for (name, ty, visibility, mutable) in &let_binding_info {
            class_fields.push(ClassFieldDef {
                name: name.clone(),
                ty: ty.clone(),
                visibility: *visibility,
                mutable: *mutable,
                declared_by: class_fqn.clone(),
            });
        }

        // Compute vtable methods
        let vtable_methods = self.compute_vtable_methods(&class_fqn, &class_sig);

        // Compute hierarchy root: walk parent chain up to the topmost class
        let mut hierarchy_root_mangled = class_mangled.clone();
        {
            let mut current_parent = parent_mangled_name.clone();
            while let Some(ref p_mn) = current_parent {
                hierarchy_root_mangled = p_mn.clone();
                current_parent = if let Some(TypeDef::Class(p_cls)) = self.class_type_defs.get(p_mn)
                {
                    p_cls.parent_mangled_name.clone()
                } else {
                    None
                };
            }
        }

        let class_type_def = ClassTypeDef {
            fqn: class_fqn.clone(),
            mangled_name: class_mangled.clone(),
            fields: class_fields,
            is_final: class.is_final,
            is_abstract: class.is_abstract,
            is_sealed: class.is_sealed,
            parent_mangled_name,
            parent_type: resolved_parent_ty.clone(),
            vtable_methods,
            hierarchy_root_mangled: hierarchy_root_mangled.clone(),
            constructor_params: constructor_typed_params,
            initializer,
            initializer_fields,
            extends_args: typed_extends_args,
            type_params: vec![],
            span: class.span.clone(),
        };
        self.class_type_defs
            .insert(class_mangled.clone(), TypeDef::Class(class_type_def));

        self.pop_scope();

        // Infer method and property bodies
        for member in &class.body {
            let method_context = self.current_type_params.clone();
            match member {
                ClassMember::Method(func) => {
                    self.check_class_method_contract(func, &class_sig);
                    if func.is_abstract && !func.type_params.is_empty() {
                        self.register_abstract_template_member(
                            func,
                            &class_fqn,
                            &[],
                            class_sig.is_final,
                            false,
                        );
                    } else if func.is_abstract {
                        self.register_abstract_method_placeholder(&class_name, func);
                    } else if !func.type_params.is_empty() {
                        self.infer_template_class_method_doubly_generic(func, &class_fqn, &[]);
                    } else {
                        self.infer_function(func);
                    }
                }
                ClassMember::Property(prop) => {
                    let func_decl = self.property_to_function_decl(prop);
                    if prop.is_abstract && !prop.type_params.is_empty() {
                        self.register_abstract_template_member(
                            &func_decl,
                            &class_fqn,
                            &[],
                            class_sig.is_final,
                            true,
                        );
                    } else if prop.is_abstract {
                        self.register_abstract_property_placeholder(&class_name, prop);
                    } else if !prop.type_params.is_empty() {
                        self.infer_template_class_method_doubly_generic(
                            &func_decl,
                            &class_fqn,
                            &[],
                        );
                    } else {
                        self.infer_function(&func_decl);
                    }
                }
                _ => {}
            }
            self.current_type_params = method_context;
        }

        // Set vtable_self_type on virtual methods: use hierarchy root class type as param 0
        {
            let root_mn = &hierarchy_root_mangled;
            let root_fqn = if let Some(TypeDef::Class(root_cls)) = self.class_type_defs.get(root_mn)
            {
                root_cls.fqn.clone()
            } else {
                class_fqn.clone()
            };
            let root_type = Type::Class(root_fqn, root_mn.clone());

            if let Some(TypeDef::Class(cls_def)) = self.class_type_defs.get(&class_mangled) {
                // Each slot's impl is found by mangling its `impl_fqn` + `param_types` —
                // the same `for_function` recipe the typechecker used when the method was
                // inserted into `typed_functions`.
                let impl_mangles: Vec<MangledName> = cls_def
                    .vtable_methods
                    .iter()
                    .map(|slot| MangledName::for_function(&slot.impl_fqn, &slot.param_types))
                    .collect();
                for impl_mn in impl_mangles {
                    if let Some(func) = self.typed_functions.get_mut(&impl_mn) {
                        func.vtable_self_type = Some(root_type.clone());
                    }
                }
            }
        }

        self.container_name = prev_container;
    }

    /// Infer a generic class declaration: typecheck method bodies with TypeParameter types
    /// in scope, register a class template TypeDef. Does not produce concrete TypedFunction
    /// entries or ClassTypeDef — those are created during instantiation (`infer_generic_class`).
    fn infer_generic_class_declaration(&mut self, class: &ClassDecl) {
        let class_name = class.name.value.clone();
        let prev_container = self.container_name.take();
        self.container_name = Some(class_name.clone());

        let class_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(class_name),
        };

        let class_sig = match self
            .registry
            .lookup_class_type(&class_fqn, &self.package_path)
        {
            Some(sig) => sig.clone(),
            None => {
                self.container_name = prev_container;
                return;
            }
        };

        self.check_inherited_class_trait_contracts(&class_sig);

        // Build type param map: T → TypeParameter(T, bounds)
        let type_param_map = self.type_param_map(&class_sig.type_params, &class_sig.trait_bounds);

        let prev_type_params = std::mem::take(&mut self.current_type_params);
        for tp in &class_sig.type_params {
            if let Some(ty) = type_param_map.get(&tp.0) {
                self.current_type_params.insert(tp.clone(), ty.clone());
            }
        }

        // Push scope with constructor params (types may contain TypeParameter).
        // Under full erasure, variance coercions are no-ops on shared WASM struct types,
        // so the variance-related mutable-field boxing mechanism is no longer needed.
        // `ClassFieldDef.boxed` stays at `false`; codegen's `boxed` branches are dead
        // but inert until cleanup in a later phase.
        self.push_scope();
        let mut class_fields: Vec<ClassFieldDef> = Vec::new();
        let mut template_constructor_params: Vec<TypedParam> = Vec::new();
        let mut template_initializer_fields: Vec<(String, Type)> = Vec::new();
        for param in &class.params {
            let ty = self.resolve_type_expr(&param.type_annotation);
            self.define_variable(VarName(param.name.value.clone()), ty.clone(), param.mutable);
            class_fields.push(ClassFieldDef {
                name: param.name.value.clone(),
                ty: ty.clone(),
                visibility: param.visibility,
                mutable: param.mutable,
                declared_by: class_fqn.clone(),
            });
            template_constructor_params.push(TypedParam {
                name: param.name.value.clone(),
                ty: ty.clone(),
                span: class_sig.span.clone(),
            });
            template_initializer_fields.push((param.name.value.clone(), ty));
        }

        // Type-check let bindings and body expressions, capturing initializer for template
        let mut template_initializer: Vec<TypedExpr> = Vec::new();
        for member in &class.body {
            match member {
                ClassMember::LetBinding(lb) => {
                    if lb.is_static {
                        // Static let bindings: typecheck in empty scope, store as template TypedGlobal
                        self.push_scope();
                        let annotated_ty = lb
                            .type_annotation
                            .as_ref()
                            .map(|ta| self.resolve_type_expr(ta));
                        let prev_expected = self.expected_type.take();
                        self.expected_type = annotated_ty.clone();
                        let value = self.infer_expr(&lb.value);
                        self.expected_type = prev_expected;
                        self.pop_scope();
                        let ty = if let Some(annotated) = annotated_ty {
                            self.check_assignable(lb.value.span(), &annotated, &value.ty);
                            annotated
                        } else {
                            value.ty.clone()
                        };
                        // Create template TypedGlobal for monomorphize
                        let global_fqn = Fqn {
                            package: class_fqn.package.clone(),
                            symbol: SymbolName(format!("{}.{}", class_fqn.symbol, lb.name.value)),
                        };
                        let global_mn = MangledName::for_global(&global_fqn);
                        // Statics on generic classes are stored as one canonical global
                        // (not a per-instantiation template) — the rules pass guarantees the
                        // declared type doesn't reference the class's type parameters, so all
                        // `Box<T>.field` accesses resolve to the same storage.
                        self.typed_globals.insert(
                            global_mn.clone(),
                            TypedGlobal {
                                visibility: lb.visibility,
                                name: global_mn,
                                mutable: lb.mutable,
                                ty,
                                initializer: value,
                                span: lb.span.clone(),
                                type_params: vec![],
                            },
                        );
                        // Do NOT add to class_fields or define as a variable
                    } else {
                        // Resolve annotation first to set expected_type (enables correct
                        // type inference for e.g. Option.None → Option<FiberResult>)
                        let annotated_ty = lb
                            .type_annotation
                            .as_ref()
                            .map(|ta| self.resolve_type_expr(ta));
                        let prev_expected = self.expected_type.take();
                        self.expected_type = annotated_ty.clone();
                        let value = self.infer_expr(&lb.value);
                        self.expected_type = prev_expected;
                        let ty = if let Some(annotated) = annotated_ty {
                            self.check_assignable(lb.value.span(), &annotated, &value.ty);
                            annotated
                        } else {
                            value.ty.clone()
                        };
                        self.define_variable(
                            VarName(lb.name.value.clone()),
                            ty.clone(),
                            lb.mutable,
                        );
                        class_fields.push(ClassFieldDef {
                            name: lb.name.value.clone(),
                            ty: ty.clone(),
                            visibility: lb.visibility,
                            mutable: lb.mutable,
                            declared_by: class_fqn.clone(),
                        });
                        template_initializer.push(TypedExpr {
                            kind: TypedExprKind::Let {
                                name: VarName(lb.name.value.clone()),
                                mutable: lb.mutable,
                                boxed: false, // Boxing is handled at struct field push time, not in the local
                                var_ty: ty.clone(),
                                value: Box::new(value),
                            },
                            ty: Type::Unit,
                            span: class_sig.span.clone(),
                        });
                        template_initializer_fields.push((lb.name.value.clone(), ty));
                    }
                }
                ClassMember::Expression(expr) => {
                    let typed = self.infer_expr(expr);
                    template_initializer.push(typed);
                }
                _ => {}
            }
        }

        // Infer extends args with TypeParameter types (constructor params still in scope)
        let template_extends_args = if let Some(ref ext) = class.extends {
            if !ext.super_args.is_empty() {
                Some(ext.super_args.iter().map(|a| self.infer_expr(a)).collect())
            } else {
                None
            }
        } else {
            None
        };

        self.pop_scope();

        // Set typechecking_class so field access works in method bodies
        let class_mangled = MangledName::for_type(&class_fqn);

        // Register class template (keyed by base mangled name) so check-mode LSP and
        // `is_generic_template()` filtering work consistently with records and enums.
        // Template has type_params set → monomorphize replaces it before codegen sees it.
        // Contains full template data: constructor_params, initializer, initializer_fields,
        // extends_args — all with TypeParameter types for monomorphize substitution.
        if !class_sig.type_params.is_empty() && !self.class_type_defs.contains_key(&class_mangled) {
            let parent_mangled_name = class_sig.parent_class.as_ref().map(MangledName::for_type);

            // Walk parent chain for hierarchy root (monomorphize will replace with concrete)
            let mut hierarchy_root_mangled = class_mangled.clone();
            {
                let mut current_parent = parent_mangled_name.clone();
                while let Some(ref p_mn) = current_parent {
                    hierarchy_root_mangled = p_mn.clone();
                    current_parent =
                        if let Some(TypeDef::Class(p_cls)) = self.class_type_defs.get(p_mn) {
                            p_cls.parent_mangled_name.clone()
                        } else {
                            None
                        };
                }
            }

            self.class_type_defs.insert(
                class_mangled.clone(),
                TypeDef::Class(ClassTypeDef {
                    fqn: class_fqn.clone(),
                    mangled_name: class_mangled.clone(),
                    fields: class_fields.clone(),
                    is_final: class_sig.is_final,
                    is_abstract: class_sig.is_abstract,
                    is_sealed: class_sig.is_sealed,
                    parent_mangled_name,
                    parent_type: None,      // Resolved during monomorphize
                    vtable_methods: vec![], // Populated after method body inference below
                    hierarchy_root_mangled,
                    constructor_params: template_constructor_params,
                    initializer: template_initializer,
                    initializer_fields: template_initializer_fields,
                    extends_args: template_extends_args,
                    type_params: class_sig.type_params.clone(),
                    span: class_sig.span.clone(),
                }),
            );
        }

        self.typechecking_class = Some((class_mangled.clone(), class_fields));

        // Create the template TypedFunctions for each member: a panic-body
        // placeholder for abstract members (so the base vtable slot has a concrete
        // function under erasure), and an inferred-body template for concrete ones.
        // The vtable LAYOUT itself comes from `compute_vtable_methods` (registry-
        // based, identical at every cross-file/package call site), set below.
        for member in &class.body {
            let method_context = self.current_type_params.clone();
            match member {
                ClassMember::Method(func) => {
                    self.check_class_method_contract(func, &class_sig);
                    if func.is_abstract {
                        self.register_abstract_template_member(
                            func,
                            &class_fqn,
                            &class_sig.type_params,
                            class_sig.is_final,
                            false,
                        );
                    } else if !func.type_params.is_empty() {
                        self.infer_template_class_method_doubly_generic(
                            func,
                            &class_fqn,
                            &class_sig.type_params,
                        );
                    } else {
                        self.infer_template_class_method(func, &class_fqn, &class_sig.type_params);
                    }
                }
                ClassMember::Property(prop) => {
                    let func_decl = self.property_to_function_decl(prop);
                    if prop.is_abstract {
                        self.register_abstract_template_member(
                            &func_decl,
                            &class_fqn,
                            &class_sig.type_params,
                            class_sig.is_final,
                            true,
                        );
                    } else if !func_decl.type_params.is_empty() {
                        self.infer_template_class_method_doubly_generic(
                            &func_decl,
                            &class_fqn,
                            &class_sig.type_params,
                        );
                    } else {
                        self.infer_template_class_method(
                            &func_decl,
                            &class_fqn,
                            &class_sig.type_params,
                        );
                    }
                }
                _ => {}
            }
            self.current_type_params = method_context;
        }

        // Set the vtable layout from the single registry-based source of truth.
        let template_vtable_methods = self.compute_vtable_methods(&class_fqn, &class_sig);
        if let Some(TypeDef::Class(cls)) = self.class_type_defs.get_mut(&class_mangled) {
            cls.vtable_methods = template_vtable_methods;
        }

        // Set vtable_self_type on virtual method templates: use the hierarchy root
        // class type as param 0, so codegen casts `self` to the slot's erased
        // signature. Mirrors the non-generic path in `infer_class`.
        {
            let (root_mn, impl_mangles) =
                if let Some(TypeDef::Class(cls_def)) = self.class_type_defs.get(&class_mangled) {
                    let impl_mangles: Vec<MangledName> = cls_def
                        .vtable_methods
                        .iter()
                        .map(|slot| MangledName::for_function(&slot.impl_fqn, &slot.param_types))
                        .collect();
                    (cls_def.hierarchy_root_mangled.clone(), impl_mangles)
                } else {
                    (class_mangled.clone(), vec![])
                };
            let root_fqn =
                if let Some(TypeDef::Class(root_cls)) = self.class_type_defs.get(&root_mn) {
                    root_cls.fqn.clone()
                } else {
                    class_fqn.clone()
                };
            let root_type = Type::Class(root_fqn, root_mn);
            for impl_mn in impl_mangles {
                if let Some(func) = self.typed_functions.get_mut(&impl_mn) {
                    func.vtable_self_type = Some(root_type.clone());
                }
            }
        }

        self.typechecking_class = None;

        // Restore state
        self.current_type_params = prev_type_params;
        self.container_name = prev_container;
    }

    /// Create trait impl function aliases for a generic class instantiation.
    /// After generic class methods are instantiated, this creates copies of those
    /// functions under the trait impl mangled name so vtable entries can find them.
    /// Try to resolve a field access on a class type.
    /// Returns `Some(TypedExpr)` if the field was found, `None` to fall through.
    ///
    /// For same-package classes: uses ClassTypeDef (has all fields including non-public).
    /// For cross-package classes: uses registry ClassTypeSignature (only public fields,
    /// with stable field_index values).
    /// Walks parent chain if field not found on the concrete class.
    pub(super) fn try_resolve_class_field(
        &mut self,
        typed_object: &TypedExpr,
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        let (fqn, _mn) = match &typed_object.ty {
            Type::Class(fqn, mn) => (fqn, mn),
            Type::GenericClass {
                fqn,
                mangled_name: mn,
                ..
            } => (fqn, mn),
            _ => return None,
        };

        // For GenericClass, use the mangled name from the type (includes type args);
        // for plain Class, use MangledName::for_type
        let class_mangled = match &typed_object.ty {
            Type::GenericClass { mangled_name, .. } => mangled_name.clone(),
            _ => MangledName::for_type(fqn),
        };

        // Check typechecking_class first (set during generic class body typechecking)
        let tc_class_fields = self
            .typechecking_class
            .as_ref()
            .filter(|(mn, _)| *mn == class_mangled || *mn == MangledName::for_type(fqn))
            .map(|(_, fields)| {
                let mut fields = fields.clone();
                if let Type::GenericClass { type_args, .. } = &typed_object.ty
                    && let Some(signature) =
                        self.registry.lookup_class_type(fqn, &self.package_path)
                {
                    let arguments: Vec<_> = type_args.iter().map(|(_, ty)| ty.clone()).collect();
                    let substitution =
                        TypeParamSubstitution::from_pairs(&signature.type_params, &arguments);
                    for field in &mut fields {
                        field.ty = apply_substitution(&substitution, &field.ty);
                    }
                }
                fields
            });

        if fqn.package == self.package_path || matches!(&typed_object.ty, Type::GenericClass { .. })
        {
            // Same package (or generic class): use ClassTypeDef for full field access
            // For generic classes in typecheck-only mode, the ClassTypeDef is stored with
            // MangledName::for_type (no type args), so also try that as fallback.
            let fields = tc_class_fields.or_else(|| {
                let cls_lookup = self
                    .class_type_defs
                    .get(&class_mangled)
                    .or_else(|| self.class_type_defs.get(&MangledName::for_type(fqn)));
                if let Some(TypeDef::Class(cls)) = cls_lookup {
                    if cls.type_params.is_empty() {
                        Some(cls.fields.clone())
                    } else {
                        // Template ClassTypeDef — apply type arg substitution to fields
                        self.substitute_template_class_fields(cls, &typed_object.ty)
                    }
                } else {
                    None
                }
            });
            if let Some(fields) = fields {
                let mut inaccessible_match = false;
                for (idx, f) in fields.iter().enumerate() {
                    if f.name == field.value {
                        if self.is_field_accessible(f.visibility, &f.declared_by) {
                            // If the field was declared by a different (parent) class, the
                            // type stored on `cls.fields` may carry the parent's TypeVar
                            // (canonical/erased). Recompute the concrete type via the registry
                            // walk that applies parent_type_expr substitution.
                            let field_ty = if &f.declared_by != fqn {
                                self.resolve_inherited_field_type_via_registry(
                                    typed_object,
                                    fqn,
                                    &field.value,
                                )
                                .unwrap_or_else(|| f.ty.clone())
                            } else {
                                f.ty.clone()
                            };
                            return Some(TypedExpr {
                                kind: TypedExprKind::FieldAccess {
                                    object: Box::new(typed_object.clone()),
                                    field_name: field.value.clone(),
                                    field_index: idx as u32,
                                    boxed: false,
                                },
                                ty: field_ty,
                                span: span.clone(),
                            });
                        }
                        inaccessible_match = true;
                    }
                }
                if inaccessible_match {
                    // Emit error for the first name match
                    for f in &fields {
                        if f.name == field.value {
                            self.check_class_field_visibility(f.visibility, &f.declared_by, span);
                            break;
                        }
                    }
                    return Some(TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    });
                }
                return None;
            }
            // Fall back to registry (handles class-defined-later-in-same-file case)
        }

        // Cross-package (or fallback): use registry — walk parent chain
        self.try_resolve_class_field_from_registry(typed_object, fqn, field, span)
    }

    /// Apply type arg substitution to a template ClassTypeDef's fields.
    /// Returns substituted fields when the object type is GenericClass with concrete type_args,
    /// None if substitution cannot be built (e.g. type is not GenericClass).
    ///
    /// The template only stores child-own fields (no inherited fields), so this also
    /// resolves parent fields from the registry with substitution applied.
    pub(super) fn substitute_template_class_fields(
        &self,
        cls: &ClassTypeDef,
        object_ty: &Type,
    ) -> Option<Vec<ClassFieldDef>> {
        if let Type::GenericClass { fqn, type_args, .. } = object_ty {
            let concrete_types: Vec<Type> = type_args.iter().map(|(_, t)| t.clone()).collect();
            let sub = TypeParamSubstitution::from_pairs(&cls.type_params, &concrete_types);

            let mut fields = Vec::new();

            // Resolve parent fields from registry with substitution
            if let Some(class_sig) = self.registry.lookup_class_type(fqn, &self.package_path) {
                let class_sig = class_sig.clone();
                if let Some(ref parent_fqn) = class_sig.parent_class {
                    // Resolve the parent type by substituting into parent_type_expr
                    let resolved_parent_type = class_sig
                        .parent_type_expr
                        .as_ref()
                        .map(|pt| apply_substitution(&sub, pt));
                    self.collect_parent_fields_from_registry(
                        parent_fqn,
                        &resolved_parent_type,
                        &mut fields,
                    );
                }
            }

            // Add child's own fields with substitution
            for f in &cls.fields {
                fields.push(ClassFieldDef {
                    name: f.name.clone(),
                    ty: apply_substitution(&sub, &f.ty),
                    visibility: f.visibility,
                    mutable: f.mutable,
                    declared_by: f.declared_by.clone(),
                });
            }

            Some(fields)
        } else {
            None
        }
    }

    /// Recursively collect parent fields from the registry, applying substitution.
    /// `parent_type` is the resolved parent type (e.g. Holder<String>) — used to extract
    /// the parent's type args for substitution into the parent's field types.
    fn collect_parent_fields_from_registry(
        &self,
        parent_fqn: &Fqn,
        parent_type: &Option<Type>,
        fields: &mut Vec<ClassFieldDef>,
    ) {
        let Some(parent_sig) = self
            .registry
            .lookup_class_type(parent_fqn, &self.package_path)
        else {
            return;
        };
        let parent_sig = parent_sig.clone();

        // Build substitution from resolved parent type args → parent's type params
        let parent_sub = if !parent_sig.type_params.is_empty() {
            if let Some(Type::GenericClass { type_args, .. }) = parent_type {
                let concrete: Vec<Type> = type_args.iter().map(|(_, t)| t.clone()).collect();
                Some(TypeParamSubstitution::from_pairs(
                    &parent_sig.type_params,
                    &concrete,
                ))
            } else {
                None
            }
        } else {
            None
        };

        // Recurse to grandparent: resolve the grandparent type through parent's substitution
        if let Some(ref grandparent_fqn) = parent_sig.parent_class {
            let grandparent_type = parent_sig.parent_type_expr.as_ref().map(|pt| {
                if let Some(ref sub) = parent_sub {
                    apply_substitution(sub, pt)
                } else {
                    pt.clone()
                }
            });
            self.collect_parent_fields_from_registry(grandparent_fqn, &grandparent_type, fields);
        }

        // Add parent's own fields with substitution
        for f in &parent_sig.fields {
            let field_ty = if let Some(ref sub) = parent_sub {
                apply_substitution(sub, &f.ty)
            } else {
                f.ty.clone()
            };
            fields.push(ClassFieldDef {
                name: f.name.clone(),
                ty: field_ty,
                visibility: f.visibility,
                mutable: f.mutable,
                declared_by: parent_fqn.clone(),
            });
        }
    }

    /// Walk the parent chain via `parent_type_expr` substitution and return the concrete
    /// type of an inherited field. Used when `cls.fields` contains a parent-declared field
    /// whose stored type still carries the parent's TypeVar (post-Phase-1 erased layout).
    fn resolve_inherited_field_type_via_registry(
        &self,
        typed_object: &TypedExpr,
        fqn: &Fqn,
        field_name: &str,
    ) -> Option<Type> {
        let class_sig = self
            .registry
            .lookup_class_type(fqn, &self.package_path)?
            .clone();
        let substitution = if let Type::GenericClass { type_args, .. } = &typed_object.ty {
            if !class_sig.type_params.is_empty() {
                let concrete_types: Vec<Type> = type_args.iter().map(|(_, t)| t.clone()).collect();
                Some(TypeParamSubstitution::from_pairs(
                    &class_sig.type_params,
                    &concrete_types,
                ))
            } else {
                None
            }
        } else {
            None
        };
        let parent_fqn = class_sig.parent_class.clone()?;
        let parent_type = class_sig.parent_type_expr.as_ref().map(|pt| {
            if let Some(ref sub) = substitution {
                super::generics::apply_substitution(sub, pt)
            } else {
                pt.clone()
            }
        });
        let parent_obj = TypedExpr {
            kind: typed_object.kind.clone(),
            ty: parent_type.unwrap_or_else(|| typed_object.ty.clone()),
            span: typed_object.span.clone(),
        };
        self.resolve_inherited_field_type_recurse(&parent_obj, &parent_fqn, field_name)
    }

    fn resolve_inherited_field_type_recurse(
        &self,
        typed_object: &TypedExpr,
        fqn: &Fqn,
        field_name: &str,
    ) -> Option<Type> {
        let class_sig = self
            .registry
            .lookup_class_type(fqn, &self.package_path)?
            .clone();
        let substitution = if let Type::GenericClass { type_args, .. } = &typed_object.ty {
            if !class_sig.type_params.is_empty() {
                let concrete_types: Vec<Type> = type_args.iter().map(|(_, t)| t.clone()).collect();
                Some(TypeParamSubstitution::from_pairs(
                    &class_sig.type_params,
                    &concrete_types,
                ))
            } else {
                None
            }
        } else {
            None
        };
        for f in class_sig.fields.iter() {
            if f.name == field_name {
                let field_ty = if let Some(ref sub) = substitution {
                    super::generics::apply_substitution(sub, &f.ty)
                } else {
                    f.ty.clone()
                };
                return Some(field_ty);
            }
        }
        if let Some(ref parent_fqn) = class_sig.parent_class {
            let parent_type = class_sig.parent_type_expr.as_ref().map(|pt| {
                if let Some(ref sub) = substitution {
                    super::generics::apply_substitution(sub, pt)
                } else {
                    pt.clone()
                }
            });
            let parent_obj = TypedExpr {
                kind: typed_object.kind.clone(),
                ty: parent_type.unwrap_or_else(|| typed_object.ty.clone()),
                span: typed_object.span.clone(),
            };
            return self.resolve_inherited_field_type_recurse(&parent_obj, parent_fqn, field_name);
        }
        None
    }

    /// Walk the parent chain in the registry to find a field.
    fn try_resolve_class_field_from_registry(
        &mut self,
        typed_object: &TypedExpr,
        class_fqn: &Fqn,
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        let class_sig = self
            .registry
            .lookup_class_type(class_fqn, &self.package_path)?
            .clone();

        // Build type param substitution for generic classes so field types are concrete.
        let substitution = if let Type::GenericClass { type_args, .. } = &typed_object.ty {
            if !class_sig.type_params.is_empty() {
                let concrete_types: Vec<Type> = type_args.iter().map(|(_, t)| t.clone()).collect();
                Some(TypeParamSubstitution::from_pairs(
                    &class_sig.type_params,
                    &concrete_types,
                ))
            } else {
                None
            }
        } else {
            None
        };

        // Compute whether this generic class has variance (for boxing mutable fields)
        let has_variance = class_sig
            .type_param_variances
            .iter()
            .any(|v| *v != crate::common::types::Variance::Invariant);

        // Count parent fields so field_index accounts for inherited fields
        let parent_field_count = self.count_parent_fields_from_registry(class_fqn, &substitution);

        for (idx, f) in class_sig.fields.iter().enumerate() {
            if f.name == field.value {
                if !self.check_class_field_visibility(f.visibility, class_fqn, span) {
                    return Some(TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    });
                }
                let field_ty = if let Some(ref sub) = substitution {
                    super::generics::apply_substitution(sub, &f.ty)
                } else {
                    f.ty.clone()
                };
                let boxed = has_variance && f.mutable;
                return Some(TypedExpr {
                    kind: TypedExprKind::FieldAccess {
                        object: Box::new(typed_object.clone()),
                        field_name: field.value.clone(),
                        field_index: (parent_field_count + idx) as u32,
                        boxed,
                    },
                    ty: field_ty,
                    span: span.clone(),
                });
            }
        }

        // Walk parent chain to find inherited fields. Use `parent_type_expr` to derive
        // the parent's concrete instantiation type so the recursive lookup can substitute
        // any type parameters in inherited field types. This applies even when the child
        // is non-generic (so `substitution` is None) but extends a generic parent with
        // concrete type args (e.g. `IntHolder extends Base<Int32>`).
        if let Some(ref parent_fqn) = class_sig.parent_class {
            let parent_type = class_sig.parent_type_expr.as_ref().map(|pt| {
                if let Some(ref sub) = substitution {
                    super::generics::apply_substitution(sub, pt)
                } else {
                    pt.clone()
                }
            });
            // Build a typed_object with the parent type for correct type substitution
            let parent_obj = if let Some(parent_ty) = parent_type {
                TypedExpr {
                    kind: typed_object.kind.clone(),
                    ty: parent_ty,
                    span: typed_object.span.clone(),
                }
            } else {
                typed_object.clone()
            };
            return self.try_resolve_class_field_from_registry(
                &parent_obj,
                parent_fqn,
                field,
                span,
            );
        }

        // Field not found — fall through
        None
    }

    /// Count total parent fields by walking the parent chain in the registry.
    fn count_parent_fields_from_registry(
        &self,
        class_fqn: &Fqn,
        _substitution: &Option<TypeParamSubstitution>,
    ) -> usize {
        let class_sig = match self
            .registry
            .lookup_class_type(class_fqn, &self.package_path)
        {
            Some(sig) => sig,
            None => return 0,
        };
        if let Some(ref parent_fqn) = class_sig.parent_class {
            let parent_sig = match self
                .registry
                .lookup_class_type(parent_fqn, &self.package_path)
            {
                Some(sig) => sig,
                None => return 0,
            };
            self.count_parent_fields_from_registry(parent_fqn, _substitution)
                + parent_sig.fields.len()
        } else {
            0
        }
    }

    /// Try to resolve a method call on a class instance.
    /// Returns `Some(TypedExpr)` if the method was found, `None` to fall through.
    /// Walks parent chain if method not found on the concrete class.
    /// Uses ClassVirtualCall for virtual methods, FunctionCall for final methods.
    pub(super) fn try_resolve_class_instance_method(
        &mut self,
        typed_object: &TypedExpr,
        method: &Spanned<String>,
        typed_args: Vec<TypedExpr>,
        explicit_method_type_args: &[crate::parser::ast::TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        let (fqn, _mn, class_type_args) = match &typed_object.ty {
            Type::Class(fqn, mn) => (fqn, mn, vec![]),
            Type::GenericClass {
                fqn,
                mangled_name: mn,
                type_args,
            } => (fqn, mn, type_args.iter().map(|(_, t)| t.clone()).collect()),
            _ => return None,
        };

        // Walk the class hierarchy looking for the method.
        // `class_type_args` tracks the concrete type args at the current level —
        // updated when walking to a parent by substituting the current args into
        // the parent's `parent_type_expr`. Without this, an instance of a final
        // subclass (e.g. `MakeWaiter extends Async<WaiterId, Never>`) would walk
        // up to `Async` with `class_type_args = []`, leaving the generic
        // method-lookup unable to bind the parent's `T` / `E`.
        let mut current_fqn = fqn.clone();
        let mut class_type_args = class_type_args;
        loop {
            let class_sig = self
                .registry
                .lookup_class_type(&current_fqn, &self.package_path)?
                .clone();
            let method_name = SymbolName(method.value.clone());

            if let Some(overloads) =
                class_sig
                    .instance_methods
                    .get(&method_name)
                    .filter(|methods| {
                        explicit_method_type_args.is_empty()
                            && methods.iter().any(|method| !method.is_property)
                    })
            {
                // Build full args including self
                let mut full_args = vec![typed_object.clone()];
                full_args.extend(typed_args);

                let arg_types: Vec<&Type> = full_args.iter().map(|a| &a.ty).collect();

                // Find matching overload
                let matching: Vec<_> = overloads
                    .iter()
                    .filter(|sig| {
                        !sig.is_property
                            && sig.matches_args(&arg_types, |p, a| self.is_assignable(p, a))
                    })
                    .collect();

                if !matching.is_empty()
                    && !self.check_class_field_visibility(
                        matching[0].visibility,
                        &current_fqn,
                        span,
                    )
                {
                    return Some(TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    });
                }

                if matching.len() == 1 {
                    let sig = matching[0];
                    let return_type = sig.return_type.clone();

                    // Check if this method should use virtual dispatch
                    if self.is_method_virtual(fqn, &method.value)
                        && let Some(vtable_slot) = self.find_vtable_slot(
                            &current_fqn,
                            &method.value,
                            &sig.params,
                            sig.is_property,
                        )
                    {
                        return Some(TypedExpr {
                            kind: TypedExprKind::ClassVirtualCall {
                                object: Box::new(typed_object.clone()),
                                vtable_slot,
                                args: full_args,
                            },
                            ty: return_type,
                            span: span.clone(),
                        });
                    }

                    // Static dispatch (final method or no vtable)
                    let candidates = vec![ResolvedFunction::Regular {
                        mangled_name: sig.mangled_name.clone(),
                        return_type: sig.return_type.clone(),
                        type_args: vec![],
                    }];
                    let display = format!("{}.{}", fqn.symbol, method.value);
                    return Some(self.resolve_overload(&display, candidates, full_args, span));
                }

                // Multiple or zero matching overloads — fall through to overload resolution
                let candidates: Vec<ResolvedFunction> = matching
                    .iter()
                    .map(|sig| ResolvedFunction::Regular {
                        mangled_name: sig.mangled_name.clone(),
                        return_type: sig.return_type.clone(),
                        type_args: vec![],
                    })
                    .collect();

                let display = format!("{}.{}", fqn.symbol, method.value);
                return Some(self.resolve_overload(&display, candidates, full_args, span));
            }

            // Check generic methods (methods on generic classes, or methods with own type params)
            if class_sig
                .generic_instance_methods
                .get(&method_name)
                .is_some_and(|methods| methods.iter().any(|method| !method.is_property))
            {
                return self.resolve_generic_class_method(
                    &current_fqn,
                    &class_type_args,
                    &method_name,
                    typed_object,
                    typed_args,
                    explicit_method_type_args,
                    false,
                    span,
                );
            }

            // Module-for-class methods: methods declared in `module ClassName<T, E>`
            // live in the registry's module table, NOT on the class itself. For a
            // direct receiver typed as the class, `resolve_concrete_type_instance_method`
            // handles this; but when the receiver is a *subclass* instance (e.g.
            // `MakeWaiter()` whose type is `Class(MakeWaiter)`), that path only
            // checks MakeWaiter's module — Async's `map` is missed. Try the module
            // for the current parent FQN here.
            if let Some(result) = self.try_resolve_module_method_for_class(
                &current_fqn,
                &class_type_args,
                &class_sig,
                method,
                typed_object,
                typed_args.clone(),
                explicit_method_type_args,
                span,
            ) {
                return Some(result);
            }

            // Walk to parent. Resolve parent_type_expr (which has TypeParameter
            // placeholders for *this* class's params) against the current class's
            // concrete type args, so the parent sees the right substitution.
            {
                let parent_fqn = class_sig.parent_class?;
                let parent_type_args = match &class_sig.parent_type_expr {
                    Some(parent_ty) => {
                        let parent_ty = if class_sig.type_params.is_empty() {
                            parent_ty.clone()
                        } else {
                            let sub = TypeParamSubstitution::from_pairs(
                                &class_sig.type_params,
                                &class_type_args,
                            );
                            apply_substitution(&sub, parent_ty)
                        };
                        match &parent_ty {
                            Type::GenericClass { type_args, .. } => {
                                type_args.iter().map(|(_, t)| t.clone()).collect()
                            }
                            _ => vec![],
                        }
                    }
                    None => vec![],
                };
                current_fqn = parent_fqn;
                class_type_args = parent_type_args;
            }
        }
    }

    /// Try to dispatch `typed_object.method(args)` to a module-for-class instance
    /// method declared on `class_fqn` (with the supplied `class_type_args` if
    /// generic). The original `typed_object` is preserved as the call's first
    /// argument so codegen sees the actual subclass struct identity — only the
    /// lookup uses the parent type. (Class-pattern `match` arms compare the
    /// concrete struct, so widening the receiver in the typed AST would break
    /// downstream dispatch.)
    #[allow(
        clippy::too_many_arguments,
        reason = "Keep the compiler context parameters explicit at this call boundary."
    )]
    fn try_resolve_module_method_for_class(
        &mut self,
        class_fqn: &Fqn,
        class_type_args: &[Type],
        class_sig: &crate::typechecker::registry::ClassTypeSignature,
        method: &Spanned<String>,
        typed_object: &TypedExpr,
        typed_args: Vec<TypedExpr>,
        explicit_method_type_args: &[crate::parser::ast::TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        // No module for this class — nothing to try.
        let module_info = self.registry.lookup_module(class_fqn).cloned()?;
        let method_sym = SymbolName(method.value.clone());
        let has_concrete = module_info.functions.contains_key(&method_sym);
        let has_generic = !module_info
            .generic_members
            .lookup_visible(&method_sym, &self.package_path, &self.current_file)
            .is_empty();
        if !has_concrete && !has_generic {
            return None;
        }

        // Build parent class type used solely for module-instance-method lookup
        // (so generic-module unification binds the parent's type params).
        let parent_ty = if class_sig.type_params.is_empty() {
            Type::Class(class_fqn.clone(), MangledName::for_type(class_fqn))
        } else {
            let type_args = class_sig
                .type_params
                .iter()
                .zip(class_sig.type_param_variances.iter())
                .zip(class_type_args.iter())
                .map(|((_, v), t)| (*v, t.clone()))
                .collect();
            Type::GenericClass {
                fqn: class_fqn.clone(),
                mangled_name: MangledName::for_type(class_fqn),
                type_args,
            }
        };

        // 1. Concrete module-for-type instance method
        if has_concrete && let Some(overloads) = module_info.functions.get(&method_sym) {
            let instance_overloads: Vec<FunctionSignature> = overloads
                .iter()
                .filter(|sig| {
                    !sig.is_property && !sig.params.is_empty() && sig.params[0].0 == "self"
                })
                .filter(|sig| {
                    self.is_member_visible(
                        sig.visibility,
                        &module_info.fqn.package,
                        &sig.source_file,
                    )
                })
                .cloned()
                .collect();
            if !instance_overloads.is_empty() {
                let mut all_args = vec![typed_object.clone()];
                all_args.extend(typed_args);
                let display = format!("{}.{}", class_fqn.symbol, method.value);
                return Some(self.resolve_overloads_with_intrinsics(
                    class_fqn,
                    &method_sym,
                    instance_overloads,
                    all_args,
                    &display,
                    span,
                ));
            }
        }

        // 2. Generic module-for-type instance method
        if has_generic {
            let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
            // No span: this is one strategy among several, and the call may
            // still resolve as a trait impl below.
            let (candidates, _) = self.resolve_generic_module_instance_method(
                &parent_ty,
                &method_sym,
                &arg_types,
                explicit_method_type_args,
                None,
            );
            if !candidates.is_empty() {
                let mut all_args = vec![typed_object.clone()];
                all_args.extend(typed_args);
                let display = format!("{}.{}", parent_ty, method.value);
                return Some(self.resolve_overload(&display, candidates, all_args, span));
            }
        }

        None
    }

    /// Try to resolve a static method call: `ClassName.method(args)`.
    /// Returns `Ok(TypedExpr)` if resolved (or error), `Err(typed_args)` to fall through.
    pub(super) fn try_resolve_class_static_method(
        &mut self,
        class_name: &str,
        method: &Spanned<String>,
        typed_args: Vec<TypedExpr>,
        receiver_type_args: &[crate::parser::ast::TypeExpr],
        method_type_args: &[crate::parser::ast::TypeExpr],
        span: &Span,
    ) -> Result<TypedExpr, Vec<TypedExpr>> {
        let class_fqn = match self.resolve_type_name(class_name, receiver_type_args, span) {
            Some(Type::Class(fqn, _)) | Some(Type::GenericClass { fqn, .. }) => fqn,
            _ => return Err(typed_args),
        };
        let class_sig = match self
            .registry
            .lookup_class_type(&class_fqn, &self.package_path)
        {
            Some(sig) => sig.clone(),
            None => return Err(typed_args),
        };
        let method_name = SymbolName(method.value.clone());

        if let Some(overloads) = class_sig
            .static_methods
            .get(&method_name)
            .filter(|_| method_type_args.is_empty())
        {
            let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
            let matching: Vec<_> = overloads
                .iter()
                .filter(|sig| sig.matches_args(&arg_types, |p, a| self.is_assignable(p, a)))
                .collect();

            if !matching.is_empty()
                && !self.check_class_field_visibility(matching[0].visibility, &class_fqn, span)
            {
                return Ok(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }

            let candidates: Vec<ResolvedFunction> = matching
                .iter()
                .map(|sig| ResolvedFunction::Regular {
                    mangled_name: sig.mangled_name.clone(),
                    return_type: sig.return_type.clone(),
                    type_args: vec![],
                })
                .collect();

            let display = format!("{}.{}", class_name, method.value);
            return Ok(self.resolve_overload(&display, candidates, typed_args, span));
        }

        // Check generic static methods (methods on generic classes, or static methods with own type params)
        let typed_args = if let Some(defs) = class_sig.generic_static_methods.get(&method_name) {
            match self.resolve_generic_class_static_method(
                &class_fqn,
                &class_sig,
                &method_name,
                defs,
                typed_args,
                receiver_type_args,
                method_type_args,
                span,
            ) {
                Ok(result) => return Ok(result),
                Err(args) => args,
            }
        } else {
            typed_args
        };

        // A class can also receive static methods from trait implementations
        // and extensions. Let the shared static dispatch try those candidates.
        Err(typed_args)
    }

    /// Resolve a generic class static method call.
    /// Handles calls like `Box<Int32>.create(5)` or `Box.create(5)` (with bidirectional inference).
    #[allow(clippy::too_many_arguments)]
    fn resolve_generic_class_static_method(
        &mut self,
        class_fqn: &Fqn,
        _class_sig: &ClassTypeSignature,
        method_name: &SymbolName,
        defs: &[GenericClassMethodDef],
        typed_args: Vec<TypedExpr>,
        explicit_type_args: &[crate::parser::ast::TypeExpr],
        explicit_method_type_args: &[crate::parser::ast::TypeExpr],
        span: &Span,
    ) -> Result<TypedExpr, Vec<TypedExpr>> {
        let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();

        for def in defs {
            // Static methods should NOT have a `self` parameter
            if def.params.first().is_some_and(|(name, _)| name == "self") {
                continue;
            }

            // Pre-populate substitution with class-level type args from explicit type args on receiver
            let mut substitution = TypeParamSubstitution::new();

            if !explicit_type_args.is_empty() {
                // Explicit: Box<Int32>.create(5)
                if explicit_type_args.len() != def.class_type_params.len() {
                    continue;
                }
                if let Some(resolved) = self.resolve_type_args(explicit_type_args) {
                    for (tp, arg) in def.class_type_params.iter().zip(resolved.iter()) {
                        substitution.insert(tp.clone(), arg.clone());
                    }
                } else {
                    continue;
                }
            }

            if !explicit_method_type_args.is_empty() {
                if explicit_method_type_args.len() != def.method_type_params.len() {
                    continue;
                }
                let Some(arguments) = self.resolve_type_args(explicit_method_type_args) else {
                    continue;
                };
                for (parameter, argument) in def.method_type_params.iter().zip(arguments) {
                    substitution.insert(parameter.clone(), argument);
                }
            }

            // Unify args against param types to bind remaining type params
            if def.params.len() != arg_types.len() {
                continue;
            }
            let mut all_unified = true;
            for ((_, param_ty), arg_ty) in def.params.iter().zip(arg_types.iter()) {
                if !substitution.unify(param_ty, arg_ty) {
                    all_unified = false;
                    break;
                }
            }
            if !all_unified {
                continue;
            }

            // Build combined type params
            let all_type_params: Vec<TypeParamName> = def
                .class_type_params
                .iter()
                .chain(def.method_type_params.iter())
                .cloned()
                .collect();

            // Resolve all type params
            self.infer_associated_bound_types(&def.trait_bounds, &mut substitution);
            let combined_type_args = match substitution.resolve_type_params(&all_type_params) {
                Some(args) => args,
                None => {
                    // Try bidirectional inference from expected type
                    if let Some(ref expected) = self.expected_type {
                        substitution.unify(&def.return_type, expected);
                        match substitution.resolve_type_params(&all_type_params) {
                            Some(args) => args,
                            None => continue,
                        }
                    } else {
                        continue;
                    }
                }
            };

            // Validate trait bounds
            if !self.check_trait_bounds(
                &def.trait_bounds,
                &all_type_params,
                &combined_type_args,
                span,
            ) {
                continue;
            }

            // Visibility check
            if !self.check_class_field_visibility(def.visibility, class_fqn, span) {
                return Ok(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }

            // Build GenericFunctionDef for template name resolution
            let generic_def = crate::typechecker::registry::GenericFunctionDef {
                visibility: def.visibility,
                type_params: all_type_params,
                params: def.params.clone(),
                return_type: def.return_type.clone(),
                body: crate::parser::ast::Expr::UnitLiteral(span.clone()),
                span: span.clone(),
                container_name: Some(class_fqn.symbol.0.clone()),
                trait_bounds: def.trait_bounds.clone(),
                is_async: def.is_async,
                is_intrinsic: false,
            };

            let qualified_symbol = SymbolName(format!("{}.{}", class_fqn.symbol, method_name));
            let effective_fqn = Fqn {
                package: class_fqn.package.clone(),
                symbol: qualified_symbol,
            };

            let (mangled, return_type) = self.resolve_generic_function_template(
                &effective_fqn,
                &generic_def,
                &combined_type_args,
                MethodKind::ClassMethod {
                    method_parameter_count: def.method_type_params.len(),
                },
            );

            return Ok(TypedExpr {
                kind: TypedExprKind::FunctionCall {
                    name: mangled,
                    args: typed_args,
                    type_params: combined_type_args,
                },
                ty: return_type,
                span: span.clone(),
            });
        }

        Err(typed_args)
    }

    /// Try to resolve a bare function call as a class constructor: `ClassName(args)` or `ClassName<T>(args)`.
    /// Returns `Ok(TypedExpr)` on success, `Err(typed_args)` when not a class constructor.
    pub(super) fn try_resolve_class_constructor(
        &mut self,
        name: &str,
        explicit_type_args: &[crate::parser::ast::TypeExpr],
        typed_args: Vec<TypedExpr>,
        span: &Span,
    ) -> Result<TypedExpr, Vec<TypedExpr>> {
        // Resolve class name as a type and extract the FQN
        let fqn = match self.resolve_type_name(name, explicit_type_args, span) {
            Some(Type::Class(fqn, _)) | Some(Type::GenericClass { fqn, .. }) => fqn,
            _ => return Err(typed_args),
        };
        let class_sig = match self.registry.lookup_class_type(&fqn, &self.package_path) {
            Some(sig) => sig.clone(),
            None => return Err(typed_args),
        };

        // Check: cannot instantiate abstract class
        if class_sig.is_abstract {
            self.diagnostics.error(
                span.clone(),
                format!("cannot instantiate abstract class '{}'", name),
            );
            return Ok(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }

        // Check constructor visibility
        if !self.check_class_field_visibility(class_sig.constructor_visibility, &fqn, span) {
            return Ok(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }

        // Handle generic classes
        if !class_sig.type_params.is_empty() {
            return self.resolve_generic_class_constructor(
                name,
                &fqn,
                &class_sig,
                explicit_type_args,
                typed_args,
                span,
            );
        }

        // Non-generic class constructor (existing path)
        // Check arg count
        if typed_args.len() != class_sig.constructor_params.len() {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "class '{}' constructor expects {} argument(s), found {}",
                    name,
                    class_sig.constructor_params.len(),
                    typed_args.len()
                ),
            );
            return Ok(self.error_call(typed_args, span));
        }

        // Check arg types
        for (arg, param) in typed_args.iter().zip(class_sig.constructor_params.iter()) {
            if !self.is_assignable(&param.ty, &arg.ty) {
                self.diagnostics.error(
                    arg.span.clone(),
                    format!(
                        "type mismatch for parameter '{}': expected '{}', found '{}'",
                        param.name, param.ty, arg.ty
                    ),
                );
            }
        }

        // Resolve class type → mangled_name
        let mangled_name = MangledName::for_type(&fqn);
        let class_type = Type::Class(fqn, mangled_name.clone());

        Ok(TypedExpr {
            kind: TypedExprKind::ClassNew {
                mangled_name,
                args: typed_args,
                type_params: vec![],
            },
            ty: class_type,
            span: span.clone(),
        })
    }

    /// Resolve a generic class constructor call: `Box<Int32>(42)` or `Box(42)`.
    fn resolve_generic_class_constructor(
        &mut self,
        name: &str,
        fqn: &Fqn,
        class_sig: &ClassTypeSignature,
        explicit_type_args: &[crate::parser::ast::TypeExpr],
        typed_args: Vec<TypedExpr>,
        span: &Span,
    ) -> Result<TypedExpr, Vec<TypedExpr>> {
        // Determine type args
        let type_args = if !explicit_type_args.is_empty() {
            // Explicit type args: `Box<Int32>(42)`
            if explicit_type_args.len() != class_sig.type_params.len() {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "class '{}' expects {} type argument(s), found {}",
                        name,
                        class_sig.type_params.len(),
                        explicit_type_args.len()
                    ),
                );
                return Ok(self.error_call(typed_args, span));
            }
            match self.resolve_type_args(explicit_type_args) {
                Some(args) => args,
                None => return Ok(self.error_call(typed_args, span)),
            }
        } else {
            // Infer type args from constructor argument types
            if typed_args.len() != class_sig.constructor_params.len() {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "class '{}' constructor expects {} argument(s), found {}",
                        name,
                        class_sig.constructor_params.len(),
                        typed_args.len()
                    ),
                );
                return Ok(self.error_call(typed_args, span));
            }
            let mut substitution = TypeParamSubstitution::new();
            for (arg, param) in typed_args.iter().zip(class_sig.constructor_params.iter()) {
                substitution.unify(&param.ty, &arg.ty);
            }
            // Fallback: use expected_type for bidirectional inference
            // e.g. `let b: Box<Int32> = Box(42)` — unify expected GenericClass type args
            if let Some(Type::GenericClass {
                fqn: expected_fqn,
                type_args: expected_args,
                ..
            }) = self.expected_type.as_ref()
                && *expected_fqn == *fqn
            {
                let placeholder_args: Vec<Type> = class_sig
                    .type_params
                    .iter()
                    .map(|tp| {
                        Type::TypeVariable(
                            tp.clone(),
                            class_sig.trait_bounds.get(tp).cloned().unwrap_or_default(),
                        )
                    })
                    .collect();
                for (placeholder, (_, expected_arg)) in
                    placeholder_args.iter().zip(expected_args.iter())
                {
                    substitution.unify(placeholder, expected_arg);
                }
            }
            match substitution.resolve_type_params(&class_sig.type_params) {
                Some(args) => args,
                None => {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "cannot infer type arguments for generic class '{}'; provide explicit type arguments",
                            name
                        ),
                    );
                    return Ok(self.error_call(typed_args, span));
                }
            }
        };

        // Resolve the generic class type and register the concrete ClassTypeDef
        let class_type = self.infer_generic_class(fqn, class_sig, &type_args, span);
        let mangled_name = match &class_type {
            Type::GenericClass { mangled_name, .. } => mangled_name.clone(),
            _ => return Ok(self.error_call(typed_args, span)),
        };

        // Check arg count
        if typed_args.len() != class_sig.constructor_params.len() {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "class '{}' constructor expects {} argument(s), found {}",
                    name,
                    class_sig.constructor_params.len(),
                    typed_args.len()
                ),
            );
            return Ok(self.error_call(typed_args, span));
        }

        // Check arg types against concrete substituted param types
        let substitution = TypeParamSubstitution::from_pairs(&class_sig.type_params, &type_args);
        for (arg, param) in typed_args.iter().zip(class_sig.constructor_params.iter()) {
            let concrete_ty = apply_substitution(&substitution, &param.ty);
            if !self.is_assignable(&concrete_ty, &arg.ty) {
                self.diagnostics.error(
                    arg.span.clone(),
                    format!(
                        "type mismatch for parameter '{}': expected '{}', found '{}'",
                        param.name, concrete_ty, arg.ty
                    ),
                );
            }
        }

        Ok(TypedExpr {
            kind: TypedExprKind::ClassNew {
                mangled_name,
                args: typed_args,
                type_params: type_args.clone(),
            },
            ty: class_type,
            span: span.clone(),
        })
    }

    /// Recover the parent's physical field prefix without specializing erased fields.
    fn collect_canonical_parent_fields(&self, parent_fqn: &Fqn, fields: &mut Vec<ClassFieldDef>) {
        let Some(parent_sig) = self
            .registry
            .lookup_class_type(parent_fqn, &self.package_path)
        else {
            return;
        };
        if let Some(ref grandparent_fqn) = parent_sig.parent_class {
            self.collect_canonical_parent_fields(grandparent_fqn, fields);
        }

        for field in &parent_sig.fields {
            fields.push(ClassFieldDef {
                name: field.name.clone(),
                ty: field.ty.clone(),
                visibility: field.visibility,
                mutable: field.mutable,
                declared_by: parent_fqn.clone(),
            });
        }
    }

    /// Compute vtable method entries for a class.
    /// Parent entries come first (overrides replaced), then new child virtual methods.
    /// Compute a class's full vtable layout purely from the REGISTRY (its own
    /// signature + recursively its parents'), so it is identical whether the
    /// class is defined in this file, another file of this package, or a
    /// dependency package. Order: parent slots first (recursively), then this
    /// class's own non-final instance members; an override replaces the
    /// inherited slot in place (preserving its index). Both non-generic
    /// (`instance_methods`) and generic (`generic_instance_methods`) members are
    /// included; doubly-generic methods (own type params) are dispatched at the
    /// call site and get no slot. This is the single source of truth shared by
    /// codegen (via the ClassTypeDef), `find_vtable_slot`, and `is_method_virtual`.
    fn compute_vtable_methods(
        &self,
        class_fqn: &Fqn,
        class_sig: &ClassTypeSignature,
    ) -> Vec<VtableSlot> {
        let mut vtable: Vec<VtableSlot> = Vec::new();

        // This class's own type params as types — the impl_type_params for methods
        // defined (or overridden) on this class. Substituting the class's params
        // with an instantiation's args over this yields the instantiation's args.
        let own_type_params: Vec<Type> = class_sig
            .type_params
            .iter()
            .map(|tp| Type::GenericParam(tp.clone(), vec![], 0))
            .collect();

        // Inherit the parent's vtable via the registry (cross-file / cross-package).
        if let Some(ref parent_fqn) = class_sig.parent_class
            && let Some(parent_sig) = self
                .registry
                .lookup_class_type(parent_fqn, &self.package_path)
        {
            let parent_sig = parent_sig.clone();
            let mut parent_vtable = self.compute_vtable_methods(parent_fqn, &parent_sig);
            // Re-express the parent's slots in THIS class's type-param space via
            // the `extends Parent<binding>` binding, so a slot inherited from a
            // generic ancestor (e.g. a non-generic class extending
            // `AsyncInputStream<TlsIoError>`) records the concrete binding rather
            // than the ancestor's abstract type param.
            let binding = parent_binding(&parent_sig, class_sig.parent_type_expr.as_ref());
            if !binding.is_empty() {
                for slot in &mut parent_vtable {
                    // Only re-express `impl_type_params` (the binding for looking up
                    // the concrete impl method). `param_types`/`return_type` stay
                    // in the impl's own (template) type-param space so
                    // `for_function(impl_fqn, param_types)` keeps matching the
                    // registered template method; the `impl_type_params` suffix is
                    // what selects the monomorphized instance.
                    slot.impl_type_params = slot
                        .impl_type_params
                        .iter()
                        .map(|t| apply_type_substitution(t, &binding))
                        .collect();
                }
            }
            vtable = parent_vtable;
        }

        let class_is_final = class_sig.is_final;
        let merge = |vtable: &mut Vec<VtableSlot>,
                     name: &SymbolName,
                     params: Vec<Type>,
                     ret: Type,
                     is_final_method: bool,
                     is_property: bool| {
            let impl_fqn = class_method_fqn(class_fqn, &name.0);
            if let Some(entry) = vtable.iter_mut().find(|slot| {
                slot.is_property == is_property
                    && slot.method_name == *name
                    && self.vtable_parameters_match(slot, &params)
            }) {
                // Override: child re-implements an inherited slot (keep its index).
                entry.impl_fqn = impl_fqn;
                entry.param_types = params;
                entry.return_type = ret;
                entry.impl_type_params = own_type_params.clone();
            } else if !is_final_method && !class_is_final {
                vtable.push(VtableSlot {
                    is_property,
                    method_name: name.clone(),
                    impl_fqn,
                    param_types: params,
                    return_type: ret,
                    impl_type_params: own_type_params.clone(),
                });
            }
        };

        // Non-generic instance methods.
        for (method_name, overloads) in &class_sig.instance_methods {
            for sig in overloads {
                let params: Vec<Type> = sig.params.iter().map(|(_, t)| t.clone()).collect();
                merge(
                    &mut vtable,
                    method_name,
                    params,
                    sig.return_type.clone(),
                    sig.is_final_method,
                    sig.is_property,
                );
            }
        }
        // Generic instance methods (generic classes). Skip doubly-generic (own type params,
        // dispatched at call site) and non-instance members.
        for (method_name, defs) in &class_sig.generic_instance_methods {
            for def in defs {
                if !def.method_type_params.is_empty() {
                    continue;
                }
                if def.params.first().map(|(n, _)| n == "self") != Some(true) {
                    continue;
                }
                let params: Vec<Type> = def.params.iter().map(|(_, t)| t.clone()).collect();
                merge(
                    &mut vtable,
                    method_name,
                    params,
                    def.return_type.clone(),
                    def.is_final_method,
                    def.is_property,
                );
            }
        }

        vtable
    }

    /// Find the vtable slot index for a method name on the given class, from the
    /// registry (works cross-file / cross-package).
    fn find_vtable_slot(
        &self,
        object_fqn: &Fqn,
        method_name: &str,
        parameters: &[(String, Type)],
        is_property: bool,
    ) -> Option<u32> {
        let sig = self
            .registry
            .lookup_class_type(object_fqn, &self.package_path)?
            .clone();
        let parameters: Vec<_> = parameters.iter().map(|(_, ty)| ty.clone()).collect();
        self.compute_vtable_methods(object_fqn, &sig)
            .iter()
            .position(|slot| {
                slot.is_property == is_property
                    && slot.method_name.0 == method_name
                    && self.vtable_parameters_match(slot, &parameters)
            })
            .map(|idx| idx as u32)
    }

    /// Compare overload signatures in the receiver class's parameter space.
    /// The implementation's original signature remains intact for mangling.
    fn vtable_parameters_match(&self, slot: &VtableSlot, parameters: &[Type]) -> bool {
        if slot.param_types.len() != parameters.len() {
            return false;
        }
        let Some((owner, _)) = slot.impl_fqn.symbol.0.rsplit_once('.') else {
            return false;
        };
        let owner = Fqn {
            package: slot.impl_fqn.package.clone(),
            symbol: SymbolName(owner.to_string()),
        };
        let Some(class) = self.registry.lookup_class_type(&owner, &self.package_path) else {
            return false;
        };
        let substitution = class
            .type_params
            .iter()
            .cloned()
            .zip(slot.impl_type_params.iter().cloned())
            .collect();
        slot.param_types
            .iter()
            .skip(1)
            .zip(parameters.iter().skip(1))
            .all(|(inherited, actual)| {
                let inherited = apply_type_substitution(inherited, &substitution);
                crate::typechecker::subtyping::identical(&inherited, actual)
            })
    }

    /// Check if a method is virtual (needs vtable dispatch) or final (direct call).
    /// Returns false for final classes (exact type known, direct dispatch is safe).
    fn is_method_virtual(&self, class_fqn: &Fqn, method_name: &str) -> bool {
        let sig = match self
            .registry
            .lookup_class_type(class_fqn, &self.package_path)
        {
            Some(s) => s.clone(),
            None => return false,
        };
        if sig.is_final {
            return false;
        }
        self.compute_vtable_methods(class_fqn, &sig)
            .iter()
            .any(|slot| slot.method_name.0 == method_name)
    }

    /// Check if a field with the given visibility and declaring class is accessible
    /// from the current context. Does not emit diagnostics.
    pub(super) fn is_field_accessible(&self, visibility: Visibility, declared_by: &Fqn) -> bool {
        match visibility {
            Visibility::Public => true,
            Visibility::Internal => self.package_path == declared_by.package,
            Visibility::Protected => {
                if let Some(ref container) = self.container_name {
                    if self.package_path == declared_by.package
                        && container == &declared_by.symbol.0
                    {
                        return true;
                    }
                    let current_fqn = Fqn {
                        package: self.package_path.clone(),
                        symbol: SymbolName(container.clone()),
                    };
                    if self.registry.class_is_subtype(&current_fqn, declared_by) {
                        return true;
                    }
                }
                false
            }
            Visibility::Private => {
                self.container_name.as_deref() == Some(&declared_by.symbol.0)
                    && self.package_path == declared_by.package
            }
        }
    }

    /// Check if access to a class field/method with the given visibility is allowed.
    /// `declared_by` is the FQN of the class that declared the member.
    /// Returns true if access is allowed, emits diagnostics if not.
    pub(super) fn check_class_field_visibility(
        &mut self,
        visibility: Visibility,
        declared_by: &Fqn,
        span: &Span,
    ) -> bool {
        if self.is_field_accessible(visibility, declared_by) {
            return true;
        }
        match visibility {
            Visibility::Public => true,
            Visibility::Internal => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "cannot access internal member of class '{}'",
                        declared_by.symbol
                    ),
                );
                false
            }
            Visibility::Protected => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "cannot access protected member of class '{}'",
                        declared_by.symbol
                    ),
                );
                false
            }
            Visibility::Private => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "cannot access private member of class '{}'",
                        declared_by.symbol
                    ),
                );
                false
            }
        }
    }

    /// Resolve a `super.method(args)` call.
    /// Looks up the parent class hierarchy to find the method, then emits a
    /// `ClassSuperCall` (direct call, no vtable dispatch).
    pub(super) fn resolve_super_method_call(
        &mut self,
        method: &Spanned<String>,
        typed_args: Vec<TypedExpr>,
        super_span: &Span,
        span: &Span,
    ) -> TypedExpr {
        // 1. Must be inside a class method
        let class_name = match &self.container_name {
            Some(name) => name.clone(),
            None => {
                self.diagnostics.error(
                    super_span.clone(),
                    "super can only be used inside a class method".to_string(),
                );
                return self.error_call(typed_args, span);
            }
        };

        // 2. Look up current class and its parent
        let class_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(class_name.clone()),
        };
        let class_sig = match self
            .registry
            .lookup_class_type(&class_fqn, &self.package_path)
        {
            Some(sig) => sig.clone(),
            None => {
                self.diagnostics.error(
                    super_span.clone(),
                    format!(
                        "super can only be used inside a class method — '{}' is not a class",
                        class_name
                    ),
                );
                return self.error_call(typed_args, span);
            }
        };

        let parent_fqn = match class_sig.parent_class {
            Some(fqn) => fqn,
            None => {
                self.diagnostics.error(
                    super_span.clone(),
                    format!("class '{}' has no parent class", class_name),
                );
                return self.error_call(typed_args, span);
            }
        };

        // 3. Walk parent hierarchy to find the method
        // Build self expr and arg types once (used for matching at each level)
        let class_mangled = MangledName::for_type(&class_fqn);
        let self_type = Type::Class(class_fqn.clone(), class_mangled);
        let self_expr = TypedExpr {
            kind: TypedExprKind::VarRef {
                name: VarName("self".to_string()),
                boxed: false,
            },
            ty: self_type,
            span: super_span.clone(),
        };
        let self_arg_type = &self_expr.ty;
        let mut arg_types: Vec<&Type> = vec![self_arg_type];
        arg_types.extend(typed_args.iter().map(|a| &a.ty));

        let method_name = SymbolName(method.value.clone());
        let mut current_fqn = Some(parent_fqn);
        while let Some(fqn) = current_fqn {
            let parent_sig = match self.registry.lookup_class_type(&fqn, &self.package_path) {
                Some(sig) => sig.clone(),
                None => break,
            };

            if let Some(overloads) = parent_sig.instance_methods.get(&method_name) {
                // Find matching overload
                let matching: Vec<_> = overloads
                    .iter()
                    .filter(|sig| sig.matches_args(&arg_types, |p, a| self.is_assignable(p, a)))
                    .collect();

                if matching.len() == 1 {
                    let sig = matching[0];
                    let mut full_args = vec![self_expr];
                    full_args.extend(typed_args);
                    return TypedExpr {
                        ty: sig.return_type.clone(),
                        kind: TypedExprKind::ClassSuperCall {
                            method_mangled: sig.mangled_name.clone(),
                            args: full_args,
                        },
                        span: span.clone(),
                    };
                } else if matching.len() > 1 {
                    let mut full_args = vec![self_expr];
                    full_args.extend(typed_args);
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "ambiguous super call: multiple overloads match for super.{}()",
                            method.value
                        ),
                    );
                    return self.error_call(full_args, span);
                }
                // No match at this level — continue walking up the hierarchy
            }

            // Walk to grandparent
            current_fqn = parent_sig.parent_class;
        }

        // Method not found in any parent
        self.diagnostics.error(
            span.clone(),
            format!(
                "no method '{}' found in parent class hierarchy",
                method.value
            ),
        );
        self.error_call(typed_args, span)
    }

    /// Infer the type for a generic class instantiation.
    /// Registers the concrete ClassTypeDef (with typed initializer/method bodies)
    /// and returns `Type::GenericClass`.
    pub(super) fn infer_generic_class(
        &mut self,
        fqn: &Fqn,
        class_sig: &ClassTypeSignature,
        type_args: &[Type],
        span: &Span,
    ) -> Type {
        // Check trait bounds
        self.check_trait_bounds(
            &class_sig.trait_bounds,
            &class_sig.type_params,
            type_args,
            span,
        );

        // Erased mangled name — all instantiations share one canonical TypeDef.
        let mangled = MangledName::for_type(fqn);

        // Pair each type arg with its declared variance
        let type_args_with_variance: Vec<(Variance, Type)> = class_sig
            .type_param_variances
            .iter()
            .zip(type_args.iter())
            .map(|(v, t)| (*v, t.clone()))
            .collect();

        Type::GenericClass {
            fqn: fqn.clone(),
            mangled_name: mangled,
            type_args: type_args_with_variance,
        }
    }

    /// Resolve a generic class method call.
    /// Handles calls on GenericClass instances where the method was stored as a
    /// GenericClassMethodDef (method has its own type params, or class is generic).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn resolve_generic_class_method(
        &mut self,
        fqn: &Fqn,
        class_type_args: &[Type],
        method_name: &SymbolName,
        typed_object: &TypedExpr,
        typed_args: Vec<TypedExpr>,
        explicit_method_type_args: &[crate::parser::ast::TypeExpr],
        is_property: bool,
        span: &Span,
    ) -> Option<TypedExpr> {
        let class_sig = self
            .registry
            .lookup_class_type(fqn, &self.package_path)?
            .clone();

        let defs = class_sig.generic_instance_methods.get(method_name)?;
        if defs.is_empty() {
            return None;
        }

        let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();

        for def in defs
            .iter()
            .filter(|definition| definition.is_property == is_property)
        {
            // Pre-populate substitution with class-level type args
            let mut substitution = TypeParamSubstitution::new();
            for (tp, arg) in def.class_type_params.iter().zip(class_type_args.iter()) {
                substitution.insert(tp.clone(), arg.clone());
            }
            if !explicit_method_type_args.is_empty() {
                if explicit_method_type_args.len() != def.method_type_params.len() {
                    continue;
                }
                let Some(arguments) = self.resolve_type_args(explicit_method_type_args) else {
                    continue;
                };
                for (parameter, argument) in def.method_type_params.iter().zip(arguments) {
                    substitution.insert(parameter.clone(), argument);
                }
            }

            // Must be instance method with self
            if def.params.is_empty() || def.params[0].0 != "self" {
                continue;
            }

            // Unify non-self args against param types to bind method-level type params
            let non_self_params = &def.params[1..];
            if non_self_params.len() != arg_types.len() {
                continue;
            }
            let mut all_unified = true;
            for ((_, param_ty), arg_ty) in non_self_params.iter().zip(arg_types.iter()) {
                let mut inferred = substitution.clone();
                if inferred.unify(param_ty, arg_ty) {
                    substitution = inferred;
                } else if !self.is_assignable(&apply_substitution(&substitution, param_ty), arg_ty)
                {
                    all_unified = false;
                    break;
                }
            }
            if !all_unified {
                continue;
            }

            // Build combined type params
            let all_type_params: Vec<TypeParamName> = def
                .class_type_params
                .iter()
                .chain(def.method_type_params.iter())
                .cloned()
                .collect();

            // Resolve all type params
            self.infer_associated_bound_types(&def.trait_bounds, &mut substitution);
            let combined_type_args = match substitution.resolve_type_params(&all_type_params) {
                Some(args) => args,
                None => {
                    // Try explicit type args for method-level params
                    if !explicit_method_type_args.is_empty()
                        && explicit_method_type_args.len() == def.method_type_params.len()
                    {
                        let class_args =
                            match substitution.resolve_type_params(&def.class_type_params) {
                                Some(a) => a,
                                None => continue,
                            };
                        let method_args = match self.resolve_type_args(explicit_method_type_args) {
                            Some(a) => a,
                            None => continue,
                        };
                        [class_args, method_args].concat()
                    } else if let Some(ref expected) = self.expected_type {
                        substitution.unify(&def.return_type, expected);
                        match substitution.resolve_type_params(&all_type_params) {
                            Some(args) => args,
                            None => continue,
                        }
                    } else {
                        continue;
                    }
                }
            };

            // Validate trait bounds
            if !self.check_trait_bounds(
                &def.trait_bounds,
                &all_type_params,
                &combined_type_args,
                span,
            ) {
                continue;
            }

            // Virtual dispatch: if this generic class's method has a vtable slot
            // (i.e. it's abstract or overridable and the class isn't final),
            // dispatch through the vtable instead of emitting a static call to
            // the (possibly abstract, body-less) template. Mirrors the
            // non-generic path in `try_resolve_class_instance_method`.
            if def.method_type_params.is_empty()
                && self.is_method_virtual(fqn, &method_name.0)
                && let Some(vtable_slot) =
                    self.find_vtable_slot(fqn, &method_name.0, &def.params, def.is_property)
            {
                let return_type = apply_substitution(&substitution, &def.return_type);
                let mut full_args = vec![typed_object.clone()];
                full_args.extend(typed_args.clone());
                return Some(TypedExpr {
                    kind: TypedExprKind::ClassVirtualCall {
                        object: Box::new(typed_object.clone()),
                        vtable_slot,
                        args: full_args,
                    },
                    ty: return_type,
                    span: span.clone(),
                });
            }

            // Build GenericFunctionDef for template name resolution
            let generic_def = crate::typechecker::registry::GenericFunctionDef {
                visibility: def.visibility,
                type_params: all_type_params,
                params: def.params.clone(),
                return_type: def.return_type.clone(),
                body: crate::parser::ast::Expr::UnitLiteral(span.clone()),
                span: span.clone(),
                container_name: Some(fqn.symbol.0.clone()),
                trait_bounds: def.trait_bounds.clone(),
                is_async: def.is_async,
                is_intrinsic: false,
            };

            let qualified_symbol = SymbolName(format!("{}.{}", fqn.symbol, method_name));
            let effective_fqn = Fqn {
                package: fqn.package.clone(),
                symbol: qualified_symbol,
            };

            let (mangled, return_type) = self.resolve_generic_function_template(
                &effective_fqn,
                &generic_def,
                &combined_type_args,
                MethodKind::ClassMethod {
                    method_parameter_count: def.method_type_params.len(),
                },
            );

            let mut full_args = vec![typed_object.clone()];
            full_args.extend(typed_args);

            return Some(TypedExpr {
                kind: TypedExprKind::FunctionCall {
                    name: mangled,
                    args: full_args,
                    type_params: combined_type_args,
                },
                ty: return_type,
                span: span.clone(),
            });
        }

        None
    }

    /// Convert a PropertyDecl into a FunctionDecl for reuse of infer_function.
    pub(super) fn property_to_function_decl(&self, prop: &PropertyDecl) -> FunctionDecl {
        FunctionDecl {
            visibility: prop.visibility,
            is_async: false,
            is_override: prop.is_override,
            is_final: prop.is_final,
            is_abstract: prop.is_abstract,
            name: prop.name.clone(),
            type_params: prop.type_params.clone(),
            params: prop.params.clone(),
            return_type: Some(prop.return_type.clone()),
            where_clause: vec![],
            body: prop
                .body
                .clone()
                .unwrap_or_else(|| crate::parser::ast::Expr::UnitLiteral(prop.span.clone())),
            doc_comment: prop.doc_comment.clone(),
            span: prop.span.clone(),
        }
    }

    /// Register a placeholder TypedFunction with Panic body for an abstract method.
    fn register_abstract_method_placeholder(&mut self, class_name: &str, func: &FunctionDecl) {
        let typed_params: Vec<TypedParam> = func
            .params
            .iter()
            .map(|p| {
                let ty = self.resolve_type_expr(&p.type_annotation);
                TypedParam {
                    name: p.name.value.clone(),
                    ty,
                    span: p.span.clone(),
                }
            })
            .collect();
        let return_type = match &func.return_type {
            Some(type_expr) => self.resolve_type_expr(type_expr),
            None => Type::Unit,
        };
        let symbol_name = format!("{}.{}", class_name, func.name.value);
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(symbol_name),
        };
        let param_types: Vec<&Type> = typed_params.iter().map(|p| &p.ty).collect();
        let name = MangledName::for_function(&fqn, &param_types);
        let display_name = super::make_display_name(&fqn.to_string(), &typed_params);
        self.typed_functions.insert(
            name.clone(),
            TypedFunction {
                visibility: func.visibility,
                name,
                source_name: fqn.symbol.0.clone(),
                type_params: vec![],
                params: typed_params,
                return_type,
                body: TypedExpr {
                    kind: TypedExprKind::Panic {
                        message: Box::new(TypedExpr {
                            kind: TypedExprKind::StringLiteral(
                                "abstract method called".to_string(),
                            ),
                            ty: Type::String,
                            span: func.span.clone(),
                        }),
                    },
                    ty: Type::Never,
                    span: func.span.clone(),
                },
                span: func.span.clone(),
                vtable_self_type: None,
                is_async: func.is_async,
                display_name,
            },
        );
    }

    /// Register a placeholder TypedFunction with Panic body for an abstract property.
    fn register_abstract_property_placeholder(&mut self, class_name: &str, prop: &PropertyDecl) {
        let func_decl = self.property_to_function_decl(prop);
        self.register_abstract_method_placeholder(class_name, &func_decl);
    }

    /// Generic-template analogue of `register_abstract_method_placeholder`: for an
    /// abstract method/property of a *generic* class, create a panic-body template
    /// `TypedFunction` (carrying the class's type params) so the base vtable slot
    /// has a concrete function under erasure, and return the `VtableSlot` so callers
    /// dispatch virtually instead of statically calling the body-less template.
    fn register_abstract_template_member(
        &mut self,
        func: &FunctionDecl,
        class_fqn: &Fqn,
        class_type_params: &[TypeParamName],
        class_is_final: bool,
        is_property: bool,
    ) -> Option<VtableSlot> {
        let method_type_params: Vec<_> = func
            .type_params
            .iter()
            .map(|parameter| TypeParamName(parameter.value.clone()))
            .collect();
        let all_type_params: Vec<_> = class_type_params
            .iter()
            .chain(&method_type_params)
            .cloned()
            .collect();
        let method_bounds =
            self.resolve_trait_bounds_from_where_clause(&func.where_clause, &all_type_params);
        let method_scope = self.type_param_map(&method_type_params, &method_bounds);
        self.current_type_params.extend(
            method_scope
                .into_iter()
                .map(|(name, ty)| (TypeParamName(name), ty)),
        );
        self.add_current_type_param_bounds(&method_bounds);
        let typed_params: Vec<TypedParam> = func
            .params
            .iter()
            .map(|p| TypedParam {
                name: p.name.value.clone(),
                ty: self.resolve_type_expr(&p.type_annotation),
                span: p.span.clone(),
            })
            .collect();
        let return_type = match &func.return_type {
            Some(type_expr) => self.resolve_type_expr(type_expr),
            None => Type::Unit,
        };
        let method_fqn = class_method_fqn(class_fqn, &func.name.value);
        let param_types_ref: Vec<&Type> = typed_params.iter().map(|p| &p.ty).collect();
        let template_mn = crate::typechecker::class_trait_methods::template_name(
            &method_fqn,
            &param_types_ref,
            method_type_params.len(),
        );
        let display_name = super::make_display_name(&method_fqn.to_string(), &typed_params);
        let slot_param_types: Vec<Type> = typed_params.iter().map(|p| p.ty.clone()).collect();
        self.typed_functions.insert(
            template_mn.clone(),
            TypedFunction {
                visibility: func.visibility,
                name: template_mn,
                source_name: method_fqn.symbol.0.clone(),
                type_params: all_type_params,
                params: typed_params,
                return_type: return_type.clone(),
                body: TypedExpr {
                    kind: TypedExprKind::Panic {
                        message: Box::new(TypedExpr {
                            kind: TypedExprKind::StringLiteral(
                                "abstract method called".to_string(),
                            ),
                            ty: Type::String,
                            span: func.span.clone(),
                        }),
                    },
                    ty: Type::Never,
                    span: func.span.clone(),
                },
                span: func.span.clone(),
                vtable_self_type: None,
                is_async: func.is_async,
                display_name,
            },
        );
        if !func.is_final
            && !class_is_final
            && method_type_params.is_empty()
            && func.params.first().is_some_and(|p| p.name.value == "self")
        {
            Some(VtableSlot {
                is_property,
                method_name: SymbolName(func.name.value.clone()),
                impl_fqn: method_fqn,
                param_types: slot_param_types,
                return_type,
                impl_type_params: class_type_params
                    .iter()
                    .map(|tp| Type::GenericParam(tp.clone(), vec![], 0))
                    .collect(),
            })
        } else {
            None
        }
    }

    /// Infer a non-generic method/property body on a generic class and produce a
    /// template TypedFunction (with TypeParameter types from the class type params).
    /// Returns the template mangled name for vtable tracking.
    fn infer_template_class_method(
        &mut self,
        func: &FunctionDecl,
        class_fqn: &Fqn,
        class_type_params: &[TypeParamName],
    ) -> MangledName {
        let method_bounds =
            self.resolve_trait_bounds_from_where_clause(&func.where_clause, class_type_params);
        self.add_current_type_param_bounds(&method_bounds);
        let return_type = match &func.return_type {
            Some(type_expr) => self.resolve_type_expr(type_expr),
            None => Type::Unit,
        };
        let typed_params: Vec<TypedParam> = func
            .params
            .iter()
            .map(|p| {
                let ty = self.resolve_type_expr(&p.type_annotation);
                TypedParam {
                    name: p.name.value.clone(),
                    ty,
                    span: p.span.clone(),
                }
            })
            .collect();
        self.push_scope();
        for param in &typed_params {
            self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
        }
        let body_expected_type = if func.is_async {
            self.resolve_awaitable_value_type(&return_type)
                .unwrap_or(return_type.clone())
        } else {
            return_type.clone()
        };
        self.push_scope();
        let prev_expected = self.expected_type.take();
        self.expected_type = Some(body_expected_type.clone());
        let prev_fn_return = self.function_return_type.take();
        self.function_return_type = Some(return_type.clone());
        let prev_async_return = self.async_return_type.take();
        if func.is_async {
            self.async_return_type = Some(return_type.clone());
        }
        let body = self.infer_expr(&func.body);
        self.async_return_type = prev_async_return;
        self.function_return_type = prev_fn_return;
        self.expected_type = prev_expected;
        self.pop_scope();
        self.pop_scope();
        self.check_assignable(body.span.clone(), &body_expected_type, &body.ty);

        // Compute template mangled name and store template TypedFunction
        let method_symbol = SymbolName(format!("{}.{}", class_fqn.symbol, func.name.value));
        let method_fqn = Fqn {
            package: class_fqn.package.clone(),
            symbol: method_symbol,
        };
        let param_types: Vec<&Type> = typed_params.iter().map(|p| &p.ty).collect();
        let template_name = MangledName::for_function(&method_fqn, &param_types);

        // For async templates, wrap the body in AsyncBlock carrying a
        // ResolvedImplMethod with TypeParameter types; after monomorphize
        // substitution the desugar step can resolve the concrete impl. Mirrors
        // the function-template path in infer/functions.rs.
        let body = self.wrap_async_body(body, &return_type, func.is_async);
        let display_name = super::make_display_name(&method_fqn.to_string(), &typed_params);
        self.typed_functions.insert(
            template_name.clone(),
            TypedFunction {
                visibility: func.visibility,
                name: template_name.clone(),
                source_name: method_fqn.symbol.0.clone(),
                type_params: class_type_params.to_vec(),
                params: typed_params,
                return_type,
                body,
                span: func.span.clone(),
                vtable_self_type: None,
                is_async: func.is_async,
                display_name,
            },
        );

        template_name
    }

    /// Create a template TypedFunction for a doubly-generic class method
    /// (has both class type params and its own type params).
    fn infer_template_class_method_doubly_generic(
        &mut self,
        func: &FunctionDecl,
        class_fqn: &Fqn,
        class_type_params: &[TypeParamName],
    ) {
        // Add function's own type params to the context
        let func_type_params: Vec<TypeParamName> = func
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        let all_type_params: Vec<TypeParamName> = class_type_params
            .iter()
            .chain(func_type_params.iter())
            .cloned()
            .collect();

        let func_trait_bounds =
            self.resolve_trait_bounds_from_where_clause(&func.where_clause, &all_type_params);
        let func_type_param_map = self.type_param_map(&func_type_params, &func_trait_bounds);
        for tp in &func_type_params {
            if let Some(ty) = func_type_param_map.get(&tp.0) {
                self.current_type_params.insert(tp.clone(), ty.clone());
            }
        }

        self.add_current_type_param_bounds(&func_trait_bounds);

        let return_type = match &func.return_type {
            Some(type_expr) => self.resolve_type_expr(type_expr),
            None => Type::Unit,
        };
        let typed_params: Vec<TypedParam> = func
            .params
            .iter()
            .map(|p| {
                let ty = self.resolve_type_expr(&p.type_annotation);
                TypedParam {
                    name: p.name.value.clone(),
                    ty,
                    span: p.span.clone(),
                }
            })
            .collect();
        self.push_scope();
        for param in &typed_params {
            self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
        }
        let body_expected_type = if func.is_async {
            self.resolve_awaitable_value_type(&return_type)
                .unwrap_or(return_type.clone())
        } else {
            return_type.clone()
        };
        self.push_scope();
        let prev_expected = self.expected_type.take();
        self.expected_type = Some(body_expected_type.clone());
        let prev_fn_return = self.function_return_type.take();
        self.function_return_type = Some(return_type.clone());
        let prev_async_return = self.async_return_type.take();
        if func.is_async {
            self.async_return_type = Some(return_type.clone());
        }
        let body = self.infer_expr(&func.body);
        self.async_return_type = prev_async_return;
        self.function_return_type = prev_fn_return;
        self.expected_type = prev_expected;
        self.pop_scope();
        self.pop_scope();
        self.check_assignable(body.span.clone(), &body_expected_type, &body.ty);

        // Remove function's own type params from context
        for tp in &func_type_params {
            self.current_type_params.remove(tp);
        }

        // Compute template mangled name with combined type params
        let method_symbol = SymbolName(format!("{}.{}", class_fqn.symbol, func.name.value));
        let method_fqn = Fqn {
            package: class_fqn.package.clone(),
            symbol: method_symbol,
        };
        let param_types: Vec<&Type> = typed_params.iter().map(|p| &p.ty).collect();
        let template_name = crate::typechecker::class_trait_methods::template_name(
            &method_fqn,
            &param_types,
            func_type_params.len(),
        );

        // For async templates, wrap the body in AsyncBlock (see the
        // singly-generic path above for rationale).
        let body = self.wrap_async_body(body, &return_type, func.is_async);
        let display_name = super::make_display_name(&method_fqn.to_string(), &typed_params);
        self.typed_functions.insert(
            template_name.clone(),
            TypedFunction {
                visibility: func.visibility,
                name: template_name,
                source_name: method_fqn.symbol.0.clone(),
                type_params: all_type_params,
                params: typed_params,
                return_type,
                body,
                span: func.span.clone(),
                vtable_self_type: None,
                is_async: func.is_async,
                display_name,
            },
        );
    }

    /// Try to resolve a field access as a class instance property.
    /// Walks the class hierarchy looking for property entries in instance_methods.
    pub(super) fn try_resolve_class_instance_property(
        &mut self,
        typed_object: &TypedExpr,
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        let (fqn, _mn, class_type_args) = match &typed_object.ty {
            Type::Class(fqn, mn) => (fqn, mn, vec![]),
            Type::GenericClass {
                fqn,
                mangled_name: mn,
                type_args,
            } => (fqn, mn, type_args.iter().map(|(_, t)| t.clone()).collect()),
            _ => return None,
        };

        let mut current_fqn = fqn.clone();
        loop {
            let class_sig = self
                .registry
                .lookup_class_type(&current_fqn, &self.package_path)?
                .clone();
            let prop_name = SymbolName(field.value.clone());

            if let Some(overloads) = class_sig.instance_methods.get(&prop_name) {
                // Find a property overload (is_property: true, exactly 1 param = self)
                let matching: Vec<_> = overloads
                    .iter()
                    .filter(|sig| sig.is_property && sig.params.len() == 1)
                    .collect();

                if !matching.is_empty() {
                    let sig = matching[0];
                    if !self.check_class_field_visibility(sig.visibility, &current_fqn, span) {
                        return Some(TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        });
                    }

                    let return_type = sig.return_type.clone();
                    let full_args = vec![typed_object.clone()];

                    // Virtual dispatch if method has vtable slot
                    if self.is_method_virtual(fqn, &field.value)
                        && let Some(vtable_slot) = self.find_vtable_slot(
                            &current_fqn,
                            &field.value,
                            &sig.params,
                            sig.is_property,
                        )
                    {
                        return Some(TypedExpr {
                            kind: TypedExprKind::ClassVirtualCall {
                                object: Box::new(typed_object.clone()),
                                vtable_slot,
                                args: full_args,
                            },
                            ty: return_type,
                            span: span.clone(),
                        });
                    }

                    // Static dispatch
                    return Some(TypedExpr {
                        ty: return_type,
                        kind: TypedExprKind::FunctionCall {
                            name: sig.mangled_name.clone(),
                            args: full_args,
                            type_params: vec![],
                        },
                        span: span.clone(),
                    });
                }
            }

            // Check generic instance methods for property
            if let Some(defs) = class_sig
                .generic_instance_methods
                .get(&SymbolName(field.value.clone()))
            {
                let prop_defs: Vec<_> = defs.iter().filter(|d| d.is_property).collect();
                if !prop_defs.is_empty() {
                    return self.resolve_generic_class_method(
                        &current_fqn,
                        &class_type_args,
                        &SymbolName(field.value.clone()),
                        typed_object,
                        vec![], // no extra args beyond self
                        &[],    // no explicit type args
                        true,
                        span,
                    );
                }
            }

            // Walk to parent
            {
                let parent_fqn = class_sig.parent_class?;
                current_fqn = parent_fqn
            }
        }
    }

    /// Try to resolve a static property access: `ClassName.property`.
    pub(super) fn try_resolve_class_static_property(
        &mut self,
        class_name: &str,
        field: &Spanned<String>,
        explicit_type_args: &[crate::parser::ast::TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        let class_fqn = match self.resolve_type_name(class_name, explicit_type_args, span) {
            Some(Type::Class(fqn, _)) | Some(Type::GenericClass { fqn, .. }) => fqn,
            _ => return None,
        };
        let class_sig = {
            let sig = self
                .registry
                .lookup_class_type(&class_fqn, &self.package_path)?;
            sig.clone()
        };
        let prop_name = SymbolName(field.value.clone());

        // Check concrete static properties
        if let Some(overloads) = class_sig.static_methods.get(&prop_name) {
            let matching: Vec<_> = overloads
                .iter()
                .filter(|sig| sig.is_property && sig.params.is_empty())
                .collect();

            if !matching.is_empty() {
                let sig = matching[0];
                if !self.check_class_field_visibility(sig.visibility, &class_fqn, span) {
                    return Some(TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    });
                }
                return Some(TypedExpr {
                    ty: sig.return_type.clone(),
                    kind: TypedExprKind::FunctionCall {
                        name: sig.mangled_name.clone(),
                        args: vec![],
                        type_params: vec![],
                    },
                    span: span.clone(),
                });
            }
        }

        // Check generic static properties (properties on generic classes or with own type params)
        if let Some(defs) = class_sig.generic_static_methods.get(&prop_name) {
            let prop_defs: Vec<_> = defs.iter().filter(|d| d.is_property).cloned().collect();
            if !prop_defs.is_empty() {
                // Use the static method resolution (it handles properties since they're 0-arg methods)
                if let Ok(result) = self.resolve_generic_class_static_method(
                    &class_fqn,
                    &class_sig,
                    &prop_name,
                    &prop_defs,
                    vec![], // no extra args
                    explicit_type_args,
                    &[],
                    span,
                ) {
                    return Some(result);
                }
            }
        }

        // Check concrete static globals (let static on non-generic classes)
        {
            let global_fqn = Fqn {
                package: class_fqn.package.clone(),
                symbol: SymbolName(format!("{}.{}", class_fqn.symbol, field.value)),
            };
            if let Some(sig) =
                self.registry
                    .lookup_global(&global_fqn, &self.package_path, &self.current_file)
            {
                if !self.check_class_field_visibility(sig.visibility, &class_fqn, span) {
                    return Some(TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    });
                }
                return Some(TypedExpr {
                    ty: sig.ty.clone(),
                    kind: TypedExprKind::GlobalRef {
                        name: sig.mangled_name.clone(),
                        type_params: vec![],
                    },
                    span: span.clone(),
                });
            }
        }

        // Check generic static globals (let static on generic classes)
        if let Some(def) = class_sig.generic_static_globals.get(&prop_name) {
            let def = def.clone();
            // Determine type args from explicit type args or bidirectional inference
            let type_args = if !explicit_type_args.is_empty() {
                if explicit_type_args.len() != def.type_params.len() {
                    return None;
                }
                self.resolve_type_args(explicit_type_args)?
            } else {
                let expected = self.expected_type.as_ref()?;
                let ty = def.ty.as_ref()?;
                let mut substitution = TypeParamSubstitution::new();
                if substitution.unify(ty, expected) {
                    substitution.resolve_type_params(&def.type_params)?
                } else {
                    return None;
                }
            };

            // Instantiate
            let substitution = TypeParamSubstitution::from_pairs(&def.type_params, &type_args);
            let concrete_ty = if let Some(ref ty) = def.ty {
                apply_substitution(&substitution, ty)
            } else {
                Type::Error
            };

            let effective_fqn = Fqn {
                package: class_fqn.package.clone(),
                symbol: SymbolName(format!("{}.{}", class_fqn.symbol, field.value)),
            };
            // Statics on generic classes share one storage across all instantiations
            // (the rules pass forbids the declared type from referencing the class's
            // type parameters, so every `Box<T>.field` resolves to the same global).
            let mangled = MangledName::for_global(&effective_fqn);

            if !self.check_class_field_visibility(def.visibility, &class_fqn, span) {
                return Some(TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                });
            }

            let _ = type_args;
            return Some(TypedExpr {
                ty: concrete_ty,
                kind: TypedExprKind::GlobalRef {
                    name: mangled,
                    type_params: vec![],
                },
                span: span.clone(),
            });
        }

        None
    }
}
