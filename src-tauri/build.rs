fn main() {
    // The compose step knows which public-host commit it copied. Prefer that
    // value: in the injected build tree the surrounding `.git` belongs to the
    // *private* repository, so `git rev-parse` here reports the wrong commit and
    // every diagnostic line names a revision nobody can find in the public repo.
    let revision = std::env::var("SAYALL_SOURCE_REVISION")
        .ok()
        .map(|revision| revision.trim().to_owned())
        .filter(|revision| revision.len() == 40 && revision.chars().all(|c| c.is_ascii_hexdigit()))
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|revision| revision.trim().to_owned())
                .filter(|revision| revision.len() == 40)
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=SAYALL_SOURCE_REVISION={revision}");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs");
    println!("cargo:rerun-if-changed=../.git/packed-refs");
    println!("cargo:rerun-if-env-changed=SAYALL_SOURCE_REVISION");
    println!("cargo:rerun-if-env-changed=SAYALL_BUILD_CHANNEL");
    println!("cargo:rerun-if-env-changed=SAYALL_RELEASE_TAG");
    tauri_build::build()
}
