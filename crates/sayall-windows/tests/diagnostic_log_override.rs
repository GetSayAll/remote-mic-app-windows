//! 回归：用户环境里遗留的 `SAYALL_GATT_LOG` 指向产品根目录（Logs 的上一层）
//! 时，启动必须把日志归一到 `Logs`，否则「打开日志目录」和本次运行日志都会
//! 落在上一层（2026-11-21 现场）。

use sayall_windows::{
    diagnostic_log_directory, gatt_note, initialize_diagnostic_log, DiagnosticLogMetadata,
};

#[test]
fn root_level_override_is_written_inside_logs() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "sayall-root-override-test-{}-{unique}",
        std::process::id()
    ));
    let product = root.join("SayAll");
    let logs = product.join("Logs");
    let default_path = logs.join("sayall-diagnostic.log");
    let override_path = product.join("login-diagnostic-2.log");
    let _ = std::fs::remove_dir_all(&root);

    // SAFETY: 本集成测试文件只有一个用例，进程内没有并发读取该环境变量。
    unsafe { std::env::set_var("SAYALL_GATT_LOG", &override_path) };
    assert!(initialize_diagnostic_log(
        default_path.clone(),
        DiagnosticLogMetadata {
            app_version: "test".to_owned(),
            app_build: "test".to_owned(),
            source_revision: "test".to_owned(),
            build_channel: "local".to_owned(),
            release_tag: "none".to_owned(),
        },
    ));
    gatt_note("diagnostic_override_test phase=completed terminal_result=passed".to_owned());

    let normalized = logs.join("login-diagnostic-2.log");
    let contents = std::fs::read_to_string(&normalized)
        .expect("root-level override should be normalized into Logs");
    assert!(contents.contains("diagnostic_override_test"));
    assert!(
        !override_path.exists(),
        "root-level file must not be created"
    );
    assert_eq!(diagnostic_log_directory().as_deref(), Some(logs.as_path()));

    unsafe { std::env::remove_var("SAYALL_GATT_LOG") };
    let _ = std::fs::remove_dir_all(&root);
}
