//! Git transport and immutable workspace-local source checkouts.
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::ManifestError;
use super::dependencies::{ResolveOptions, error};

pub(super) fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(super) fn repository(url: &str) -> Result<String, ManifestError> {
    if url.starts_with('-') || url.contains(['\n', '\r', '\0']) {
        return Err(error("invalid Git repository URL"));
    }
    if url.contains("://") {
        let parsed = tower_lsp::lsp_types::Url::parse(url).map_err(|_| error("invalid Git URL"))?;
        if (matches!(parsed.scheme(), "https" | "http") && !parsed.username().is_empty())
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(error(
                "Git URLs must not contain credentials, queries, or fragments; configure Git authentication separately",
            ));
        }
    }
    // Preserve transport/host distinctions, including SSH aliases.
    Ok(url
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string())
}

pub(super) fn cancelled(options: &ResolveOptions) -> Result<(), ManifestError> {
    if options
        .cancelled
        .as_ref()
        .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
    {
        return Err(error("dependency resolution cancelled"));
    }
    Ok(())
}

pub(super) fn lock(root: &Path, options: &ResolveOptions) -> Result<File, ManifestError> {
    fs::create_dir_all(root).map_err(|e| error(format!("cannot create dependency store: {e}")))?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join("resolver.lock"))
        .map_err(|e| error(format!("cannot open dependency lock: {e}")))?;
    loop {
        cancelled(options)?;
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(error(format!("cannot lock dependency store: {e}"))),
        }
    }
}

fn run(directory: &Path, args: &[&str], options: &ResolveOptions) -> Result<String, ManifestError> {
    cancelled(options)?;
    // Files avoid pipe deadlocks while the parent polls for cancellation.
    let mut stdout = tempfile::tempfile().map_err(|e| error(e.to_string()))?;
    let stderr = tempfile::tempfile().map_err(|e| error(e.to_string()))?;
    let mut command = Command::new(
        options
            .git_executable
            .as_deref()
            .unwrap_or(Path::new("git")),
    );
    command
        .current_dir(directory)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("GCM_INTERACTIVE", "never")
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().map_err(|e| error(e.to_string()))?)
        .stderr(stderr);
    if std::env::var_os("GIT_SSH_COMMAND").is_none() && std::env::var_os("GIT_SSH").is_none() {
        command.env(
            "GIT_SSH_COMMAND",
            "ssh -oBatchMode=yes -oStrictHostKeyChecking=yes",
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Git starts transport and credential-helper subprocesses. Give the
        // operation its own group so cancellation also stops those children.
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| error(format!("cannot run Git: {e}")))?;
    let status = loop {
        if cancelled(options).is_err() {
            terminate_operation(&mut child);
            return Err(error("dependency resolution cancelled"));
        }
        if let Some(status) = child.try_wait().map_err(|e| error(e.to_string()))? {
            break status;
        }
        thread::sleep(Duration::from_millis(25));
    };
    if !status.success() {
        // Git stderr can contain credential-helper output and authenticated URLs.
        return Err(error(format!(
            "Git {} failed; check repository/ref existence and configured Git credentials (HTTPS helper or SSH agent/known hosts)",
            args.first().unwrap_or(&"operation")
        )));
    }
    use std::io::{Seek, SeekFrom};
    stdout
        .seek(SeekFrom::Start(0))
        .map_err(|e| error(e.to_string()))?;
    let mut output = String::new();
    stdout
        .read_to_string(&mut output)
        .map_err(|e| error(e.to_string()))?;
    Ok(output.trim().to_string())
}

fn terminate_operation(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        // The process-group ID is the child PID because spawn used
        // process_group(0). This never addresses our own process group.
        libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn complete_checkout(directory: &Path, commit: &str) -> bool {
    let marker = directory.join(".dovetail-checkout-complete");
    // Neither the checkout root nor its completion marker may redirect through
    // a symlink. A marker only certifies the exact revision we published.
    fs::symlink_metadata(directory).is_ok_and(|m| m.file_type().is_dir())
        && fs::symlink_metadata(&marker).is_ok_and(|m| m.file_type().is_file())
        && fs::read_to_string(marker).is_ok_and(|contents| contents == commit)
}

pub(super) fn checkout(
    store: &Path,
    url: &str,
    selector: &str,
    commit: Option<&str>,
    options: &ResolveOptions,
) -> Result<(String, PathBuf), ManifestError> {
    let repo_dir = store.join(digest(&repository(url)?));
    if let Some(commit) = commit {
        validate_commit(commit)?;
        let dest = repo_dir.join(commit);
        if complete_checkout(&dest, commit) {
            return Ok((commit.to_string(), dest));
        }
    }
    if options.offline {
        return Err(error(format!(
            "dependency {url} ({selector}) is unavailable offline; run dovetail deps fetch"
        )));
    }
    if let Some(progress) = &options.progress {
        progress(&format!("Fetching {url} ({selector})"));
    }
    fs::create_dir_all(&repo_dir).map_err(|e| error(e.to_string()))?;
    let stage = tempfile::Builder::new()
        .prefix(".fetch-")
        .tempdir_in(&repo_dir)
        .map_err(|e| error(e.to_string()))?;
    run(stage.path(), &["init", "--quiet"], options)?;
    run(stage.path(), &["remote", "add", "origin", url], options)?;
    let requested = commit.unwrap_or(selector);
    run(
        stage.path(),
        &[
            "fetch",
            "--quiet",
            "--depth=1",
            "--no-recurse-submodules",
            "origin",
            requested,
        ],
        options,
    )
    .map_err(|e| error(format!("{url} ({requested}): {e}")))?;
    let resolved = run(stage.path(), &["rev-parse", "FETCH_HEAD^{commit}"], options)?;
    validate_commit(&resolved)?;
    if commit.is_some_and(|expected| expected != resolved) {
        return Err(error("Git returned a different commit than the lockfile"));
    }
    let dest = repo_dir.join(&resolved);
    if complete_checkout(&dest, &resolved) {
        return Ok((resolved, dest));
    }
    run(
        stage.path(),
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "checkout",
            "--quiet",
            "--detach",
            &resolved,
        ],
        options,
    )?;
    use std::io::Write;
    let mut marker = File::options()
        .write(true)
        .create_new(true)
        .open(stage.path().join(".dovetail-checkout-complete"))
        .map_err(|e| {
            error(format!(
                "cannot create reserved .dovetail-checkout-complete marker; repositories must not contain this path: {e}"
            ))
        })?;
    marker
        .write_all(resolved.as_bytes())
        .map_err(|e| error(e.to_string()))?;
    cancelled(options)?;
    if dest.exists() {
        return Err(error(format!(
            "incomplete dependency checkout {}; remove it and retry",
            dest.display()
        )));
    }
    protect_sources(stage.path())?;
    fs::rename(stage.path(), &dest).map_err(|e| error(e.to_string()))?;
    Ok((resolved, dest))
}

fn validate_commit(commit: &str) -> Result<(), ManifestError> {
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(error("invalid full Git commit in dependency lock"));
    }
    Ok(())
}

pub(super) fn contained(root: &Path, path: &Path) -> Result<PathBuf, ManifestError> {
    let canonical = path.canonicalize().map_err(|e| {
        error(format!(
            "cannot access {}: {e}; required submodule contents are not fetched automatically",
            path.display()
        ))
    })?;
    let root = root.canonicalize().map_err(|e| error(e.to_string()))?;
    if !canonical.starts_with(&root) {
        return Err(error(format!(
            "dependency path {} escapes {}",
            path.display(),
            root.display()
        )));
    }
    Ok(canonical)
}

/// Protect source files without following symlinks or changing Git's metadata.
fn protect_sources(directory: &Path) -> Result<(), ManifestError> {
    for entry in fs::read_dir(directory).map_err(|e| error(e.to_string()))? {
        let entry = entry.map_err(|e| error(e.to_string()))?;
        if entry.file_name() == ".git" {
            continue;
        }
        let kind = entry.file_type().map_err(|e| error(e.to_string()))?;
        if kind.is_dir() {
            protect_sources(&entry.path())?;
        } else if kind.is_file() {
            let mut permissions = entry
                .metadata()
                .map_err(|e| error(e.to_string()))?
                .permissions();
            permissions.set_readonly(true);
            fs::set_permissions(entry.path(), permissions).map_err(|e| error(e.to_string()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_checkout_requires_matching_marker_contents() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join(".dovetail-checkout-complete");
        fs::write(&marker, "wrong commit").unwrap();
        assert!(!complete_checkout(directory.path(), "expected commit"));
        fs::write(&marker, "expected commit").unwrap();
        assert!(complete_checkout(directory.path(), "expected commit"));
    }

    #[cfg(unix)]
    #[test]
    fn completed_checkout_rejects_symlink_markers_and_roots() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().unwrap();
        let checkout = directory.path().join("checkout");
        fs::create_dir(&checkout).unwrap();
        let outside = directory.path().join("outside");
        fs::write(&outside, "commit").unwrap();
        let marker = checkout.join(".dovetail-checkout-complete");
        symlink(&outside, &marker).unwrap();
        assert!(!complete_checkout(&checkout, "commit"));
        fs::remove_file(&marker).unwrap();
        fs::write(&marker, "commit").unwrap();
        let redirected = directory.path().join("redirected");
        symlink(&checkout, &redirected).unwrap();
        assert!(!complete_checkout(&redirected, "commit"));
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_stops_git_transport_descendants() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        use std::time::Instant;

        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("git-wrapper");
        fs::write(
            &executable,
            "#!/bin/sh\n(printf ready > \"$1\"; sleep 1; printf survived > \"$2\") &\nwait\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let ready = directory.path().join("ready");
        let survived = directory.path().join("survived");
        let flag = Arc::new(AtomicBool::new(false));
        let options = ResolveOptions {
            git_executable: Some(executable),
            cancelled: Some(flag.clone()),
            ..Default::default()
        };
        thread::scope(|scope| {
            scope.spawn(|| {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !ready.exists() && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(10));
                }
                flag.store(true, Ordering::SeqCst);
            });
            let failure = run(
                directory.path(),
                &[ready.to_str().unwrap(), survived.to_str().unwrap()],
                &options,
            )
            .unwrap_err();
            assert!(failure.to_string().contains("cancelled"));
        });
        assert!(ready.exists(), "the fake Git transport must have started");
        thread::sleep(Duration::from_millis(1100));
        assert!(!survived.exists(), "a Git transport survived cancellation");
    }
}
