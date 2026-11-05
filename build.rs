use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=GIT_COMMIT_HASH");
    println!("cargo:rerun-if-changed=.git/HEAD");

    let commit_hash = env::var("GIT_COMMIT_HASH")
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        });

    println!("cargo:rustc-env=GIT_COMMIT_HASH={}", commit_hash);
}
