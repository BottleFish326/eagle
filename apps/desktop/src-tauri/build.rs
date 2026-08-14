use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=EAGLE_GIT_COMMIT");
    println!("cargo:rerun-if-changed=../../../.git/HEAD");

    let git_commit = git_commit();
    let build_target = environment("TARGET");
    let build_profile = environment("PROFILE");
    let rustc_version = rustc_version();
    emit("EAGLE_GIT_COMMIT", &git_commit);
    emit("EAGLE_BUILD_TARGET", &build_target);
    emit("EAGLE_BUILD_PROFILE", &build_profile);
    emit("EAGLE_RUSTC_VERSION", &rustc_version);

    tauri_build::build();
}

fn emit(name: &str, value: &str) {
    println!("cargo:rustc-env={name}={value}");
}

fn environment(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| "unknown".into())
}

fn git_commit() -> String {
    if let Ok(commit) = std::env::var("EAGLE_GIT_COMMIT") {
        return commit;
    }
    command_output("git", &["rev-parse", "--short=12", "HEAD"])
}

fn rustc_version() -> String {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    command_output(&rustc, &["--version"])
}

fn command_output(program: &str, arguments: &[&str]) -> String {
    Command::new(program)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|output| output.trim().to_owned())
        .filter(|output| !output.is_empty())
        .unwrap_or_else(|| "unknown".into())
}
