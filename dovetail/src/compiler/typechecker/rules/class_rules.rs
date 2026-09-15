use std::collections::BTreeSet;

use crate::common::diagnostics::Diagnostics;
use crate::common::types::{Fqn, PackagePath, SymbolName, Visibility};
use crate::parser::ast::{ClassMember, Declaration, SourceFile};
use crate::typechecker::registry::Registry;

/// Check class rules: abstract constraints, final class extension, override validity,
/// final method overrides, and concrete subclass must implement abstract methods.
pub fn check_class_rules(
    package_registry: &Registry,
    package_path: &PackagePath,
    source_files: &[&SourceFile],
    diagnostics: &mut Diagnostics,
) {
    for source_file in source_files {
        for decl in &source_file.declarations {
            if let Declaration::Class(class) = decl {
                // Look up the class signature
                let class_fqn_str = format!(
                    "{}.{}",
                    source_file
                        .package
                        .path
                        .iter()
                        .map(|s| s.value.as_str())
                        .collect::<Vec<_>>()
                        .join("."),
                    class.name.value,
                );
                let Some(class_fqn) = Fqn::from_dotted(&class_fqn_str) else {
                    continue;
                };
                let Some(class_sig) = package_registry.lookup_class_type(&class_fqn, package_path)
                else {
                    continue;
                };

                // --- Checks for ALL classes (not just those with extends) ---

                // 1. abstract + final → error
                if class.is_abstract && class.is_final {
                    diagnostics.error(
                        class.name.span.clone(),
                        format!(
                            "class '{}' cannot be both abstract and final",
                            class.name.value
                        ),
                    );
                }

                // 1b. sealed without abstract → error
                if class.is_sealed && !class.is_abstract {
                    diagnostics.error(
                        class.name.span.clone(),
                        format!(
                            "class '{}' is sealed but not abstract; 'sealed' requires 'abstract'",
                            class.name.value
                        ),
                    );
                }

                // 2. Per-method checks
                for member in &class.body {
                    if let ClassMember::Method(func) = member
                        && func.is_abstract
                    {
                        // a. abstract method in non-abstract class → error
                        if !class.is_abstract {
                            diagnostics.error(
                                    func.name.span.clone(),
                                    format!(
                                        "abstract method '{}' cannot be declared in non-abstract class '{}'",
                                        func.name.value, class.name.value
                                    ),
                                );
                        }
                        // b. abstract static method (no self) → error
                        let is_instance =
                            func.params.first().is_some_and(|p| p.name.value == "self");
                        if !is_instance {
                            diagnostics.error(
                                    func.name.span.clone(),
                                    format!(
                                        "abstract method '{}' must be an instance method (must have 'self' parameter)",
                                        func.name.value
                                    ),
                                );
                        }
                        // c. abstract + final → error
                        if func.is_final {
                            diagnostics.error(
                                func.name.span.clone(),
                                format!(
                                    "method '{}' cannot be both abstract and final",
                                    func.name.value
                                ),
                            );
                        }
                        // d. abstract + override → error
                        if func.is_override {
                            diagnostics.error(
                                func.name.span.clone(),
                                format!(
                                    "method '{}' cannot be both abstract and override",
                                    func.name.value
                                ),
                            );
                        }
                    }
                    if let ClassMember::Property(prop) = member
                        && prop.is_abstract
                    {
                        if !class.is_abstract {
                            diagnostics.error(
                                    prop.name.span.clone(),
                                    format!(
                                        "abstract property '{}' cannot be declared in non-abstract class '{}'",
                                        prop.name.value, class.name.value
                                    ),
                                );
                        }
                        let is_instance =
                            prop.params.first().is_some_and(|p| p.name.value == "self");
                        if !is_instance {
                            diagnostics.error(
                                    prop.name.span.clone(),
                                    format!(
                                        "abstract property '{}' must be an instance property (must have 'self' parameter)",
                                        prop.name.value
                                    ),
                                );
                        }
                        if prop.is_final {
                            diagnostics.error(
                                prop.name.span.clone(),
                                format!(
                                    "property '{}' cannot be both abstract and final",
                                    prop.name.value
                                ),
                            );
                        }
                        if prop.is_override {
                            diagnostics.error(
                                prop.name.span.clone(),
                                format!(
                                    "property '{}' cannot be both abstract and override",
                                    prop.name.value
                                ),
                            );
                        }
                    }
                }

                // --- Checks for classes with extends ---

                let Some(ref parent_fqn) = class_sig.parent_class else {
                    continue;
                };
                let Some(parent_sig) = package_registry.lookup_class_type(parent_fqn, package_path)
                else {
                    continue;
                };
                let ext = class.extends.as_ref().unwrap();

                // Check: cannot extend final class
                if parent_sig.is_final {
                    diagnostics.error(
                        ext.span.clone(),
                        format!("cannot extend final class '{}'", parent_fqn.symbol),
                    );
                }

                // Check: sealed class rules
                if parent_sig.is_sealed {
                    // Subclasses of a sealed class must be in the same package
                    if class_fqn.package != parent_fqn.package {
                        diagnostics.error(
                            ext.span.clone(),
                            format!(
                                "cannot extend sealed class '{}' from a different package; all subclasses must be in the same package",
                                parent_fqn.symbol
                            ),
                        );
                    }
                    // Subclasses of a sealed class must be either final or sealed abstract
                    if !class.is_final && !(class.is_sealed && class.is_abstract) {
                        diagnostics.error(
                            class.name.span.clone(),
                            format!(
                                "class '{}' extending sealed class '{}' must be 'final' or 'sealed abstract'",
                                class.name.value, parent_fqn.symbol
                            ),
                        );
                    }
                }

                // Check: non-private field shadowing (walk entire hierarchy)
                let mut ancestor_field_names: BTreeSet<&str> = BTreeSet::new();
                let mut current = Some(parent_sig);
                while let Some(sig) = current {
                    for f in &sig.fields {
                        if f.visibility != Visibility::Private {
                            ancestor_field_names.insert(&f.name);
                        }
                    }
                    current = sig
                        .parent_class
                        .as_ref()
                        .and_then(|fqn| package_registry.lookup_class_type(fqn, package_path));
                }

                for f in &class_sig.fields {
                    if f.visibility == Visibility::Private {
                        continue;
                    }
                    if ancestor_field_names.contains(f.name.as_str()) {
                        diagnostics.error(
                            class.name.span.clone(),
                            format!(
                                "field '{}' in class '{}' shadows a non-private field from an ancestor class",
                                f.name, class.name.value
                            ),
                        );
                    }
                }

                // Check methods and properties for override/final violations
                // Walk the full ancestor chain (not just the immediate parent)
                for member in &class.body {
                    // Get name, is_override, is_abstract, is_property for both methods and properties
                    let (member_name, member_span, is_override, is_abstract_member, is_property) =
                        match member {
                            ClassMember::Method(func) => (
                                func.name.value.clone(),
                                func.name.span.clone(),
                                func.is_override,
                                func.is_abstract,
                                false,
                            ),
                            ClassMember::Property(prop) => (
                                prop.name.value.clone(),
                                prop.name.span.clone(),
                                prop.is_override,
                                prop.is_abstract,
                                true,
                            ),
                            _ => continue,
                        };

                    let method_name = SymbolName(member_name.clone());
                    let kind_label = if is_property { "property" } else { "method" };

                    // Walk ancestors to find the method/property
                    let mut ancestor_has_method = false;
                    let mut ancestor_method_is_final = false;
                    let mut walk = Some(parent_fqn.clone());
                    while let Some(ref anc_fqn) = walk {
                        if let Some(anc_sig) =
                            package_registry.lookup_class_type(anc_fqn, package_path)
                        {
                            if let Some(overloads) = anc_sig.instance_methods.get(&method_name) {
                                ancestor_has_method = true;
                                if overloads.iter().any(|s| s.is_final_method) {
                                    ancestor_method_is_final = true;
                                }
                                break;
                            }
                            // Also check generic_instance_methods for generic classes
                            if let Some(defs) = anc_sig.generic_instance_methods.get(&method_name) {
                                ancestor_has_method = true;
                                if defs.iter().any(|d| d.is_final_method) {
                                    ancestor_method_is_final = true;
                                }
                                break;
                            }
                            walk = anc_sig.parent_class.clone();
                        } else {
                            break;
                        }
                    }

                    if is_override && !ancestor_has_method {
                        diagnostics.error(
                            member_span.clone(),
                            format!(
                                "{kind_label} '{}' is marked override but no matching {kind_label} exists in parent class '{}'",
                                member_name, parent_fqn.symbol
                            ),
                        );
                    }

                    // Check: redefining parent method without 'override' keyword
                    if ancestor_has_method && !is_override && !is_abstract_member {
                        diagnostics.error(
                            member_span.clone(),
                            format!(
                                "{kind_label} '{}' shadows a {kind_label} from a parent class; use 'override' to override it",
                                member_name
                            ),
                        );
                    }

                    if ancestor_method_is_final {
                        diagnostics.error(
                            member_span,
                            format!(
                                "cannot override final {kind_label} '{}' from parent class '{}'",
                                member_name, parent_fqn.symbol
                            ),
                        );
                    }
                }

                // Check: concrete subclass must implement all inherited abstract methods
                if !class_sig.is_abstract {
                    let mut abstract_methods: BTreeSet<String> = BTreeSet::new();

                    // Walk ancestors from root to parent, collecting/removing abstract methods
                    let mut ancestors = Vec::new();
                    let mut walk_fqn = Some(parent_fqn.clone());
                    while let Some(ref fqn) = walk_fqn {
                        if let Some(sig) = package_registry.lookup_class_type(fqn, package_path) {
                            ancestors.push(sig.clone());
                            walk_fqn = sig.parent_class.clone();
                        } else {
                            break;
                        }
                    }
                    ancestors.reverse(); // root → parent order

                    for ancestor in &ancestors {
                        for (method_name, overloads) in &ancestor.instance_methods {
                            for sig in overloads {
                                if sig.is_abstract_method {
                                    abstract_methods.insert(method_name.0.clone());
                                } else {
                                    abstract_methods.remove(&method_name.0);
                                }
                            }
                        }
                        // Also check generic_instance_methods for generic classes
                        for (method_name, defs) in &ancestor.generic_instance_methods {
                            for def in defs {
                                if def.is_abstract_method {
                                    abstract_methods.insert(method_name.0.clone());
                                } else {
                                    abstract_methods.remove(&method_name.0);
                                }
                            }
                        }
                    }

                    // Check current class's methods
                    for method_name in class_sig.instance_methods.keys() {
                        abstract_methods.remove(&method_name.0);
                    }
                    for method_name in class_sig.generic_instance_methods.keys() {
                        abstract_methods.remove(&method_name.0);
                    }

                    for name in &abstract_methods {
                        diagnostics.error(
                            class.name.span.clone(),
                            format!(
                                "concrete class '{}' must implement abstract method '{}'",
                                class.name.value, name
                            ),
                        );
                    }
                }
            }
        }
    }
}
