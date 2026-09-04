pub const ASR_RUNTIME_DIRECTORY: &str = "asr-runtime";

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum ExecutableLocation {
    Desktop,
    Runtime,
}

pub fn link_arg(os: &str, location: ExecutableLocation) -> Option<String> {
    let prefix = match (os, location) {
        ("linux", ExecutableLocation::Desktop) => "$ORIGIN/../lib/muniment",
        ("linux", ExecutableLocation::Runtime) => "$ORIGIN",
        ("macos", ExecutableLocation::Desktop) => "@executable_path/../Resources",
        ("macos", ExecutableLocation::Runtime) => "@executable_path/../../Resources",
        ("windows", _) => return None,
        _ => panic!("unsupported ASR target OS: {os}"),
    };
    Some(format!("-Wl,-rpath,{prefix}/{ASR_RUNTIME_DIRECTORY}"))
}
