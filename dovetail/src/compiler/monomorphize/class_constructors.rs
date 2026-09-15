use std::collections::{BTreeMap, BTreeSet};

use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName, VarName, Visibility};
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{
    ClassTypeDef, Type, TypeDef, TypedExpr, TypedExprKind, TypedFunction, TypedModule, TypedPattern,
};

use super::substitute::{apply_type_substitution, substitute_types_in_expr};

/// Executable constructor bodies need specialization even though class layouts are erased.
/// Move constructors with generic ancestry into ordinary functions so their operators,
/// closures, and nested generic calls follow the existing monomorphization pipeline.
pub(super) fn lower_generic_constructors(module: &mut TypedModule, registry: &Registry) {
    let classes: BTreeMap<_, _> = module
        .types
        .iter()
        .filter_map(|(name, def)| match def {
            TypeDef::Class(class) => Some((name.clone(), class.clone())),
            _ => None,
        })
        .collect();
    let selected = classes_with_generic_ancestry(&classes);
    let constructors: BTreeMap<_, _> = selected
        .iter()
        .filter(|name| !classes[*name].is_abstract)
        .map(|name| {
            let function = constructor_function(&classes[name], &classes, registry);
            (name.clone(), function)
        })
        .collect();
    let names: BTreeMap<_, _> = constructors
        .iter()
        .map(|(class, function)| (class.clone(), function.name.clone()))
        .collect();
    module
        .functions
        .extend(constructors.into_values().map(|f| (f.name.clone(), f)));
    rewrite_module_constructions(module, &names);
}

fn classes_with_generic_ancestry(
    classes: &BTreeMap<MangledName, ClassTypeDef>,
) -> BTreeSet<MangledName> {
    let mut selected: BTreeSet<_> = classes
        .iter()
        .filter(|(_, class)| !class.type_params.is_empty())
        .map(|(name, _)| name.clone())
        .collect();
    loop {
        let previous_len = selected.len();
        for (name, class) in classes {
            if class
                .parent_mangled_name
                .as_ref()
                .is_some_and(|parent| selected.contains(parent))
            {
                selected.insert(name.clone());
            }
        }
        if selected.len() == previous_len {
            return selected;
        }
    }
}

fn rewrite_module_constructions(
    module: &mut TypedModule,
    names: &BTreeMap<MangledName, MangledName>,
) {
    for function in module.functions.values_mut() {
        rewrite_constructions(&mut function.body, names);
    }
    for global in module.globals.values_mut() {
        rewrite_constructions(&mut global.initializer, names);
    }
    for test in &mut module.tests {
        rewrite_constructions(&mut test.body, names);
    }
    for block in &mut module.implement_blocks {
        for method in block.methods.iter_mut().chain(&mut block.properties) {
            rewrite_constructions(&mut method.body, names);
        }
    }
    for block in &mut module.extension_blocks {
        for method in block.methods.iter_mut().chain(&mut block.properties) {
            rewrite_constructions(&mut method.body, names);
        }
    }
    for def in module.types.values_mut() {
        if let TypeDef::Class(class) = def {
            for expr in class
                .initializer
                .iter_mut()
                .chain(class.extends_args.iter_mut().flatten())
            {
                rewrite_constructions(expr, names);
            }
        }
    }
}

fn constructor_function(
    class: &ClassTypeDef,
    classes: &BTreeMap<MangledName, ClassTypeDef>,
    registry: &Registry,
) -> TypedFunction {
    let fqn = Fqn {
        package: class.fqn.package.clone(),
        symbol: SymbolName(format!("{}.$constructor", class.fqn.symbol)),
    };
    let name = MangledName::for_function(
        &fqn,
        &class
            .constructor_params
            .iter()
            .map(|p| &p.ty)
            .collect::<Vec<_>>(),
    );
    let return_type = constructor_return_type(class, registry);
    let (mut statements, fields) = initializer_body(class, &BTreeMap::new(), classes, registry, 0);
    statements.push(TypedExpr {
        kind: TypedExprKind::ClassStructCreate {
            target_mangled_name: class.mangled_name.clone(),
            type_params: class
                .type_params
                .iter()
                .map(|p| Type::TypeVariable(p.clone(), vec![]))
                .collect(),
            fields,
        },
        ty: return_type.clone(),
        span: class.span.clone(),
    });
    TypedFunction {
        visibility: Visibility::Private,
        name,
        type_params: class.type_params.clone(),
        params: class.constructor_params.clone(),
        return_type: return_type.clone(),
        body: TypedExpr {
            kind: TypedExprKind::Block(statements),
            ty: return_type,
            span: class.span.clone(),
        },
        span: class.span.clone(),
        vtable_self_type: None,
        is_async: false,
        display_name: format!("{} constructor", class.fqn),
        source_name: fqn.symbol.0,
    }
}

fn constructor_return_type(class: &ClassTypeDef, registry: &Registry) -> Type {
    if class.type_params.is_empty() {
        return Type::Class(class.fqn.clone(), class.mangled_name.clone());
    }
    let sig = registry
        .get_class_type(&class.fqn)
        .expect("class signature");
    Type::GenericClass {
        fqn: class.fqn.clone(),
        mangled_name: class.mangled_name.clone(),
        type_args: sig
            .type_param_variances
            .iter()
            .zip(&class.type_params)
            .map(|(variance, name)| (*variance, Type::TypeVariable(name.clone(), vec![])))
            .collect(),
    }
}

fn initializer_body(
    class: &ClassTypeDef,
    substitution: &BTreeMap<TypeParamName, Type>,
    classes: &BTreeMap<MangledName, ClassTypeDef>,
    registry: &Registry,
    depth: usize,
) -> (Vec<TypedExpr>, Vec<TypedExpr>) {
    let mut statements = Vec::new();
    let mut fields = Vec::new();
    if let (Some(parent_name), Some(args)) = (&class.parent_mangled_name, &class.extends_args) {
        let parent = &classes[parent_name];
        let parent_type = class.parent_type.clone().or_else(|| {
            registry
                .get_class_type(&class.fqn)
                .and_then(|sig| sig.parent_type_expr.clone())
        });
        let parent_substitution = match parent_type {
            Some(Type::GenericClass { type_args, .. }) => parent
                .type_params
                .iter()
                .cloned()
                .zip(
                    type_args
                        .iter()
                        .map(|(_, ty)| apply_type_substitution(ty, substitution)),
                )
                .collect(),
            _ => BTreeMap::new(),
        };

        // Evaluate every extends argument in the child scope before introducing parent names.
        let mut parent_statements = Vec::new();
        for (index, (param, arg)) in parent.constructor_params.iter().zip(args).enumerate() {
            let temp = format!("$constructor$argument${depth}${index}");
            let value = substitute_types_in_expr(arg.clone(), substitution);
            let ty = apply_type_substitution(&param.ty, &parent_substitution);
            statements.push(binding(&temp, ty.clone(), value, &class.span));
            parent_statements.push(binding(
                &param.name,
                ty.clone(),
                reference(&temp, ty, &class.span),
                &class.span,
            ));
        }
        let (inherited_statements, inherited_fields) =
            initializer_body(parent, &parent_substitution, classes, registry, depth + 1);
        parent_statements.extend(inherited_statements);
        let (parent_binding, parent_fields) =
            save_parent_fields(parent_statements, inherited_fields, depth, &class.span);
        statements.push(parent_binding);
        fields.extend(parent_fields);
    }
    statements.extend(
        class
            .initializer
            .iter()
            .cloned()
            .map(|expr| substitute_types_in_expr(expr, substitution)),
    );
    fields.extend(
        class.initializer_fields.iter().map(|(name, ty)| {
            reference(name, apply_type_substitution(ty, substitution), &class.span)
        }),
    );
    (statements, fields)
}

/// Preserve parent locals outside their block without exposing their names to the child.
fn save_parent_fields(
    mut statements: Vec<TypedExpr>,
    fields: Vec<TypedExpr>,
    depth: usize,
    span: &Span,
) -> (TypedExpr, Vec<TypedExpr>) {
    let tuple_types: Vec<_> = fields.iter().map(|field| field.ty.clone()).collect();
    let tuple_type = Type::Tuple(tuple_types.clone(), MangledName::for_tuple(&tuple_types));
    statements.push(TypedExpr {
        kind: TypedExprKind::TupleLiteral { elements: fields },
        ty: tuple_type.clone(),
        span: span.clone(),
    });
    let names: Vec<_> = tuple_types
        .iter()
        .enumerate()
        .map(|(index, _)| format!("$constructor$field${depth}${index}"))
        .collect();
    let binding = TypedExpr {
        kind: TypedExprKind::LetDestructure {
            pattern: TypedPattern::Tuple {
                element_patterns: names
                    .iter()
                    .zip(&tuple_types)
                    .map(|(name, ty)| TypedPattern::Variable(VarName(name.clone()), ty.clone()))
                    .collect(),
                tuple_type: tuple_type.clone(),
            },
            var_ty: tuple_type.clone(),
            value: Box::new(TypedExpr {
                kind: TypedExprKind::Block(statements),
                ty: tuple_type,
                span: span.clone(),
            }),
        },
        ty: Type::Unit,
        span: span.clone(),
    };
    let fields = names
        .iter()
        .zip(tuple_types)
        .map(|(name, ty)| reference(name, ty, span))
        .collect();
    (binding, fields)
}

fn reference(name: &str, ty: Type, span: &Span) -> TypedExpr {
    TypedExpr {
        kind: TypedExprKind::VarRef {
            name: VarName(name.to_string()),
            boxed: false,
        },
        ty,
        span: span.clone(),
    }
}

fn binding(name: &str, ty: Type, value: TypedExpr, span: &Span) -> TypedExpr {
    TypedExpr {
        kind: TypedExprKind::Let {
            name: VarName(name.to_string()),
            mutable: false,
            boxed: false,
            var_ty: ty,
            value: Box::new(value),
        },
        ty: Type::Unit,
        span: span.clone(),
    }
}

fn rewrite_constructions(expr: &mut TypedExpr, names: &BTreeMap<MangledName, MangledName>) {
    super::visit_expr_children_mut(expr, |child| rewrite_constructions(child, names));
    if let TypedExprKind::ClassNew {
        mangled_name,
        args,
        type_params,
    } = &mut expr.kind
        && let Some(name) = names.get(mangled_name)
    {
        expr.kind = TypedExprKind::FunctionCall {
            name: name.clone(),
            args: std::mem::take(args),
            type_params: std::mem::take(type_params),
        };
    }
}
