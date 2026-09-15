use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName};
use crate::parser::ast::GlobalVarDecl;

use crate::typechecker::types::TypedGlobal;

use super::Inference;

impl Inference<'_> {
    /// Infer types for a single global variable declaration and add it to the typed module.
    pub(super) fn infer_global(&mut self, global: &GlobalVarDecl) {
        let symbol_name = if let Some(ref module_name) = self.container_name {
            format!("{}.{}", module_name, global.name.value)
        } else {
            global.name.value.clone()
        };
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(symbol_name),
        };
        let mangled_name = MangledName::for_global(&fqn);

        // Look up the global's registered type from the registry
        let registered_sig =
            self.registry
                .lookup_global(&fqn, &self.package_path, &self.current_file);
        let expected_ty = registered_sig.map(|sig| sig.ty.clone());

        // Global initializers run in an empty scope (no local variables)
        self.push_scope();
        let previous_expected = self.expected_type.take();
        self.expected_type = expected_ty.clone();
        let typed_init = self.infer_expr(&global.value);
        self.expected_type = previous_expected;
        self.pop_scope();

        // Check assignability if explicitly typed
        if let Some(ref expected) = expected_ty {
            self.check_assignable(typed_init.span.clone(), expected, &typed_init.ty);
        }

        let ty = if let Some(expected) = expected_ty {
            expected
        } else {
            // For untyped globals, verify the inferred type matches the registry
            let inferred_ty = typed_init.ty.clone();
            if let Some(sig) =
                self.registry
                    .lookup_global(&fqn, &self.package_path, &self.current_file)
                && sig.ty != inferred_ty
                && !inferred_ty.is_error()
            {
                self.diagnostics.error(
                        typed_init.span.clone(),
                        format!(
                            "type mismatch: global '{}' registered as '{}' but initializer has type '{}'",
                            global.name.value, sig.ty, inferred_ty
                        ),
                    );
            }
            inferred_ty
        };

        self.typed_globals.insert(
            mangled_name.clone(),
            TypedGlobal {
                visibility: global.visibility,
                name: mangled_name,
                mutable: global.mutable,
                ty,
                initializer: typed_init,
                span: global.span.clone(),
                type_params: vec![],
            },
        );
    }

    /// Infer a global variable on a generic module.
    /// Storage is canonical (one per declaration, shared across instantiations) — the rules
    /// pass forbids the declared type from referencing the module's type parameters.
    pub(super) fn infer_global_template(
        &mut self,
        global: &GlobalVarDecl,
        _module_type_params: &[TypeParamName],
    ) {
        let symbol_name = if let Some(ref module_name) = self.container_name {
            format!("{}.{}", module_name, global.name.value)
        } else {
            global.name.value.clone()
        };
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(symbol_name),
        };
        let mangled_name = MangledName::for_global(&fqn);

        let registered_sig =
            self.registry
                .lookup_global(&fqn, &self.package_path, &self.current_file);
        let expected_ty = registered_sig.map(|sig| sig.ty.clone());

        self.push_scope();
        let prev_expected = self.expected_type.take();
        self.expected_type = expected_ty.clone();
        let typed_init = self.infer_expr(&global.value);
        self.expected_type = prev_expected;
        self.pop_scope();

        if let Some(ref expected) = expected_ty {
            self.check_assignable(typed_init.span.clone(), expected, &typed_init.ty);
        }

        let ty = expected_ty.unwrap_or_else(|| typed_init.ty.clone());

        self.typed_globals.insert(
            mangled_name.clone(),
            TypedGlobal {
                visibility: global.visibility,
                name: mangled_name,
                mutable: global.mutable,
                ty,
                initializer: typed_init,
                span: global.span.clone(),
                type_params: vec![],
            },
        );
    }

    /// Look up a global variable by bare name.
    /// Checks: (1) import scope for symbol imports, (2) same-package.
    /// Also handles generic module globals when inside a module body.
    /// Returns (mangled_name, type, mutable, type_args) if found.
    /// For non-generic globals, type_args is empty.
    pub(super) fn lookup_global(
        &mut self,
        name: &str,
    ) -> Option<(
        MangledName,
        crate::typechecker::types::Type,
        bool,
        Vec<crate::typechecker::types::Type>,
    )> {
        let fqn = self.resolve_fqn(name, super::types::SymbolKind::Global)?;
        // Try concrete globals first
        if let Some(sig) = self
            .registry
            .lookup_global(&fqn, &self.package_path, &self.current_file)
        {
            return Some((
                sig.mangled_name.clone(),
                sig.ty.clone(),
                sig.mutable,
                vec![],
            ));
        }
        // Try generic module global (e.g., fqn.symbol = "Box.count")
        if let Some(dot_pos) = fqn.symbol.0.rfind('.') {
            let module_name = &fqn.symbol.0[..dot_pos];
            let global_name = SymbolName(fqn.symbol.0[dot_pos + 1..].to_string());
            let module_fqn = Fqn {
                package: fqn.package.clone(),
                symbol: SymbolName(module_name.to_string()),
            };
            if let Some(module_info) = self.registry.lookup_module(&module_fqn).cloned() {
                let def = module_info.generic_globals.get(&global_name)?.clone();
                // Build type args from current_type_params
                let type_args: Vec<crate::typechecker::types::Type> = def
                    .type_params
                    .iter()
                    .map(|tp| {
                        self.current_type_params.get(tp).cloned().unwrap_or(
                            crate::typechecker::types::Type::TypeVariable(tp.clone(), vec![]),
                        )
                    })
                    .collect();
                return self.instantiate_and_resolve_generic_global(
                    &module_info,
                    &global_name,
                    &def,
                    type_args,
                );
            }
        }
        None
    }
}
