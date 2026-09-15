//! Filesystem adapter for `dovetail fmt`.
use super::{FormatError, format_source};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn run(paths: &[PathBuf], check: bool) -> Result<bool, FormatError> {
    let paths = select_files(paths)?;
    let mut changes = vec![];
    let mut errors = vec![];
    for path in paths {
        match prepare(&path) {
            Ok(Some(output)) => changes.push((path, output)),
            Ok(None) => {}
            Err(error) => {
                let message = error.to_string();
                errors.push(if message.starts_with(path.to_string_lossy().as_ref()) {
                    message
                } else {
                    format!("{}: {message}", path.display())
                });
            }
        }
    }
    if !errors.is_empty() {
        return Err(FormatError(errors.join("\n")));
    }
    let changed = !changes.is_empty();
    for (path, output) in changes {
        if !check {
            replace(&path, &output)?;
        }
        println!("{}", path.display());
    }
    Ok(check && changed)
}

fn prepare(path: &Path) -> Result<Option<String>, FormatError> {
    let source = std::fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let output = format_source(&source, path.to_string_lossy().as_ref().into())?;
    Ok((source != output).then_some(output))
}

fn replace(path: &Path, output: &str) -> Result<(), FormatError> {
    let permissions = std::fs::metadata(path)
        .map_err(|error| io_error(path, error))?
        .permissions();
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
        .map_err(|error| io_error(path, error))?;
    temporary
        .write_all(output.as_bytes())
        .map_err(|error| io_error(path, error))?;
    temporary
        .as_file()
        .set_permissions(permissions)
        .map_err(|error| io_error(path, error))?;
    temporary
        .persist(path)
        .map_err(|error| io_error(path, error.error))?;
    Ok(())
}

fn select_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>, FormatError> {
    let mut files = paths.to_vec();
    if files.is_empty() {
        let cwd = std::env::current_dir().map_err(|error| FormatError(error.to_string()))?;
        let root = cwd
            .ancestors()
            .find(|path| path.join("Dovetail.toml").is_file())
            .ok_or_else(|| {
                FormatError("could not find Dovetail.toml; pass explicit .dove files".into())
            })?;
        for directory in crate::manifest::formatting_directories(root).map_err(FormatError)? {
            for entry in
                std::fs::read_dir(&directory).map_err(|error| io_error(&directory, error))?
            {
                let entry = entry.map_err(|error| io_error(&directory, error))?;
                if entry
                    .file_type()
                    .map_err(|error| io_error(&entry.path(), error))?
                    .is_file()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "dove")
                {
                    files.push(entry.path());
                }
            }
        }
    }
    let mut canonical = vec![];
    for path in files {
        if !path.is_file() || path.extension().is_none_or(|extension| extension != "dove") {
            return Err(FormatError(format!(
                "{}: expected a .dove file",
                path.display()
            )));
        }
        canonical.push(
            path.canonicalize()
                .map_err(|error| io_error(&path, error))?,
        );
    }
    canonical.sort();
    canonical.dedup();
    Ok(canonical)
}

fn io_error(path: &Path, error: std::io::Error) -> FormatError {
    FormatError(format!("{}: {error}", path.display()))
}
