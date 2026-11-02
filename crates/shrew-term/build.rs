//! Builds libghostty-vt from vendor/ghostty with Zig for the current Rust target
//! and links it statically. Set SHREW_GHOSTTY_VT_DIR to a prefix that already
//! contains lib/libghostty-vt.a to skip the Zig build.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=SHREW_GHOSTTY_VT_DIR");
    println!("cargo:rerun-if-env-changed=ANDROID_NDK_HOME");
    println!("cargo:rerun-if-env-changed=ZIG");

    let target = std::env::var("TARGET").unwrap();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let prefix = match std::env::var_os("SHREW_GHOSTTY_VT_DIR") {
        Some(p) => PathBuf::from(p),
        None => build_with_zig(&target, &out),
    };
    let archive = prefix.join("lib/libghostty-vt.a");
    assert!(archive.exists(), "{} is missing", archive.display());
    // Link from a directory holding only the archive, so the linker can't
    // pick the shared library that Zig installs next to it.
    let lib = out.join("static");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::copy(&archive, lib.join("libghostty-vt.a")).unwrap();
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=ghostty-vt");
    println!("cargo:include={}", prefix.join("include").display());
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    }
}

fn zig_target(rust: &str) -> &'static str {
    match rust {
        "aarch64-apple-darwin" => "aarch64-macos",
        "x86_64-apple-darwin" => "x86_64-macos",
        "aarch64-apple-ios" => "aarch64-ios",
        "aarch64-apple-ios-sim" => "aarch64-ios-simulator",
        "x86_64-apple-ios" => "x86_64-ios-simulator",
        "aarch64-linux-android" => "aarch64-linux-android",
        "x86_64-linux-android" => "x86_64-linux-android",
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "x86_64-unknown-linux-musl" => "x86_64-linux-musl",
        "aarch64-unknown-linux-musl" => "aarch64-linux-musl",
        other => panic!("shrew-term: no Zig target mapping for {other}"),
    }
}

fn build_with_zig(target: &str, out: &Path) -> PathBuf {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../vendor/ghostty");
    let root = root.canonicalize().expect("vendor/ghostty is missing; run `git submodule update --init`");
    println!("cargo:rerun-if-changed={}", root.join("src/terminal").display());
    println!("cargo:rerun-if-changed={}", root.join("include").display());
    let prefix = out.join("ghostty-vt");
    let zig = std::env::var("ZIG").unwrap_or_else(|_| "zig".into());
    let mut cmd = Command::new(zig);
    cmd.current_dir(&root).args([
        "build",
        "-Demit-lib-vt=true",
        "-Demit-xcframework=false",
        "-Doptimize=ReleaseFast",
        &format!("-Dtarget={}", zig_target(target)),
        "--prefix",
    ]);
    cmd.arg(&prefix);
    // Keep Zig's cache next to Cargo's so `cargo clean` clears both.
    cmd.arg("--cache-dir").arg(out.join("zig-cache"));
    if target.contains("android") && std::env::var_os("ANDROID_NDK_HOME").is_none()
        && let Some(ndk) = find_ndk() {
            cmd.env("ANDROID_NDK_HOME", ndk);
        }
    let status = cmd.status().expect("failed to run zig (install Zig 0.17)");
    assert!(status.success(), "zig build of libghostty-vt failed for {target}");
    prefix
}

fn find_ndk() -> Option<PathBuf> {
    let sdk = std::env::var_os("ANDROID_HOME")
        .or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Android/sdk")))?;
    let mut versions: Vec<_> = std::fs::read_dir(sdk.join("ndk")).ok()?.flatten().map(|e| e.path()).collect();
    versions.sort();
    versions.pop()
}
