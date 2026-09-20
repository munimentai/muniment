use std::sync::atomic::{AtomicPtr, Ordering};
static BROKER: AtomicPtr<u8> = AtomicPtr::new(std::ptr::null_mut());

pub fn broker() -> *mut u8 {
    BROKER.load(Ordering::Acquire)
}

// CEF's Chromium-built bootstrap creates the broker before loading this DLL.
// All CEF child processes use that same bootstrap and exported entry point.
#[no_mangle]
pub unsafe extern "C" fn RunWinMain(
    instance: cef::sys::HINSTANCE,
    _command_line: *const u8,
    _command_show: i32,
    sandbox_info: *mut u8,
) -> i32 {
    if sandbox_info.is_null() {
        return 78;
    }
    BROKER.store(sandbox_info, Ordering::Release);
    cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::MainArgs { instance };
    let code = cef::execute_process(Some(&args), None::<&mut cef::App>, sandbox_info);
    if code >= 0 {
        return code;
    }
    crate::run();
    0
}
