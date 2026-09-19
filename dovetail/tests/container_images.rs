use std::process::Command;

/// Exercise the real CLI pipeline using the same application fixture as Linux
/// image CI. The native artifact executes on the test host; cross-Linux AOT is
/// covered separately by the image module tests and native Linux CI matrix.
#[test]
fn executes_precompiled_application_without_sources_and_rejects_corruption() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    std::fs::create_dir_all(root.join("app/src")).unwrap();
    std::fs::write(
        root.join("Dovetail.toml"),
        include_str!("fixtures/image/Dovetail.toml"),
    )
    .unwrap();
    std::fs::write(
        root.join("app/src/main.dove"),
        include_str!("fixtures/image/app/src/main.dove"),
    )
    .unwrap();
    let compiler = env!("CARGO_BIN_EXE_dovetail");
    let build = Command::new(compiler)
        .current_dir(root)
        .args(["build", "app"])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let wasm = std::fs::read(root.join("build/app.wasm")).unwrap();
    for platform in [
        dovetail::image::config::Platform::Amd64,
        dovetail::image::config::Platform::Arm64,
    ] {
        let artifact = dovetail::image::runtime::precompile(&wasm, platform).unwrap();
        assert!(matches!(
            wasmtime::Engine::detect_precompiled(&artifact),
            Some(wasmtime::Precompiled::Component)
        ));
    }
    let engine = dovetail::p3::p3_engine().unwrap();
    let compiled = engine.precompile_component(&wasm).unwrap();
    let component = root.join("application.cwasm");
    std::fs::write(&component, &compiled).unwrap();
    let config = dovetail::image::runtime::ExecutionConfig {
        runtime_id: dovetail::image::runtime::RUNTIME_ID.into(),
        component: component.clone(),
        digest: dovetail::image::archive::digest(&compiled),
        wasi: Default::default(),
    };
    let config_path = root.join("execution.json");
    std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    std::fs::remove_dir_all(root.join("app")).unwrap();
    std::fs::remove_file(root.join("Dovetail.toml")).unwrap();
    let run = || {
        Command::new(compiler)
            .current_dir(root)
            .args(["image", "run", "--config"])
            .arg(&config_path)
            .output()
            .unwrap()
    };
    let result = run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::write(component, b"damaged native artifact").unwrap();
    let result = run();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("digest mismatch"));
}
