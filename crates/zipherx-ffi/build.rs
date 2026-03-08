fn main() {
    uniffi::generate_scaffolding("src/zipherx.udl").unwrap();

    // reqwest pulls in system-configuration on Apple, which needs
    // the SystemConfiguration framework at link time.
    // In build.rs, #[cfg] checks the HOST, not the TARGET.
    // Use CARGO_CFG_TARGET_OS to check the actual build target.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" || target_os == "ios" {
        println!("cargo:rustc-link-lib=framework=SystemConfiguration");
    }
}
