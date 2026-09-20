use super::*;

fn dependency(identity: &str) -> ProjectDeclarations {
    let mut diagnostics = Diagnostics::new();
    let mut declarations = parse_declarations(
        include_str!("../../tests/fixtures/query/library/src/types.dove"),
        "types.dove",
        &mut diagnostics,
    );
    declarations.extend(parse_declarations(
        include_str!("../../tests/fixtures/query/library/src/Connection.dove"),
        "Connection.dove",
        &mut diagnostics,
    ));
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    ProjectDeclarations {
        name: "library".to_owned(),
        identity: identity.to_owned(),
        local: false,
        declarations,
    }
}

fn args(name: &str, all: bool) -> QueryArgs {
    QueryArgs {
        project: None,
        all,
        command: QueryCommand::Definition {
            name: name.to_owned(),
        },
    }
}

#[test]
fn dependency_visibility_filters_containers_and_members() {
    let projects = [dependency("version-one")];
    let (text, found) =
        output::query(&projects, &args("example.api.Connection", false), None).unwrap();
    assert!(found);
    assert!(text.contains("open("));
    assert!(!text.contains("implementationDetail"));
    assert!(
        !output::query(&projects, &args("example.api.hidden", false), None)
            .unwrap()
            .1
    );
    let (text, found) = output::query(
        &projects,
        &args("example.api.Connection.implementationDetail", true),
        None,
    )
    .unwrap();
    assert!(found);
    assert!(text.contains("private function implementationDetail"));
    let (text, _) = output::query(&projects, &args("example.api.Counter", false), None).unwrap();
    assert!(text.contains("protected function description"));
    assert!(!text.contains("private let label"));
}

#[test]
fn distinct_dependency_identities_are_never_combined() {
    let projects = [dependency("version-one"), dependency("version-two")];
    let error = output::query(&projects, &args("example.api.Connection", false), None).unwrap_err();
    let error = error.to_string();
    assert!(error.contains("--project"));
    assert!(error.contains("version-one"));
    assert!(error.contains("version-two"));
}

#[test]
fn source_bounds_and_declaration_shapes_are_preserved() {
    let projects = [dependency("version-one")];
    for (name, expected) in [
        ("add", "where L: Add<R, Output = O>"),
        ("Box", "record Box<out T>"),
        ("NamedReadable", "interface NamedReadable extends Readable"),
        ("Outcome", "Connected(Connection)"),
    ] {
        let (text, found) = output::query(
            &projects,
            &args(&format!("example.api.{name}"), false),
            None,
        )
        .unwrap();
        assert!(found);
        assert!(text.contains(expected), "{text}");
        assert!(!text.contains("left + right"));
    }
}

#[test]
fn analyzed_dependency_keeps_its_owner_visibility() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/query");
    let mut workspace = crate::manifest::load_manifest(&root).unwrap();
    let library = workspace
        .projects
        .iter_mut()
        .find(|p| p.name.0 == "library")
        .unwrap();
    let previous = library.identity();
    library.resolved_identity = Some("dependency-version".to_owned());
    for project in &mut workspace.projects {
        for dependency in &mut project.depends {
            if dependency.0 == previous {
                dependency.0 = "dependency-version".to_owned();
            }
        }
    }
    let query = args("example.api.Connection", false);
    let mut diagnostics = Diagnostics::new();
    let projects = analyze(&workspace, &query, &mut diagnostics);
    assert!(!diagnostics.has_errors(), "{diagnostics:?}");
    let (text, found) = output::query(&projects, &query, Some(&workspace)).unwrap();
    assert!(found);
    assert!(text.contains("open("));
    assert!(!text.contains("implementationDetail"));
    let query = args("example.api.Connection", true);
    let (text, _) = output::query(&projects, &query, Some(&workspace)).unwrap();
    assert!(text.contains("private function implementationDetail"));
}

#[test]
fn inferred_nominal_types_remain_qualified_inside_composite_types() {
    use crate::common::types::{Fqn, PackagePath, SymbolName};
    use crate::typechecker::types::Type;
    let identifier = Type::Newtype(
        Fqn {
            package: PackagePath(vec!["example".into(), "model".into()]),
            symbol: SymbolName("Identifier".into()),
        },
        Box::new(Type::Int32),
    );
    let function = Type::Function(
        vec![Type::Array(Box::new(identifier.clone()))],
        Box::new(identifier),
    );
    assert_eq!(
        types::resolved_type(&function),
        "(Array<example.model.Identifier>) => example.model.Identifier"
    );
}
