//! Recover declared parameter types after the ordinary resolver selects a call.
use super::{ArgumentBinding, Inference, Parameters};
use crate::common::types::{Fqn, SymbolName, TypeParamName};
use crate::typechecker::infer::generics::apply_substitution;
use crate::typechecker::infer::type_param_substitution::TypeParamSubstitution;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

impl Inference<'_> {
    pub(super) fn named_parameter_types(
        &self,
        call: &TypedExpr,
        binding: &ArgumentBinding,
    ) -> Vec<Type> {
        let Some(arguments) =
            crate::compiler::named_calls::explicit_arguments(call, binding.names.len())
        else {
            return Vec::new();
        };
        for parameters in &binding.signatures {
            let mut substitution = self.named_call_substitution(call, parameters);
            for ((_, expected), argument) in parameters.iter().zip(&arguments) {
                substitution.unify_arg(expected, &argument.ty);
            }
            let types: Vec<_> = parameters
                .iter()
                .map(|(_, ty)| apply_substitution(&substitution, ty))
                .collect();
            if types
                .iter()
                .zip(&arguments)
                .all(|(expected, argument)| self.is_assignable(expected, &argument.ty))
            {
                return types;
            }
        }
        // Error recovery may leave an incompletely resolved argument. Preserve
        // declaration types rather than substituting the argument's narrower type.
        binding.signatures[0]
            .iter()
            .map(|(_, ty)| ty.clone())
            .collect()
    }

    fn named_call_substitution(
        &self,
        call: &TypedExpr,
        parameters: &Parameters,
    ) -> TypeParamSubstitution {
        let mut substitution = TypeParamSubstitution::new();
        let receiver = match &call.kind {
            TypedExprKind::ClassVirtualCall { object, .. } => Some(&object.ty),
            TypedExprKind::InterfaceObjectMethodCall { receiver, .. } => Some(&receiver.ty),
            TypedExprKind::ImplFunctionCall { for_type, .. }
            | TypedExprKind::ExtFunctionCall { for_type, .. } => Some(for_type),
            TypedExprKind::ClassNew { .. } => Some(&call.ty),
            _ => None,
        };
        if let Some(receiver) = receiver {
            substitution = substitution.with_self_type(receiver.clone());
            let application = match receiver {
                Type::GenericClass { fqn, type_args, .. } => self
                    .registry
                    .get_class_type(fqn)
                    .map(|decl| (&decl.type_params, type_args)),
                Type::GenericRecord { fqn, type_args, .. } => self
                    .registry
                    .get_record_type(fqn)
                    .map(|decl| (&decl.type_params, type_args)),
                Type::GenericEnum { fqn, type_args, .. } => self
                    .registry
                    .get_enum_type(fqn)
                    .map(|decl| (&decl.type_params, type_args)),
                Type::GenericNewtype { fqn, type_args, .. } => self
                    .registry
                    .get_newtype_type(fqn)
                    .map(|decl| (&decl.type_params, type_args)),
                _ => None,
            };
            if let Some((names, arguments)) = application {
                for (name, (_, ty)) in names.iter().zip(arguments) {
                    substitution.insert(name.clone(), ty.clone());
                }
            }
        }
        self.named_method_substitution(call, parameters, &mut substitution);
        if let TypedExprKind::FunctionCall {
            name, type_params, ..
        } = &call.kind
            && let Some(fqn) = Fqn::from_dotted(name.0.split(['$', '#']).next().unwrap_or(&name.0))
            && let Some(definitions) = self
                .registry
                .lookup_generic_function(&fqn, &self.package_path)
            && let Some(definition) = definitions
                .iter()
                .find(|definition| parameters_match(&definition.params, parameters, &substitution))
        {
            for (name, ty) in definition.type_params.iter().zip(type_params) {
                substitution.insert(name.clone(), ty.clone());
            }
        }
        substitution
    }
    fn named_method_substitution(
        &self,
        call: &TypedExpr,
        parameters: &Parameters,
        substitution: &mut TypeParamSubstitution,
    ) {
        match &call.kind {
            TypedExprKind::ImplFunctionCall { .. } => {
                self.named_implementation_substitution(call, parameters, substitution)
            }
            TypedExprKind::ExtFunctionCall { .. } => {
                self.named_extension_substitution(call, parameters, substitution)
            }
            TypedExprKind::FunctionCall { .. } => {
                self.named_container_substitution(call, parameters, substitution)
            }
            _ => {}
        }
    }

    fn named_implementation_substitution(
        &self,
        call: &TypedExpr,
        parameters: &Parameters,
        substitution: &mut TypeParamSubstitution,
    ) {
        let TypedExprKind::ImplFunctionCall {
            trait_fqn,
            for_type,
            method_name,
            method_type_params,
            ..
        } = &call.kind
        else {
            return;
        };

        let Some(fqn) = for_type.try_to_fqn() else {
            return;
        };
        for (block, method) in self
            .registry
            .find_impl_blocks_for_type(&fqn)
            .into_iter()
            .flat_map(|block| {
                block
                    .methods
                    .iter()
                    .filter(|method| method.dispatch_name == *method_name)
                    .map(move |method| (block, method))
            })
        {
            if block.trait_fqn != *trait_fqn {
                continue;
            }
            let mut receiver_substitution = TypeParamSubstitution::new();
            if !receiver_substitution.unify(&block.for_type, for_type) {
                continue;
            }
            if parameters_match(&method.params, parameters, &receiver_substitution) {
                insert_types(
                    substitution,
                    block.type_params.iter().chain(&method.method_type_params),
                    method_type_params,
                );
                return;
            }
        }
        // Explicit trait calls expose the contract's names and type variables.
        if let Some(declaration) = self.registry.get_trait(trait_fqn)
            && let Some(method) = declaration
                .methods
                .iter()
                .find(|method| declaration.method_dispatch_name(method) == *method_name)
        {
            let offset = method_type_params
                .len()
                .saturating_sub(method.type_params.len());
            insert_types(
                substitution,
                method.type_params.iter(),
                &method_type_params[offset..],
            );
        }
    }

    fn named_extension_substitution(
        &self,
        call: &TypedExpr,
        parameters: &Parameters,
        substitution: &mut TypeParamSubstitution,
    ) {
        let TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type,
            method_name,
            type_params,
            ..
        } = &call.kind
        else {
            return;
        };

        for block in self.registry.lookup_extension_blocks_by_fqn(ext_fqn) {
            let mut receiver_substitution = TypeParamSubstitution::new();
            if !receiver_substitution.unify(&block.for_type, for_type) {
                continue;
            }
            for method in &block.methods {
                if method.name == *method_name
                    && parameters_match(&method.params, parameters, &receiver_substitution)
                {
                    insert_types(
                        substitution,
                        block.type_params.iter().chain(&method.method_type_params),
                        type_params,
                    );
                    return;
                }
            }
        }
    }

    fn named_container_substitution(
        &self,
        call: &TypedExpr,
        parameters: &Parameters,
        substitution: &mut TypeParamSubstitution,
    ) {
        let TypedExprKind::FunctionCall {
            name, type_params, ..
        } = &call.kind
        else {
            return;
        };

        let path = name.0.split(['$', '#']).next().unwrap_or(&name.0);
        let Some((container, member)) = path.rsplit_once('.') else {
            return;
        };
        let Some(container) = Fqn::from_dotted(container) else {
            return;
        };
        let member = SymbolName(member.into());
        if let Some(module) = self.registry.lookup_module(&container) {
            for method in module.generic_members.lookup(&member).into_iter().flatten() {
                if parameters_match(&method.params, parameters, substitution) {
                    insert_types(
                        substitution,
                        method.type_params.iter().chain(&method.method_type_params),
                        type_params,
                    );
                    return;
                }
            }
        }
        if let Some(class) = self.registry.get_class_type(&container) {
            for method in class
                .generic_instance_methods
                .get(&member)
                .into_iter()
                .flatten()
                .chain(
                    class
                        .generic_static_methods
                        .get(&member)
                        .into_iter()
                        .flatten(),
                )
            {
                if parameters_match(&method.params, parameters, substitution) {
                    insert_types(
                        substitution,
                        method
                            .class_type_params
                            .iter()
                            .chain(&method.method_type_params),
                        type_params,
                    );
                    return;
                }
            }
        }
    }
}

fn parameters_match(
    declaration: &Parameters,
    parameters: &Parameters,
    substitution: &TypeParamSubstitution,
) -> bool {
    let offset = declaration.len().saturating_sub(parameters.len());
    declaration[offset..]
        .iter()
        .map(|(name, ty)| (name.clone(), apply_substitution(substitution, ty)))
        .eq(parameters.iter().cloned())
}

fn insert_types<'a>(
    substitution: &mut TypeParamSubstitution,
    names: impl Iterator<Item = &'a TypeParamName>,
    types: &[Type],
) {
    for (name, ty) in names.zip(types) {
        substitution.insert(name.clone(), ty.clone());
    }
}
