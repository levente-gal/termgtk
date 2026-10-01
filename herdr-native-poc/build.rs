use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

const PIN: &str = "b40acce58dcf77df52231c3798ea58e924647c89";
const ZIG_REQUIRED: &str = "0.16.0";
const ZIG_TARBALL_BIN: &str = "/tmp/zig-x86_64-linux-0.16.0/zig";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=vendor/ghostty/PIN.txt");

    // Default `cargo test` must stay green without linking Ghostty.
    if env::var_os("CARGO_FEATURE_GHOSTTY").is_none() {
        return;
    }

    println!("cargo:rerun-if-env-changed=ZIG");
    println!("cargo:rerun-if-env-changed=ZIG_GLOBAL_CACHE_DIR");

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let vendor = manifest_dir.join("vendor/ghostty");
    let prefix = out_dir.join("ghostty-vt");
    let lib = prefix.join("lib/libghostty-vt.a");

    let zig = find_zig();
    let zig_global = if Path::new("/tmp/zig-global-cache").is_dir() {
        PathBuf::from("/tmp/zig-global-cache")
    } else {
        out_dir.join("zig-global-cache")
    };
    let zig_local = out_dir.join("zig-local-cache");
    std::fs::create_dir_all(&zig_global).ok();
    std::fs::create_dir_all(&zig_local).ok();

    if !lib.is_file() {
        let status = Command::new(&zig)
            .current_dir(&vendor)
            .env("ZIG_GLOBAL_CACHE_DIR", &zig_global)
            .env("ZIG_LOCAL_CACHE_DIR", &zig_local)
            .args([
                "build",
                "-Demit-lib-vt",
                "-Doptimize=ReleaseFast",
                "--prefix",
            ])
            .arg(&prefix)
            .status();

        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                panic!(
                    "zig build of libghostty-vt failed (exit {s}). \
                     Need Zig {ZIG_REQUIRED}, Ghostty pin {PIN}, \
                     and a writable cache (ZIG_GLOBAL_CACHE_DIR). \
                     On Fedora: `dnf install zig` (0.16.0)."
                );
            }
            Err(err) => {
                panic!(
                    "failed to run zig ({zig:?}): {err}. \
                     Need Zig {ZIG_REQUIRED} to build Ghostty pin {PIN}. \
                     On Fedora: `dnf install zig` (0.16.0), or set ZIG to \
                     the 0.16.0 binary (e.g. {ZIG_TARBALL_BIN})."
                );
            }
        }
    }

    if !lib.is_file() {
        panic!(
            "libghostty-vt.a missing after zig build at {}. \
             Need Zig {ZIG_REQUIRED} and Ghostty pin {PIN}. \
             On Fedora: `dnf install zig`.",
            lib.display()
        );
    }

    println!(
        "cargo:rustc-link-search=native={}",
        prefix.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=ghostty-vt");
    println!("cargo:rustc-link-lib=m");
    println!("cargo:rustc-link-lib=rt");
}

fn find_zig() -> PathBuf {
    if let Ok(path) = env::var("ZIG") {
        let p = PathBuf::from(path);
        if p.is_file() {
            verify_zig_version(&p);
            return p;
        }
        panic!(
            "ZIG={p:?} is not a file. Need Zig {ZIG_REQUIRED} for Ghostty pin {PIN}. \
             On Fedora: `dnf install zig`."
        );
    }

    for candidate in [ZIG_TARBALL_BIN, "zig"] {
        let p = PathBuf::from(candidate);
        let resolved = if p.is_file() {
            p
        } else if candidate == "zig" {
            which_zig().unwrap_or_else(|| PathBuf::from("zig"))
        } else {
            continue;
        };
        if resolved == Path::new("zig") || resolved.is_file() {
            if let Ok(ver) = zig_version(&resolved) {
                if ver.starts_with(ZIG_REQUIRED) {
                    return resolved;
                }
            }
        }
    }

    panic!(
        "Zig {ZIG_REQUIRED} not found (looked for ZIG, {ZIG_TARBALL_BIN}, and PATH). \
         Required to build Ghostty pin {PIN}. On Fedora: `dnf install zig`."
    );
}

fn which_zig() -> Option<PathBuf> {
    Command::new("which")
        .arg("zig")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(PathBuf::from(s))
            }
        })
}

fn zig_version(zig: &Path) -> Result<String, String> {
    let out = Command::new(zig)
        .arg("version")
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("zig version failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn verify_zig_version(zig: &Path) {
    match zig_version(zig) {
        Ok(ver) if ver.starts_with(ZIG_REQUIRED) => {}
        Ok(ver) => panic!(
            "Zig at {zig:?} reports {ver}, need {ZIG_REQUIRED} for Ghostty pin {PIN}. \
             On Fedora: `dnf install zig`."
        ),
        Err(err) => {
            panic!("cannot run `{zig:?} version`: {err}. Need Zig {ZIG_REQUIRED} for pin {PIN}.")
        }
    }
}
