use std::collections::{BTreeMap, VecDeque};

use super::ManifestError;
use super::toml_schema::RawProject;

/// Topological sort of projects by their `depends` graph (Kahn's algorithm).
///
/// Returns project indices in dependency order (dependencies first).
/// Accumulates errors for duplicate names, unknown dependencies, and cycles.
pub(super) fn topological_sort(projects: &[RawProject]) -> Result<Vec<usize>, Vec<ManifestError>> {
    let mut errors = Vec::new();

    // Build name → index map, check for duplicates.
    let mut name_to_index: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, p) in projects.iter().enumerate() {
        if name_to_index.insert(&p.name, i).is_some() {
            errors.push(ManifestError::DuplicateProject {
                name: p.name.clone(),
            });
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // Build in-degree counts and adjacency (dep → dependents).
    let n = projects.len();
    let mut in_degree = vec![0usize; n];
    let mut dependents: Vec<Vec<usize>> = vec![vec![]; n];

    for (i, p) in projects.iter().enumerate() {
        for dep_name in &p.depends {
            match name_to_index.get(dep_name.as_str()) {
                Some(&dep_idx) => {
                    dependents[dep_idx].push(i);
                    in_degree[i] += 1;
                }
                None => {
                    errors.push(ManifestError::UnknownDependency {
                        project: p.name.clone(),
                        dependency: dep_name.clone(),
                    });
                }
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // Kahn's BFS.
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut order = Vec::with_capacity(n);
    while let Some(idx) = queue.pop_front() {
        order.push(idx);
        for &dep_idx in &dependents[idx] {
            in_degree[dep_idx] -= 1;
            if in_degree[dep_idx] == 0 {
                queue.push_back(dep_idx);
            }
        }
    }

    if order.len() != n {
        // Remaining nodes with in_degree > 0 are in cycles.
        let cycle: Vec<String> = (0..n)
            .filter(|&i| in_degree[i] > 0)
            .map(|i| projects[i].name.clone())
            .collect();
        return Err(vec![ManifestError::CyclicDependency { cycle }]);
    }

    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(name: &str, depends: &[&str]) -> RawProject {
        RawProject {
            image: None,
            name: name.to_string(),
            path: None,
            root_package: format!("com.example.{name}"),
            depends: depends.iter().map(|s| s.to_string()).collect(),
            packages: vec![".".to_string()],
            main: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
        }
    }

    #[test]
    fn single_project_no_deps() {
        let projects = vec![project("app", &[])];
        let order = topological_sort(&projects).unwrap();
        assert_eq!(order, vec![0]);
    }

    #[test]
    fn two_projects_linear_dependency() {
        let projects = vec![project("app", &["lib"]), project("lib", &[])];
        let order = topological_sort(&projects).unwrap();
        // lib (index 1) must come before app (index 0).
        assert_eq!(order, vec![1, 0]);
    }

    #[test]
    fn diamond_dependency() {
        // D has no deps; B depends on D; C depends on D; A depends on B and C.
        let projects = vec![
            project("a", &["b", "c"]),
            project("b", &["d"]),
            project("c", &["d"]),
            project("d", &[]),
        ];
        let order = topological_sort(&projects).unwrap();
        // d must come first, a must come last.
        assert_eq!(order[0], 3); // d
        assert_eq!(order[3], 0); // a
        // b and c in middle (either order).
        assert!(order[1] == 1 || order[1] == 2);
        assert!(order[2] == 1 || order[2] == 2);
    }

    #[test]
    fn duplicate_project_name() {
        let projects = vec![project("app", &[]), project("app", &[])];
        let errors = topological_sort(&projects).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], ManifestError::DuplicateProject { name } if name == "app"));
    }

    #[test]
    fn unknown_dependency() {
        let projects = vec![project("app", &["nonexistent"])];
        let errors = topological_sort(&projects).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ManifestError::UnknownDependency { project, dependency } if project == "app" && dependency == "nonexistent")
        );
    }

    #[test]
    fn cycle_two_projects() {
        let projects = vec![project("a", &["b"]), project("b", &["a"])];
        let errors = topological_sort(&projects).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ManifestError::CyclicDependency { cycle } if cycle.len() == 2)
        );
    }

    #[test]
    fn cycle_three_projects() {
        let projects = vec![
            project("a", &["b"]),
            project("b", &["c"]),
            project("c", &["a"]),
        ];
        let errors = topological_sort(&projects).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ManifestError::CyclicDependency { cycle } if cycle.len() == 3)
        );
    }

    #[test]
    fn self_dependency() {
        let projects = vec![project("a", &["a"])];
        let errors = topological_sort(&projects).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ManifestError::CyclicDependency { cycle } if cycle.len() == 1)
        );
    }

    #[test]
    fn multiple_independent_projects() {
        let projects = vec![project("a", &[]), project("b", &[]), project("c", &[])];
        let order = topological_sort(&projects).unwrap();
        assert_eq!(order.len(), 3);
        // All three should appear (any order is valid).
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, vec![0, 1, 2]);
    }
}
