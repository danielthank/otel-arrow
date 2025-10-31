use std::env;
use std::path::PathBuf;

fn main() {
    // Path to OpenZL installation
    let openzl_root = env::var("OPENZL_ROOT")
        .unwrap_or_else(|_| "/usr/local".to_string());

    let openzl_include = format!("{}/include", openzl_root);

    // Check for lib64 or lib directory (varies by system)
    let openzl_lib = if std::path::Path::new(&format!("{}/lib64", openzl_root)).exists() {
        format!("{}/lib64", openzl_root)
    } else {
        format!("{}/lib", openzl_root)
    };

    // Path to OTAP parser library (in the same repository)
    let otap_parser_dir = "../../cpp/openzl-arrow-parser";
    let otap_parser_include = format!("{}", otap_parser_dir);
    let otap_parser_lib = format!("{}/build", otap_parser_dir);

    println!("cargo:rerun-if-changed=wrapper.h");
    println!("cargo:rerun-if-env-changed=OPENZL_ROOT");
    println!("cargo:rerun-if-changed={}/otap_parser.h", otap_parser_dir);
    println!("cargo:rerun-if-changed={}/libotap_parser.a", otap_parser_lib);

    // Link to OpenZL library (static)
    println!("cargo:rustc-link-search=native={}", openzl_lib);
    println!("cargo:rustc-link-lib=static=openzl");

    // Link to OTAP parser library (static)
    println!("cargo:rustc-link-search=native={}", otap_parser_lib);
    println!("cargo:rustc-link-lib=static=otap_parser");

    // Zstd is bundled with OpenZL in the same directory
    println!("cargo:rustc-link-lib=static=zstd");

    // Link C++ standard library (OpenZL uses C++)
    println!("cargo:rustc-link-lib=dylib=stdc++");

    // Generate bindings
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", openzl_include))
        .clang_arg(format!("-I{}", otap_parser_include))
        .clang_arg("-xc++")  // Parse as C++ to handle extern "C" blocks
        .clang_arg("-std=c++17")  // Use C++17 standard
        // Core types and errors
        .allowlist_type("ZL_.*")
        .allowlist_function("ZL_.*")
        .allowlist_var("ZL_.*")
        // OTAP parser functions
        .allowlist_function("otap_.*")
        // Generate simplified bindings
        .derive_debug(true)
        .derive_default(true)
        // Wrap extern blocks in unsafe for Rust 2024
        .wrap_unsafe_ops(true)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
