#![cfg(target_os = "windows")]

use muniment_core::windows_sid::current_process_user_sid;
use muniment_core::windows_task::{
    render_task_definition_xml, RemovalScope, SidError, TaskDefinition, TaskRegistrationPlan,
    TaskRemovalPlan,
};
use muniment_core::windows_task_service::{
    apply_task_removal, ensure_task_registration, read_observed_registration,
    start_registered_task, EnsureTaskRegistrationError, ReadObservedRegistrationError,
    StartRegisteredTaskError, StartRegisteredTaskResult,
};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use windows::core::BSTR;
use windows::Win32::Foundation::SCHED_E_TASK_NOT_RUNNING;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::TaskScheduler::{
    IRunningTask, ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN,
    TASK_LOGON_SERVICE_ACCOUNT, TASK_LOGON_TYPE, TASK_STATE_RUNNING,
};
use windows::Win32::System::Variant::VARIANT;

const TASK_FOLDER: &str = r"\Muniment";
const SERVICE_ACCOUNT_SID: &str = "S-1-5-18";
const TEST_ROOT: &str = r"C:\MunimentTaskPreflight";
const MACHINE_ROOT: &str = r"C:\MunimentTaskPreflight\Machine";
const USER_ROOT: &str = r"C:\MunimentTaskPreflight\User";
const PAYLOAD: &str = r"C:\MunimentTaskPreflight\User\muniment-runtime.exe";
const MACHINE_PAYLOAD: &str = r"C:\MunimentTaskPreflight\Machine\muniment-runtime.exe";
const FOREIGN_PAYLOAD: &str = r"C:\MunimentTaskPreflight\Foreign\muniment-runtime.exe";
const COM_HANDLER_CLASS_ID: &str = "{00000000-0000-0000-0000-000000000001}";
const COM_HANDLER_DATA: &str = "preserve-this-action";
static SCHEDULER_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn absent_runtime_task_returns_none_without_a_write() {
    let _guard = SCHEDULER_TEST_LOCK.lock().unwrap();
    let sid = "S-1-5-999999999";
    let observed = read_observed_registration(sid).unwrap();
    let scope = RemovalScope::PerUser {
        user_sid: sid.to_owned(),
        payload_path: PathBuf::from(PAYLOAD),
        machine_payload_path: None,
    };

    assert_eq!(observed, None);
    assert_eq!(
        apply_task_removal(sid, &scope).unwrap(),
        TaskRemovalPlan::LeaveUnchanged
    );
}

#[test]
fn non_canonical_sid_is_rejected_before_scheduler_access() {
    let error = read_observed_registration("S-1-05-21").unwrap_err();

    assert_eq!(
        error,
        ReadObservedRegistrationError::InvalidSid(SidError::NotCanonical)
    );
}

#[test]
fn writes_registration_and_applies_each_removal_plan() {
    let _guard = SCHEDULER_TEST_LOCK.lock().unwrap();
    let sid = current_process_user_sid().unwrap();
    let mut fixture = SchedulerFixture::new(sid.as_str());
    assert_eq!(read_observed_registration(sid.as_str()).unwrap(), None);

    fixture.owns_task = true;
    assert_eq!(
        ensure_task_registration(sid.as_str(), PAYLOAD, MACHINE_ROOT, USER_ROOT).unwrap(),
        TaskRegistrationPlan::Register
    );
    assert_eq!(
        ensure_task_registration(sid.as_str(), PAYLOAD, MACHINE_ROOT, USER_ROOT).unwrap(),
        TaskRegistrationPlan::LeaveUnchanged
    );
    fixture.register_mixed_task(sid.as_str());

    let per_user_scope = RemovalScope::PerUser {
        user_sid: sid.as_str().to_owned(),
        payload_path: PathBuf::from(PAYLOAD),
        machine_payload_path: Some(PathBuf::from(MACHINE_PAYLOAD)),
    };
    let mut expected_repointed = read_observed_registration(sid.as_str()).unwrap().unwrap();
    expected_repointed.action_path = PathBuf::from(MACHINE_PAYLOAD);
    assert_eq!(
        apply_task_removal(sid.as_str(), &per_user_scope).unwrap(),
        TaskRemovalPlan::RepointTo(PathBuf::from(MACHINE_PAYLOAD))
    );
    assert_eq!(
        read_observed_registration(sid.as_str()).unwrap().unwrap(),
        expected_repointed
    );
    let repointed_xml = fixture.task_xml();
    assert!(repointed_xml.contains(COM_HANDLER_CLASS_ID));
    assert!(repointed_xml.contains(COM_HANDLER_DATA));
    fixture.register_machine_task();

    let machine_scope = RemovalScope::Machine {
        payload_path: PathBuf::from(MACHINE_PAYLOAD),
        per_user_payload_path: None,
    };
    let running_task = fixture.start_controlled_task();
    assert_eq!(
        apply_task_removal(SERVICE_ACCOUNT_SID, &machine_scope).unwrap(),
        TaskRemovalPlan::StopAndDelete
    );
    match unsafe { running_task.State() } {
        Ok(state) => assert_ne!(state, TASK_STATE_RUNNING),
        Err(error) => assert_eq!(error.code(), SCHED_E_TASK_NOT_RUNNING),
    }
    assert_eq!(
        read_observed_registration(SERVICE_ACCOUNT_SID).unwrap(),
        None
    );
    assert!(unsafe { fixture.service.GetFolder(&BSTR::from(TASK_FOLDER)) }.is_err());

    fixture.task_name = format!("Runtime-{}", sid.as_str());
    assert_eq!(
        ensure_task_registration(sid.as_str(), PAYLOAD, MACHINE_ROOT, USER_ROOT).unwrap(),
        TaskRegistrationPlan::Register
    );
    fixture.register_foreign_task(sid.as_str());
    let foreign = read_observed_registration(sid.as_str()).unwrap().unwrap();
    assert_eq!(foreign.action_path, PathBuf::from(FOREIGN_PAYLOAD));
    assert_eq!(
        ensure_task_registration(sid.as_str(), PAYLOAD, MACHINE_ROOT, USER_ROOT),
        Err(EnsureTaskRegistrationError::Refused)
    );
    assert_eq!(
        apply_task_removal(sid.as_str(), &per_user_scope).unwrap(),
        TaskRemovalPlan::LeaveUnchanged
    );
    assert_eq!(
        read_observed_registration(sid.as_str()).unwrap().unwrap(),
        foreign
    );
}

#[test]
fn starting_an_absent_task_does_not_clear_the_crash_window() {
    let _guard = SCHEDULER_TEST_LOCK.lock().unwrap();
    let mut cleared = false;
    let result = start_registered_task("S-1-5-999999999", || {
        cleared = true;
        Ok::<(), ()>(())
    });

    assert_eq!(result, Err(StartRegisteredTaskError::Missing));
    assert!(!cleared);
}

#[test]
fn starting_a_foreign_task_is_refused_without_clearing_the_crash_window() {
    let _guard = SCHEDULER_TEST_LOCK.lock().unwrap();
    let sid = current_process_user_sid().unwrap();
    let mut fixture = SchedulerFixture::new(sid.as_str());
    fixture.owns_task = true;
    fixture.register_task(sid.as_str(), Path::new(FOREIGN_PAYLOAD));
    let mut cleared = false;

    let result = start_registered_task(sid.as_str(), || {
        cleared = true;
        Ok::<(), ()>(())
    });

    assert_eq!(result, Err(StartRegisteredTaskError::Refused));
    assert!(!cleared);
}

#[test]
fn starts_a_registered_task_that_exits_at_once() {
    let _guard = SCHEDULER_TEST_LOCK.lock().unwrap();
    let sid = current_process_user_sid().unwrap();
    let mut fixture = SchedulerFixture::new(sid.as_str());
    let payload = runtime_payload_path();
    let system_root = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
    std::fs::copy(system_root.join("System32").join("where.exe"), &payload).unwrap();
    fixture.payload = Some(payload.clone());
    fixture.owns_task = true;
    fixture.register_task(sid.as_str(), &payload);
    let mut clear_count = 0;

    let result = start_registered_task(sid.as_str(), || {
        clear_count += 1;
        Ok::<(), ()>(())
    });

    assert_eq!(result, Ok(StartRegisteredTaskResult::Started));
    assert_eq!(clear_count, 1);
    fixture.wait_until_task_stops();
}

fn runtime_payload_path() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.set_file_name("muniment-runtime.exe");
    path
}

struct SchedulerFixture {
    service: ITaskService,
    task_name: String,
    owns_task: bool,
    remove_folder: bool,
    remove_test_root: bool,
    remove_machine_root: bool,
    remove_machine_payload: bool,
    payload: Option<PathBuf>,
    _apartment: TestComApartment,
}

impl SchedulerFixture {
    fn new(sid: &str) -> Self {
        let apartment = TestComApartment::new();
        let service: ITaskService =
            unsafe { CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).unwrap() };
        let empty = VARIANT::default();
        unsafe { service.Connect(&empty, &empty, &empty, &empty) }.unwrap();
        let remove_folder = unsafe { service.GetFolder(&BSTR::from(TASK_FOLDER)) }.is_err();

        Self {
            service,
            task_name: format!("Runtime-{sid}"),
            owns_task: false,
            remove_folder,
            remove_test_root: false,
            remove_machine_root: false,
            remove_machine_payload: false,
            payload: None,
            _apartment: apartment,
        }
    }

    fn register_foreign_task(&self, sid: &str) {
        self.register_task(sid, Path::new(FOREIGN_PAYLOAD));
    }

    fn register_task(&self, sid: &str, payload: &Path) {
        let definition = TaskDefinition::new(sid, payload).unwrap();
        self.register_xml(
            render_task_definition_xml(&definition).unwrap(),
            TASK_LOGON_INTERACTIVE_TOKEN,
        );
    }

    fn register_mixed_task(&self, sid: &str) {
        let definition = TaskDefinition::new(sid, PAYLOAD).unwrap();
        let xml = render_task_definition_xml(&definition).unwrap();
        let exec_start = "  <Actions Context=\"Author\">\n    <Exec>";
        let mixed_start = format!(
            "  <Actions Context=\"Author\">\n    <ComHandler>\n      <ClassId>{COM_HANDLER_CLASS_ID}</ClassId>\n      <Data>{COM_HANDLER_DATA}</Data>\n    </ComHandler>\n    <Exec>"
        );
        let xml = xml.replacen(exec_start, &mixed_start, 1);
        assert_ne!(xml, render_task_definition_xml(&definition).unwrap());
        self.register_xml(xml, TASK_LOGON_INTERACTIVE_TOKEN);
    }

    fn register_machine_task(&mut self) {
        let folder = unsafe { self.service.GetFolder(&BSTR::from(TASK_FOLDER)) }.unwrap();
        unsafe { folder.DeleteTask(&BSTR::from(self.task_name.as_str()), 0) }.unwrap();
        self.task_name = format!("Runtime-{SERVICE_ACCOUNT_SID}");

        self.remove_test_root = !Path::new(TEST_ROOT).exists();
        self.remove_machine_root = !Path::new(MACHINE_ROOT).exists();
        std::fs::create_dir_all(MACHINE_ROOT).unwrap();
        assert!(!Path::new(MACHINE_PAYLOAD).exists());
        self.remove_machine_payload = true;
        std::fs::copy(
            env!("CARGO_BIN_EXE_windows-task-test-helper"),
            MACHINE_PAYLOAD,
        )
        .unwrap();

        let mut definition = TaskDefinition::new(SERVICE_ACCOUNT_SID, MACHINE_PAYLOAD).unwrap();
        definition.triggers.clear();
        let xml = render_task_definition_xml(&definition).unwrap();
        self.register_xml(xml, TASK_LOGON_SERVICE_ACCOUNT);
    }

    fn register_xml(&self, xml: String, logon_type: TASK_LOGON_TYPE) {
        let task = unsafe { self.service.NewTask(0) }.unwrap();
        unsafe { task.SetXmlText(&BSTR::from(xml)) }.unwrap();
        let principal = unsafe { task.Principal() }.unwrap();
        unsafe { principal.SetLogonType(logon_type) }.unwrap();
        let empty = VARIANT::default();
        let user_id = if logon_type == TASK_LOGON_SERVICE_ACCOUNT {
            VARIANT::from("SYSTEM")
        } else {
            VARIANT::default()
        };
        let folder = match unsafe { self.service.GetFolder(&BSTR::from(TASK_FOLDER)) } {
            Ok(folder) => folder,
            Err(_) => {
                let root = unsafe { self.service.GetFolder(&BSTR::from(r"\")) }.unwrap();
                unsafe { root.CreateFolder(&BSTR::from("Muniment"), &empty) }.unwrap()
            }
        };
        unsafe {
            folder.RegisterTaskDefinition(
                &BSTR::from(self.task_name.as_str()),
                &task,
                TASK_CREATE_OR_UPDATE.0,
                &user_id,
                &empty,
                logon_type,
                &empty,
            )
        }
        .unwrap();
    }

    fn task_xml(&self) -> String {
        let folder = unsafe { self.service.GetFolder(&BSTR::from(TASK_FOLDER)) }.unwrap();
        let task = unsafe { folder.GetTask(&BSTR::from(self.task_name.as_str())) }.unwrap();
        String::try_from(&unsafe { task.Xml() }.unwrap()).unwrap()
    }

    fn start_controlled_task(&mut self) -> IRunningTask {
        let folder = unsafe { self.service.GetFolder(&BSTR::from(TASK_FOLDER)) }.unwrap();
        let task = unsafe { folder.GetTask(&BSTR::from(self.task_name.as_str())) }.unwrap();
        let running = unsafe { task.Run(&VARIANT::default()) }.unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match unsafe { running.State() } {
                Ok(state) if state == TASK_STATE_RUNNING => return running,
                Ok(_) => {}
                Err(error) if error.code() == SCHED_E_TASK_NOT_RUNNING => {}
                Err(error) => panic!("could not read the controlled task state: {error}"),
            }
            if Instant::now() >= deadline {
                let state = unsafe { task.State() };
                let last_task_result = unsafe { task.LastTaskResult() };
                let last_run_time = unsafe { task.LastRunTime() };
                panic!(
                    "the controlled task did not start: State={state:?}, LastTaskResult={last_task_result:?}, LastRunTime={last_run_time:?}"
                );
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_until_task_stops(&self) {
        let folder = unsafe { self.service.GetFolder(&BSTR::from(TASK_FOLDER)) }.unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let task = unsafe { folder.GetTask(&BSTR::from(self.task_name.as_str())) }.unwrap();
            if unsafe { task.State() }.unwrap() != TASK_STATE_RUNNING {
                return;
            }
            assert!(Instant::now() < deadline, "the test task did not stop");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for SchedulerFixture {
    fn drop(&mut self) {
        if self.owns_task {
            if let Ok(folder) = unsafe { self.service.GetFolder(&BSTR::from(TASK_FOLDER)) } {
                if let Ok(task) = unsafe { folder.GetTask(&BSTR::from(self.task_name.as_str())) } {
                    let _ = unsafe { task.Stop(0) };
                }
                let _ = unsafe { folder.DeleteTask(&BSTR::from(self.task_name.as_str()), 0) };
            }
        }
        if self.remove_machine_payload {
            let _ = std::fs::remove_file(MACHINE_PAYLOAD);
        }
        if self.remove_machine_root {
            let _ = std::fs::remove_dir(MACHINE_ROOT);
        }
        if self.remove_test_root {
            let _ = std::fs::remove_dir(TEST_ROOT);
        }
        if self.remove_folder {
            if let Ok(root) = unsafe { self.service.GetFolder(&BSTR::from(r"\")) } {
                let _ = unsafe { root.DeleteFolder(&BSTR::from("Muniment"), 0) };
            }
        }
        if let Some(payload) = &self.payload {
            let _ = std::fs::remove_file(payload);
        }
    }
}

struct TestComApartment;

impl TestComApartment {
    fn new() -> Self {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.unwrap();
        Self
    }
}

impl Drop for TestComApartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}
