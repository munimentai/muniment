use crate::cef_browser::Bounds;
use cef::*;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
use tauri::Manager;

type ResultHandler = Arc<dyn Fn(i32, bool, Vec<u8>) + Send + Sync>;
type Navigation = Arc<dyn Fn(String) -> bool + Send + Sync>;
struct Page {
    browser: Browser,
    _observer: Registration,
    loaded: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    container: LinuxContainer,
}
// GTK can use a 32-bit visual for its window while Chromium uses the default
// 24-bit visual. An explicit colormap and border let X11 join those depths.
#[cfg(target_os = "linux")]
struct LinuxContainer {
    xlib: x11_dl::xlib::Xlib,
    display: *mut x11_dl::xlib::Display,
    window: u64,
}
#[cfg(target_os = "linux")]
impl LinuxContainer {
    fn new(parent: u64, bounds: &Bounds) -> Result<Self, String> {
        use x11_dl::xlib::*;
        let xlib = Xlib::open().map_err(|e| e.to_string())?;
        unsafe {
            let display = (xlib.XOpenDisplay)(std::ptr::null());
            if display.is_null() {
                return Err("The browser needs an X11 display.".into());
            }
            let screen = (xlib.XDefaultScreen)(display);
            let mut attrs: XSetWindowAttributes = std::mem::zeroed();
            attrs.colormap = (xlib.XDefaultColormap)(display, screen);
            attrs.border_pixel = 0;
            let window = (xlib.XCreateWindow)(
                display,
                parent,
                bounds.x as i32,
                bounds.y as i32,
                bounds.width.max(1.0) as u32,
                bounds.height.max(1.0) as u32,
                0,
                (xlib.XDefaultDepth)(display, screen),
                InputOutput as u32,
                (xlib.XDefaultVisual)(display, screen),
                CWColormap | CWBorderPixel,
                &mut attrs,
            );
            (xlib.XMapWindow)(display, window);
            (xlib.XSync)(display, 0);
            Ok(Self {
                xlib,
                display,
                window,
            })
        }
    }
}
#[cfg(target_os = "linux")]
impl Drop for LinuxContainer {
    fn drop(&mut self) {
        unsafe {
            (self.xlib.XCloseDisplay)(self.display);
        }
    }
}
thread_local! {
    static PAGES: RefCell<HashMap<String, Page>> = RefCell::new(HashMap::new());
    static CONTEXTS: RefCell<HashMap<String, (RequestContext, Arc<AtomicBool>)>> = RefCell::new(HashMap::new());
    #[cfg(target_os="macos")]
    static LOADER: RefCell<Option<cef::library_loader::LibraryLoader>> = const { RefCell::new(None) };
}
static BROWSER_COUNT: AtomicUsize = AtomicUsize::new(0);
static RUNNING: AtomicBool = AtomicBool::new(false);
fn on_main<T: Send + 'static>(
    app: &tauri::AppHandle,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::sync_channel(1);
    app.run_on_main_thread(move || {
        let _ = tx.send(f());
    })
    .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(15))
        .map_err(|_| "The browser did not answer.".to_string())?
}
fn with_page<T>(label: &str, f: impl FnOnce(&Browser) -> Result<T, String>) -> Result<T, String> {
    PAGES.with(|pages| {
        pages
            .borrow()
            .get(label)
            .ok_or_else(|| "Browser view is closed.".into())
            .and_then(|p| f(&p.browser))
    })
}
#[derive(Clone)]
pub struct View {
    app: tauri::AppHandle,
    label: String,
}
pub fn view(app: &tauri::AppHandle, label: &str) -> Result<View, String> {
    if !matches!(label, "browser" | "artifact") {
        return Err("Unknown browser view.".into());
    }
    Ok(View {
        app: app.clone(),
        label: label.into(),
    })
}
impl View {
    pub fn url(&self) -> Result<tauri::Url, String> {
        let label = self.label.clone();
        on_main(&self.app, move || {
            with_page(&label, |b| {
                let url = CefString::from(&b.main_frame().ok_or("No page.")?.url()).to_string();
                tauri::Url::parse(if url.is_empty() { "about:blank" } else { &url })
                    .map_err(|e| e.to_string())
            })
        })
    }
    pub fn navigate(&self, url: tauri::Url) -> Result<(), String> {
        let label = self.label.clone();
        on_main(&self.app, move || {
            with_page(&label, |b| {
                b.main_frame()
                    .ok_or("No page.")?
                    .load_url(Some(&CefString::from(url.as_str())));
                Ok(())
            })
        })
    }
    pub fn send_dev_tools_message(&self, message: &[u8]) -> Result<(), String> {
        let label = self.label.clone();
        let message = message.to_vec();
        on_main(&self.app, move || {
            let loaded = PAGES.with(|pages| {
                pages
                    .borrow()
                    .get(&label)
                    .is_some_and(|p| p.loaded.load(Ordering::Acquire))
            });
            if !loaded {
                return Err("The page is still loading.".into());
            }
            with_page(&label, |b| {
                if b.host()
                    .ok_or("No browser host.")?
                    .send_dev_tools_message(Some(&message))
                    == 1
                {
                    Ok(())
                } else {
                    Err("Browser command failed.".into())
                }
            })
        })
    }
}
wrap_dev_tools_message_observer! {
    struct Observer { handler: ResultHandler }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self,_browser:Option<&mut Browser>,id:i32,success:i32,result:Option<&[u8]>) {
            (self.handler)(id,success!=0,result.unwrap_or_default().to_vec());
        }
    }
}
wrap_request_handler! {
    struct Requests { navigation: Navigation }
    impl RequestHandler {
        fn on_before_browse(&self,_browser:Option<&mut Browser>,frame:Option<&mut Frame>,request:Option<&mut Request>,_gesture:i32,_redirect:i32)->i32 {
            if frame.is_some_and(|f|f.is_main()!=0) {
                return i32::from(!request.is_some_and(|r|(self.navigation)(CefString::from(&r.url()).to_string())));
            }
            0
        }
    }
}
wrap_permission_handler! {
    struct Permissions;
    impl PermissionHandler {
        fn on_request_media_access_permission(&self,_browser:Option<&mut Browser>,_frame:Option<&mut Frame>,_origin:Option<&CefString>,_permissions:u32,callback:Option<&mut MediaAccessCallback>)->i32 {
            if let Some(callback)=callback {callback.cancel();} 1
        }
        fn on_show_permission_prompt(&self,_browser:Option<&mut Browser>,_id:u64,_origin:Option<&CefString>,_permissions:u32,callback:Option<&mut PermissionPromptCallback>)->i32 {
            if let Some(callback)=callback {callback.cont(PermissionRequestResult::DENY);} 1
        }
    }
}
wrap_life_span_handler! {
    struct Lifetime;
    impl LifeSpanHandler {
        fn on_after_created(&self,_browser:Option<&mut Browser>) {BROWSER_COUNT.fetch_add(1,Ordering::AcqRel);}
        fn on_before_close(&self,_browser:Option<&mut Browser>) {BROWSER_COUNT.fetch_sub(1,Ordering::AcqRel);}
        fn on_before_popup(&self,_browser:Option<&mut Browser>,_frame:Option<&mut Frame>,_id:i32,_url:Option<&CefString>,_name:Option<&CefString>,_disposition:WindowOpenDisposition,_gesture:i32,_features:Option<&PopupFeatures>,_window:Option<&mut WindowInfo>,_client:Option<&mut Option<Client>>,_settings:Option<&mut BrowserSettings>,_extra:Option<&mut Option<DictionaryValue>>,_access:Option<&mut i32>)->i32 {1}
    }
}
wrap_load_handler! {
    struct Loading { loaded: Arc<AtomicBool> }
    impl LoadHandler {
        fn on_loading_state_change(&self, _browser: Option<&mut Browser>, loading: i32, _back: i32, _forward: i32) {
            self.loaded.store(loading == 0, Ordering::Release);
        }
    }
}
wrap_completion_callback! {
    struct Flushed { done: Arc<AtomicBool> }
    impl CompletionCallback {
        fn on_complete(&self) { self.done.store(true, Ordering::Release); }
    }
}
wrap_client! {
    struct BrowserClient { navigation: Navigation, loaded: Arc<AtomicBool> }
    impl Client {
        fn load_handler(&self)->Option<LoadHandler>{Some(Loading::new(self.loaded.clone()))}
        fn request_handler(&self)->Option<RequestHandler>{Some(Requests::new(self.navigation.clone()))}
        fn permission_handler(&self)->Option<PermissionHandler>{Some(Permissions::new())}
        fn life_span_handler(&self)->Option<LifeSpanHandler>{Some(Lifetime::new())}
    }
}
wrap_request_context_handler! {
    struct Profile { ready: Arc<AtomicBool> }
    impl RequestContextHandler {
        fn on_request_context_initialized(&self,context:Option<&mut RequestContext>){
            if let (Some(context),Some(value))=(context,value_create()) {
                value.set_int(1);let mut error=CefString::default();
                context.set_preference(Some(&CefString::from("session.restore_on_startup")),Some(&mut value.clone()),Some(&mut error));
            }
            self.ready.store(true, Ordering::Release);
        }
    }
}
// A browser-process handler selects CEF's external pump. Without this handler,
// CEF falls back to AppKit's own event pump and can trap a quit event inside it.
wrap_browser_process_handler! {
    struct Pump;
    impl BrowserProcessHandler {
        fn on_schedule_message_pump_work(&self,_delay_ms:i64) {}
    }
}
wrap_app! {
    struct BrowserApp;
    impl App {
        fn browser_process_handler(&self)->Option<BrowserProcessHandler>{Some(Pump::new())}
        fn on_before_command_line_processing(&self,_process:Option<&CefString>,command:Option<&mut CommandLine>){
            if let Some(command)=command {
                command.append_switch(Some(&CefString::from("no-first-run")));
            }
        }
    }
}
pub fn initialize(_app: Option<&tauri::AppHandle>, root: &Path) -> Result<(), String> {
    if RUNNING.load(Ordering::Acquire) {
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    if crate::cef_windows::broker().is_null() {
        return Err("Start the app through its sandbox host.".into());
    }
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn muniment_cef_install_app_protocol();
        }
        unsafe {
            muniment_cef_install_app_protocol();
        }
        let loader = cef::library_loader::LibraryLoader::new(
            &std::env::current_exe().map_err(|e| e.to_string())?,
            false,
        );
        if !loader.load() {
            return Err("Cannot load the browser framework.".into());
        }
        LOADER.with(|slot| *slot.borrow_mut() = Some(loader));
    }
    cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);
    let args = cef::args::Args::new();
    #[cfg(target_os = "macos")]
    let helper = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .unwrap()
        .join("../Frameworks/muniment CEF Helper.app/Contents/MacOS/muniment CEF Helper");
    #[cfg(windows)]
    let helper = std::env::current_exe().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    let helper = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .unwrap()
        .join("muniment-cef-helper");
    #[cfg(windows)]
    let sandbox = crate::cef_windows::broker();
    #[cfg(not(windows))]
    let sandbox = std::ptr::null_mut();
    let settings = Settings {
        no_sandbox: 0,
        external_message_pump: 1,
        root_cache_path: CefString::from(root.join("profiles").to_string_lossy().as_ref()),
        persist_session_cookies: 1,
        browser_subprocess_path: CefString::from(helper.to_string_lossy().as_ref()),
        log_file: CefString::from(root.join("cef.log").to_string_lossy().as_ref()),
        ..Default::default()
    };
    if cef::initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut BrowserApp::new()),
        sandbox,
    ) != 1
    {
        let code = cef::get_exit_code();
        // Chromium relaunches an elevated Windows browser as the normal user.
        // Its original process must exit without turning that handoff into a panic.
        #[cfg(windows)]
        if code == 38 {
            std::process::exit(0);
        }
        return Err(format!("Cannot start the browser (CEF exit {code})."));
    }
    RUNNING.store(true, Ordering::Release);
    #[cfg(target_os = "macos")]
    {
        extern "C" fn pump() {
            if RUNNING.load(Ordering::Acquire) {
                cef::do_message_loop_work();
            }
        }
        unsafe extern "C" {
            fn muniment_cef_start_pump(callback: extern "C" fn());
        }
        unsafe {
            muniment_cef_start_pump(pump);
        }
    }
    #[cfg(target_os = "linux")]
    {
        // Run outside Tao's event callback so Chromium can dispatch GTK work.
        unsafe extern "C" {
            fn g_timeout_add(
                interval: u32,
                callback: extern "C" fn(*mut std::ffi::c_void) -> i32,
                data: *mut std::ffi::c_void,
            ) -> u32;
        }
        extern "C" fn pump(_: *mut std::ffi::c_void) -> i32 {
            if !RUNNING.load(Ordering::Acquire) {
                return 0;
            }
            cef::do_message_loop_work();
            1
        }
        unsafe {
            g_timeout_add(10, pump, std::ptr::null_mut());
        }
    }
    #[cfg(windows)]
    {
        let handle = _app.ok_or("The browser needs the desktop window.")?.clone();
        // CEF work stays on Tao's main thread. One queued pump prevents a busy UI
        // from accumulating timer callbacks or entering CEF from multiple threads.
        std::thread::spawn(move || {
            while RUNNING.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(10));
                if on_main(&handle, || {
                    if RUNNING.load(Ordering::Acquire) {
                        cef::do_message_loop_work();
                    }
                    Ok(())
                })
                .is_err()
                {
                    break;
                }
            }
        });
    }
    Ok(())
}
fn set_bounds(browser: &Browser, b: &Bounds, hidden: bool) -> Result<(), String> {
    let host = browser.host().ok_or("No browser host.")?;
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn muniment_cef_bounds(
                handle: *mut std::ffi::c_void,
                x: f64,
                y: f64,
                width: f64,
                height: f64,
                hidden: bool,
            );
        }
        unsafe {
            muniment_cef_bounds(
                host.window_handle().cast(),
                b.x,
                b.y,
                b.width,
                b.height,
                hidden,
            );
        }
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            MoveWindow, ShowWindow, SW_HIDE, SW_SHOWNA,
        };
        let hwnd = host.window_handle().0.cast();
        if MoveWindow(
            hwnd,
            b.x as i32,
            b.y as i32,
            b.width as i32,
            b.height as i32,
            1,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        ShowWindow(hwnd, if hidden { SW_HIDE } else { SW_SHOWNA });
    }
    #[cfg(target_os = "linux")]
    unsafe {
        let xlib = x11_dl::xlib::Xlib::open().map_err(|e| e.to_string())?;
        let display = (xlib.XOpenDisplay)(std::ptr::null());
        if display.is_null() {
            return Err("The browser needs an X11 display.".into());
        }
        let child = host.window_handle();
        let window = PAGES
            .with(|pages| {
                pages
                    .borrow()
                    .values()
                    .find(|p| p.browser.identifier() == browser.identifier())
                    .map(|p| p.container.window)
            })
            .ok_or("Browser container is closed.")?;
        (xlib.XMoveResizeWindow)(
            display,
            child,
            0,
            0,
            b.width.max(1.0) as u32,
            b.height.max(1.0) as u32,
        );
        (xlib.XMoveResizeWindow)(
            display,
            window,
            b.x as i32,
            b.y as i32,
            b.width.max(1.0) as u32,
            b.height.max(1.0) as u32,
        );
        if hidden {
            (xlib.XUnmapWindow)(display, window);
        } else {
            (xlib.XMapWindow)(display, window);
        }
        (xlib.XFlush)(display);
        (xlib.XCloseDisplay)(display);
    }
    Ok(())
}
pub fn hide_all(app: &tauri::AppHandle) -> Result<(), String> {
    on_main(app, || {
        PAGES.with(|pages| {
            for page in pages.borrow().values() {
                set_bounds(
                    &page.browser,
                    &Bounds {
                        x: 0.0,
                        y: 0.0,
                        width: 1.0,
                        height: 1.0,
                    },
                    true,
                )?;
            }
            Ok(())
        })
    })
}
pub fn layout(
    app: &tauri::AppHandle,
    label: &str,
    root: &Path,
    bounds: Bounds,
    url: Option<String>,
    result: impl Fn(i32, bool, Vec<u8>) + Send + Sync + 'static,
    navigation: impl Fn(String) -> bool + Send + Sync + 'static,
) -> Result<(), String> {
    #[cfg(not(target_os = "macos"))]
    let bounds = {
        let handle = app.clone();
        let scale = on_main(app, move || {
            handle
                .get_webview_window("main")
                .ok_or("Main window is closed.")?
                .scale_factor()
                .map_err(|e| e.to_string())
        })?;
        Bounds {
            x: bounds.x * scale,
            y: bounds.y * scale,
            width: bounds.width * scale,
            height: bounds.height * scale,
        }
    };
    let label = label.to_owned();
    let root = root.to_owned();
    let initial_label = label.clone();
    let initial_url = url.clone();
    let initial_bounds = bounds.clone();
    let ready = on_main(app, move || {
        if let Some(browser) = PAGES.with(|pages| {
            pages
                .borrow()
                .get(&initial_label)
                .map(|p| p.browser.clone())
        }) {
            if let Some(url) = initial_url {
                browser
                    .main_frame()
                    .ok_or("No page.")?
                    .load_url(Some(&CefString::from(url.as_str())));
            }
            set_bounds(&browser, &initial_bounds, false)?;
            return Ok(None);
        }
        if initial_label == "artifact" && initial_url.is_none() {
            return Ok(None);
        }
        CONTEXTS.with(|contexts| {
            let mut contexts = contexts.borrow_mut();
            if let Some((_, ready)) = contexts.get(&initial_label) {
                return Ok(Some(ready.clone()));
            }
            let ready = Arc::new(AtomicBool::new(false));
            let settings = RequestContextSettings {
                cache_path: CefString::from(
                    root.join("profiles")
                        .join(&initial_label)
                        .to_string_lossy()
                        .as_ref(),
                ),
                persist_session_cookies: 1,
                ..Default::default()
            };
            let context = request_context_create_context(
                Some(&settings),
                Some(&mut Profile::new(ready.clone())),
            )
            .ok_or("Cannot open browser profile.")?;
            contexts.insert(initial_label, (context, ready.clone()));
            Ok(Some(ready))
        })
    })?;
    let Some(ready) = ready else { return Ok(()) };
    // Chromium opens disk profiles asynchronously. Wait outside the UI thread
    // so its message pump can finish initialization before browser creation.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !ready.load(Ordering::Acquire) {
        if std::time::Instant::now() >= deadline {
            return Err("The browser profile did not open.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let handle = app.clone();
    on_main(app, move || {
        let mut context = CONTEXTS
            .with(|contexts| {
                contexts
                    .borrow()
                    .get(&label)
                    .map(|(context, _)| context.clone())
            })
            .ok_or("Browser profile is closed.")?;
        let window = handle
            .get_webview_window("main")
            .ok_or("Main window is closed.")?;
        #[cfg(target_os = "macos")]
        let parent = {
            unsafe extern "C" {
                fn muniment_cef_parent(window: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
            }
            unsafe { muniment_cef_parent(window.ns_window().map_err(|e| e.to_string())?).cast() }
        };
        #[cfg(windows)]
        let parent = cef::sys::HWND(window.hwnd().map_err(|e| e.to_string())?.0.cast());
        #[cfg(target_os = "linux")]
        let parent = {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
                RawWindowHandle::Xlib(handle) => handle.window,
                RawWindowHandle::Xcb(handle) => handle.window.get() as _,
                _ => return Err("The browser needs an X11 display.".into()),
            }
        };
        #[cfg(target_os = "linux")]
        let container = LinuxContainer::new(parent, &bounds)?;
        #[cfg(target_os = "linux")]
        let parent = container.window;
        let info = WindowInfo {
            runtime_style: RuntimeStyle::ALLOY,
            ..Default::default()
        }
        .set_as_child(
            parent,
            &Rect {
                x: if cfg!(target_os = "linux") {
                    0
                } else {
                    bounds.x as i32
                },
                y: if cfg!(target_os = "linux") {
                    0
                } else {
                    bounds.y as i32
                },
                width: bounds.width as i32,
                height: bounds.height as i32,
            },
        );
        let loaded = Arc::new(AtomicBool::new(false));
        let mut client = BrowserClient::new(Arc::new(navigation), loaded.clone());
        let browser = browser_host_create_browser_sync(
            Some(&info),
            Some(&mut client),
            Some(&CefString::from(
                url.as_deref().unwrap_or("https://example.org"),
            )),
            Some(&BrowserSettings::default()),
            None,
            Some(&mut context),
        )
        .ok_or("Cannot create browser view.")?;
        browser
            .host()
            .ok_or("No browser host.")?
            .set_accessibility_state(State::ENABLED);
        let observer = browser
            .host()
            .ok_or("No browser host.")?
            .add_dev_tools_message_observer(Some(&mut Observer::new(Arc::new(result))))
            .ok_or("Cannot observe browser commands.")?;
        PAGES.with(|pages| {
            pages.borrow_mut().insert(
                label,
                Page {
                    browser: browser.clone(),
                    _observer: observer,
                    loaded,
                    #[cfg(target_os = "linux")]
                    container,
                },
            )
        });
        set_bounds(&browser, &bounds, false)
    })
}
pub fn shutdown() {
    if !RUNNING.swap(false, Ordering::AcqRel) {
        return;
    }
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn muniment_cef_stop_pump();
        }
        unsafe {
            muniment_cef_stop_pump();
        }
    }
    // Save session cookies before the network process closes. A short browser
    // visit can otherwise end before Chromium's periodic disk write.
    let flushes = CONTEXTS.with(|contexts| {
        contexts
            .borrow()
            .values()
            .filter_map(|(context, _)| {
                let manager = context.cookie_manager(None)?;
                let done = Arc::new(AtomicBool::new(false));
                if manager.flush_store(Some(&mut Flushed::new(done.clone()))) == 1 {
                    Some(done)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    });
    let flush_deadline = std::time::Instant::now() + Duration::from_secs(5);
    while flushes.iter().any(|done| !done.load(Ordering::Acquire)) {
        cef::do_message_loop_work();
        if std::time::Instant::now() >= flush_deadline {
            eprintln!("Browser cookie flush timed out.");
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    PAGES.with(|pages| {
        for page in pages.borrow().values() {
            if let Some(host) = page.browser.host() {
                host.close_browser(1);
            }
        }
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while BROWSER_COUNT.load(Ordering::Acquire) > 0 {
        cef::do_message_loop_work();
        if std::time::Instant::now() >= deadline {
            eprintln!("Browser shutdown timed out.");
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    PAGES.with(|pages| pages.borrow_mut().clear());
    CONTEXTS.with(|contexts| contexts.borrow_mut().clear());
    cef::shutdown();
}
