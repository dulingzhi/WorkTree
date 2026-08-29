fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // `rust_i18n::i18n!` reads `locales/` at macro-expansion time; without
    // this, editing a catalog alone would not rebuild the crate.
    println!("cargo:rerun-if-changed=locales");
}
