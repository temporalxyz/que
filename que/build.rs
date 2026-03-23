fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_LOOM").is_some() {
        println!("cargo:rustc-cfg=loom");
    }
}
