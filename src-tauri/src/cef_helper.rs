#[path = "cef_locale.rs"]
mod cef_locale;

fn main() {
    // Read the language pack before the sandbox closes file access.
    let mut app = cef_locale::from_args().map(cef_locale::LocaleApp::new);
    let args = cef::args::Args::new();
    #[cfg(target_os = "macos")]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };
    #[cfg(target_os = "macos")]
    let _loader = {
        let loader =
            cef::library_loader::LibraryLoader::new(&std::env::current_exe().unwrap(), true);
        assert!(loader.load());
        loader
    };
    cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let code = cef::execute_process(
        Some(args.as_main_args()),
        app.as_mut(),
        std::ptr::null_mut(),
    );
    std::process::exit(code.max(0));
}
