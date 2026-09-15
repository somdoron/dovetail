use std::collections::BTreeMap;

use crate::common::span::{Span, Spanned};
use crate::common::types::{Fqn, MangledName};
use crate::parser::ast::{FieldInit, TypeExpr};
use crate::typechecker::registry::RecordTypeSignature;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

use super::Inference;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;

impl Inference<'_> {
    /// Try to resolve a field access on a record type.
    /// Returns `Some(TypedExpr)` if the field was found on the record, `None` to fall through.
    /// Returns `Some(error)` for unknown record types.
    pub(super) fn try_resolve_record_field(
        &mut self,
        typed_object: &TypedExpr,
        field: &Spanned<String>,
        span: &Span,
    ) -> Option<TypedExpr> {
        match &typed_object.ty {
            Type::Record(fqn, _mn) => {
                let info_opt = self
                    .registry
                    .lookup_record_type(fqn, &self.package_path, &self.current_file)
                    .cloned();
                if let Some(info) = info_opt.as_ref() {
                    if let Some((idx, (_, def_field_ty))) = info
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(_, (name, _))| *name == field.value)
                    {
                        return Some(TypedExpr {
                            kind: TypedExprKind::FieldAccess {
                                object: Box::new(typed_object.clone()),
                                field_name: field.value.clone(),
                                field_index: idx as u32,
                                boxed: false,
                            },
                            ty: def_field_ty.clone(),
                            span: span.clone(),
                        });
                    }
                    // Field not found on record — fall through
                    None
                } else {
                    self.diagnostics.error(
                        span.clone(),
                        format!("unknown record type: '{}'", fqn.symbol),
                    );
                    Some(TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    })
                }
            }
            Type::GenericRecord { fqn, type_args, .. } => {
                let def_opt = self
                    .registry
                    .lookup_record_type(fqn, &self.package_path, &self.current_file)
                    .cloned();
                if let Some(def) = def_opt.as_ref() {
                    if let Some((idx, (_, def_field_ty))) = def
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(_, (name, _))| *name == field.value)
                    {
                        let just_types: Vec<Type> =
                            type_args.iter().map(|(_, t)| t.clone()).collect();
                        let substitution =
                            TypeParamSubstitution::from_pairs(&def.type_params, &just_types);
                        let result_ty = apply_substitution(&substitution, def_field_ty);
                        return Some(TypedExpr {
                            kind: TypedExprKind::FieldAccess {
                                object: Box::new(typed_object.clone()),
                                field_name: field.value.clone(),
                                field_index: idx as u32,
                                boxed: false,
                            },
                            ty: result_ty,
                            span: span.clone(),
                        });
                    }
                    // Field not found on generic record — fall through
                    None
                } else {
                    self.diagnostics.error(
                        span.clone(),
                        format!("unknown record type: '{}'", fqn.symbol),
                    );
                    Some(TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    })
                }
            }
            Type::Tuple(types, _) => {
                // Tuple field access: _0, _1, _2, ...
                if let Some(idx_str) = field.value.strip_prefix('_')
                    && let Ok(idx) = idx_str.parse::<usize>()
                    && idx < types.len()
                {
                    return Some(TypedExpr {
                        kind: TypedExprKind::FieldAccess {
                            object: Box::new(typed_object.clone()),
                            field_name: field.value.clone(),
                            field_index: idx as u32,
                            boxed: false,
                        },
                        ty: types[idx].clone(),
                        span: span.clone(),
                    });
                }
                // Field not found on tuple — fall through
                None
            }
            Type::Newtype(fqn, inner) => {
                if field.value == "value" {
                    let resolved_inner = if let Some(sig) = self.registry.lookup_newtype_type(
                        fqn,
                        &self.package_path,
                        &self.current_file,
                    ) {
                        let sig = sig.clone();
                        if !self.check_newtype_inner_access(&sig, span, "access .value on") {
                            return Some(TypedExpr {
                                kind: TypedExprKind::UnitLiteral,
                                ty: Type::Error,
                                span: span.clone(),
                            });
                        }
                        sig.inner_type
                    } else {
                        *inner.clone()
                    };
                    Some(TypedExpr {
                        kind: TypedExprKind::NewtypeValue {
                            value: Box::new(typed_object.clone()),
                        },
                        ty: resolved_inner,
                        span: span.clone(),
                    })
                } else {
                    // Not `.value` — fall through to extension method resolution
                    None
                }
            }
            Type::GenericNewtype {
                fqn,
                concrete_inner_type,
                ..
            } => {
                if field.value == "value" {
                    if let Some(sig) = self.registry.lookup_newtype_type(
                        fqn,
                        &self.package_path,
                        &self.current_file,
                    ) {
                        let sig = sig.clone();
                        if !self.check_newtype_inner_access(&sig, span, "access .value on") {
                            return Some(TypedExpr {
                                kind: TypedExprKind::UnitLiteral,
                                ty: Type::Error,
                                span: span.clone(),
                            });
                        }
                    }
                    Some(TypedExpr {
                        kind: TypedExprKind::NewtypeValue {
                            value: Box::new(typed_object.clone()),
                        },
                        ty: *concrete_inner_type.clone(),
                        span: span.clone(),
                    })
                } else {
                    // Not `.value` — fall through to extension method resolution
                    None
                }
            }
            _ => None,
        }
    }

    /// Infer a `with` expression that creates a new record with some fields overridden.
    pub(super) fn infer_record_with(
        &mut self,
        object: &crate::parser::ast::Expr,
        fields: &[FieldInit],
        span: &Span,
    ) -> TypedExpr {
        let typed_object = self.infer_expr(object);

        if typed_object.ty.is_error() {
            for f in fields {
                self.infer_expr(&f.value);
            }
            return TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            };
        }

        match &typed_object.ty {
            Type::Record(fqn, mn) => {
                let fqn = fqn.clone();
                let mn = mn.clone();
                // Non-generic: look up in registry
                let info_opt = self
                    .registry
                    .lookup_record_type(&fqn, &self.package_path, &self.current_file)
                    .cloned();
                let info = match info_opt {
                    Some(info) => info,
                    None => {
                        self.diagnostics.error(
                            span.clone(),
                            format!("unknown record type: '{}'", fqn.symbol),
                        );
                        for f in fields {
                            self.infer_expr(&f.value);
                        }
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                };

                self.check_private_type_access(
                    &info.fqn,
                    info.construction_private,
                    "record",
                    span,
                    "update with 'with' on",
                );

                let mut overrides = Vec::new();
                let mut seen = std::collections::BTreeSet::new();

                for field_init in fields {
                    let field_name = &field_init.name.value;

                    if !seen.insert(field_name.clone()) {
                        self.diagnostics.error(
                            field_init.name.span.clone(),
                            format!("duplicate field '{}' in with expression", field_name),
                        );
                    }

                    // Set expected_type to the field's declared type
                    let prev_expected = self.expected_type.take();
                    if let Some((_, def_field_ty)) =
                        info.fields.iter().find(|(name, _)| name == field_name)
                    {
                        self.expected_type = Some(def_field_ty.clone());
                    }
                    let typed_value = self.infer_expr(&field_init.value);
                    self.expected_type = prev_expected;

                    if let Some((idx, (_, def_field_ty))) = info
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(_, (name, _))| name == field_name)
                    {
                        self.check_assignable(
                            typed_value.span.clone(),
                            def_field_ty,
                            &typed_value.ty,
                        );
                        overrides.push((field_name.clone(), idx as u32, typed_value));
                    } else {
                        self.diagnostics.error(
                            field_init.name.span.clone(),
                            format!("no field '{}' on record '{}'", field_name, fqn.symbol),
                        );
                    }
                }

                TypedExpr {
                    kind: TypedExprKind::RecordWith {
                        object: Box::new(typed_object),
                        fqn: fqn.clone(),
                        overrides,
                        type_params: vec![],
                    },
                    ty: Type::Record(fqn, mn),
                    span: span.clone(),
                }
            }
            Type::GenericRecord {
                fqn,
                mangled_name: mn,
                type_args,
            } => {
                let fqn = fqn.clone();
                let mn = mn.clone();
                let type_args = type_args.clone();
                // Generic: look up definition from registry
                let def = match self
                    .registry
                    .lookup_record_type(&fqn, &self.package_path, &self.current_file)
                    .cloned()
                {
                    Some(def) => def,
                    None => {
                        self.diagnostics.error(
                            span.clone(),
                            format!("unknown record type: '{}'", fqn.symbol),
                        );
                        for f in fields {
                            self.infer_expr(&f.value);
                        }
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        };
                    }
                };

                self.check_private_type_access(
                    &def.fqn,
                    def.construction_private,
                    "record",
                    span,
                    "update with 'with' on",
                );

                let just_types: Vec<Type> = type_args.iter().map(|(_, t)| t.clone()).collect();
                let substitution = TypeParamSubstitution::from_pairs(&def.type_params, &just_types);

                let mut overrides = Vec::new();
                let mut seen = std::collections::BTreeSet::new();

                for field_init in fields {
                    let field_name = &field_init.name.value;

                    if !seen.insert(field_name.clone()) {
                        self.diagnostics.error(
                            field_init.name.span.clone(),
                            format!("duplicate field '{}' in with expression", field_name),
                        );
                    }

                    // Set expected_type to the substituted field type
                    let prev_expected = self.expected_type.take();
                    if let Some((_, def_field_ty)) =
                        def.fields.iter().find(|(name, _)| name == field_name)
                    {
                        let field_expected = apply_substitution(&substitution, def_field_ty);
                        self.expected_type = Some(field_expected);
                    }
                    let typed_value = self.infer_expr(&field_init.value);
                    self.expected_type = prev_expected;

                    if let Some((idx, (_, def_field_ty))) = def
                        .fields
                        .iter()
                        .enumerate()
                        .find(|(_, (name, _))| name == field_name)
                    {
                        let expected_ty = apply_substitution(&substitution, def_field_ty);
                        self.check_assignable(
                            typed_value.span.clone(),
                            &expected_ty,
                            &typed_value.ty,
                        );
                        overrides.push((field_name.clone(), idx as u32, typed_value));
                    } else {
                        self.diagnostics.error(
                            field_init.name.span.clone(),
                            format!("no field '{}' on record '{}'", field_name, fqn.symbol),
                        );
                    }
                }

                let obj_ty = Type::GenericRecord {
                    fqn: fqn.clone(),
                    mangled_name: mn.clone(),
                    type_args: type_args.clone(),
                };
                TypedExpr {
                    kind: TypedExprKind::RecordWith {
                        object: Box::new(typed_object),
                        fqn: fqn.clone(),
                        overrides,
                        type_params: type_args.iter().map(|(_, t)| t.clone()).collect(),
                    },
                    ty: obj_ty,
                    span: span.clone(),
                }
            }
            _ => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "'with' expression requires a record type, found '{}'",
                        typed_object.ty
                    ),
                );
                for f in fields {
                    self.infer_expr(&f.value);
                }
                TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                }
            }
        }
    }

    /// Infer a record construction expression.
    /// Handles both non-generic (`Point { x = 1 }`) and generic (`Box<Int32> { value = 42 }`)
    /// records, including type argument inference for generic records (`Box { value = 42 }`).
    pub(super) fn infer_record_create(
        &mut self,
        type_name: &Spanned<String>,
        type_args: &[TypeExpr],
        fields: &[FieldInit],
        span: &Span,
    ) -> TypedExpr {
        // Preserve ordinary record-name precedence over prelude variant shortcuts.
        if type_args.is_empty() && self.resolve_record_type(&type_name.value).is_none() {
            let enum_name = match type_name.value.as_str() {
                "Some" => Some("Option"),
                "Ok" | "Error" => Some("Result"),
                _ => None,
            };
            if let Some(enum_name) = enum_name {
                return self.infer_enum_variant_record_create(
                    &Spanned::new(enum_name.to_string(), type_name.span.clone()),
                    type_name,
                    fields,
                    span,
                );
            }
        }

        // Path A: Explicit type args — must be a generic record
        if !type_args.is_empty() {
            return self
                .resolve_generic_record_with_type_params(type_name, type_args, fields, span);
        }

        // Resolve the record name (generic or non-generic)
        let fqn = match self.resolve_fqn(&type_name.value, super::types::SymbolKind::Record) {
            Some(fqn) => fqn,
            None => {
                let msg =
                    if let Some(fqn) = self.registry.suggest_import_for_record(&type_name.value) {
                        format!(
                            "unknown record type: '{}'; try adding 'import {}'",
                            type_name.value, fqn
                        )
                    } else {
                        format!("unknown record type: '{}'", type_name.value)
                    };
                self.diagnostics.error(type_name.span.clone(), msg);
                for f in fields {
                    self.infer_expr(&f.value);
                }
                return self.error_expr(span);
            }
        };

        let def = self
            .registry
            .lookup_record_type(&fqn, &self.package_path, &self.current_file)
            .cloned()
            .unwrap();

        self.check_private_type_access(
            &def.fqn,
            def.construction_private,
            "record",
            span,
            "construct",
        );

        // Check if it's a generic record — infer type args from fields
        if !def.type_params.is_empty() {
            return self.resolve_generic_record_create(fqn, &def, type_name, fields, span);
        }

        // Non-generic record — direct field check
        let typed_fields = self.check_record_fields(&def.fields, type_name, fields, None, span);
        let mangled = MangledName::for_type(&fqn);
        TypedExpr {
            kind: TypedExprKind::RecordCreate {
                fqn: fqn.clone(),
                fields: typed_fields,
                type_params: vec![],
            },
            ty: Type::Record(fqn, mangled),
            span: span.clone(),
        }
    }

    /// Resolve a generic record with explicit type arguments (e.g. `Box<Int32> { value = 42 }`).
    fn resolve_generic_record_with_type_params(
        &mut self,
        type_name: &Spanned<String>,
        type_params: &[TypeExpr],
        fields: &[FieldInit],
        span: &Span,
    ) -> TypedExpr {
        let fqn = match self.resolve_fqn(&type_name.value, super::types::SymbolKind::Record) {
            Some(fqn) => fqn,
            None => {
                self.diagnostics.error(
                    type_name.span.clone(),
                    format!("unknown generic record type: '{}'", type_name.value),
                );
                for f in fields {
                    self.infer_expr(&f.value);
                }
                return self.error_expr(span);
            }
        };

        let def = self
            .registry
            .lookup_record_type(&fqn, &self.package_path, &self.current_file)
            .unwrap()
            .clone();

        self.check_private_type_access(
            &def.fqn,
            def.construction_private,
            "record",
            span,
            "construct",
        );

        if def.type_params.is_empty() {
            self.diagnostics.error(
                type_name.span.clone(),
                format!(
                    "'{}' is not a generic record and does not take type arguments",
                    type_name.value
                ),
            );
            for f in fields {
                self.infer_expr(&f.value);
            }
            return self.error_expr(span);
        }

        if type_params.len() != def.type_params.len() {
            self.diagnostics.error(
                type_name.span.clone(),
                format!(
                    "expected {} type argument(s) for '{}', found {}",
                    def.type_params.len(),
                    type_name.value,
                    type_params.len()
                ),
            );
            for f in fields {
                self.infer_expr(&f.value);
            }
            return self.error_expr(span);
        }

        let resolved_type_params = match self.resolve_type_args(type_params) {
            Some(args) => args,
            None => {
                for f in fields {
                    self.infer_expr(&f.value);
                }
                return self.error_expr(span);
            }
        };

        self.build_generic_record(
            fqn,
            &def,
            resolved_type_params,
            type_name,
            fields,
            None,
            span,
        )
    }

    /// Infer type arguments for a generic record from field value types,
    /// then instantiate and validate fields.
    fn resolve_generic_record_create(
        &mut self,
        fqn: Fqn,
        def: &RecordTypeSignature,
        type_name: &Spanned<String>,
        fields: &[FieldInit],
        span: &Span,
    ) -> TypedExpr {
        // Infer field types
        let mut typed_field_map: BTreeMap<String, TypedExpr> = BTreeMap::new();
        for field_init in fields {
            let typed_value = self.infer_expr(&field_init.value);
            typed_field_map.insert(field_init.name.value.clone(), typed_value);
        }

        // Unify field types with definition to resolve type parameters
        let mut substitution = TypeParamSubstitution::new();
        for (def_field_name, def_field_ty) in &def.fields {
            if let Some(typed_value) = typed_field_map.get(def_field_name) {
                if typed_value.ty.is_error() {
                    continue;
                }
                if !substitution.unify(def_field_ty, &typed_value.ty) {
                    self.diagnostics.error(
                        type_name.span.clone(),
                        format!(
                            "cannot infer type arguments for generic record '{}'; provide explicit type arguments",
                            type_name.value
                        ),
                    );
                    return self.error_expr(span);
                }
            }
        }

        // Collect resolved type params in declaration order
        let resolved_type_params = substitution.resolve_type_params(&def.type_params);

        let resolved_type_params = match resolved_type_params {
            Some(args) => args,
            None => {
                self.diagnostics.error(
                    type_name.span.clone(),
                    format!(
                        "cannot infer type arguments for generic record '{}'; provide explicit type arguments",
                        type_name.value
                    ),
                );
                return self.error_expr(span);
            }
        };

        self.build_generic_record(
            fqn,
            def,
            resolved_type_params,
            type_name,
            fields,
            Some(typed_field_map),
            span,
        )
    }

    /// After resolving type args (explicit or inferred), instantiate the generic record
    /// and validate fields.
    #[allow(clippy::too_many_arguments)]
    fn build_generic_record(
        &mut self,
        fqn: Fqn,
        def: &RecordTypeSignature,
        resolved_type_params: Vec<Type>,
        type_name: &Spanned<String>,
        fields: &[FieldInit],
        pre_inferred: Option<BTreeMap<String, TypedExpr>>,
        span: &Span,
    ) -> TypedExpr {
        let ty = self.resolve_generic_record(&fqn, def, &resolved_type_params, span);

        // Build concrete fields from the definition
        let substitution =
            TypeParamSubstitution::from_pairs(&def.type_params, &resolved_type_params);
        let concrete_fields: Vec<(String, Type)> = def
            .fields
            .iter()
            .map(|(n, ty)| (n.clone(), apply_substitution(&substitution, ty)))
            .collect();

        let typed_fields =
            self.check_record_fields(&concrete_fields, type_name, fields, pre_inferred, span);

        let kind = TypedExprKind::RecordCreate {
            fqn: fqn.clone(),
            fields: typed_fields,
            type_params: resolved_type_params.clone(),
        };

        TypedExpr {
            kind,
            ty,
            span: span.clone(),
        }
    }

    /// Validate record fields against the definition and return the typed fields
    /// in declaration order. If `pre_inferred` is Some, uses those typed expressions
    /// instead of re-inferring.
    pub(super) fn check_record_fields(
        &mut self,
        def_fields: &[(String, Type)],
        type_name: &Spanned<String>,
        fields: &[FieldInit],
        pre_inferred: Option<BTreeMap<String, TypedExpr>>,
        span: &Span,
    ) -> Vec<(String, TypedExpr)> {
        let expected_fields: std::collections::BTreeSet<&str> =
            def_fields.iter().map(|(name, _)| name.as_str()).collect();
        let mut provided_fields = std::collections::BTreeSet::new();

        let mut typed_field_map: BTreeMap<String, TypedExpr> = BTreeMap::new();

        for field_init in fields {
            let field_name = &field_init.name.value;

            if !provided_fields.insert(field_name.as_str()) {
                self.diagnostics.error(
                    field_init.name.span.clone(),
                    format!("duplicate field '{}' in record construction", field_name),
                );
            }

            let typed_value = if let Some(ref map) = pre_inferred {
                map.get(field_name)
                    .cloned()
                    .unwrap_or_else(|| self.infer_expr(&field_init.value))
            } else {
                // Set expected_type to the field's declared type before inferring
                let prev_expected = self.expected_type.take();
                if let Some((_, expected_ty)) =
                    def_fields.iter().find(|(name, _)| name == field_name)
                {
                    self.expected_type = Some(expected_ty.clone());
                }
                let typed = self.infer_expr(&field_init.value);
                self.expected_type = prev_expected;
                typed
            };

            if let Some((_, expected_ty)) = def_fields.iter().find(|(name, _)| name == field_name) {
                self.check_assignable(typed_value.span.clone(), expected_ty, &typed_value.ty);
            } else {
                self.diagnostics.error(
                    field_init.name.span.clone(),
                    format!(
                        "unknown field '{}' in record '{}'",
                        field_name, type_name.value
                    ),
                );
            }

            typed_field_map.insert(field_name.clone(), typed_value);
        }

        for expected in &expected_fields {
            if !provided_fields.contains(expected) {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "missing field '{}' in record '{}'",
                        expected, type_name.value
                    ),
                );
            }
        }

        def_fields
            .iter()
            .filter_map(|(name, _)| {
                typed_field_map
                    .remove(name)
                    .map(|expr| (name.clone(), expr))
            })
            .collect()
    }
}
