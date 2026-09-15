//! User-authored function bodies include trait defaults, which stay outside
//! `TypedModule::functions` until the compiler materializes implementations.

use crate::common::types::MangledName;
use crate::typechecker::types::{TypedFunction, TypedModule};

pub(super) fn iter(module: &TypedModule) -> impl Iterator<Item = (&MangledName, &TypedFunction)> {
    module
        .functions
        .iter()
        .chain(module.default_templates.iter())
}

pub(super) fn values(module: &TypedModule) -> impl Iterator<Item = &TypedFunction> {
    iter(module).map(|(_, function)| function)
}

pub(super) fn get<'a>(module: &'a TypedModule, name: &MangledName) -> Option<&'a TypedFunction> {
    module
        .functions
        .get(name)
        .or_else(|| module.default_templates.get(name))
}
