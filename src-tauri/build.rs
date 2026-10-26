use std::env;
use std::path::PathBuf;

fn main() {
    tauri_build::build();
    if env::var_os("CARGO_FEATURE_NEEDLE").is_some() {
        link_needle();
    }
}

/// Links the official prebuilt Needle engine (`libneedle.a`), fetched by
/// `bun run fetch:needle` into `vendor/needle/<platform>/`, or taken from
/// `NEEDLE_LIB_DIR`.
fn link_needle() {
    println!("cargo:rerun-if-env-changed=NEEDLE_LIB_DIR");
    println!("cargo:rerun-if-env-changed=NEEDLE_CXX_STATIC");
    println!("cargo:rerun-if-env-changed=LIBCXX_DIR");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let platform = match (os.as_str(), arch.as_str()) {
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-arm64",
        ("macos", "aarch64") => "macos-arm64",
        ("windows", "x86_64") => "windows-x86_64",
        ("windows", "aarch64") => "windows-arm64",
        _ => panic!(
            "Needle publishes no prebuilt engine for {os}/{arch}. \
             Build with `--no-default-features` to disable speech recognition."
        ),
    };
    let dir = env::var_os("NEEDLE_LIB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
                .join("vendor/needle")
                .join(platform)
        });
    let lib = dir.join("libneedle.a");
    println!("cargo:rerun-if-changed={}", lib.display());
    if !lib.exists() {
        panic!(
            "Needle engine not found at {}.\n\
             Run `bun run fetch:needle` (downloads and verifies the official \
             prebuilt engine), set NEEDLE_LIB_DIR, or build with \
             `--no-default-features`.",
            lib.display()
        );
    }
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=static=needle");

    // libneedle is built against LLVM libc++.
    match os.as_str() {
        "linux" => {
            // Static by default, so the app has no runtime libc++ dependency.
            // Distributions keep libc++.a in different places (Arch: /usr/lib,
            // Debian/Ubuntu: /usr/lib/llvm-N/lib), so ask the compiler.
            let static_dirs = (env::var("NEEDLE_CXX_STATIC").as_deref() != Ok("0"))
                .then(|| Some((find_static("libc++.a")?, find_static("libc++abi.a")?)))
                .flatten();
            match static_dirs {
                Some((cxx, cxxabi)) => {
                    println!("cargo:rustc-link-search=native={}", cxx.display());
                    println!("cargo:rustc-link-search=native={}", cxxabi.display());
                    println!("cargo:rustc-link-lib=static=c++");
                    println!("cargo:rustc-link-lib=static=c++abi");
                }
                None => {
                    println!(
                        "cargo:warning=static libc++ not found; linking libc++ dynamically \
                         (the app will need libc++ at runtime)"
                    );
                    println!("cargo:rustc-link-lib=dylib=c++");
                    println!("cargo:rustc-link-lib=dylib=c++abi");
                }
            }
        }
        "macos" => println!("cargo:rustc-link-lib=dylib=c++"),
        // The Windows archives are MinGW (Itanium ABI) objects and need an
        // LLVM-MinGW toolchain (`*-pc-windows-gnullvm`). Not yet verified.
        "windows" => println!("cargo:rustc-link-lib=dylib=c++"),
        _ => {}
    }
}

/// Directory containing `name`, as reported by the C++ compiler driver.
fn find_static(name: &str) -> Option<PathBuf> {
    if let Some(dir) = env::var_os("LIBCXX_DIR") {
        let dir = PathBuf::from(dir);
        return dir.join(name).exists().then_some(dir);
    }
    for cc in ["clang++", "c++", "g++"] {
        let Ok(out) = std::process::Command::new(cc)
            .arg(format!("-print-file-name={name}"))
            .output()
        else {
            continue;
        };
        let path = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        // Without a match, the driver echoes the bare name back.
        if path.is_absolute() && path.exists() {
            return path.canonicalize().ok()?.parent().map(PathBuf::from);
        }
    }
    None
}
