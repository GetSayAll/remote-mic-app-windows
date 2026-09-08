use sayall_windows::{gatt_note, initialize_diagnostic_log, DiagnosticLogMetadata};

#[test]
fn production_default_log_path_writes_structured_metadata_without_environment_override() {
    let path = std::env::temp_dir().join(format!(
        "sayall-production-log-test-{}.log",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    // SAFETY: 集成测试拥有独立进程，只在初始化 OnceLock 前修改本进程环境。
    unsafe { std::env::remove_var("SAYALL_GATT_LOG") };
    assert!(initialize_diagnostic_log(
        path.clone(),
        DiagnosticLogMetadata {
            app_version: "0.2.2-test".to_owned(),
            app_build: "42".to_owned(),
            source_revision: "0123456789012345678901234567890123456789".to_owned(),
            build_channel: "local".to_owned(),
            release_tag: "none".to_owned(),
        },
    ));
    gatt_note("diagnostic_test phase=completed terminal_result=passed".to_owned());

    let contents = std::fs::read_to_string(&path).expect("default diagnostic log was not written");
    let _ = std::fs::remove_file(path);
    assert!(
        contents.starts_with("20"),
        "missing UTC timestamp: {contents}"
    );
    assert!(contents.contains("ver=0.2.2-test build=42"));
    assert!(contents.contains("build_channel=local release_tag=none"));
    assert!(contents.contains("diagnostic_test phase=completed terminal_result=passed"));
}
