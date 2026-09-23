const PROBE_FLAG: &str = "--probe-runtime-notice";

pub(crate) fn install<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    if !std::env::args_os().any(|arg| arg == PROBE_FLAG) {
        return;
    }
    // Read the rendered notice without an Accessibility permission or a test WebDriver.
    let _ = webview.eval(
        r#"(() => {
            const timer = setInterval(() => {
                const notice = document.querySelector('[data-testid="runtime-notice"]');
                if (!notice || !notice.getClientRects().length) return;
                const text = notice.querySelector('p')?.textContent;
                if (!['The runtime connection closed.', 'The runtime exited.'].includes(text)) return;
                const controls = [...notice.querySelectorAll('button')];
                window.__TAURI__.core.invoke('runtime_notice_observed', {
                    text,
                    control: controls[0]?.textContent ?? '',
                    controls: controls.length,
                }).catch(() => {
                    clearInterval(timer);
                    console.error('The runtime notice probe failed.');
                });
            }, 250);
        })()"#,
    );
}

#[tauri::command]
pub(crate) fn runtime_notice_observed(
    text: String,
    control: String,
    controls: usize,
) -> Result<(), &'static str> {
    if !std::env::args_os().any(|arg| arg == PROBE_FLAG) {
        return Err("The runtime notice probe is inactive.");
    }
    if !matches!(text.as_str(), "The runtime connection closed." | "The runtime exited.")
        || control != "Start runtime"
        || controls != 1
    {
        return Err("The runtime notice probe found unexpected content.");
    }
    println!("runtime_notice={text} control={control} controls={controls}");
    Ok(())
}
