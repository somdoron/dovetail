use std::collections::BTreeMap;

use crate::common::types::{Fqn, MangledName, PackagePath, SymbolName};
use crate::parser::ast::{Declaration, SourceFile};

use crate::typechecker::registry::{Registry, VariantPayload};
use crate::typechecker::types::{EnumTypeDef, EnumVariantDef, TypeDef};

/// Collect enum type definitions from AST declarations.
/// Non-generic enums: concrete TypeDef with empty `type_params`.
/// Generic enums: template TypeDef (keyed by base mangled name, with TypeParameter payloads).
/// Monomorphize creates concrete TypeDefs from templates for each instantiation.
pub fn collect_enum_type_defs(
    package_path: &PackagePath,
    files: &[&SourceFile],
    registry: &Registry,
) -> BTreeMap<MangledName, TypeDef> {
    let mut types = BTreeMap::new();
    for file in files {
        for decl in &file.declarations {
            if let Declaration::Enum(e) = decl {
                let fqn = Fqn {
                    package: package_path.clone(),
                    symbol: SymbolName(e.name.value.clone()),
                };
                if let Some(info) = registry.lookup_enum_type(&fqn, package_path, &e.name.span.file)
                {
                    // Use the base mangled name (no type args) as the key.
                    // For non-generic enums this is the only entry.
                    // For generic enums this is the template entry.
                    let mangled_name = MangledName::for_type(&info.fqn);
                    types.insert(
                        mangled_name.clone(),
                        TypeDef::Enum(EnumTypeDef {
                            fqn: info.fqn.clone(),
                            mangled_name,
                            // For generic enums, payload_types still contain TypeParameter types.
                            // Monomorphize substitutes them when creating concrete TypeDefs.
                            type_params: info.type_params.clone(),
                            variants: info
                                .variants
                                .iter()
                                .map(|(name, payload)| EnumVariantDef {
                                    name: name.clone(),
                                    payload_types: match payload {
                                        VariantPayload::None => vec![],
                                        VariantPayload::Tuple(types) => types.clone(),
                                        VariantPayload::Record(fields) => {
                                            fields.iter().map(|(_, ty)| ty.clone()).collect()
                                        }
                                    },
                                })
                                .collect(),
                            span: info.span.clone(),
                        }),
                    );
                }
            }
        }
    }
    types
}
