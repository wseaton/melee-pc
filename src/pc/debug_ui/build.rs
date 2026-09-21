use std::env;
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=DAWN_INCLUDE_DIR");
    let include_dir = PathBuf::from(
        env::var("DAWN_INCLUDE_DIR")
            .map_err(|_| "DAWN_INCLUDE_DIR must point at Dawn's include directory")?,
    );
    let header = include_dir.join("dawn").join("webgpu.h");
    println!("cargo:rerun-if-changed={}", header.display());

    let bindings = bindgen::Builder::default()
        .header(header.to_string_lossy())
        .clang_arg(format!("-I{}", include_dir.display()))
        .allowlist_function("wgpu.*")
        .allowlist_type("WGPU.*")
        .allowlist_var("WGPU.*")
        .prepend_enum_name(false)
        .derive_default(true)
        .layout_tests(false)
        .generate()?;

    let out = PathBuf::from(env::var("OUT_DIR")?).join("webgpu.rs");
    bindings.write_to_file(out)?;
    Ok(())
}
