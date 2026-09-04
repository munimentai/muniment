#[path = "../asr_rpath.rs"]
mod asr_rpath;

use asr_rpath::ExecutableLocation;

fn main() {
    let os = std::env::var("CARGO_CFG_TARGET_OS").expect("target OS is set by Cargo");
    if let Some(link_arg) = asr_rpath::link_arg(&os, ExecutableLocation::Runtime) {
        println!("cargo:rustc-link-arg={link_arg}");
    }
}
