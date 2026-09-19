use super::*;

#[test]
fn image_defaults_are_precompiled_multi_platform() {
    let config: config::ImageConfig = toml::from_str("name = 'ghcr.io/acme/api'").unwrap();
    assert_eq!(config.platforms, [Platform::Amd64, Platform::Arm64]);
    assert!(toml::from_str::<config::ImageConfig>("name = 'a'\nprecompile = false").is_err());
}

fn project() -> config::ImageProject {
    config::ImageProject {
        directory: std::path::PathBuf::new(),
        name: "api".into(),
        config: config::ImageConfig {
            name: "ghcr.io/acme/api".into(),
            ..Default::default()
        },
    }
}
fn fixture_archive(root: &Path, output: &Path) {
    let project = project();
    let manifests = [Platform::Amd64, Platform::Arm64].into_iter().map(|platform| {
        let config = json!({"architecture": platform.architecture(), "os": "linux", "config": {"Env": ["BASE=preserved", "PORT=80"]}, "rootfs": {"type": "layers", "diff_ids": []}});
        assemble(root, &project, platform, b"test runtime", platform.architecture().as_bytes(), config, vec![]).unwrap()
    }).collect();
    archive::export(root, &archive::index(manifests), output).unwrap();
}

#[test]
fn archive_roundtrip_preserves_platforms_layers_and_execution_settings() {
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("api.oci.tar");
    fixture_archive(temporary.path(), &output);
    let image = archive::import(&output).unwrap();
    let descriptors = archive::children(&image.index).unwrap();
    assert_eq!(descriptors.len(), 2);
    for (descriptor, architecture) in descriptors.iter().zip(["amd64", "arm64"]) {
        assert_eq!(
            descriptor.platform.as_ref().unwrap()["architecture"],
            architecture
        );
        let manifest: Value =
            serde_json::from_slice(&archive::read(image.directory.path(), descriptor).unwrap())
                .unwrap();
        let children = archive::children(&manifest).unwrap();
        let config: Value =
            serde_json::from_slice(&archive::read(image.directory.path(), &children[0]).unwrap())
                .unwrap();
        assert_eq!(config["config"]["Entrypoint"][1], "image");
        assert_eq!(config["config"]["User"], "65532:65532");
        assert_eq!(config["rootfs"]["diff_ids"].as_array().unwrap().len(), 2);
        let layer = archive::read(image.directory.path(), &children[2]).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(layer.as_slice());
        let mut tar_bytes = vec![];
        std::io::Read::read_to_end(&mut decoder, &mut tar_bytes).unwrap();
        assert_eq!(archive::digest(&tar_bytes), config["rootfs"]["diff_ids"][1]);
        let mut tar = tar::Archive::new(tar_bytes.as_slice());
        let paths: Vec<_> = tar
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap().path().unwrap().into_owned())
            .collect();
        assert!(paths.contains(&std::path::PathBuf::from("app/application.cwasm")));
    }
    assert_eq!(descriptors[0].platform.as_ref().unwrap()["os"], "linux");
}

#[test]
fn reproducible_archives_and_corruption_detection() {
    let temporary = tempfile::tempdir().unwrap();
    let first = temporary.path().join("first.tar");
    let second = temporary.path().join("second.tar");
    fixture_archive(temporary.path(), &first);
    fixture_archive(temporary.path(), &second);
    assert_eq!(
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap()
    );
    let image = archive::import(&first).unwrap();
    let descriptor = archive::children(&image.index).unwrap().remove(0);
    std::fs::write(
        archive::blob_path(image.directory.path(), &descriptor.digest).unwrap(),
        b"corrupt",
    )
    .unwrap();
    assert!(archive::validate(image.directory.path(), &image.index).is_err());
    assert!(archive::blob_path(temporary.path(), "sha256:../../escape").is_err());
}

#[test]
fn rejects_links_and_duplicate_archive_entries() {
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("bad.tar");
    let mut tar = tar::Builder::new(std::fs::File::create(&output).unwrap());
    archive::append(&mut tar, "index.json", b"{}", 0o644).unwrap();
    archive::append(&mut tar, "index.json", b"{}", 0o644).unwrap();
    tar.finish().unwrap();
    assert!(
        archive::import(&output)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    let mut tar = tar::Builder::new(std::fs::File::create(&output).unwrap());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_cksum();
    tar.append_link(&mut header, "index.json", "/etc/passwd")
        .unwrap();
    tar.finish().unwrap();
    assert!(
        archive::import(&output)
            .unwrap_err()
            .to_string()
            .contains("regular files")
    );
}

#[test]
fn cross_precompilation_produces_distinct_native_artifacts() {
    let wasm = wat::parse_str("(component (core module (func (export \"f\"))))").unwrap();
    let amd64 = runtime::precompile(&wasm, Platform::Amd64).unwrap();
    let arm64 = runtime::precompile(&wasm, Platform::Arm64).unwrap();
    assert_ne!(amd64, arm64);
    assert_ne!(&amd64[..4], b"\0asm");
    assert!(matches!(
        wasmtime::Engine::detect_precompiled(&amd64),
        Some(wasmtime::Precompiled::Component)
    ));
    assert!(matches!(
        wasmtime::Engine::detect_precompiled(&arm64),
        Some(wasmtime::Precompiled::Component)
    ));
    assert!(runtime::precompile(b"invalid wasm", Platform::Amd64).is_err());
}

#[test]
fn checks_linux_runtime_architecture_and_build_identity() {
    let mut binary = vec![0; 20];
    binary[..6].copy_from_slice(b"\x7fELF\x02\x01");
    binary[18] = 62;
    assert!(runtime::validate_binary(&binary, Platform::Amd64).is_err());
    binary.extend_from_slice(runtime::RUNTIME_ID.as_bytes());
    runtime::validate_binary(&binary, Platform::Amd64).unwrap();
    assert!(runtime::validate_binary(&binary, Platform::Arm64).is_err());
}

#[test]
fn manifest_roundtrip_retains_image_configuration() {
    let temporary = tempfile::tempdir().unwrap();
    crate::manifest::init_workspace(temporary.path(), "api").unwrap();
    let path = temporary.path().join("Dovetail.toml");
    let mut manifest = std::fs::read_to_string(&path).unwrap();
    manifest
        .push_str("\n[project.image]\nname = 'ghcr.io/acme/api'\noutput = 'dist/api.oci.tar'\n");
    std::fs::write(&path, manifest).unwrap();
    crate::manifest::add_project(temporary.path(), "library").unwrap();
    let projects = crate::manifest::image_projects(temporary.path()).unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(
        projects[0].archive(temporary.path()),
        temporary.path().join("dist/api.oci.tar")
    );
    assert_eq!(
        projects[0]
            .reference(Some("revision-123"))
            .unwrap()
            .to_string(),
        "ghcr.io/acme/api:revision-123"
    );
    assert!(projects[0].reference(Some("other/repo:tag")).is_err());
}

#[test]
fn push_preflights_all_archives_before_authentication() {
    let temporary = tempfile::tempdir().unwrap();
    let first = project();
    fixture_archive(temporary.path(), &first.archive(temporary.path()));
    let second = config::ImageProject {
        directory: std::path::PathBuf::new(),
        name: "missing".into(),
        config: config::ImageConfig {
            name: "ghcr.io/acme/missing".into(),
            ..first.config.clone()
        },
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let error = runtime
        .block_on(push(temporary.path(), &[first, second], None))
        .unwrap_err();
    assert!(error.to_string().contains("run dovetail image build first"));
}

#[test]
fn base_lock_and_offline_modes_do_not_silently_resolve_tags() {
    let temporary = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut pins = registry::BasePins::new();
    let error = runtime
        .block_on(registry::base(
            temporary.path(),
            "example.invalid/base:latest",
            Platform::Amd64,
            &mut pins,
            true,
            true,
        ))
        .unwrap_err();
    assert!(error.to_string().contains("Dovetail.images.lock"));
    let (base, layers) = runtime
        .block_on(registry::base(
            temporary.path(),
            "scratch",
            Platform::Arm64,
            &mut pins,
            true,
            true,
        ))
        .unwrap();
    assert_eq!(base["architecture"], "arm64");
    assert!(layers.is_empty());
}

#[test]
fn docker_inline_credentials_and_docker_hub_names() {
    let reference = "ghcr.io/acme/api:latest".parse().unwrap();
    let config = br#"{"auths":{"ghcr.io":{"auth":"dXNlcjp0b2tlbg=="}},"credsStore":""}"#;
    assert_eq!(
        auth::from_config(&reference, config).unwrap(),
        Some(docker_credential::DockerCredential::UsernamePassword(
            "user".into(),
            "token".into()
        ))
    );
    assert!(
        auth::from_config(&reference, br#"{"auths":{}}"#)
            .unwrap()
            .is_none()
    );
    let reference = "alpine:latest".parse().unwrap();
    assert_eq!(
        auth::credential_server(&reference),
        "https://index.docker.io/v1/"
    );
    let config = br#"{"auths":{"https://index.docker.io/v1/":{"identitytoken":"refresh-token"}}}"#;
    assert_eq!(
        auth::from_config(&reference, config).unwrap(),
        Some(docker_credential::DockerCredential::IdentityToken(
            "refresh-token".into()
        ))
    );
}

#[test]
fn docker_helper_precedence_and_errors_do_not_expose_inline_secrets() {
    let reference = "ghcr.io/acme/api:latest".parse().unwrap();
    let config = br#"{"credsStore":"dovetail-nonexistent-test-helper","auths":{"ghcr.io":{"auth":"dXNlcjp0b2tlbg=="}}}"#;
    let error = auth::from_config(&reference, config)
        .unwrap_err()
        .to_string();
    assert!(error.contains("credential helper"));
    assert!(!error.contains("token"));
}

#[tokio::test]
async fn registry_push_uploads_both_manifests_before_the_tagged_index() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let temporary = tempfile::tempdir().unwrap();
    let output = temporary.path().join("api.oci.tar");
    fixture_archive(temporary.path(), &output);
    let imported = archive::import(&output).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, Vec<u8>)>::new()));
    let captured = requests.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            let header_end = loop {
                let mut buffer = [0; 4096];
                let length = socket.read(&mut buffer).await.unwrap();
                if length == 0 {
                    return;
                }
                bytes.extend_from_slice(&buffer[..length]);
                if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                    break end + 4;
                }
            };
            let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
            let request = header.lines().next().unwrap().to_string();
            let length: usize = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse().unwrap())
                })
                .unwrap_or(0);
            while bytes.len() < header_end + length {
                let mut buffer = [0; 8192];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
            }
            captured
                .lock()
                .unwrap()
                .push((request.clone(), bytes[header_end..].to_vec()));
            let status = if request.starts_with("HEAD ") {
                "404 Not Found"
            } else if request.starts_with("POST ") {
                "202 Accepted"
            } else if request.starts_with("PUT ") {
                "201 Created"
            } else {
                "200 OK"
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: 0\r\nLocation: http://{address}/upload\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let client = oci_client::Client::new(oci_client::client::ClientConfig {
        protocol: oci_client::client::ClientProtocol::Http,
        use_monolithic_push: true,
        ..Default::default()
    });
    let reference = format!("{address}/acme/api:test").parse().unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        registry::push_with_client(
            &client,
            &imported,
            &reference,
            oci_client::secrets::RegistryAuth::Anonymous,
        ),
    )
    .await;
    server.abort();
    let digest = result.unwrap().unwrap();
    let requests = requests.lock().unwrap();
    let manifests: Vec<_> = requests
        .iter()
        .filter(|(request, _)| request.starts_with("PUT /v2/acme/api/manifests/"))
        .collect();
    assert_eq!(manifests.len(), 3);
    assert!(manifests[0].0.contains("sha256:"));
    assert!(manifests[1].0.contains("sha256:"));
    assert!(manifests[2].0.contains("/manifests/test "));
    assert_eq!(archive::digest(&manifests[2].1), digest);
    let index: Value = serde_json::from_slice(&manifests[2].1).unwrap();
    assert_eq!(index["manifests"].as_array().unwrap().len(), 2);
}

#[test]
fn includes_project_files_and_rejects_generated_file_overrides() {
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir(temporary.path().join("public")).unwrap();
    std::fs::write(temporary.path().join("public/index.html"), b"hello").unwrap();
    let mut project = project();
    project.directory = temporary.path().to_owned();
    project.config.files.push(config::ImageFile {
        source: "public".into(),
        destination: "/app/public".into(),
    });
    let (descriptor, _) = files::layer(temporary.path(), &project).unwrap().unwrap();
    let compressed = archive::read(temporary.path(), &descriptor).unwrap();
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(compressed.as_slice()));
    let paths: Vec<_> = tar
        .entries()
        .unwrap()
        .map(|entry| entry.unwrap().path().unwrap().into_owned())
        .collect();
    assert!(paths.contains(&std::path::PathBuf::from("app/public/index.html")));
    for destination in [
        "/app/./application.cwasm",
        "/usr/local/bin/dovetail",
        "/app/.wh.application.cwasm",
        "/app/../etc",
    ] {
        project.config.files[0].destination = destination.into();
        assert!(project.validate().is_err(), "{destination}");
    }
}
