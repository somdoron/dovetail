//! Normalize collected aliases only after every implementation is registered.
//! This keeps concrete projections out of ordinary signatures and stored fields.

use super::*;
use crate::typechecker::collect::{substitute_trait_bounds, substitute_trait_type_params};

fn normalize(ty: &mut Type) {
    *ty = substitute_trait_type_params(ty, &BTreeMap::new());
}

fn normalize_parameters(parameters: &mut [(String, Type)]) {
    for (_, ty) in parameters {
        normalize(ty);
    }
}

fn normalize_bounds(bounds: &mut TraitBounds) {
    *bounds = substitute_trait_bounds(bounds, &BTreeMap::new());
}

fn normalize_function(fqn: &Fqn, function: &mut FunctionSignature) {
    let previous = function.params.clone();
    normalize_parameters(&mut function.params);
    normalize(&mut function.return_type);
    if previous != function.params {
        let parameters: Vec<_> = function.params.iter().map(|(_, ty)| ty).collect();
        function.mangled_name = MangledName::for_function(fqn, &parameters);
    }
}

fn normalize_members(container: &Fqn, methods: &mut BTreeMap<SymbolName, Vec<FunctionSignature>>) {
    for (name, methods) in methods {
        let fqn = Fqn {
            package: container.package.clone(),
            symbol: SymbolName(format!("{}.{}", container.symbol, name)),
        };
        for method in methods {
            normalize_function(&fqn, method);
        }
    }
}

fn normalize_generic_methods(methods: &mut BTreeMap<SymbolName, Vec<GenericClassMethodDef>>) {
    for method in methods.values_mut().flatten() {
        normalize_parameters(&mut method.params);
        normalize(&mut method.return_type);
        normalize_bounds(&mut method.trait_bounds);
    }
}

fn normalize_class(class: &mut ClassTypeSignature) {
    for field in &mut class.fields {
        normalize(&mut field.ty);
    }
    for parameter in &mut class.constructor_params {
        normalize(&mut parameter.ty);
    }
    if let Some(parent) = &mut class.parent_type_expr {
        normalize(parent);
    }
    normalize_bounds(&mut class.trait_bounds);
    for (_, parameters) in &mut class.trait_impls {
        for ty in parameters {
            normalize(ty);
        }
    }
    normalize_members(&class.fqn, &mut class.instance_methods);
    normalize_members(&class.fqn, &mut class.static_methods);
    normalize_generic_methods(&mut class.generic_instance_methods);
    normalize_generic_methods(&mut class.generic_static_methods);
    for global in class.generic_static_globals.values_mut() {
        if let Some(ty) = &mut global.ty {
            normalize(ty);
        }
    }
    for member in &mut class.body_members {
        if let ClassBodyMemberDef::LetBinding(binding)
        | ClassBodyMemberDef::StaticLetBinding(binding) = member
        {
            if let Some(ty) = &mut binding.resolved_type {
                normalize(ty);
            }
        }
    }
}

fn normalize_trait(signature: &mut TraitSignature) {
    signature.method_dispatch_names = signature
        .methods
        .iter()
        .map(|method| signature.method_dispatch_name(method))
        .collect();
    for parent in &mut signature.supers {
        for ty in &mut parent.type_args {
            normalize(ty);
        }
    }
    for (_, parameters) in &mut signature.super_closure {
        for ty in parameters {
            normalize(ty);
        }
    }
    for method in &mut signature.methods {
        normalize_parameters(&mut method.params);
        normalize(&mut method.return_type);
        normalize_bounds(&mut method.trait_bounds);
        if let Some((_, parameters)) = &mut method.origin {
            for ty in parameters {
                normalize(ty);
            }
        }
    }
    for property in &mut signature.properties {
        normalize_parameters(&mut property.params);
        normalize(&mut property.return_type);
        if let Some((_, parameters)) = &mut property.origin {
            for ty in parameters {
                normalize(ty);
            }
        }
    }
    for associated in &mut signature.associated_types {
        if let Some((_, parameters)) = &mut associated.origin {
            for ty in parameters {
                normalize(ty);
            }
        }
    }
}

fn normalize_module(module: &mut ModuleInfo) {
    normalize_members(&module.fqn, &mut module.functions);
    for global in module.globals.values_mut() {
        normalize(&mut global.ty);
    }
    for global in module.generic_globals.values_mut() {
        normalize(&mut global.ty);
    }
    for member in module.generic_members.0.values_mut().flatten() {
        normalize(&mut member.for_type);
        normalize_parameters(&mut member.params);
        normalize(&mut member.return_type);
        normalize_bounds(&mut member.trait_bounds);
    }
    normalize_bounds(&mut module.trait_bounds);
}

impl Registry {
    /// Compare the evidence consumed by projection normalization, excluding
    /// method bodies and other collection outputs that cannot unlock a type.
    pub(crate) fn same_projection_evidence(&self, other: &Self) -> bool {
        self.types == other.types
            && self
                .implement_blocks
                .iter()
                .map(|block| {
                    (
                        &block.trait_fqn,
                        &block.type_fqn,
                        &block.for_type,
                        &block.type_params,
                        &block.trait_type_args,
                        &block.trait_bounds,
                        &block.associated_type_defs,
                    )
                })
                .eq(other.implement_blocks.iter().map(|block| {
                    (
                        &block.trait_fqn,
                        &block.type_fqn,
                        &block.for_type,
                        &block.type_params,
                        &block.trait_type_args,
                        &block.trait_bounds,
                        &block.associated_type_defs,
                    )
                }))
            && self
                .type_alias_types
                .iter()
                .map(|(name, alias)| (name, &alias.expanded_type, &alias.trait_bounds))
                .eq(other
                    .type_alias_types
                    .iter()
                    .map(|(name, alias)| (name, &alias.expanded_type, &alias.trait_bounds)))
            && self
                .class_types
                .iter()
                .map(|(name, class)| {
                    (
                        name,
                        &class.trait_impls,
                        &class.trait_bounds,
                        &class.parent_class,
                        &class.parent_type_expr,
                    )
                })
                .eq(other.class_types.iter().map(|(name, class)| {
                    (
                        name,
                        &class.trait_impls,
                        &class.trait_bounds,
                        &class.parent_class,
                        &class.parent_type_expr,
                    )
                }))
            && self.traits.len() == other.traits.len()
            && self
                .traits
                .iter()
                .zip(&other.traits)
                .all(|((name, left), (other_name, right))| {
                    name == other_name
                        && left.super_closure == right.super_closure
                        && left
                            .associated_types
                            .iter()
                            .map(|associated| {
                                (
                                    &associated.name,
                                    &associated.type_params,
                                    &associated.origin,
                                )
                            })
                            .eq(right.associated_types.iter().map(|associated| {
                                (
                                    &associated.name,
                                    &associated.type_params,
                                    &associated.origin,
                                )
                            }))
                })
    }

    /// The caller installs a normalization scope using the complete merged
    /// registry. Only this package's declarations are updated here.
    pub(crate) fn normalize_associated_types(&mut self) {
        for ty in self.types.values_mut() {
            normalize(ty);
        }
        for (fqn, signatures) in &mut self.functions {
            for signature in signatures {
                normalize_function(fqn, signature);
            }
        }
        for definition in self.generic_functions.values_mut().flatten() {
            normalize_parameters(&mut definition.params);
            normalize(&mut definition.return_type);
            normalize_bounds(&mut definition.trait_bounds);
        }
        for global in self.globals.values_mut() {
            normalize(&mut global.ty);
        }
        for record in self.record_types.values_mut() {
            normalize_parameters(&mut record.fields);
            normalize_bounds(&mut record.trait_bounds);
        }
        for enumeration in self.enum_types.values_mut() {
            for (_, payload) in &mut enumeration.variants {
                match payload {
                    VariantPayload::Tuple(types) => {
                        for ty in types {
                            normalize(ty);
                        }
                    }
                    VariantPayload::Record(fields) => normalize_parameters(fields),
                    VariantPayload::None => {}
                }
            }
            normalize_bounds(&mut enumeration.trait_bounds);
        }
        for newtype in self.newtype_types.values_mut() {
            normalize(&mut newtype.inner_type);
            normalize_bounds(&mut newtype.trait_bounds);
        }
        for alias in self.type_alias_types.values_mut() {
            normalize(&mut alias.expanded_type);
            normalize_bounds(&mut alias.trait_bounds);
        }
        for class in self.class_types.values_mut() {
            normalize_class(class);
        }
        for signature in self.traits.values_mut() {
            normalize_trait(signature);
        }
        for module in self.modules.values_mut() {
            normalize_module(module);
        }
        self.normalize_implementation_types();
    }

    fn normalize_implementation_types(&mut self) {
        for block in &mut self.implement_blocks {
            normalize(&mut block.for_type);
            for ty in &mut block.trait_type_args {
                normalize(ty);
            }
            normalize_bounds(&mut block.trait_bounds);
            for (_, definition) in block.associated_type_defs.values_mut() {
                normalize(definition);
            }
            for method in block.methods.iter_mut().chain(&mut block.properties) {
                normalize_parameters(&mut method.params);
                normalize(&mut method.return_type);
                normalize_bounds(&mut method.trait_bounds);
            }
        }
        for block in &mut self.extension_blocks {
            normalize(&mut block.for_type);
            normalize_bounds(&mut block.trait_bounds);
            for method in block.methods.iter_mut().chain(&mut block.properties) {
                normalize_parameters(&mut method.params);
                normalize(&mut method.return_type);
                normalize_bounds(&mut method.trait_bounds);
            }
        }
    }
}
