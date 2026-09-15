use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use dovetail::common::diagnostics::Diagnostics;

#[derive(Parser)]
#[command(name = "dovetail", version, about = "The Dovetail compiler")]
struct Cli {
    /// Require an unchanged dependency lockfile.
    #[arg(long, global = true)]
    locked: bool,
    /// Resolve dependencies without network access.
    #[arg(long, global = true)]
    offline: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Format Dovetail source files (the local workspace when files are omitted).
    Fmt {
        /// Check formatting without changing files.
        #[arg(long)]
        check: bool,
        /// Explicit .dove source files.
        files: Vec<PathBuf>,
    },
    /// Fetch or update Git dependencies.
    Deps {
        #[command(subcommand)]
        command: DependencyCommand,
    },
    /// Compile Dovetail source to WASM
    Build {
        /// Optional project name (builds all projects if omitted)
        project: Option<String>,

        /// Output directory for WASM files
        #[arg(short, long, default_value = "build")]
        output_dir: String,
    },
    /// Type-check only (no codegen)
    Check {
        /// Optional project name (checks all projects if omitted)
        project: Option<String>,
    },
    /// Build and run a Dovetail project
    Run {
        /// Project name to run (required if workspace has multiple projects)
        project: Option<String>,
        /// Preopen the current working directory at `.` for the component.
        #[arg(long)]
        allow_cwd: bool,
        /// Preopen the host filesystem root at `/` for the component.
        #[arg(long)]
        allow_root: bool,
        /// Preopen a specific host path; mounted at the same path inside the
        /// component. May be repeated.
        #[arg(long, value_name = "PATH")]
        allow_path: Vec<std::path::PathBuf>,
        /// Allow the component to use the network (TCP, UDP, DNS lookups).
        /// Default-deny — without this flag all network operations fail.
        #[arg(long)]
        allow_network: bool,
        /// Inherit all environment variables from the host process.
        #[arg(long)]
        inherit_env: bool,
        /// Pass an environment variable to the component. May be repeated.
        /// Format: KEY=VALUE. Later flags override earlier ones for the same
        /// key. With `--inherit-env` these override the inherited values.
        #[arg(long, value_name = "KEY=VALUE")]
        env: Vec<String>,
        /// Arguments to pass to the component, supplied after `--`.
        #[arg(last = true, value_name = "ARGS")]
        args: Vec<String>,
    },
    /// Run tests in a Dovetail project
    Test {
        /// Optional project name (tests all projects if omitted)
        project: Option<String>,
        /// Run only tests whose FQTN contains the given substring (multiple OR'd)
        #[arg(short, long)]
        filter: Vec<String>,
        /// Run only tests from the specified source file
        #[arg(long)]
        file: Option<String>,
    },
    /// Initialize a new workspace with a single project
    Init {
        /// Project name
        project: String,
    },
    /// Manage projects in the workspace
    Projects {
        #[command(subcommand)]
        subcommand: ProjectsCommands,
    },
    /// Start the LSP server
    LspServer {
        /// Use TCP transport instead of stdio
        #[arg(long)]
        tcp: bool,
        /// TCP port to listen on (only used with --tcp)
        #[arg(long, default_value = "9257")]
        port: u16,
        /// Enable verbose logging to stderr
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Subcommand)]
enum ProjectsCommands {
    /// Add a new project to the workspace
    Add {
        /// Project name
        name: String,
    },
}

#[derive(Subcommand)]
enum DependencyCommand {
    Fetch,
    Update { alias: Option<String> },
}

fn main() {
    let cli = Cli::parse();

    let options = dovetail::manifest::ResolveOptions {
        locked: cli.locked, offline: cli.offline,
        progress: Some(std::sync::Arc::new(|message| eprintln!("{message}"))),
        ..Default::default()
    };
    match cli.command {
        Commands::Fmt { check, files } => match dovetail::formatter::files::run(&files, check) {
            Ok(true) => process::exit(1),
            Ok(false) => {},
            Err(error) => {
                eprintln!("{error}");
                process::exit(2);
            }
        },
        Commands::Deps { command } => {
            let mut options = options;
            options.fetch_all = true;
            options.update = match command {
                DependencyCommand::Fetch => dovetail::manifest::UpdateRequest::None,
                DependencyCommand::Update { alias: Some(alias) } => dovetail::manifest::UpdateRequest::Alias(alias),
                DependencyCommand::Update { alias: None } => dovetail::manifest::UpdateRequest::All,
            };
            let root = std::env::current_dir().unwrap_or_else(|e| { eprintln!("{e}"); process::exit(1) });
            if let Err(errors) = dovetail::manifest::load_manifest_with_options(&root, &options) {
                for error in errors { eprintln!("error: {error}"); }
                process::exit(1);
            }
        }
        Commands::Build {
            project,
            output_dir,
        } => {
            if let Err(()) = build(project.as_deref(), &output_dir, &options) {
                process::exit(1);
            }
        }
        Commands::Check { project } => {
            if let Err(()) = check(project.as_deref(), &options) {
                process::exit(1);
            }
        }
        Commands::Run {
            project,
            allow_cwd,
            allow_root,
            allow_path,
            allow_network,
            inherit_env,
            env,
            args,
        } => {
            let fs_permissions = dovetail::runner::FsPermissions {
                allow_cwd,
                allow_root,
                allow_paths: allow_path,
            };
            let net_permissions = dovetail::runner::NetPermissions { allow_network };
            let env_permissions =
                match dovetail::runner::EnvPermissions::from_flags(inherit_env, &env, args) {
                    Ok(p) => p,
                    Err(message) => {
                        eprintln!("error: {message}");
                        process::exit(1);
                    }
                };
            if let Err(()) = run(
                project.as_deref(),
                &fs_permissions,
                &env_permissions,
                &net_permissions,
                &options,
            ) {
                process::exit(1);
            }
        }
        Commands::Test { project, filter, file } => {
            match test(project.as_deref(), &filter, file.as_deref(), &options) {
                Ok(()) => {}
                Err(code) => process::exit(code),
            }
        }
        Commands::Init { project } => {
            if let Err(()) = init(&project) {
                process::exit(1);
            }
        }
        Commands::Projects { subcommand } => match subcommand {
            ProjectsCommands::Add { name } => {
                if let Err(()) = projects_add(&name) {
                    process::exit(1);
                }
            }
        },
        Commands::LspServer { tcp, port, verbose } => {
            dovetail::lsp::run_server(tcp, port, verbose);
        }
    }
}

fn build(project_filter: Option<&str>, output_dir: &str, options: &dovetail::manifest::ResolveOptions) -> Result<(), ()> {
    let workspace_root = std::env::current_dir().map_err(|e| {
        eprintln!("error: cannot determine current directory: {e}");
    })?;

    let workspace = dovetail::manifest::load_manifest_with_options(&workspace_root, &dovetail::manifest::ResolveOptions { target: project_filter.map(str::to_string), ..options.clone() }).map_err(|errors| {
        for error in &errors {
            eprintln!("error: {error}");
        }
    })?;

    if let Some(name) = project_filter {
        if !workspace.projects.iter().any(|p| p.name.0 == name) {
            eprintln!("error: project '{name}' not found in Dovetail.toml");
            return Err(());
        }
    }

    let result = dovetail::build_workspace(&workspace, project_filter, dovetail::BuildMode::Build, &std::collections::HashMap::new(), false, None);

    report_diagnostics(&result.diagnostics);
    if result.diagnostics.has_errors() {
        return Err(());
    }

    let output_path = PathBuf::from(output_dir);
    std::fs::create_dir_all(&output_path).map_err(|e| {
        eprintln!(
            "error: cannot create output directory '{}': {e}",
            output_path.display()
        );
    })?;

    for (project_name, project_result) in &result.project_results {
        if let Some(ref wasm) = project_result.wasm {
            let wasm_path = output_path.join(format!("{project_name}.wasm"));
            std::fs::write(&wasm_path, wasm).map_err(|e| {
                eprintln!("error: cannot write '{}': {e}", wasm_path.display());
            })?;
            eprintln!("compiled {project_name} -> {}", wasm_path.display());
        }
    }

    Ok(())
}

fn check(project_filter: Option<&str>, options: &dovetail::manifest::ResolveOptions) -> Result<(), ()> {
    let workspace_root = std::env::current_dir().map_err(|e| {
        eprintln!("error: cannot determine current directory: {e}");
    })?;

    let workspace = dovetail::manifest::load_manifest_with_options(&workspace_root, &dovetail::manifest::ResolveOptions { target: project_filter.map(str::to_string), ..options.clone() }).map_err(|errors| {
        for error in &errors {
            eprintln!("error: {error}");
        }
    })?;

    if let Some(name) = project_filter {
        if !workspace.projects.iter().any(|p| p.name.0 == name) {
            eprintln!("error: project '{name}' not found in Dovetail.toml");
            return Err(());
        }
    }

    let result = dovetail::build_workspace(&workspace, project_filter, dovetail::BuildMode::Check, &std::collections::HashMap::new(), false, None);

    report_diagnostics(&result.diagnostics);
    if result.diagnostics.has_errors() {
        return Err(());
    }

    if let Some(name) = project_filter {
        eprintln!("check passed: {name}");
    } else {
        eprintln!("check passed: all projects");
    }

    Ok(())
}

fn run(
    project_filter: Option<&str>,
    fs_permissions: &dovetail::runner::FsPermissions,
    env_permissions: &dovetail::runner::EnvPermissions,
    net_permissions: &dovetail::runner::NetPermissions,
    options: &dovetail::manifest::ResolveOptions,
) -> Result<(), ()> {
    let workspace_root = std::env::current_dir().map_err(|e| {
        eprintln!("error: cannot determine current directory: {e}");
    })?;

    let workspace = dovetail::manifest::load_manifest_with_options(&workspace_root, &dovetail::manifest::ResolveOptions { target: project_filter.map(str::to_string), ..options.clone() }).map_err(|errors| {
        for error in &errors {
            eprintln!("error: {error}");
        }
    })?;

    // Determine which project to run
    let project_name = match project_filter {
        Some(name) => {
            if !workspace.projects.iter().any(|p| p.name.0 == name) {
                eprintln!("error: project '{name}' not found in Dovetail.toml");
                return Err(());
            }
            name.to_string()
        }
        None => {
            if workspace.projects.iter().filter(|p| workspace.is_local(p)).count() != 1 {
                eprintln!(
                    "error: workspace has {} projects; specify which project to run",
                    workspace.projects.iter().filter(|p| workspace.is_local(p)).count()
                );
                return Err(());
            }
            workspace.projects.iter().find(|p| workspace.is_local(p)).unwrap().name.0.clone()
        }
    };

    let result = dovetail::build_workspace(&workspace, Some(&project_name), dovetail::BuildMode::Build, &std::collections::HashMap::new(), false, None);

    report_diagnostics(&result.diagnostics);
    if result.diagnostics.has_errors() {
        return Err(());
    }

    let (_, project_result) = result
        .project_results
        .iter()
        .find(|(n, _)| n == &project_name)
        .expect("target project must be in results");

    let wasm_bytes = project_result.wasm.as_ref().ok_or_else(|| {
        eprintln!("error: project '{project_name}' produced no WASM output");
    })?;

    dovetail::runner::run_component(wasm_bytes, fs_permissions, env_permissions, net_permissions)
        .map_err(|e| {
            eprintln!("error: {}", e.message);
        })
}

// -- Color helpers --

struct Colors {
    green: &'static str,
    red: &'static str,
    yellow: &'static str,
    bold: &'static str,
    dim: &'static str,
    reset: &'static str,
}

impl Colors {
    fn detect() -> Self {
        use std::io::IsTerminal;
        if std::io::stderr().is_terminal() {
            Self {
                green: "\x1b[32m",
                red: "\x1b[31m",
                yellow: "\x1b[33m",
                bold: "\x1b[1m",
                dim: "\x1b[2m",
                reset: "\x1b[0m",
            }
        } else {
            Self {
                green: "",
                red: "",
                yellow: "",
                bold: "",
                dim: "",
                reset: "",
            }
        }
    }
}

fn format_summary_counts(passed: usize, failed: usize, skipped: usize, c: &Colors) -> String {
    let mut parts = Vec::new();
    if passed > 0 {
        parts.push(format!("{}{passed} passed{}", c.green, c.reset));
    }
    if failed > 0 {
        parts.push(format!("{}{failed} failed{}", c.red, c.reset));
    }
    if skipped > 0 {
        parts.push(format!("{}{skipped} skipped{}", c.yellow, c.reset));
    }
    parts.join(", ")
}

// -- Test runner --

fn test(project_filter: Option<&str>, filters: &[String], file_filter: Option<&str>, options: &dovetail::manifest::ResolveOptions) -> Result<(), i32> {
    let c = Colors::detect();

    let workspace_root = std::env::current_dir().map_err(|e| {
        eprintln!("error: cannot determine current directory: {e}");
        2
    })?;

    let workspace = dovetail::manifest::load_manifest_with_options(&workspace_root, &dovetail::manifest::ResolveOptions { target: project_filter.map(str::to_string), ..options.clone() }).map_err(|errors| {
        for error in &errors {
            eprintln!("error: {error}");
        }
        2
    })?;

    if let Some(name) = project_filter {
        if !workspace.projects.iter().any(|p| p.name.0 == name) {
            eprintln!("error: project '{name}' not found in Dovetail.toml");
            return Err(2);
        }
    }

    let result = dovetail::build_workspace(&workspace, project_filter, dovetail::BuildMode::Test, &std::collections::HashMap::new(), false, None);

    report_diagnostics(&result.diagnostics);
    if result.diagnostics.has_errors() {
        return Err(2);
    }

    let mut total_tests = 0usize;
    let mut total_passed = 0usize;
    let mut total_failed = 0usize;
    let mut total_skipped = 0usize;

    let has_filters = !filters.is_empty() || file_filter.is_some();

    for (project_name, project_result) in &result.project_results {
        if project_result.test_exports.is_empty() {
            continue;
        }

        let filtered_exports: Vec<dovetail::TestExportInfo> = project_result.test_exports.iter()
            .filter(|t| {
                if let Some(fp) = file_filter {
                    if !t.source_file.contains(fp) {
                        return false;
                    }
                }
                if !filters.is_empty() {
                    return filters.iter().any(|f| t.fqtn.contains(f.as_str()));
                }
                true
            })
            .cloned()
            .collect();

        if filtered_exports.is_empty() {
            continue;
        }

        total_tests += filtered_exports.len();

        let wasm_bytes = match &project_result.wasm {
            Some(bytes) => bytes,
            None => {
                eprintln!("error: project '{project_name}' produced no WASM output");
                return Err(2);
            }
        };

        eprintln!(
            "\n{}Running {} tests from '{project_name}'{}",
            c.bold,
            filtered_exports.len(),
            c.reset,
        );

        // Escape hatch for debugging codegen: `DOVETAIL_DUMP_TEST_WASM=<dir>` writes
        // each project's test component so it can be inspected with wasm-tools.
        if let Ok(dir) = std::env::var("DOVETAIL_DUMP_TEST_WASM") {
            let path = std::path::Path::new(&dir).join(format!("{project_name}-tests.wasm"));
            if let Err(e) = std::fs::write(&path, wasm_bytes) {
                eprintln!("warning: could not dump test wasm to {}: {e}", path.display());
            } else {
                eprintln!("dumped test component -> {}", path.display());
            }
        }

        match dovetail::test_runner::run_tests(wasm_bytes, &filtered_exports) {
            Ok(run_result) => {
                let mut proj_passed = 0usize;
                let mut proj_failed = 0usize;
                let mut proj_skipped = 0usize;

                // Group results by source file, preserving order of first appearance
                let mut file_order: Vec<String> = Vec::new();
                let mut by_file: std::collections::HashMap<String, Vec<&dovetail::test_runner::TestResult>> = std::collections::HashMap::new();
                for tr in &run_result.results {
                    let file = tr.source_file.clone();
                    by_file.entry(file.clone()).or_default().push(tr);
                    if !file_order.contains(&file) {
                        file_order.push(file);
                    }
                }

                for file in &file_order {
                    let tests = &by_file[file];
                    eprintln!("  {}{}{}", c.dim, file, c.reset);

                    let mut file_passed = 0usize;
                    let mut file_failed = 0usize;
                    let mut file_skipped = 0usize;

                    for tr in tests {
                        match &tr.status {
                            dovetail::test_runner::TestStatus::Pass => {
                                file_passed += 1;
                                eprintln!("    {}✓{} {}", c.green, c.reset, tr.name);
                            }
                            dovetail::test_runner::TestStatus::Skip { reason } => {
                                file_skipped += 1;
                                let suffix = match reason {
                                    Some(r) => format!(" ({r})"),
                                    None => String::new(),
                                };
                                eprintln!("    {}○{} {}{}", c.yellow, c.reset, tr.name, suffix);
                            }
                            dovetail::test_runner::TestStatus::Fail { message } => {
                                file_failed += 1;
                                eprintln!("    {}✗{} {} — {}", c.red, c.reset, tr.name, message);
                            }
                        }
                    }

                    if tests.len() > 1 {
                        eprintln!(
                            "    {}{}{}: {}",
                            c.dim, file, c.reset,
                            format_summary_counts(file_passed, file_failed, file_skipped, &c),
                        );
                    }

                    proj_passed += file_passed;
                    proj_failed += file_failed;
                    proj_skipped += file_skipped;
                }

                // Project summary
                let status_color = if proj_failed > 0 { c.red } else { c.green };
                eprintln!(
                    "  {}{}{project_name}{}: {}",
                    status_color, c.bold, c.reset,
                    format_summary_counts(proj_passed, proj_failed, proj_skipped, &c),
                );

                total_passed += proj_passed;
                total_failed += proj_failed;
                total_skipped += proj_skipped;
            }
            Err(e) => {
                total_failed += filtered_exports.len();
                eprintln!("{}error running tests for '{project_name}': {e}{}", c.red, c.reset);
            }
        }
    }

    eprintln!();

    if total_tests == 0 {
        if has_filters {
            eprintln!("No tests matched the filter.");
        } else {
            eprintln!("No tests found.");
        }
        return Ok(());
    }

    // Workspace summary
    if total_failed > 0 {
        eprintln!(
            "{}{}{total_tests} tests: {}{}",
            c.bold, c.red,
            format_summary_counts(total_passed, total_failed, total_skipped, &c),
            c.reset,
        );
        Err(1)
    } else {
        eprintln!(
            "{}{}{total_tests} tests passed.{}",
            c.bold, c.green, c.reset,
        );
        Ok(())
    }
}

fn init(project_name: &str) -> Result<(), ()> {
    let workspace_root = std::env::current_dir().map_err(|e| {
        eprintln!("error: cannot determine current directory: {e}");
    })?;

    dovetail::manifest::init_workspace(&workspace_root, project_name).map_err(|errors| {
        for error in &errors {
            eprintln!("error: {error}");
        }
    })?;

    eprintln!("initialized workspace with project '{project_name}'");
    Ok(())
}

fn projects_add(project_name: &str) -> Result<(), ()> {
    let workspace_root = std::env::current_dir().map_err(|e| {
        eprintln!("error: cannot determine current directory: {e}");
    })?;

    dovetail::manifest::add_project(&workspace_root, project_name).map_err(|errors| {
        for error in &errors {
            eprintln!("error: {error}");
        }
    })?;

    eprintln!("added project '{project_name}'");
    Ok(())
}

fn report_diagnostics(diagnostics: &Diagnostics) {
    for diag in diagnostics.iter() {
        let severity = match diag.severity {
            dovetail::common::diagnostics::Severity::Error => "error",
            dovetail::common::diagnostics::Severity::Warning => "warning",
        };
        eprintln!(
            "{}:{}:{}: {}: {}",
            diag.span.file, diag.span.line, diag.span.column, severity, diag.message
        );
    }
}
