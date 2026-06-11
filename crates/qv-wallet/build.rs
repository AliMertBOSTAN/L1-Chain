//! Build script — release binary'sine git commit hash'i ve build
//! timestamp'ini gömer. `qv-wallet --version` çıktısı bu damgayı
//! gösterir, böylece bir kullanıcı hangi tag/revizyondan derlendiği
//! binary'yi taşıdığını söyleyebilir.
//!
//! Sıralama:
//!  1. `QV_BUILD_GIT_HASH` env var set'liyse (CI), onu kullan.
//!  2. Aksi halde `git rev-parse --short=12 HEAD` çağır.
//!  3. Hiçbiri çalışmazsa "unknown" damgala (offline tarball build).

use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=QV_BUILD_GIT_HASH");
    println!("cargo:rerun-if-env-changed=QV_BUILD_TAG");
    // git index değiştiğinde script'i yeniden çalıştır.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");

    let git_hash = std::env::var("QV_BUILD_GIT_HASH")
        .ok()
        .map(|s| s.chars().take(12).collect::<String>())
        .or_else(git_hash_from_command)
        .unwrap_or_else(|| "unknown".to_string());

    let tag = std::env::var("QV_BUILD_TAG").unwrap_or_else(|_| "dev".to_string());

    let built_at = format_timestamp();

    println!("cargo:rustc-env=QV_GIT_HASH={git_hash}");
    println!("cargo:rustc-env=QV_BUILD_TAG={tag}");
    println!("cargo:rustc-env=QV_BUILT_AT={built_at}");
}

fn git_hash_from_command() -> Option<String> {
    let out = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn format_timestamp() -> String {
    // build script'te chrono pulling sebepsiz; UNIX timestamp ile yetiniyoruz.
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("unix:{}", d.as_secs()))
        .unwrap_or_else(|_| "unknown".to_string())
}
