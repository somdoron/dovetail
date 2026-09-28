use serde::{Deserialize, Serialize};

use super::ManifestError;

/// The raw TOML structure of Dovetail.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawManifest {
    #[serde(rename = "compiler-version")]
    pub compiler_version: String,
    #[serde(
        default,
        rename = "standard-tag",
        skip_serializing_if = "Option::is_none"
    )]
    pub standard_tag: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<RawDependency>,
    pub project: Vec<RawProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawDependency {
    pub git: String,
    pub branch: Option<String>,
    pub tag: Option<String>,
    pub rev: Option<String>,
    pub manifest: Option<String>,
    pub projects: Vec<ProjectSelection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum ProjectSelection {
    Name(String),
    Aliased { project: String, alias: String },
}

impl ProjectSelection {
    pub fn names(&self) -> (&str, &str) {
        match self {
            Self::Name(name) => (name, name),
            Self::Aliased { project, alias } => (project, alias),
        }
    }
}

/// A single `[[project]]` entry in Dovetail.toml.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawProject {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<crate::image::config::ImageConfig>,
    pub name: String,
    /// Project directory relative to the manifest; defaults to the project name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub root_package: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends: Vec<String>,
    pub packages: Vec<String>,
    /// Optional main function FQN (e.g. "a.utils.entry"). If omitted, auto-detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<String>,
    /// Optional list of binary resource paths (relative to project_dir) to
    /// embed into the WASM. Each entry becomes accessible from this package's
    /// source code via `Resource.bytes("<path>")`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
    /// Optional `[[project.macro]]` entries declaring derive macros this
    /// project ships. Each entry points at a Rhai script that the macro
    /// phase invokes when an `@derive(Name)` annotation resolves to this
    /// macro's FQN. Empty by default.
    #[serde(default, rename = "macro", skip_serializing_if = "Vec::is_empty")]
    pub macros: Vec<RawMacro>,
    /// Optional `[[project.component]]` entries declaring WASM component
    /// dependencies. Each component's exported WIT interface is projected
    /// into a generated Dovetail bindings package, and the component is
    /// composed into the final output at build time. Empty by default.
    #[serde(default, rename = "component", skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<RawComponent>,
}

/// A `[[project.component]]` entry: a WASM component dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawComponent {
    /// Path to a component `.wasm` file, relative to the project directory.
    pub path: String,
    /// Dovetail package path the generated bindings are exposed as
    /// (e.g. `"sqlite.raw"`).
    pub package: String,
    /// Exported interface to import, when the component exports several.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
}

/// A `[[project.macro]]` entry: declares a derive macro this project provides.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RawMacro {
    /// The macro's short name (e.g. `"JsonCodec"`). Combined with `package`
    /// to form the macro's FQN.
    pub name: String,
    /// The package FQN this macro lives in. Must match one of the project's
    /// declared packages (or the `root_package`).
    pub package: String,
    /// The kind of macro. Currently only `"derive"` is supported.
    pub kind: String,
    /// Optional trait FQN this derive targets. Used only for diagnostics —
    /// the macro phase does not validate it against the generated AST.
    #[serde(default, rename = "trait", skip_serializing_if = "Option::is_none")]
    pub trait_: Option<String>,
    /// Path to the macro's script source, relative to the project directory.
    pub script: String,
}

/// Parse the contents of a Dovetail.toml file into a `RawManifest`.
pub(super) fn parse_manifest(content: &str) -> Result<RawManifest, ManifestError> {
    toml::from_str(content).map_err(|e| ManifestError::TomlError {
        message: e.to_string(),
    })
}

/// Serialize a `RawManifest` to a TOML string.
pub(super) fn serialize_manifest(manifest: &RawManifest) -> Result<String, ManifestError> {
    toml::to_string_pretty(manifest).map_err(|e| ManifestError::TomlError {
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_project() {
        let toml = r#"compiler-version = "0.1.4"
[[project]]
name = "myapp"
root_package = "com.example.myapp"
packages = ["."]
"#;
        let manifest = parse_manifest(toml).unwrap();
        assert_eq!(manifest.project.len(), 1);
        assert_eq!(manifest.project[0].name, "myapp");
        assert_eq!(manifest.project[0].root_package, "com.example.myapp");
        assert!(manifest.project[0].depends.is_empty());
        assert_eq!(manifest.project[0].packages, vec!["."]);
    }

    #[test]
    fn parse_multiple_projects_with_depends() {
        let toml = r#"compiler-version = "0.1.4"
[[project]]
name = "mylib"
root_package = "com.example.mylib"
packages = ["."]

[[project]]
name = "myapp"
root_package = "com.example.myapp"
depends = ["mylib"]
packages = ["utils", "."]
"#;
        let manifest = parse_manifest(toml).unwrap();
        assert_eq!(manifest.project.len(), 2);
        assert_eq!(manifest.project[1].depends, vec!["mylib"]);
        assert_eq!(manifest.project[1].packages, vec!["utils", "."]);
    }

    #[test]
    fn parse_empty_depends_default() {
        let toml = r#"compiler-version = "0.1.4"
[[project]]
name = "myapp"
root_package = "com.example.myapp"
packages = ["."]
"#;
        let manifest = parse_manifest(toml).unwrap();
        assert!(manifest.project[0].depends.is_empty());
    }

    #[test]
    fn parse_missing_required_field_name() {
        let toml = r#"compiler-version = "0.1.4"
[[project]]
root_package = "com.example.myapp"
packages = ["."]
"#;
        assert!(parse_manifest(toml).is_err());
    }

    #[test]
    fn parse_missing_required_field_packages() {
        let toml = r#"compiler-version = "0.1.4"
[[project]]
name = "myapp"
root_package = "com.example.myapp"
"#;
        assert!(parse_manifest(toml).is_err());
    }

    #[test]
    fn parse_invalid_toml() {
        let toml = "this is not valid toml [[[";
        assert!(parse_manifest(toml).is_err());
    }
}
