//! Expose la cible de compilation (`aarch64-apple-darwin`…) au code, pour
//! choisir le bon binaire de connecteur dans le registre.

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=DBPHOQUE_TARGET={target}");
    println!("cargo:rerun-if-changed=build.rs");
}
