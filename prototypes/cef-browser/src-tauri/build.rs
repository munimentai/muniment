fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
        let target = out.ancestors().nth(3).unwrap();
        let library = target.join("libmuniment_cef_keychain.dylib");
        let status = cc::Build::new()
            .get_compiler()
            .to_command()
            .args([
                "-dynamiclib",
                "-compatibility_version",
                "1.0",
                "-current_version",
                "1.0",
                "-Werror",
                "-Wno-deprecated-declarations",
                "-framework",
                "CoreFoundation",
                "-Wl,-reexport_framework,Security",
                "-Wl,-install_name,@rpath/libmuniment_cef_keychain.dylib",
                "src/keychain_macos.c",
                "-o",
            ])
            .arg(&library)
            .status()
            .expect("Compile Keychain bridge");
        assert!(status.success(), "Keychain bridge build failed");
        println!("cargo:rustc-link-search=native={}", target.display());
        println!("cargo:rustc-link-lib=dylib=muniment_cef_keychain");
        println!("cargo:rerun-if-changed=src/keychain_macos.c");
    }
    tauri_build::build()
}
