fn main() {
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|revision| revision.trim().to_owned())
        .filter(|revision| revision.len() == 40)
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=SAYALL_SOURCE_REVISION={revision}");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");
    println!("cargo:rerun-if-changed=../.git/packed-refs");
    println!("cargo:rerun-if-env-changed=SAYALL_BUILD_CHANNEL");
    println!("cargo:rerun-if-env-changed=SAYALL_RELEASE_TAG");
    tauri_build::build()
}
