#![cfg(target_os = "windows")]

use muniment_core::windows_sid::current_process_user_sid;
use muniment_core::windows_task::{
    render_task_definition_xml, RemovalScope, SidError, TaskDefinition, TaskRegistrationPlan,
    TaskRemovalPlan,
};
use muniment_core::windows_task_service::{
    apply_task_removal, ensure_task_registration, read_observed_registration,
    EnsureTaskRegistrationError, ReadObservedRegistrationError,
};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use windows::core::BSTR;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::TaskScheduler::{
    IRunningTask, ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE, TASK_LOGON_INTERACTIVE_TOKEN,
    TASK_STATE_RUNNING,
};
use windows::Win32::System::Variant::VARIANT;

const TASK_FOLDER: &str = r"\Muniment";
const TEST_ROOT: &str = r"C:\MunimentTaskPreflight";
const MACHINE_ROOT: &str = r"C:\MunimentTaskPreflight\Machine";
const USER_ROOT: &str = r"C:\MunimentTaskPreflight\User";
const PAYLOAD: &str = r"C:\MunimentTaskPreflight\User\muniment-runtime.exe";
const MACHINE_PAYLOAD: &str = r"C:\MunimentTaskPreflight\Machine\muniment-runtime.exe";
const FOREIGN_PAYLOAD: &str = r"C:\MunimentTaskPreflight\Foreign\muniment-runtime.exe";
const COM_HANDLER_CLASS_ID: &str = "{00000000-0000-0000-0000-000000000001}";
const COM_HANDLER_DATA: &str = "preserve-this-action";

#[test]
fn absent_runtime_task_returns_none_without_a_write() {
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

    let machine_scope = RemovalScope::Machine {
        payload_path: PathBuf::from(MACHINE_PAYLOAD),
        per_user_payload_path: None,
    };
    let _running_task = fixture.start_controlled_task();
    assert_eq!(
        apply_task_removal(sid.as_str(), &machine_scope).unwrap(),
        TaskRemovalPlan::StopAndDelete
    );
    assert_eq!(read_observed_registration(sid.as_str()).unwrap(), None);

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

struct SchedulerFixture {
    service: ITaskService,
    task_name: String,
    owns_task: bool,
    remove_folder: bool,
    remove_test_root: bool,
    remove_machine_root: bool,
    remove_machine_payload: bool,
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
            _apartment: apartment,
        }
    }

    fn register_foreign_task(&self, sid: &str) {
        let definition = TaskDefinition::new(sid, FOREIGN_PAYLOAD).unwrap();
        self.register_xml(render_task_definition_xml(&definition).unwrap());
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
        self.register_xml(xml);
    }

    fn register_xml(&self, xml: String) {
        let task = unsafe { self.service.NewTask(0) }.unwrap();
        unsafe { task.SetXmlText(&BSTR::from(xml)) }.unwrap();
        let folder = unsafe { self.service.GetFolder(&BSTR::from(TASK_FOLDER)) }.unwrap();
        let empty = VARIANT::default();
        unsafe {
            folder.RegisterTaskDefinition(
                &BSTR::from(self.task_name.as_str()),
                &task,
                TASK_CREATE_OR_UPDATE.0,
                &empty,
                &empty,
                TASK_LOGON_INTERACTIVE_TOKEN,
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
        self.remove_test_root = !Path::new(TEST_ROOT).exists();
        self.remove_machine_root = !Path::new(MACHINE_ROOT).exists();
        std::fs::create_dir_all(MACHINE_ROOT).unwrap();
        assert!(!Path::new(MACHINE_PAYLOAD).exists());
        self.remove_machine_payload = true;
        let notepad = PathBuf::from(std::env::var_os("WINDIR").unwrap())
            .join("System32")
            .join("notepad.exe");
        std::fs::copy(notepad, MACHINE_PAYLOAD).unwrap();

        let folder = unsafe { self.service.GetFolder(&BSTR::from(TASK_FOLDER)) }.unwrap();
        let task = unsafe { folder.GetTask(&BSTR::from(self.task_name.as_str())) }.unwrap();
        let running = unsafe { task.Run(&VARIANT::default()) }.unwrap();
        for _ in 0..50 {
            if unsafe { running.State() }.unwrap() == TASK_STATE_RUNNING {
                return running;
            }
            thread::sleep(Duration::from_millis(100));
        }
        panic!("the controlled task did not start");
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
