use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let rust_atom_release = manifest_dir.join("../rust_atom/target/release");

    println!("cargo:rustc-link-search=native={}", rust_atom_release.display());
    println!("cargo:rustc-link-lib=static=rust_atom");
    println!("cargo:rustc-link-lib=xml2");

    #[cfg(target_os = "linux")]
    {
        println!("cargo:rustc-link-lib=dl");
        println!("cargo:rustc-link-lib=pthread");
        println!("cargo:rustc-link-lib=gcc_s");
        println!("cargo:rustc-link-lib=m");
    }
}
