use std::env;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let srcdir = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("expected Modules/cpython-sys to live under the source tree");
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    let builddir = env::var("PYTHON_BUILD_DIR").ok();
    if gil_disabled(srcdir, builddir.as_deref()) {
        println!("cargo:rustc-cfg=py_gil_disabled");
    }
    let target = env::var("TARGET").unwrap_or_default();
    generate_c_api_bindings(srcdir, builddir.as_deref(), out_path.as_path(), &target);
    // TODO(emmatyping): generate bindings to the internal parser API
    // The parser includes things slightly differently, so we should generate
    // it's bindings independently
    //generate_parser_bindings(srcdir, &out_path.as_path());
}

fn gil_disabled(srcdir: &Path, builddir: Option<&str>) -> bool {
    let mut candidates = Vec::new();
    if let Some(build) = builddir {
        candidates.push(PathBuf::from(build));
    }
    candidates.push(srcdir.to_path_buf());
    for base in candidates {
        let path = base.join("pyconfig.h");
        if let Ok(contents) = std::fs::read_to_string(&path)
            && contents.contains("Py_GIL_DISABLED 1")
        {
            return true;
        }
    }
    false
}

fn generate_c_api_bindings(srcdir: &Path, builddir: Option<&str>, out_path: &Path, target: &str) {
    let mut builder = bindgen::Builder::default().header("wrapper.h");

    let host = env::var("HOST").unwrap_or_default();
    let is_cross_compiling = !target.is_empty() && target != host;

    // For cross-compilation, don't auto-detect host system include paths
    if is_cross_compiling {
        builder = builder.detect_include_paths(false);
    }

    // Always search the source dir and the public headers.
    let mut include_dirs = vec![srcdir.to_path_buf(), srcdir.join("Include")];
    // Include the build directory if provided; out-of-tree builds place
    // the generated pyconfig.h there.
    if let Some(build) = builddir {
        include_dirs.push(PathBuf::from(build));
    }
    for dir in include_dirs {
        builder = builder.clang_arg(format!("-I{}", dir.display()));
    }

    // Set target triple for cross-compilation
    if !target.is_empty() {
        builder = builder.clang_arg(format!("--target={}", target));
    }

    // Handle WASI SDK sysroot and clang includes
    if target.contains("wasi") {
        if let Ok(wasi_sdk) = env::var("WASI_SDK_PATH") {
            let wasi_sdk_path = PathBuf::from(&wasi_sdk);
            let sysroot = wasi_sdk_path.join("share/wasi-sysroot");
            if sysroot.exists() {
                builder = builder.clang_arg(format!("--sysroot={}", sysroot.display()));
            }
            // Add clang's built-in headers (stddef.h, etc.) from WASI SDK
            if let Ok(entries) = std::fs::read_dir(wasi_sdk_path.join("lib/clang")) {
                for entry in entries.flatten() {
                    let clang_include = entry.path().join("include");
                    if clang_include.exists() {
                        builder = builder.clang_arg(format!("-I{}", clang_include.display()));
                        break;
                    }
                }
            }
        }
    }

    let bindings = builder
        .allowlist_function("_?Py.*")
        .allowlist_type("_?Py.*")
        .allowlist_var("_?Py.*")
        .blocklist_type("^PyMethodDef$")
        .blocklist_type("PyObject")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/c_api.rs file.
    bindings
        .write_to_file(out_path.join("c_api.rs"))
        .expect("Couldn't write bindings!");
}
