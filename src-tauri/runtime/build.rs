#[path = "../asr_rpath.rs"]
mod asr_rpath;

use asr_rpath::ExecutableLocation;

/// The app version in `package.json`, which the runtime announces to paired
/// clients so they compare it with their own app version.
fn app_version() -> String {
    let manifest = std::fs::read_to_string("../../package.json").expect("package.json is readable");
    manifest
        .split_once("\"version\"")
        .and_then(|(_, rest)| rest.split('"').nth(1))
        .expect("package.json names a version")
        .to_owned()
}

fn main() {
    println!("cargo:rerun-if-changed=../../package.json");
    println!("cargo:rustc-env=MUNIMENT_APP_VERSION={}", app_version());
    let os = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS is set by Cargo");
    if let Some(link_arg) = asr_rpath::link_arg(&os, ExecutableLocation::Runtime) {
        println!("cargo:rustc-link-arg={link_arg}");
    }
}
