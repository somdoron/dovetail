use std::fmt;

/// A package path like `a` or `com.example.myapp.utils`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PackagePath(pub Vec<String>);

impl PackagePath {
    /// Parse a dotted string like `"com.example.myapp"` into a `PackagePath`.
    pub fn from_dotted(s: &str) -> Self {
        Self(s.split('.').map(String::from).collect())
    }

    /// Check if this package path starts with (is equal to or a sub-path of) the given prefix.
    pub fn starts_with(&self, prefix: &PackagePath) -> bool {
        self.0.len() >= prefix.0.len() && self.0[..prefix.0.len()] == prefix.0[..]
    }
}

impl fmt::Display for PackagePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for segment in &self.0 {
            if !first {
                write!(f, ".")?;
            }
            write!(f, "{}", segment)?;
            first = false;
        }
        Ok(())
    }
}

/// A symbol name within a package.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymbolName(pub String);

impl fmt::Display for SymbolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Fully-qualified name: package path + symbol name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fqn {
    pub package: PackagePath,
    pub symbol: SymbolName,
}

impl Fqn {
    /// Parse a dotted string like `"a.utils.entry"` into an Fqn.
    /// Last segment = symbol name, preceding segments = package path.
    /// Returns None if fewer than 2 segments or if any segment is empty.
    pub fn from_dotted(s: &str) -> Option<Self> {
        let segments: Vec<&str> = s.split('.').collect();
        if segments.len() < 2 || segments.iter().any(|seg| seg.is_empty()) {
            return None;
        }
        let symbol = SymbolName(segments.last().unwrap().to_string());
        let package = PackagePath(
            segments[..segments.len() - 1]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        Some(Self { package, symbol })
    }
}

impl fmt::Display for Fqn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.package, self.symbol)
    }
}

/// A mangled name for codegen — the flattened representation of an FQN
/// used as WASM export names and internal identifiers.
///
/// Format: `package.name` for no-arg functions, `package.name$Type1$Type2` for overloaded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MangledName(pub String);

impl MangledName {
    /// Build a mangled name for a function with no parameters: `package.name`.
    pub fn for_function_no_params(fqn: &Fqn) -> Self {
        Self(format!("{}.{}", fqn.package, fqn.symbol))
    }

    /// Build a mangled name for a global variable: `package.name`.
    pub fn for_global(fqn: &Fqn) -> Self {
        Self(format!("{}.{}", fqn.package, fqn.symbol))
    }

    /// Build a mangled name for a type (record, enum, class): `package.TypeName`.
    pub fn for_type(fqn: &Fqn) -> Self {
        Self(format!("{}.{}", fqn.package, fqn.symbol))
    }

    /// Build a mangled name for a trait instantiation. Interface objects require per-instantiation
    /// vtable discrimination (different `trait_type_args` produce different vtables with
    /// different method signatures), so the type args remain in the mangled name even under
    /// full type erasure.
    pub fn for_trait_with_type_args(trait_fqn: &Fqn, type_args: &[impl fmt::Display]) -> Self {
        if type_args.is_empty() {
            Self::for_type(trait_fqn)
        } else {
            let parts: Vec<String> = type_args.iter().map(|t| t.to_string()).collect();
            Self(format!(
                "{}.{}${}",
                trait_fqn.package,
                trait_fqn.symbol,
                parts.join("$")
            ))
        }
    }

    /// Build a mangled name for a function: `package.name$Type1$Type2`.
    pub fn for_function(fqn: &Fqn, param_types: &[impl fmt::Display]) -> Self {
        if param_types.is_empty() {
            Self::for_function_no_params(fqn)
        } else {
            let suffix: Vec<String> = param_types.iter().map(|t| t.to_string()).collect();
            Self(format!(
                "{}.{}${}",
                fqn.package,
                fqn.symbol,
                suffix.join("$")
            ))
        }
    }

    /// Build a mangled name for a trait impl method.
    /// Format: `trait_fqn$type_fqn$method_name` for non-generic traits,
    /// or `trait_fqn$arg1,arg2$type_fqn$method_name` for generic traits.
    pub fn for_impl_method(
        trait_fqn: &Fqn,
        type_fqn: &Fqn,
        method_name: &SymbolName,
        trait_type_args: &[impl fmt::Display],
    ) -> Self {
        if trait_type_args.is_empty() {
            Self(format!("{}${}${}", trait_fqn, type_fqn, method_name))
        } else {
            let args: Vec<String> = trait_type_args.iter().map(|a| a.to_string()).collect();
            Self(format!(
                "{}${}${}${}",
                trait_fqn,
                args.join(","),
                type_fqn,
                method_name
            ))
        }
    }

    /// Template key for a trait member's default body:
    /// `{trait_fqn}$$default${member}` — e.g. `standard.prelude.Greeter$$default$greet`.
    /// No other name family can produce the substring `$$` (every existing
    /// constructor puts non-empty text between `$` separators), so this key
    /// cannot collide with any real impl, function, wrapper, or type name.
    pub fn for_trait_default(trait_fqn: &Fqn, member: &SymbolName) -> Self {
        Self(format!("{}$$default${}", trait_fqn, member))
    }

    /// Build a mangled name for a trait impl method with a caller-computed
    /// type segment (see `Type::impl_segment`). Byte-identical to
    /// `for_impl_method` when the segment is the type's bare FQN rendering.
    pub fn for_impl_block_method(
        trait_fqn: &Fqn,
        type_segment: &str,
        method_name: &SymbolName,
        trait_type_args: &[impl fmt::Display],
    ) -> Self {
        if trait_type_args.is_empty() {
            Self(format!("{}${}${}", trait_fqn, type_segment, method_name))
        } else {
            let args: Vec<String> = trait_type_args.iter().map(|a| a.to_string()).collect();
            Self(format!(
                "{}${}${}${}",
                trait_fqn,
                args.join(","),
                type_segment,
                method_name
            ))
        }
    }

    /// Build a mangled name for an array type specialization: `$Array$ElementType`.
    pub fn for_array_type(element_type: &impl fmt::Display) -> Self {
        Self(format!("$Array${}", element_type))
    }

    /// Build a mangled name for a tuple type: `$Tuple$Type1$Type2`.
    pub fn for_tuple(element_types: &[impl fmt::Display]) -> Self {
        let parts: Vec<String> = element_types.iter().map(|t| t.to_string()).collect();
        Self(format!("$Tuple${}", parts.join("$")))
    }

    /// Build a mangled name for a interface object type: `$IfaceObj$trait_mangled`.
    /// Uses the trait's mangled name (which already encodes type params for generic traits).
    pub fn for_interface_object_type(trait_mangled: &MangledName) -> Self {
        Self(format!("$IfaceObj${}", trait_mangled))
    }

    /// Build the **per-trait** interface-object mangled name — `$IfaceObj$package.Trait`, with the
    /// trait's type args deliberately dropped. Interface objects are de-monomorphized: one WASM type
    /// per trait, with the vtable slot signatures erasing the trait's generic params to `anyref`
    /// (the receiver is already `anyref`), so `Showable<Int32>` and `Showable<Int64>` share it.
    /// (The concrete impl *functions* are still mangled per-instantiation via `for_impl_method`.)
    pub fn for_interface_object_per_interface(trait_fqn: &Fqn) -> Self {
        Self::for_interface_object_type(&Self::for_type(trait_fqn))
    }

    /// Build the mangled name for an interface-object type over a **sorted** set of
    /// interfaces — `$IfaceObj$pkg.A` for a single interface (byte-identical to
    /// [`Self::for_interface_object_per_interface`]) and `$IfaceObj$pkg.A&pkg.B` for an
    /// intersection. Type args are deliberately dropped, as for the single form.
    pub fn for_interface_object_set(trait_fqns: &[Fqn]) -> Self {
        let parts: Vec<String> = trait_fqns.iter().map(|f| Self::for_type(f).0).collect();
        Self(format!("$IfaceObj${}", parts.join("&")))
    }

    /// Build a mangled name for a named extension method: `package.ExtensionName.methodName$ForType[$Param1$Param2…]`.
    ///
    /// `for_type` is always included as the leading discriminator so that a named
    /// extension with multiple blocks targeting different `for_type`s — e.g.
    /// `extension TimeExtension for Instant` and `extension TimeExtension for LocalDate`
    /// — does not collide on static methods/properties (which have no `self` param).
    /// For instance methods the for_type is duplicated with the self param; the
    /// resulting name is still unique.
    pub fn for_named_extension_method(
        package: &PackagePath,
        extension_name: &SymbolName,
        method_name: &SymbolName,
        for_type: &impl fmt::Display,
        param_types: &[impl fmt::Display],
    ) -> Self {
        let mut parts: Vec<String> = Vec::with_capacity(1 + param_types.len());
        parts.push(for_type.to_string());
        for t in param_types {
            parts.push(t.to_string());
        }
        Self(format!(
            "{}.{}.{}${}",
            package,
            extension_name,
            method_name,
            parts.join("$")
        ))
    }

    /// Build a mangled name for a test declaration: `$test$package.sanitized_name`.
    pub fn for_test(package_path: &PackagePath, test_name: &str) -> Self {
        let sanitized: String = test_name
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
            .collect();
        Self(format!("$test${}.{}", package_path, sanitized))
    }

    /// Append generic type arguments to a mangled name for disambiguation.
    /// Used by the monomorphize pass and `instantiate_generic_classes` to ensure
    /// different type-argument instantiations always produce distinct mangled names.
    /// Format: `base_name#TypeArg1,TypeArg2,...`
    pub fn with_type_args(self, type_args: &[impl fmt::Display]) -> Self {
        if type_args.is_empty() {
            self
        } else {
            let parts: Vec<String> = type_args.iter().map(|t| t.to_string()).collect();
            Self(format!("{}#{}", self.0, parts.join(",")))
        }
    }
}

impl fmt::Display for MangledName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Visibility of a declaration.
/// `Protected` is used for class members only; it acts like `Public` for registry
/// purposes (not stripped), but is only callable from subclasses. Since we don't
/// have subclasses yet, protected effectively disallows calling from outside.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    #[default]
    Internal,
    Private,
    Protected,
}

/// A local variable name (function-scoped).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct VarName(pub String);

impl fmt::Display for VarName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A type parameter name (e.g. `T`, `A`, `B`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeParamName(pub String);

/// Identifies a interface object method/property slot, independent of the concrete implementing type.
/// Encodes method name + non-self parameter types for overloading disambiguation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct InterfaceMemberName(pub String);

impl InterfaceMemberName {
    /// Construct from method name + non-self parameter type strings.
    pub fn new(method_name: &str, non_self_param_types: &[String]) -> Self {
        if non_self_param_types.is_empty() {
            Self(method_name.to_string())
        } else {
            Self(format!("{}${}", method_name, non_self_param_types.join("$")))
        }
    }
}

impl fmt::Display for InterfaceMemberName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Variance of a type parameter on a generic type (record, enum, class).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Variance {
    /// No annotation — invariant in this type parameter.
    Invariant,
    /// `out T` — covariant: if `A <: B` then `G<A> <: G<B>`.
    Covariant,
    /// `in T` — contravariant: if `A <: B` then `G<B> <: G<A>`.
    Contravariant,
}

impl fmt::Display for Variance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Variance::Invariant => write!(f, "invariant"),
            Variance::Covariant => write!(f, "covariant"),
            Variance::Contravariant => write!(f, "contravariant"),
        }
    }
}

impl fmt::Display for TypeParamName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fqn_from_dotted_two_segments() {
        let fqn = Fqn::from_dotted("a.main").unwrap();
        assert_eq!(fqn.package, PackagePath(vec!["a".to_string()]));
        assert_eq!(fqn.symbol, SymbolName("main".to_string()));
    }

    #[test]
    fn fqn_from_dotted_three_segments() {
        let fqn = Fqn::from_dotted("a.utils.entry").unwrap();
        assert_eq!(
            fqn.package,
            PackagePath(vec!["a".to_string(), "utils".to_string()])
        );
        assert_eq!(fqn.symbol, SymbolName("entry".to_string()));
    }

    #[test]
    fn fqn_from_dotted_single_segment_returns_none() {
        assert!(Fqn::from_dotted("main").is_none());
    }

    #[test]
    fn fqn_from_dotted_empty_returns_none() {
        assert!(Fqn::from_dotted("").is_none());
    }

    #[test]
    fn fqn_from_dotted_trailing_dot_returns_none() {
        assert!(Fqn::from_dotted("a.").is_none());
    }

    #[test]
    fn fqn_from_dotted_leading_dot_returns_none() {
        assert!(Fqn::from_dotted(".main").is_none());
    }

    #[test]
    fn fqn_from_dotted_consecutive_dots_returns_none() {
        assert!(Fqn::from_dotted("a..main").is_none());
    }

    // ── Base constructors (non-generic paths) ───────────────────────

    #[test]
    fn test_mangled_name_for_function() {
        let fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("add".to_string()),
        };
        let mangled = MangledName::for_function(&fqn, &["Int32", "Int32"]);
        assert_eq!(mangled.0, "myapp.add$Int32$Int32");
    }

    #[test]
    fn test_mangled_name_for_function_no_params() {
        let fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("main".to_string()),
        };
        let mangled = MangledName::for_function_no_params(&fqn);
        assert_eq!(mangled.0, "myapp.main");
    }

    #[test]
    fn test_mangled_name_for_function_empty_params_uses_no_params() {
        let fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("main".to_string()),
        };
        let mangled = MangledName::for_function(&fqn, &[] as &[String]);
        assert_eq!(mangled.0, "myapp.main");
    }

    #[test]
    fn test_mangled_name_for_impl_method() {
        let trait_fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Display".to_string()),
        };
        let type_fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("MyType".to_string()),
        };
        let method_name = SymbolName("to_string".to_string());
        let mangled =
            MangledName::for_impl_method(&trait_fqn, &type_fqn, &method_name, &[] as &[String]);
        assert_eq!(mangled.0, "myapp.Display$myapp.MyType$to_string");
    }

    #[test]
    fn test_mangled_name_for_impl_method_with_trait_type_args() {
        let trait_fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Into".to_string()),
        };
        let type_fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("MyType".to_string()),
        };
        let method_name = SymbolName("into".to_string());
        let mangled =
            MangledName::for_impl_method(&trait_fqn, &type_fqn, &method_name, &["String"]);
        assert_eq!(mangled.0, "myapp.Into$String$myapp.MyType$into");
    }

    #[test]
    fn test_mangled_name_for_named_extension_method() {
        let package = PackagePath(vec!["myapp".to_string()]);
        let ext_name = SymbolName("ArrayOps".to_string());
        let method_name = SymbolName("map".to_string());
        let mangled = MangledName::for_named_extension_method(
            &package,
            &ext_name,
            &method_name,
            &"Array<Int32>",
            &["Array<Int32>", "Fn(Int32) -> String"],
        );
        assert_eq!(
            mangled.0,
            "myapp.ArrayOps.map$Array<Int32>$Array<Int32>$Fn(Int32) -> String"
        );
    }

    #[test]
    fn test_mangled_name_for_named_extension_method_no_params() {
        let package = PackagePath(vec!["myapp".to_string()]);
        let ext_name = SymbolName("ArrayOps".to_string());
        let method_name = SymbolName("length".to_string());
        let mangled = MangledName::for_named_extension_method(
            &package,
            &ext_name,
            &method_name,
            &"Array<Int32>",
            &[] as &[String],
        );
        assert_eq!(mangled.0, "myapp.ArrayOps.length$Array<Int32>");
    }

    // ── Module functions (non-generic base) ───────────────────────

    #[test]
    fn test_mangled_name_for_module_function_with_params() {
        // Module functions use for_function with FQN symbol = "Mod.func"
        let fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Box.wrap".to_string()),
        };
        let mangled = MangledName::for_function(&fqn, &["Int32"]);
        assert_eq!(mangled.0, "myapp.Box.wrap$Int32");
    }

    #[test]
    fn test_mangled_name_for_module_function_no_params() {
        let fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Box.increment".to_string()),
        };
        let mangled = MangledName::for_function(&fqn, &[] as &[String]);
        assert_eq!(mangled.0, "myapp.Box.increment");
    }

    // ── with_type_args (generic instantiation disambiguation) ─────

    #[test]
    fn test_with_type_args_empty_is_identity() {
        let mangled = MangledName("myapp.func$Int32".to_string()).with_type_args(&[] as &[String]);
        assert_eq!(mangled.0, "myapp.func$Int32");
    }

    #[test]
    fn test_with_type_args_single() {
        let mangled = MangledName("myapp.identity$Int32".to_string()).with_type_args(&["Int32"]);
        assert_eq!(mangled.0, "myapp.identity$Int32#Int32");
    }

    #[test]
    fn test_with_type_args_multiple() {
        let mangled =
            MangledName("myapp.convert$Int32".to_string()).with_type_args(&["Int32", "String"]);
        assert_eq!(mangled.0, "myapp.convert$Int32#Int32,String");
    }

    // ── Full generic mangled names (base + type args) ─────────────

    #[test]
    fn test_generic_free_function_mangled_name() {
        // identity<T>(x: T): T instantiated with T=Int32
        let fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("identity".to_string()),
        };
        let mangled = MangledName::for_function(&fqn, &["Int32"]).with_type_args(&["Int32"]);
        assert_eq!(mangled.0, "myapp.identity$Int32#Int32");
    }

    #[test]
    fn test_generic_free_function_no_params_mangled_name() {
        // default<T>(): T instantiated with T=Int32 vs T=Bool
        let fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("default".to_string()),
        };
        let int_mangled =
            MangledName::for_function(&fqn, &[] as &[String]).with_type_args(&["Int32"]);
        let bool_mangled =
            MangledName::for_function(&fqn, &[] as &[String]).with_type_args(&["Bool"]);
        assert_eq!(int_mangled.0, "myapp.default#Int32");
        assert_eq!(bool_mangled.0, "myapp.default#Bool");
        assert_ne!(int_mangled, bool_mangled);
    }

    #[test]
    fn test_generic_module_function_no_params_disambiguated() {
        // Box<T>.increment() instantiated with T=Int32 vs T=Bool
        let fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Box.increment".to_string()),
        };
        let int_mangled =
            MangledName::for_function(&fqn, &[] as &[String]).with_type_args(&["Int32"]);
        let bool_mangled =
            MangledName::for_function(&fqn, &[] as &[String]).with_type_args(&["Bool"]);
        assert_eq!(int_mangled.0, "myapp.Box.increment#Int32");
        assert_eq!(bool_mangled.0, "myapp.Box.increment#Bool");
        assert_ne!(int_mangled, bool_mangled);
    }

    #[test]
    fn test_generic_module_function_with_params_disambiguated() {
        // Box<T>.wrap(value: T) instantiated with T=Int32 vs T=Bool
        let fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Box.wrap".to_string()),
        };
        let int_mangled = MangledName::for_function(&fqn, &["Int32"]).with_type_args(&["Int32"]);
        let bool_mangled = MangledName::for_function(&fqn, &["Bool"]).with_type_args(&["Bool"]);
        assert_eq!(int_mangled.0, "myapp.Box.wrap$Int32#Int32");
        assert_eq!(bool_mangled.0, "myapp.Box.wrap$Bool#Bool");
        assert_ne!(int_mangled, bool_mangled);
    }

    #[test]
    fn test_generic_named_extension_mangled_name() {
        // ArrayOps<T>.map<U>(self, f): Array<U> with T=Int32, U=String
        let package = PackagePath(vec!["myapp".to_string()]);
        let ext_name = SymbolName("ArrayOps".to_string());
        let method_name = SymbolName("map".to_string());
        let mangled = MangledName::for_named_extension_method(
            &package,
            &ext_name,
            &method_name,
            &"Array<Int32>",
            &["Array<Int32>", "Fn(Int32) -> String"],
        )
        .with_type_args(&["Int32", "String"]);
        assert_eq!(
            mangled.0,
            "myapp.ArrayOps.map$Array<Int32>$Array<Int32>$Fn(Int32) -> String#Int32,String"
        );
    }

    #[test]
    fn test_generic_impl_method_mangled_name() {
        // impl<T> Trait for Wrapper — method(self): T with T=Int32
        let trait_fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Trait".to_string()),
        };
        let type_fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Wrapper".to_string()),
        };
        let method_name = SymbolName("method".to_string());
        let mangled =
            MangledName::for_impl_method(&trait_fqn, &type_fqn, &method_name, &[] as &[String])
                .with_type_args(&["Int32"]);
        assert_eq!(mangled.0, "myapp.Trait$myapp.Wrapper$method#Int32");
    }

    #[test]
    fn test_generic_impl_method_with_trait_type_args_mangled_name() {
        // impl<T> Into<Array<T>> for Wrapper — into(self) with T=Int32
        let trait_fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Into".to_string()),
        };
        let type_fqn = Fqn {
            package: PackagePath(vec!["myapp".to_string()]),
            symbol: SymbolName("Wrapper".to_string()),
        };
        let method_name = SymbolName("into".to_string());
        let mangled =
            MangledName::for_impl_method(&trait_fqn, &type_fqn, &method_name, &["Array<Int32>"])
                .with_type_args(&["Int32"]);
        assert_eq!(
            mangled.0,
            "myapp.Into$Array<Int32>$myapp.Wrapper$into#Int32"
        );
    }
}
