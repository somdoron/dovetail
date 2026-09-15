use std::collections::BTreeMap;

use crate::common::types::{MangledName, PackagePath, SymbolName};
use crate::parser::ast::{Declaration, SourceFile};

use crate::typechecker::registry::Registry;
use crate::typechecker::types::{RecordTypeDef, TypeDef};

/// Collect record type definitions from AST declarations.
/// Non-generic records: concrete TypeDef with empty `type_params`.
/// Generic records: template TypeDef (keyed by base mangled name, with TypeParameter fields).
/// Monomorphize creates concrete TypeDefs from templates for each instantiation.
pub fn collect_record_type_defs(
    package_path: &PackagePath,
    files: &[&SourceFile],
    registry: &Registry,
) -> BTreeMap<MangledName, TypeDef> {
    let mut types = BTreeMap::new();
    for file in files {
        for decl in &file.declarations {
            if let Declaration::Record(rec) = decl {
                let fqn = crate::common::types::Fqn {
                    package: package_path.clone(),
                    symbol: SymbolName(rec.name.value.clone()),
                };
                if let Some(info) = registry.lookup_record_type(&fqn, package_path, &rec.name.span.file) {
                    // Use the base mangled name (no type args) as the key.
                    // For non-generic records this is the only entry.
                    // For generic records this is the template entry.
                    let mangled_name = MangledName::for_type(&info.fqn);
                    types.insert(
                        mangled_name.clone(),
                        TypeDef::Record(RecordTypeDef {
                            fqn: info.fqn.clone(),
                            mangled_name,
                            // For generic records, fields contain TypeParameter types.
                            // Monomorphize substitutes them when creating concrete TypeDefs.
                            fields: info.fields.clone(),
                            type_params: info.type_params.clone(),
                            span: info.span.clone(),
                        }),
                    );
                }
            }
        }
    }
    types
}
