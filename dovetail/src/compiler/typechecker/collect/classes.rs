use std::collections::BTreeMap;

use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName, Variance, Visibility};
use crate::parser::ast::{ClassDecl, ClassMember, TypeExpr};

use crate::typechecker::registry::{
    ClassBodyMemberDef, ClassFieldInfo, ClassLetBindingDef, ClassTypeSignature, ConstructorParam,
    FunctionSignature, GenericClassMethodDef, GenericClassStaticGlobalDef, TraitSignature,
};
use crate::typechecker::types::{TraitBounds, Type};

use super::Collector;
use super::implements::{substitute_self, substitute_trait_type_params};

impl Collector<'_> {
    /// Pre-register a class type name so that forward references resolve.
    /// Called in Pass 0 before full collection.
    /// For generic classes, also registers a preliminary ClassTypeSignature with type_params
    /// so that `resolve_generic_type_expr` can validate type args (e.g. `self: Box<T>`).
    pub(super) fn pre_register_class(&mut self, class: &ClassDecl) {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(class.name.value.clone()),
        };
        let mangled_name = MangledName::for_type(&fqn);
        let ty = Type::Class(fqn.clone(), mangled_name);
        self.package_registry.register_type(fqn.clone(), ty);

        // For generic classes, pre-register ClassTypeSignature with type_params
        // so that resolve_generic_type_expr can look up type param count during collect.
        if !class.type_params.is_empty() {
            let type_params: Vec<TypeParamName> = class
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.name.value.clone()))
                .collect();
            let type_param_variances: Vec<Variance> =
                class.type_params.iter().map(|tp| tp.variance).collect();
            let trait_bounds = self.resolve_trait_bounds(&class.where_clause, &type_params);

            // Pass 0 exists so that declarations can refer to each other regardless
            // of order, but it resolves these param types while it is still only
            // part-way through registering names — so a type declared later (in
            // this file or a later one) is not there yet and resolves to
            // `Type::Error`. Pass 1's `collect_class` re-resolves every param
            // against the complete registry and re-registers the signature, so the
            // provisional answer here is always superseded; only its diagnostics
            // would survive, and they are wrong by construction. Discard them and
            // let pass 1 be the one that reports.
            //
            // Non-generic classes never had this problem: they skip this block
            // entirely and are resolved for the first time in pass 1.
            let before = self.diagnostics.len();
            let constructor_params: Vec<ConstructorParam> = class
                .params
                .iter()
                .map(|p| {
                    let type_params_map = Type::type_param_map(&type_params, &trait_bounds);
                    let ty = self
                        .resolve_type_expr_with_type_params(&p.type_annotation, &type_params_map);
                    ConstructorParam {
                        name: p.name.value.clone(),
                        ty,
                        visibility: p.visibility,
                        mutable: p.mutable,
                    }
                })
                .collect();
            self.diagnostics.truncate(before);

            self.package_registry.register_class_type(
                fqn.clone(),
                ClassTypeSignature {
                    visibility: class.visibility,
                    is_string_literal: class.string_literal.is_some(),
                    is_final: class.is_final,
                    is_abstract: class.is_abstract,
                    is_sealed: class.is_sealed,
                    fqn: fqn.clone(),
                    parent_class: None,
                    constructor_visibility: class.constructor_visibility,
                    fields: vec![],
                    instance_methods: BTreeMap::new(),
                    static_methods: BTreeMap::new(),
                    generic_instance_methods: BTreeMap::new(),
                    generic_static_methods: BTreeMap::new(),
                    generic_static_globals: BTreeMap::new(),
                    source_file: class.name.span.file.clone(),
                    span: class.span.clone(),
                    constructor_params,
                    type_params,
                    type_param_variances,
                    trait_bounds,
                    body_members: vec![],
                    parent_type_expr: None,
                    extends_args_ast: vec![],
                    trait_impls: vec![],
                    default_supplied_members: Default::default(),
                },
            );
        }
    }

    /// Finish inferred public fields after functions and trait implementations exist.
    pub(super) fn resolve_class_field_types(&mut self, class: &ClassDecl) {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(class.name.value.clone()),
        };
        let Some(mut signature) = self
            .package_registry
            .lookup_class_type(&fqn, &self.package_path)
            .cloned()
        else {
            return;
        };
        let mut locals: Vec<(String, Type)> = signature
            .constructor_params
            .iter()
            .map(|param| (param.name.clone(), param.ty.clone()))
            .collect();
        for member in &class.body {
            let ClassMember::LetBinding(binding) = member else {
                continue;
            };
            if binding.is_static {
                continue;
            }
            let Some(field) = signature
                .fields
                .iter_mut()
                .find(|field| field.name == binding.name.value)
            else {
                locals.push((binding.name.value.clone(), Type::Error));
                continue;
            };
            if binding.type_annotation.is_none() && field.ty == Type::Error {
                let local_refs: Vec<(&str, &Type)> = locals
                    .iter()
                    .map(|(name, ty)| (name.as_str(), ty))
                    .collect();
                if let Some(ty) = self
                    .try_infer_expr_type_with_locals(
                        &binding.value,
                        &class.name.span.file,
                        &local_refs,
                    )
                    .filter(|ty| *ty != Type::Error)
                {
                    field.ty = ty;
                } else {
                    self.diagnostics.error(
                        binding.name.span.clone(),
                        format!(
                            "cannot infer type for let binding '{}' in class '{}'; add a type annotation",
                            binding.name.value, class.name.value
                        ),
                    );
                }
            }
            locals.push((binding.name.value.clone(), field.ty.clone()));
        }
        self.package_registry.register_class_type(fqn, signature);
    }

    /// Collect a class declaration into the package registry.
    pub(super) fn collect_class(&mut self, class: &ClassDecl) {
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(class.name.value.clone()),
        };

        if let Some(doc) = &class.doc_comment {
            self.package_registry
                .register_doc_comment(fqn.clone(), doc.clone());
        }

        let is_generic = !class.type_params.is_empty();

        // For generic classes, build type param names, variances, bounds, and type_params_map
        let type_params: Vec<TypeParamName> = class
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.name.value.clone()))
            .collect();

        let type_param_variances: Vec<Variance> =
            class.type_params.iter().map(|tp| tp.variance).collect();

        let trait_bounds = if is_generic {
            self.resolve_trait_bounds(&class.where_clause, &type_params)
        } else {
            TraitBounds::empty()
        };

        let type_params_map = if is_generic {
            Type::type_param_map(&type_params, &trait_bounds)
        } else {
            BTreeMap::new()
        };

        // Resolve constructor param types (with type params in scope for generic classes)
        let mut fields = Vec::new();
        let mut all_param_types: Vec<(String, Type)> = Vec::new();
        let mut seen_params = std::collections::BTreeSet::new();
        for param in &class.params {
            let ty = if is_generic {
                self.resolve_type_expr_with_type_params(&param.type_annotation, &type_params_map)
            } else {
                self.resolve_type_expr(&param.type_annotation)
            };
            if !seen_params.insert(param.name.value.clone()) {
                self.diagnostics.error(
                    param.name.span.clone(),
                    format!(
                        "duplicate constructor parameter '{}' in class '{}'",
                        param.name.value, class.name.value
                    ),
                );
            }
            all_param_types.push((param.name.value.clone(), ty.clone()));
            // All params go into registry fields; visibility is enforced at lookup time.
            fields.push(ClassFieldInfo {
                name: param.name.value.clone(),
                ty,
                visibility: param.visibility,
                mutable: param.mutable,
            });
        }

        // Build locals from ALL constructor params for type inference of let bindings
        let mut locals: Vec<(String, Type)> = all_param_types
            .iter()
            .map(|(n, t)| (n.clone(), t.clone()))
            .collect();

        // Own constructor params
        let constructor_params: Vec<ConstructorParam> = class
            .params
            .iter()
            .zip(all_param_types.iter())
            .map(|(p, (_, ty))| ConstructorParam {
                name: p.name.value.clone(),
                ty: ty.clone(),
                visibility: p.visibility,
                mutable: p.mutable,
            })
            .collect();

        // Collect body members (let bindings and expressions) for generic classes (deferred inference)
        let mut body_member_defs: Vec<ClassBodyMemberDef> = Vec::new();

        // Collect methods
        let mut instance_methods: BTreeMap<SymbolName, Vec<FunctionSignature>> = BTreeMap::new();
        let mut static_methods: BTreeMap<SymbolName, Vec<FunctionSignature>> = BTreeMap::new();
        let mut generic_instance_methods: BTreeMap<SymbolName, Vec<GenericClassMethodDef>> =
            BTreeMap::new();
        let mut generic_static_methods: BTreeMap<SymbolName, Vec<GenericClassMethodDef>> =
            BTreeMap::new();
        let mut generic_static_globals: BTreeMap<SymbolName, GenericClassStaticGlobalDef> =
            BTreeMap::new();

        for member in &class.body {
            match member {
                ClassMember::LetBinding(lb) if lb.is_static => {
                    // Static let bindings: register as globals, NOT instance fields.
                    if is_generic {
                        // Generic class: store for deferred instantiation
                        let resolved = lb.type_annotation.as_ref().map(|ta| {
                            self.resolve_type_expr_with_type_params(ta, &type_params_map)
                        });
                        generic_static_globals.insert(
                            SymbolName(lb.name.value.clone()),
                            GenericClassStaticGlobalDef {
                                visibility: lb.visibility,
                                type_params: type_params.clone(),
                                ty: resolved.clone(),
                                mutable: lb.mutable,
                                body: lb.value.clone(),
                                source_file: lb.name.span.file.clone(),
                                package: self.package_path.clone(),
                            },
                        );
                        body_member_defs.push(ClassBodyMemberDef::StaticLetBinding(
                            ClassLetBindingDef {
                                name: lb.name.value.clone(),
                                resolved_type: resolved,
                                body: lb.value.clone(),
                                visibility: lb.visibility,
                                mutable: lb.mutable,
                            },
                        ));
                    } else {
                        // Non-generic class: register as a concrete global with FQN ClassName.fieldName
                        let global_fqn = Fqn {
                            package: self.package_path.clone(),
                            symbol: SymbolName(format!("{}.{}", class.name.value, lb.name.value)),
                        };
                        let mangled_name = MangledName::for_global(&global_fqn);
                        let source_file = lb.name.span.file.clone();

                        if let Some(ref type_ann) = lb.type_annotation {
                            let ty = self.resolve_type_expr(type_ann);
                            self.package_registry.register_global(
                                global_fqn,
                                crate::typechecker::registry::GlobalSignature {
                                    visibility: lb.visibility,
                                    mangled_name,
                                    ty,
                                    mutable: lb.mutable,
                                    source_file,
                                },
                            );
                        } else {
                            // Try to infer type; if we can't, it will be inferred during inference phase.
                            // For now, register with Error type placeholder.
                            if let Some(ty) = self.try_infer_expr_type(&lb.value, &source_file) {
                                self.package_registry.register_global(
                                    global_fqn,
                                    crate::typechecker::registry::GlobalSignature {
                                        visibility: lb.visibility,
                                        mangled_name,
                                        ty,
                                        mutable: lb.mutable,
                                        source_file,
                                    },
                                );
                            } else {
                                // Defer: register without type, inference will handle it
                                self.package_registry.register_global(
                                    global_fqn,
                                    crate::typechecker::registry::GlobalSignature {
                                        visibility: lb.visibility,
                                        mangled_name,
                                        ty: Type::Error,
                                        mutable: lb.mutable,
                                        source_file,
                                    },
                                );
                            }
                        }
                    }
                }
                ClassMember::LetBinding(lb) => {
                    if is_generic && lb.visibility != Visibility::Private {
                        // Non-private let bindings on generic classes: resolve type at collect time.
                        let local_refs: Vec<(&str, &Type)> =
                            locals.iter().map(|(n, t)| (n.as_str(), t)).collect();
                        let ty = if let Some(ref type_ann) = lb.type_annotation {
                            self.resolve_type_expr_with_type_params(type_ann, &type_params_map)
                        } else if let Some(inferred) = self.try_infer_expr_type_with_locals(
                            &lb.value,
                            &class.name.span.file,
                            &local_refs,
                        ) {
                            inferred
                        } else {
                            // Operator implementations in this package are collected later.
                            // Retry unresolved public fields once those signatures exist.
                            Type::Error
                        };
                        locals.push((lb.name.value.clone(), ty.clone()));
                        fields.push(ClassFieldInfo {
                            name: lb.name.value.clone(),
                            ty,
                            visibility: lb.visibility,
                            mutable: lb.mutable,
                        });
                        // Store AST body for deferred inference during instantiation
                        let resolved = lb.type_annotation.as_ref().map(|ta| {
                            self.resolve_type_expr_with_type_params(ta, &type_params_map)
                        });
                        body_member_defs.push(ClassBodyMemberDef::LetBinding(ClassLetBindingDef {
                            name: lb.name.value.clone(),
                            resolved_type: resolved,
                            body: lb.value.clone(),
                            visibility: lb.visibility,
                            mutable: lb.mutable,
                        }));
                    } else if is_generic {
                        // Private let bindings on generic classes: defer to inference phase.
                        locals.push((lb.name.value.clone(), Type::Error));
                        // Store AST body for deferred inference during instantiation
                        let resolved = lb.type_annotation.as_ref().map(|ta| {
                            self.resolve_type_expr_with_type_params(ta, &type_params_map)
                        });
                        body_member_defs.push(ClassBodyMemberDef::LetBinding(ClassLetBindingDef {
                            name: lb.name.value.clone(),
                            resolved_type: resolved,
                            body: lb.value.clone(),
                            visibility: lb.visibility,
                            mutable: lb.mutable,
                        }));
                    } else if lb.visibility != Visibility::Private {
                        // Non-private let bindings: resolve type at collect time.
                        let local_refs: Vec<(&str, &Type)> =
                            locals.iter().map(|(n, t)| (n.as_str(), t)).collect();
                        let ty = if let Some(ref type_ann) = lb.type_annotation {
                            self.resolve_type_expr(type_ann)
                        } else if let Some(inferred) = self.try_infer_expr_type_with_locals(
                            &lb.value,
                            &class.name.span.file,
                            &local_refs,
                        ) {
                            inferred
                        } else {
                            // Operator implementations in this package are collected later.
                            // Retry unresolved public fields once those signatures exist.
                            Type::Error
                        };
                        locals.push((lb.name.value.clone(), ty.clone()));
                        fields.push(ClassFieldInfo {
                            name: lb.name.value.clone(),
                            ty,
                            visibility: lb.visibility,
                            mutable: lb.mutable,
                        });
                    } else {
                        // Non-public let bindings: defer to inference phase.
                        locals.push((lb.name.value.clone(), Type::Error));
                    }
                }
                ClassMember::Method(func) => {
                    let is_instance = func.params.first().is_some_and(|p| p.name.value == "self");

                    let method_has_own_type_params = !func.type_params.is_empty();

                    // If either the class is generic OR the method has its own type params,
                    // we need to store as a deferred generic method def.
                    if is_generic || method_has_own_type_params {
                        // Build method's own type params
                        let method_type_params: Vec<TypeParamName> = func
                            .type_params
                            .iter()
                            .map(|tp| TypeParamName(tp.value.clone()))
                            .collect();

                        let all_method_type_params: Vec<_> = type_params
                            .iter()
                            .chain(method_type_params.iter())
                            .cloned()
                            .collect();
                        let method_bounds = self.resolve_method_trait_bounds(
                            &func.where_clause,
                            &all_method_type_params,
                            &method_type_params,
                            &trait_bounds,
                        );
                        let mut merged_bounds = trait_bounds.clone();
                        merged_bounds.merge(&method_bounds);
                        let combined_type_params_map =
                            Type::type_param_map(&all_method_type_params, &merged_bounds);

                        // Resolve return type with combined type params
                        let return_type = match &func.return_type {
                            Some(type_expr) => self.resolve_type_expr_with_type_params(
                                type_expr,
                                &combined_type_params_map,
                            ),
                            None => Type::Unit,
                        };

                        // Resolve parameter types with combined type params
                        let params: Vec<(String, Type)> = func
                            .params
                            .iter()
                            .map(|p| {
                                let ty = self.resolve_type_expr_with_type_params(
                                    &p.type_annotation,
                                    &combined_type_params_map,
                                );
                                (p.name.value.clone(), ty)
                            })
                            .collect();

                        let def = GenericClassMethodDef {
                            visibility: func.visibility,
                            class_type_params: type_params.clone(),
                            method_type_params,
                            params,
                            return_type,
                            trait_bounds: merged_bounds,
                            is_final_method: func.is_final,
                            is_abstract_method: func.is_abstract,
                            is_property: false,
                            is_async: func.is_async,
                        };

                        let method_name = SymbolName(func.name.value.clone());
                        if is_instance {
                            generic_instance_methods
                                .entry(method_name)
                                .or_default()
                                .push(def);
                        } else {
                            generic_static_methods
                                .entry(method_name)
                                .or_default()
                                .push(def);
                        }
                    } else {
                        // Non-generic method on non-generic class: existing path
                        self.resolve_trait_bounds(&func.where_clause, &[]);
                        let return_type = match &func.return_type {
                            Some(type_expr) => self.resolve_type_expr(type_expr),
                            None => Type::Unit,
                        };

                        let params: Vec<(String, Type)> = func
                            .params
                            .iter()
                            .map(|p| {
                                let ty = self.resolve_type_expr(&p.type_annotation);
                                (p.name.value.clone(), ty)
                            })
                            .collect();

                        let qualified_symbol =
                            SymbolName(format!("{}.{}", class.name.value, func.name.value));
                        let method_fqn = Fqn {
                            package: self.package_path.clone(),
                            symbol: qualified_symbol,
                        };

                        let param_types: Vec<&Type> = params.iter().map(|(_, ty)| ty).collect();
                        let method_mangled = MangledName::for_function(&method_fqn, &param_types);

                        let sig = FunctionSignature {
                            visibility: func.visibility,
                            mangled_name: method_mangled.clone(),
                            params,
                            return_type,
                            source_file: func.name.span.file.clone(),
                            is_intrinsic: false,
                            is_property: false,
                            is_final_method: func.is_final,
                            is_abstract_method: func.is_abstract,
                        };

                        // Register in main function registry with qualified FQN
                        self.package_registry
                            .register_function(method_fqn, sig.clone());

                        let method_name = SymbolName(func.name.value.clone());
                        if is_instance {
                            instance_methods.entry(method_name).or_default().push(sig);
                        } else {
                            static_methods.entry(method_name).or_default().push(sig);
                        }
                    }
                }
                ClassMember::Property(prop) => {
                    let is_instance = prop.params.first().is_some_and(|p| p.name.value == "self");

                    let prop_has_own_type_params = !prop.type_params.is_empty();

                    if is_generic || prop_has_own_type_params {
                        let method_type_params: Vec<TypeParamName> = prop
                            .type_params
                            .iter()
                            .map(|tp| TypeParamName(tp.value.clone()))
                            .collect();

                        let mut combined_type_params_map = type_params_map.clone();
                        if prop_has_own_type_params {
                            let method_bounds = self.resolve_trait_bounds(&[], &method_type_params);
                            let method_tp_map =
                                Type::type_param_map(&method_type_params, &method_bounds);
                            combined_type_params_map.extend(method_tp_map);
                        }

                        let mut merged_bounds = trait_bounds.clone();
                        if prop_has_own_type_params {
                            let method_bounds = self.resolve_trait_bounds(&[], &method_type_params);
                            merged_bounds.merge(&method_bounds);
                        }

                        let return_type = self.resolve_type_expr_with_type_params(
                            &prop.return_type,
                            &combined_type_params_map,
                        );

                        let params: Vec<(String, Type)> = prop
                            .params
                            .iter()
                            .map(|p| {
                                let ty = self.resolve_type_expr_with_type_params(
                                    &p.type_annotation,
                                    &combined_type_params_map,
                                );
                                (p.name.value.clone(), ty)
                            })
                            .collect();

                        let def = GenericClassMethodDef {
                            visibility: prop.visibility,
                            class_type_params: type_params.clone(),
                            method_type_params,
                            params,
                            return_type,
                            trait_bounds: merged_bounds,
                            is_final_method: prop.is_final,
                            is_abstract_method: prop.is_abstract,
                            is_property: true,
                            is_async: false,
                        };

                        let prop_name = SymbolName(prop.name.value.clone());
                        if is_instance {
                            generic_instance_methods
                                .entry(prop_name)
                                .or_default()
                                .push(def);
                        } else {
                            generic_static_methods
                                .entry(prop_name)
                                .or_default()
                                .push(def);
                        }
                    } else {
                        let return_type = self.resolve_type_expr(&prop.return_type);

                        let params: Vec<(String, Type)> = prop
                            .params
                            .iter()
                            .map(|p| {
                                let ty = self.resolve_type_expr(&p.type_annotation);
                                (p.name.value.clone(), ty)
                            })
                            .collect();

                        let qualified_symbol =
                            SymbolName(format!("{}.{}", class.name.value, prop.name.value));
                        let method_fqn = Fqn {
                            package: self.package_path.clone(),
                            symbol: qualified_symbol,
                        };

                        let param_types: Vec<&Type> = params.iter().map(|(_, ty)| ty).collect();
                        let method_mangled = MangledName::for_function(&method_fqn, &param_types);

                        let sig = FunctionSignature {
                            visibility: prop.visibility,
                            mangled_name: method_mangled.clone(),
                            params,
                            return_type,
                            source_file: prop.name.span.file.clone(),
                            is_intrinsic: false,
                            is_property: true,
                            is_final_method: prop.is_final,
                            is_abstract_method: prop.is_abstract,
                        };

                        self.package_registry
                            .register_function(method_fqn, sig.clone());

                        let prop_name = SymbolName(prop.name.value.clone());
                        if is_instance {
                            instance_methods.entry(prop_name).or_default().push(sig);
                        } else {
                            static_methods.entry(prop_name).or_default().push(sig);
                        }
                    }
                }
                ClassMember::Expression(expr) => {
                    if is_generic {
                        body_member_defs.push(ClassBodyMemberDef::Expression(expr.clone()));
                    }
                    // Non-generic expressions are type-checked during inference
                }
            }
        }

        // Resolve parent class if extends clause present
        let parent_class = if let Some(ref ext) = class.extends {
            match &ext.parent_type {
                crate::parser::ast::TypeExpr::Named(named) => {
                    let parent_ty = if is_generic {
                        self.resolve_type_expr_with_type_params(&ext.parent_type, &type_params_map)
                    } else {
                        self.resolve_type_expr(&ext.parent_type)
                    };
                    match &parent_ty {
                        Type::Class(parent_fqn, _)
                        | Type::GenericClass {
                            fqn: parent_fqn, ..
                        } => Some(parent_fqn.clone()),
                        Type::Error => None,
                        _ => {
                            self.diagnostics.error(
                                named.span.clone(),
                                format!(
                                    "'{}' is not a class type and cannot be extended",
                                    named.name.value
                                ),
                            );
                            None
                        }
                    }
                }
                _ => {
                    self.diagnostics.error(
                        ext.span.clone(),
                        "extends clause must specify a class type name".to_string(),
                    );
                    None
                }
            }
        } else {
            None
        };

        // Store the resolved parent type. For generic classes the parent type may carry
        // TypeParameter placeholders (resolved against the child's type params). For
        // non-generic classes the parent type is fully concrete (e.g. `Base<Int32>`) but
        // we still need it so field-access substitution can recover the concrete inherited
        // field types.
        let parent_type_expr = class.extends.as_ref().map(|ext| {
            if is_generic {
                self.resolve_type_expr_with_type_params(&ext.parent_type, &type_params_map)
            } else {
                self.resolve_type_expr(&ext.parent_type)
            }
        });
        let extends_args_ast = if is_generic {
            class
                .extends
                .as_ref()
                .map(|ext| ext.super_args.clone())
                .unwrap_or_default()
        } else {
            vec![]
        };

        // Register class type signature
        self.package_registry.register_class_type(
            fqn.clone(),
            ClassTypeSignature {
                visibility: class.visibility,
                is_string_literal: class.string_literal.is_some(),
                is_final: class.is_final,
                is_abstract: class.is_abstract,
                is_sealed: class.is_sealed,
                fqn: fqn.clone(),
                parent_class,
                constructor_visibility: class.constructor_visibility,
                fields,
                instance_methods,
                static_methods,
                generic_instance_methods,
                generic_static_methods,
                generic_static_globals,
                source_file: class.name.span.file.clone(),
                span: class.span.clone(),
                constructor_params,
                type_params,
                type_param_variances,
                trait_bounds,
                body_members: body_member_defs,
                parent_type_expr,
                extends_args_ast,
                trait_impls: vec![],
                default_supplied_members: Default::default(),
            },
        );

        // Process implements clause
        if !class.implements.is_empty() {
            self.collect_generic_class_defaults(class, &fqn);
            if is_generic {
                self.collect_generic_class_implements(class, &fqn);
            } else {
                self.collect_class_implements(class, &fqn);
            }
        }
    }

    /// Process the `implements` clause on a class declaration.
    /// For each trait, validate that the class provides all required methods
    /// (either directly, via inheritance, or as abstract declarations),
    /// then register implement blocks.
    fn collect_class_implements(&mut self, class: &ClassDecl, class_fqn: &Fqn) {
        let class_sig = match self
            .package_registry
            .lookup_class_type(class_fqn, &self.package_path)
        {
            Some(sig) => sig.clone(),
            None => return,
        };

        let class_type = Type::Class(class_fqn.clone(), MangledName::for_type(class_fqn));

        for trait_type_expr in &class.implements {
            self.collect_one_class_trait_impl(
                class,
                class_fqn,
                &class_sig,
                &class_type,
                trait_type_expr,
            );
        }
    }

    fn collect_one_class_trait_impl(
        &mut self,
        class: &ClassDecl,
        class_fqn: &Fqn,
        class_sig: &ClassTypeSignature,
        class_type: &Type,
        trait_type_expr: &TypeExpr,
    ) {
        // Resolve the trait type expression
        let (trait_name, trait_type_args_exprs) = match trait_type_expr {
            TypeExpr::Named(named) => (&named.name, &named.type_args),
            _ => {
                self.diagnostics.error(
                    trait_type_expr.span(),
                    "implements clause must specify a trait name".to_string(),
                );
                return;
            }
        };

        let (trait_fqn, trait_sig) = match self.resolve_trait(&trait_name.value) {
            Some(result) => result,
            None => {
                self.diagnostics.error(
                    trait_name.span.clone(),
                    format!("unknown trait: '{}'", trait_name.value),
                );
                return;
            }
        };

        if crate::typechecker::types::is_tuple_constraint(&trait_fqn) {
            self.diagnostics.error(
                trait_name.span.clone(),
                "Tuple is a built-in structural constraint and cannot be implemented",
            );
            return;
        }

        // Validate trait type args
        let trait_type_param_count = trait_sig.type_params.len();
        let impl_type_arg_count = trait_type_args_exprs.len();

        if trait_type_param_count > 0 && impl_type_arg_count == 0 {
            self.diagnostics.error(
                trait_name.span.clone(),
                format!(
                    "trait '{}' has {} type parameter(s), but no type arguments were provided",
                    trait_name.value, trait_type_param_count
                ),
            );
            return;
        }
        if trait_type_param_count == 0 && impl_type_arg_count > 0 {
            self.diagnostics.error(
                trait_name.span.clone(),
                format!(
                    "trait '{}' has no type parameters, but {} type argument(s) were provided",
                    trait_name.value, impl_type_arg_count
                ),
            );
            return;
        }
        if trait_type_param_count != impl_type_arg_count {
            self.diagnostics.error(
                trait_name.span.clone(),
                format!(
                    "trait '{}' expects {} type argument(s), but {} were provided",
                    trait_name.value, trait_type_param_count, impl_type_arg_count
                ),
            );
            return;
        }

        let resolved_trait_type_args: Vec<Type> = trait_type_args_exprs
            .iter()
            .map(|te| self.resolve_type_expr(te))
            .collect();

        if self.class_trait_applications_require_overloads(
            class_fqn,
            &trait_sig,
            &resolved_trait_type_args,
        ) {
            self.diagnostics.error(trait_name.span.clone(), format!(
                "class '{}' cannot implement different applications of trait '{}' that require overloaded members; use separate implementing types",
                class.name.value, trait_name.value,
            ));
            return;
        }

        let trait_subst: BTreeMap<TypeParamName, Type> = trait_sig
            .type_params
            .iter()
            .zip(resolved_trait_type_args.iter())
            .map(|(param, arg)| (param.clone(), arg.clone()))
            .collect();

        // Defaulted trait members the class omits: (name, params, return, is_property).
        // (member, params, return type, is_property, supplying trait)
        type SynthesizedDefaultMember = (String, Vec<(String, Type)>, Type, bool, Option<Fqn>);
        let mut synthesized_default_members: Vec<SynthesizedDefaultMember> = Vec::new();

        // Substitute Self → class_type and trait type params → concrete types
        #[allow(clippy::type_complexity)]
        let substituted_methods: Vec<(String, Vec<(String, Type)>, Type)> = trait_sig
            .methods
            .iter()
            .map(|m| {
                let mut member_substitution = trait_subst.clone();
                for parameter in &m.type_params {
                    member_substitution.insert(
                        parameter.clone(),
                        Type::TypeVariable(
                            TypeParamName(format!("$method${}", parameter.0)),
                            vec![],
                        ),
                    );
                }
                member_substitution.insert(TypeParamName("Self".to_string()), class_type.clone());
                let params: Vec<(String, Type)> = m
                    .params
                    .iter()
                    .map(|(name, ty)| {
                        let ty = substitute_trait_type_params(ty, &member_substitution);
                        let ty = substitute_self(&ty, class_type);
                        (name.clone(), ty)
                    })
                    .collect();
                let return_type =
                    substitute_trait_type_params(&m.return_type, &member_substitution);
                let return_type = substitute_self(&return_type, class_type);
                (m.name.clone(), params, return_type)
            })
            .collect();

        // Match class methods to trait requirements
        let mut matched_trait_methods = std::collections::BTreeSet::new();

        for ((trait_method_name, trait_params, trait_return_type), trait_method) in
            substituted_methods.iter().zip(&trait_sig.methods)
        {
            let method_name = SymbolName(trait_method_name.clone());

            // Only ONE default body can back a class member: the class's
            // method table is name-keyed and the materialized body is shared,
            // so a second trait's default for the same name would silently
            // reuse the first (or collide on one mangled name). Checked
            // BEFORE the lookup below, which would otherwise match the entry
            // synthesized for the first trait and look satisfied.
            // Identity is the default source and its substituted type arguments,
            // not the spelled trait name: the same source application reached
            // through multiple supers is one default.
            let this_default_source = trait_method.default_source.clone();
            // Already queued in THIS clause (both applications of one
            // declaration are processed before any registration happens).
            if let Some((_, _, _, _, queued_src)) =
                synthesized_default_members
                    .iter()
                    .find(|(name, parameters, _, _, _)| {
                        name == trait_method_name && parameters == trait_params
                    })
                && *queued_src == this_default_source
            {
                matched_trait_methods.insert(trait_method_name.clone());
                continue;
            }
            // Already supplied by an earlier `implements` clause. Same
            // source application → one declaration reached twice, already satisfied
            // (`class_sig` here is a snapshot taken before the clause loop,
            // so the lookup below would not see it).
            let recorded = if trait_sig
                .methods
                .iter()
                .filter(|method| method.name == *trait_method_name)
                .count()
                > 1
            {
                None
            } else {
                self.package_registry
                    .get_class_type(class_fqn)
                    .and_then(|sig| sig.default_supplied_members.get(&method_name))
                    .cloned()
            };
            if let Some(recorded_src) = &recorded
                && Some(recorded_src) == this_default_source.as_ref()
            {
                if self.class_default_application_conflicts(
                    class_fqn,
                    &method_name,
                    recorded_src,
                    &trait_fqn,
                    &resolved_trait_type_args,
                ) {
                    self.diagnostics.error(
                            trait_name.span.clone(),
                            format!(
                                "class '{}' inherits default implementations of '{}' from different applications of trait '{}'; define '{}' in the class to choose one",
                                class.name.value, trait_method_name, recorded_src.symbol, trait_method_name,
                            ),
                        );
                }
                matched_trait_methods.insert(trait_method_name.clone());
                continue;
            }
            if let Some(other_trait) = recorded
                .filter(|src| Some(src) != this_default_source.as_ref())
                .map(|src| src.symbol.0.clone())
            {
                self.diagnostics.error(
                    trait_name.span.clone(),
                    format!(
                        "class '{}' inherits default implementations of '{}' from both trait '{}' and trait '{}'; define '{}' in the class to choose one",
                        class.name.value, trait_method_name, other_trait,
                        trait_name.value, trait_method_name,
                    ),
                );
                matched_trait_methods.insert(trait_method_name.clone());
                continue;
            }

            // Search own instance methods
            let found_sig = self.find_matching_method_in_class(
                class_sig,
                &method_name,
                trait_params,
                trait_return_type,
                false,
            );

            // If not found, walk parent hierarchy
            let found_sig = found_sig.or_else(|| {
                self.find_matching_method_in_parents(
                    class_sig,
                    &method_name,
                    trait_params,
                    trait_return_type,
                    false,
                )
            });

            let found_sig = found_sig.filter(|_| trait_method.type_params.is_empty());

            let own_parameters = trait_method
                .type_params
                .iter()
                .map(|name| TypeParamName(format!("$method${}", name.0)))
                .collect::<Vec<_>>();
            let generic_found = self.has_matching_generic_method_in_class(
                class_sig,
                &method_name,
                trait_params,
                trait_return_type,
                &own_parameters,
                false,
            );
            if found_sig.is_some() || generic_found {
                matched_trait_methods.insert(trait_method_name.clone());
            } else {
                // Check if class has an abstract declaration for this method
                let has_abstract = class.body.iter().any(|m| {
                    if let ClassMember::Method(func) = m {
                        func.name.value == *trait_method_name && func.is_abstract
                    } else {
                        false
                    }
                });
                // Defaulted trait member: the class may omit it — register a
                // synthesized class method signature; the body is materialized
                // from the default template at monomorphize.
                let has_default = trait_method.default_source.is_some();

                if has_abstract {
                    matched_trait_methods.insert(trait_method_name.clone());
                } else if has_default {
                    matched_trait_methods.insert(trait_method_name.clone());
                    synthesized_default_members.push((
                        trait_method_name.clone(),
                        trait_params.clone(),
                        trait_return_type.clone(),
                        false,
                        this_default_source.clone(),
                    ));
                } else {
                    self.diagnostics.error(
                        trait_name.span.clone(),
                        format!(
                            "class '{}' must implement method '{}' from trait '{}' or declare it abstract",
                            class.name.value, trait_method_name, trait_name.value
                        ),
                    );
                }
            }
        }

        // Match class properties to trait property requirements
        for trait_prop in &trait_sig.properties {
            let prop_params: Vec<(String, Type)> = trait_prop
                .params
                .iter()
                .map(|(name, ty)| {
                    let ty = substitute_trait_type_params(ty, &trait_subst);
                    let ty = substitute_self(&ty, class_type);
                    (name.clone(), ty)
                })
                .collect();
            let prop_return_type =
                substitute_trait_type_params(&trait_prop.return_type, &trait_subst);
            let prop_return_type = substitute_self(&prop_return_type, class_type);

            let prop_name = SymbolName(trait_prop.name.clone());

            // Search own instance methods for a matching property entry
            let found_sig = self.find_matching_method_in_class(
                class_sig,
                &prop_name,
                &prop_params,
                &prop_return_type,
                true,
            );

            let found_sig = found_sig.or_else(|| {
                self.find_matching_method_in_parents(
                    class_sig,
                    &prop_name,
                    &prop_params,
                    &prop_return_type,
                    true,
                )
            });

            if found_sig.is_some() {
                matched_trait_methods.insert(trait_prop.name.clone());
            } else {
                // Check if class has an abstract property declaration
                let has_abstract = class.body.iter().any(|m| {
                    if let ClassMember::Property(p) = m {
                        p.name.value == trait_prop.name && p.is_abstract
                    } else {
                        false
                    }
                });

                let has_default = trait_prop.default_source.is_some();
                if has_abstract {
                    matched_trait_methods.insert(trait_prop.name.clone());
                } else if has_default {
                    matched_trait_methods.insert(trait_prop.name.clone());
                    let this_default_source = trait_sig
                        .properties
                        .iter()
                        .find(|p| p.name == trait_prop.name)
                        .and_then(|p| p.default_source.clone());
                    if let Some(source) = &this_default_source
                        && self.class_default_application_conflicts(
                            class_fqn,
                            &prop_name,
                            source,
                            &trait_fqn,
                            &resolved_trait_type_args,
                        )
                    {
                        self.diagnostics.error(
                                trait_name.span.clone(),
                                format!(
                                    "class '{}' inherits default implementations of '{}' from different applications of trait '{}'; define '{}' in the class to choose one",
                                    class.name.value, trait_prop.name, source.symbol, trait_prop.name,
                                ),
                            );
                        continue;
                    }
                    if let Some(other_trait) = self
                        .package_registry
                        .get_class_type(class_fqn)
                        .and_then(|sig| {
                            sig.default_supplied_members
                                .get(&SymbolName(trait_prop.name.clone()))
                        })
                        .filter(|src| Some(*src) != this_default_source.as_ref())
                        .map(|src| src.symbol.0.clone())
                    {
                        self.diagnostics.error(
                            trait_name.span.clone(),
                            format!(
                                "class '{}' inherits default implementations of '{}' from both trait '{}' and trait '{}'; define '{}' in the class to choose one",
                                class.name.value, trait_prop.name, other_trait,
                                trait_name.value, trait_prop.name,
                            ),
                        );
                        continue;
                    }
                    synthesized_default_members.push((
                        trait_prop.name.clone(),
                        prop_params.clone(),
                        prop_return_type.clone(),
                        true,
                        this_default_source.clone(),
                    ));
                } else {
                    self.diagnostics.error(
                        trait_name.span.clone(),
                        format!(
                            "class '{}' must implement property '{}' from trait '{}' or declare it abstract",
                            class.name.value, trait_prop.name, trait_name.value
                        ),
                    );
                }
            }
        }

        // Register synthesized class-method signatures for defaulted members —
        // downstream dispatch/vtable resolution finds them by name; the bodies
        // are materialized from the default templates at monomorphize.
        for (member_name, params, return_type, is_property, _from_trait) in
            synthesized_default_members
        {
            // A defaulted STATIC trait member must not be registered as an
            // instance method: downstream dispatch would hand it a receiver
            // and materialization would emit a self-taking body for a static
            // call site (invalid WASM). Classes cannot provide static trait
            // members through the default machinery today — the class must
            // define the static itself, which `find_matching_method_in_class`
            // now accepts.
            if !params.first().is_some_and(|(n, _)| n == "self") {
                self.diagnostics.error(
                    class.name.span.clone(),
                    format!(
                        "class '{}' must define static function '{}' from trait '{}': a trait's default body cannot supply a static member to a class",
                        class.name.value, member_name, trait_name.value,
                    ),
                );
                continue;
            }
            let qualified_symbol = SymbolName(format!("{}.{}", class.name.value, member_name));
            let method_fqn = Fqn {
                package: self.package_path.clone(),
                symbol: qualified_symbol,
            };
            let param_types: Vec<&Type> = params.iter().map(|(_, ty)| ty).collect();
            let method_mangled = MangledName::for_function(&method_fqn, &param_types);
            let sig = FunctionSignature {
                visibility: Visibility::Public,
                mangled_name: method_mangled,
                params,
                return_type,
                source_file: class.name.span.file.clone(),
                is_intrinsic: false,
                is_property,
                is_final_method: false,
                is_abstract_method: false,
            };
            let member_sym = SymbolName(member_name);
            self.package_registry
                .add_class_instance_method(class_fqn, member_sym.clone(), sig);
            if let Some(src) = _from_trait {
                self.package_registry
                    .note_default_supplied_member(class_fqn, member_sym, src);
            }
        }

        // Check for duplicate trait impl before registering
        let merged_has_dup = self.dependency_registry.has_trait_impl_with_args(
            &trait_fqn,
            class_fqn,
            &resolved_trait_type_args,
        );
        let pkg_has_dup = self.package_registry.has_trait_impl_with_args(
            &trait_fqn,
            class_fqn,
            &resolved_trait_type_args,
        );

        if merged_has_dup || pkg_has_dup {
            self.diagnostics.error(
                trait_name.span.clone(),
                format!(
                    "type '{}' already implements trait '{}'",
                    class_fqn.symbol.0, trait_name.value
                ),
            );
        } else {
            self.package_registry.add_class_trait_impl(
                class_fqn,
                trait_fqn.clone(),
                resolved_trait_type_args.clone(),
            );
        }
    }

    /// Interface/class member routing cannot distinguish same-name overloads
    /// required by different generic applications of a trait.
    fn class_trait_applications_require_overloads(
        &self,
        class_fqn: &Fqn,
        trait_sig: &TraitSignature,
        trait_args: &[Type],
    ) -> bool {
        let Some(class_sig) = self.package_registry.get_class_type(class_fqn) else {
            return false;
        };
        let current: BTreeMap<_, _> = trait_sig
            .type_params
            .iter()
            .cloned()
            .zip(trait_args.iter().cloned())
            .collect();
        class_sig
            .trait_impls
            .iter()
            .any(|(prior_trait, prior_args)| {
                if *prior_trait != trait_sig.fqn || prior_args == trait_args {
                    return false;
                }
                let prior: BTreeMap<_, _> = trait_sig
                    .type_params
                    .iter()
                    .cloned()
                    .zip(prior_args.iter().cloned())
                    .collect();
                let differs = |params: &[(String, Type)], ret: &Type| {
                    params
                        .iter()
                        .map(|(_, ty)| ty)
                        .chain(std::iter::once(ret))
                        .any(|ty| {
                            substitute_trait_type_params(ty, &prior)
                                != substitute_trait_type_params(ty, &current)
                        })
                };
                trait_sig
                    .methods
                    .iter()
                    .any(|method| differs(&method.params, &method.return_type))
                    || trait_sig
                        .properties
                        .iter()
                        .any(|property| differs(&property.params, &property.return_type))
            })
    }

    /// The same default declaration can have different bodies/signatures after
    /// substitution. Sharing a class member requires the same source application,
    /// including when that source is reached through different subtraits.
    fn class_default_application_conflicts(
        &self,
        class_fqn: &Fqn,
        member: &SymbolName,
        source: &Fqn,
        trait_fqn: &Fqn,
        trait_args: &[Type],
    ) -> bool {
        let Some(class_sig) = self.package_registry.get_class_type(class_fqn) else {
            return false;
        };
        if class_sig.default_supplied_members.get(member) != Some(source) {
            return false;
        }
        let source_args = |fqn: &Fqn, args: &[Type]| {
            if fqn == source {
                Some(args.to_vec())
            } else {
                self.package_registry
                    .super_closure_args(fqn, args, source)
                    .or_else(|| {
                        self.dependency_registry
                            .super_closure_args(fqn, args, source)
                    })
            }
        };
        let current_args = source_args(trait_fqn, trait_args);
        class_sig.trait_impls.iter().any(|(prior_fqn, prior_args)| {
            let Some(prior_sig) = self
                .package_registry
                .get_trait(prior_fqn)
                .or_else(|| self.dependency_registry.get_trait(prior_fqn))
            else {
                return false;
            };
            let supplies_member = prior_sig
                .methods
                .iter()
                .any(|m| m.name == member.0 && m.default_source.as_ref() == Some(source))
                || prior_sig
                    .properties
                    .iter()
                    .any(|p| p.name == member.0 && p.default_source.as_ref() == Some(source));
            supplies_member && source_args(prior_fqn, prior_args) != current_args
        })
    }

    /// Find a matching instance method in the class's own methods.
    fn find_matching_method_in_class(
        &self,
        class_sig: &ClassTypeSignature,
        method_name: &SymbolName,
        trait_params: &[(String, Type)],
        trait_return_type: &Type,
        is_property: bool,
    ) -> Option<FunctionSignature> {
        // A STATIC trait member (no `self` param) is satisfied by a static
        // class function, never by an instance method — classes support both,
        // and matching across the two kinds would give one the other's
        // calling convention.
        let table = if trait_params.first().is_some_and(|(n, _)| n == "self") {
            &class_sig.instance_methods
        } else {
            &class_sig.static_methods
        };
        if let Some(overloads) = table.get(method_name) {
            for sig in overloads {
                if sig.is_property == is_property
                    && self.method_signature_matches(sig, trait_params, trait_return_type)
                {
                    return Some(sig.clone());
                }
            }
        }
        None
    }

    /// Walk parent class hierarchy to find a method matching the trait requirement.
    fn find_matching_method_in_parents(
        &self,
        class_sig: &ClassTypeSignature,
        method_name: &SymbolName,
        trait_params: &[(String, Type)],
        trait_return_type: &Type,
        is_property: bool,
    ) -> Option<FunctionSignature> {
        let mut current_parent = class_sig.parent_class.clone();
        while let Some(ref parent_fqn) = current_parent {
            let parent_sig = self
                .package_registry
                .lookup_class_type(parent_fqn, &self.package_path)
                .or_else(|| {
                    self.dependency_registry
                        .lookup_class_type(parent_fqn, &self.package_path)
                })?;
            if let Some(overloads) = parent_sig.instance_methods.get(method_name) {
                for sig in overloads {
                    if sig.is_property == is_property
                        && self.method_signature_matches(sig, trait_params, trait_return_type)
                    {
                        return Some(sig.clone());
                    }
                }
            }
            current_parent = parent_sig.parent_class.clone();
        }
        None
    }

    /// Check if a function signature matches the expected trait method params and return type.
    /// For the `self` parameter (first), allows class hierarchy matches (parent type accepts child).
    /// For other parameters and return type, requires exact match.
    fn method_signature_matches(
        &self,
        sig: &FunctionSignature,
        trait_params: &[(String, Type)],
        trait_return_type: &Type,
    ) -> bool {
        if sig.params.len() != trait_params.len() {
            return false;
        }
        for (i, ((_, sig_ty), (_, trait_ty))) in
            sig.params.iter().zip(trait_params.iter()).enumerate()
        {
            if *sig_ty == *trait_ty {
                continue;
            }
            // For `self` parameter (index 0), allow class hierarchy match:
            // the actual method may take a parent type while the trait expects the child type.
            if i == 0
                && let (Type::Class(sig_fqn, _), Type::Class(trait_fqn, _)) = (sig_ty, trait_ty)
                && (self.package_registry.class_is_subtype(trait_fqn, sig_fqn)
                    || self
                        .dependency_registry
                        .class_is_subtype(trait_fqn, sig_fqn))
            {
                continue;
            }
            return false;
        }
        sig.return_type == *trait_return_type
    }

    /// Process the `implements` clause on a generic class declaration.
    /// Registers implement blocks with TypeParameter placeholders
    /// in `for_type`, matching the pattern used by `collect_generic_implement` for generic impl blocks.
    fn collect_generic_class_implements(&mut self, class: &ClassDecl, class_fqn: &Fqn) {
        let class_sig = match self
            .package_registry
            .lookup_class_type(class_fqn, &self.package_path)
        {
            Some(sig) => sig.clone(),
            None => return,
        };

        // Build for_type with TypeParameter placeholders
        let type_params = &class_sig.type_params;
        let type_params_map = Type::type_param_map(type_params, &class_sig.trait_bounds);

        let placeholder_types: Vec<Type> = type_params
            .iter()
            .map(|tp| {
                let bounds = class_sig.trait_bounds.get(tp).cloned().unwrap_or_default();
                Type::TypeVariable(tp.clone(), bounds)
            })
            .collect();

        let placeholder_type_args: Vec<(crate::common::types::Variance, Type)> = class_sig
            .type_param_variances
            .iter()
            .zip(placeholder_types.iter())
            .map(|(v, t)| (*v, t.clone()))
            .collect();

        let mangled_name = MangledName::for_type(class_fqn);

        let for_type = Type::GenericClass {
            fqn: class_fqn.clone(),
            mangled_name,
            type_args: placeholder_type_args,
        };

        for trait_type_expr in &class.implements {
            self.collect_one_generic_class_trait_impl(
                class,
                class_fqn,
                &class_sig,
                &for_type,
                trait_type_expr,
                &type_params_map,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn collect_one_generic_class_trait_impl(
        &mut self,
        class: &ClassDecl,
        class_fqn: &Fqn,
        class_sig: &ClassTypeSignature,
        for_type: &Type,
        trait_type_expr: &TypeExpr,
        type_params_map: &std::collections::BTreeMap<String, Type>,
    ) {
        // Resolve the trait type expression
        let (trait_name, trait_type_args_exprs) = match trait_type_expr {
            TypeExpr::Named(named) => (&named.name, &named.type_args),
            _ => {
                self.diagnostics.error(
                    trait_type_expr.span(),
                    "implements clause must specify a trait name".to_string(),
                );
                return;
            }
        };

        let (trait_fqn, trait_sig) = match self.resolve_trait(&trait_name.value) {
            Some(result) => result,
            None => {
                self.diagnostics.error(
                    trait_name.span.clone(),
                    format!("unknown trait: '{}'", trait_name.value),
                );
                return;
            }
        };

        if crate::typechecker::types::is_tuple_constraint(&trait_fqn) {
            self.diagnostics.error(
                trait_name.span.clone(),
                "Tuple is a built-in structural constraint and cannot be implemented",
            );
            return;
        }

        // Validate trait type args count
        let trait_type_param_count = trait_sig.type_params.len();
        let impl_type_arg_count = trait_type_args_exprs.len();
        if trait_type_param_count != impl_type_arg_count {
            if trait_type_param_count > 0 {
                self.diagnostics.error(
                    trait_name.span.clone(),
                    format!(
                        "trait '{}' expects {} type argument(s), but {} were provided",
                        trait_name.value, trait_type_param_count, impl_type_arg_count
                    ),
                );
            }
            return;
        }

        // Resolve trait type args (may contain TypeParameters from the class)
        let resolved_trait_type_args: Vec<Type> = trait_type_args_exprs
            .iter()
            .map(|te| self.resolve_type_expr_with_type_params(te, type_params_map))
            .collect();

        if self.class_trait_applications_require_overloads(
            class_fqn,
            &trait_sig,
            &resolved_trait_type_args,
        ) {
            self.diagnostics.error(trait_name.span.clone(), format!(
                "class '{}' cannot implement different applications of trait '{}' that require overloaded members; use separate implementing types",
                class.name.value, trait_name.value,
            ));
            return;
        }

        let trait_subst: std::collections::BTreeMap<crate::common::types::TypeParamName, Type> =
            trait_sig
                .type_params
                .iter()
                .zip(resolved_trait_type_args.iter())
                .map(|(param, arg)| (param.clone(), arg.clone()))
                .collect();

        // Substitute Self → for_type and trait type params → resolved types
        #[allow(clippy::type_complexity)]
        let substituted_methods: Vec<(String, Vec<(String, Type)>, Type)> = trait_sig
            .methods
            .iter()
            .map(|m| {
                let mut member_substitution = trait_subst.clone();
                for parameter in &m.type_params {
                    member_substitution.insert(
                        parameter.clone(),
                        Type::TypeVariable(
                            TypeParamName(format!("$method${}", parameter.0)),
                            vec![],
                        ),
                    );
                }
                member_substitution.insert(TypeParamName("Self".to_string()), for_type.clone());
                let params: Vec<(String, Type)> = m
                    .params
                    .iter()
                    .map(|(name, ty)| {
                        let ty = substitute_trait_type_params(ty, &member_substitution);
                        let ty = substitute_self(&ty, for_type);
                        (name.clone(), ty)
                    })
                    .collect();
                let return_type =
                    substitute_trait_type_params(&m.return_type, &member_substitution);
                let return_type = substitute_self(&return_type, for_type);
                (m.name.clone(), params, return_type)
            })
            .collect();

        for ((trait_method_name, trait_params, trait_return_type), trait_method) in
            substituted_methods.iter().zip(&trait_sig.methods)
        {
            let method_name = SymbolName(trait_method_name.clone());

            // Search generic_instance_methods (all methods on generic classes are stored here)
            let has_member = self.has_matching_generic_method_in_class(
                class_sig,
                &method_name,
                trait_params,
                trait_return_type,
                &trait_method
                    .type_params
                    .iter()
                    .map(|name| TypeParamName(format!("$method${}", name.0)))
                    .collect::<Vec<_>>(),
                false,
            );

            if !has_member {
                // Check if class has an abstract declaration for this method
                let has_abstract = class.body.iter().any(|m| {
                    if let ClassMember::Method(func) = m {
                        func.name.value == *trait_method_name && func.is_abstract
                    } else {
                        false
                    }
                });

                if !has_abstract {
                    self.diagnostics.error(
                        trait_name.span.clone(),
                        format!(
                            "class '{}' must implement method '{}' from trait '{}' or declare it abstract",
                            class.name.value, trait_method_name, trait_name.value
                        ),
                    );
                }
            }
        }

        // Match class properties to trait property requirements
        for trait_prop in &trait_sig.properties {
            let prop_params: Vec<(String, Type)> = trait_prop
                .params
                .iter()
                .map(|(name, ty)| {
                    let ty = substitute_trait_type_params(ty, &trait_subst);
                    let ty = substitute_self(&ty, for_type);
                    (name.clone(), ty)
                })
                .collect();
            let prop_return_type =
                substitute_trait_type_params(&trait_prop.return_type, &trait_subst);
            let prop_return_type = substitute_self(&prop_return_type, for_type);

            let prop_name = SymbolName(trait_prop.name.clone());

            let has_member = self.has_matching_generic_method_in_class(
                class_sig,
                &prop_name,
                &prop_params,
                &prop_return_type,
                &[],
                true,
            );

            if !has_member {
                let has_abstract = class.body.iter().any(|m| {
                    if let ClassMember::Property(p) = m {
                        p.name.value == trait_prop.name && p.is_abstract
                    } else {
                        false
                    }
                });

                if !has_abstract {
                    self.diagnostics.error(
                        trait_name.span.clone(),
                        format!(
                            "class '{}' must implement property '{}' from trait '{}' or declare it abstract",
                            class.name.value, trait_prop.name, trait_name.value
                        ),
                    );
                }
            }
        }

        self.package_registry
            .add_class_trait_impl(class_fqn, trait_fqn, resolved_trait_type_args);
    }

    /// Find a matching method in generic_instance_methods that matches the trait requirement.
    /// Compare method parameters up to renaming, separately from class parameters.
    fn has_matching_generic_method_in_class(
        &self,
        class_sig: &ClassTypeSignature,
        method_name: &SymbolName,
        trait_params: &[(String, Type)],
        trait_return_type: &Type,
        method_type_params: &[TypeParamName],
        is_property: bool,
    ) -> bool {
        if let Some(defs) = class_sig.generic_instance_methods.get(method_name) {
            for def in defs {
                if def.is_property != is_property
                    || def.method_type_params.len() != method_type_params.len()
                {
                    continue;
                }
                let substitution: BTreeMap<_, _> = method_type_params
                    .iter()
                    .cloned()
                    .zip(
                        def.method_type_params
                            .iter()
                            .map(|name| Type::TypeVariable(name.clone(), vec![])),
                    )
                    .collect();
                // Check param count
                if def.params.len() != trait_params.len() {
                    continue;
                }
                // Check param types match structurally (both contain TypeParameters)
                let params_match = def.params.iter().zip(trait_params.iter()).all(
                    |((_, def_ty), (_, trait_ty))| {
                        *def_ty == substitute_trait_type_params(trait_ty, &substitution)
                    },
                );

                if params_match
                    && def.return_type
                        == substitute_trait_type_params(trait_return_type, &substitution)
                {
                    return true;
                }
            }
        }
        self.has_inherited_trait_member(
            class_sig,
            method_name,
            trait_params,
            trait_return_type,
            method_type_params,
            is_property,
        )
    }
}
