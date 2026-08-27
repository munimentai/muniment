//! Live registration access through the Windows Task Scheduler.

use crate::windows_task::{
    parse_observed_registration, plan_task_registration, plan_task_removal,
    render_task_definition_xml, task_uri, ObservedRegistration, ParseObservedRegistrationError,
    RemovalScope, RenderTaskDefinitionError, SidError, TaskDefinition, TaskDefinitionError,
    TaskRegistrationPlan, TaskRemovalPlan,
};
use std::fmt;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::{Interface, BSTR, HRESULT};
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, ITaskDefinition, ITaskFolder, ITaskService, TaskScheduler, TASK_CREATE_OR_UPDATE,
    TASK_ENUM_HIDDEN, TASK_LOGON_INTERACTIVE_TOKEN, TASK_LOGON_NONE, TASK_LOGON_TYPE, TASK_UPDATE,
};
use windows::Win32::System::Variant::VARIANT;

const TASK_FOLDER: &str = r"\Muniment";
const HRESULT_FILE_NOT_FOUND: HRESULT = HRESULT(0x8007_0002_u32 as i32);
const HRESULT_PATH_NOT_FOUND: HRESULT = HRESULT(0x8007_0003_u32 as i32);
const HRESULT_ALREADY_EXISTS: HRESULT = HRESULT(0x8007_00b7_u32 as i32);
const SCHED_E_TASK_NOT_FOUND: HRESULT = HRESULT(0x8004_130f_u32 as i32);

/// A failure while reading a runtime task registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadObservedRegistrationError {
    InvalidSid(SidError),
    InitializeCom(HRESULT),
    CreateTaskService(HRESULT),
    ConnectTaskService(HRESULT),
    OpenTaskFolder(HRESULT),
    OpenTask(HRESULT),
    ReadTaskXml(HRESULT),
    InvalidTaskXmlText,
    ParseTaskXml(ParseObservedRegistrationError),
}

impl fmt::Display for ReadObservedRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidSid(_) => "the task SID is not canonical",
            Self::InitializeCom(_) => "could not initialize COM",
            Self::CreateTaskService(_) => "could not create the Task Scheduler service",
            Self::ConnectTaskService(_) => "could not connect to the local Task Scheduler",
            Self::OpenTaskFolder(_) => "could not open the Muniment task folder",
            Self::OpenTask(_) => "could not open the runtime task",
            Self::ReadTaskXml(_) => "could not read the runtime task XML",
            Self::InvalidTaskXmlText => "the runtime task XML contains invalid text",
            Self::ParseTaskXml(_) => "could not parse the runtime task XML",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ReadObservedRegistrationError {}

/// A failure while writing a runtime task registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnsureTaskRegistrationError {
    InvalidSid(SidError),
    InvalidTaskDefinition(TaskDefinitionError),
    InitializeCom(HRESULT),
    CreateTaskService(HRESULT),
    ConnectTaskService(HRESULT),
    OpenTaskFolder(HRESULT),
    OpenTask(HRESULT),
    ReadTaskXml(HRESULT),
    InvalidTaskXmlText,
    ParseTaskXml(ParseObservedRegistrationError),
    OpenRootFolder(HRESULT),
    CreateTaskFolder(HRESULT),
    CreateTaskDefinition(HRESULT),
    RenderTaskXml(RenderTaskDefinitionError),
    SetTaskXml(HRESULT),
    RegisterTaskDefinition(HRESULT),
    Refused,
}

impl fmt::Display for EnsureTaskRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidSid(_) => "the task SID is not canonical",
            Self::InvalidTaskDefinition(_) => "the runtime task definition is invalid",
            Self::InitializeCom(_) => "could not initialize COM",
            Self::CreateTaskService(_) => "could not create the Task Scheduler service",
            Self::ConnectTaskService(_) => "could not connect to the local Task Scheduler",
            Self::OpenTaskFolder(_) => "could not open the Muniment task folder",
            Self::OpenTask(_) => "could not open the runtime task",
            Self::ReadTaskXml(_) => "could not read the runtime task XML",
            Self::InvalidTaskXmlText => "the runtime task XML contains invalid text",
            Self::ParseTaskXml(_) => "could not parse the runtime task XML",
            Self::OpenRootFolder(_) => "could not open the Task Scheduler root folder",
            Self::CreateTaskFolder(_) => "could not create the Muniment task folder",
            Self::CreateTaskDefinition(_) => "could not create the runtime task definition",
            Self::RenderTaskXml(_) => "could not render the runtime task XML",
            Self::SetTaskXml(_) => "could not set the runtime task XML",
            Self::RegisterTaskDefinition(_) => "could not register the runtime task",
            Self::Refused => "refused to replace a foreign runtime task",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for EnsureTaskRegistrationError {}

/// A failure while removing or repointing a runtime task registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyTaskRemovalError {
    InvalidSid(SidError),
    InitializeCom(HRESULT),
    CreateTaskService(HRESULT),
    ConnectTaskService(HRESULT),
    OpenTaskFolder(HRESULT),
    OpenTask(HRESULT),
    ReadTaskXml(HRESULT),
    InvalidTaskXmlText,
    ParseTaskXml(ParseObservedRegistrationError),
    StopTask(HRESULT),
    DeleteTask(HRESULT),
    ReadRemainingTasks(HRESULT),
    CountRemainingTasks(HRESULT),
    OpenRootFolder(HRESULT),
    DeleteTaskFolder(HRESULT),
    ReadTaskDefinition(HRESULT),
    ReadTaskActions(HRESULT),
    ReadTaskAction(HRESULT),
    ReadExecAction(HRESULT),
    SetActionPath(HRESULT),
    RegisterTaskDefinition(HRESULT),
}

impl fmt::Display for ApplyTaskRemovalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::InvalidSid(_) => "the task SID is not canonical",
            Self::InitializeCom(_) => "could not initialize COM",
            Self::CreateTaskService(_) => "could not create the Task Scheduler service",
            Self::ConnectTaskService(_) => "could not connect to the local Task Scheduler",
            Self::OpenTaskFolder(_) => "could not open the Muniment task folder",
            Self::OpenTask(_) => "could not open the runtime task",
            Self::ReadTaskXml(_) => "could not read the runtime task XML",
            Self::InvalidTaskXmlText => "the runtime task XML contains invalid text",
            Self::ParseTaskXml(_) => "could not parse the runtime task XML",
            Self::StopTask(_) => "could not stop the runtime task",
            Self::DeleteTask(_) => "could not delete the runtime task",
            Self::ReadRemainingTasks(_) => "could not read the remaining Muniment tasks",
            Self::CountRemainingTasks(_) => "could not count the remaining Muniment tasks",
            Self::OpenRootFolder(_) => "could not open the Task Scheduler root folder",
            Self::DeleteTaskFolder(_) => "could not delete the Muniment task folder",
            Self::ReadTaskDefinition(_) => "could not read the runtime task definition",
            Self::ReadTaskActions(_) => "could not read the runtime task actions",
            Self::ReadTaskAction(_) => "could not read the runtime task action",
            Self::ReadExecAction(_) => "the runtime task action is not executable",
            Self::SetActionPath(_) => "could not set the runtime task action path",
            Self::RegisterTaskDefinition(_) => "could not register the runtime task",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ApplyTaskRemovalError {}

/// Reads the runtime task registration for a canonical user SID.
pub fn read_observed_registration(
    sid: &str,
) -> Result<Option<ObservedRegistration>, ReadObservedRegistrationError> {
    let uri = task_uri(sid).map_err(ReadObservedRegistrationError::InvalidSid)?;
    let task_name = uri
        .strip_prefix(r"\Muniment\")
        .expect("task_uri always returns a task in the Muniment folder");

    let _apartment = ComApartment::initialize()?;
    let service: ITaskService = unsafe {
        CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| ReadObservedRegistrationError::CreateTaskService(error.code()))?
    };
    let empty = VARIANT::default();
    unsafe { service.Connect(&empty, &empty, &empty, &empty) }
        .map_err(|error| ReadObservedRegistrationError::ConnectTaskService(error.code()))?;

    let folder = match unsafe { service.GetFolder(&BSTR::from(TASK_FOLDER)) } {
        Ok(folder) => folder,
        Err(error) if is_absent(error.code()) => return Ok(None),
        Err(error) => return Err(ReadObservedRegistrationError::OpenTaskFolder(error.code())),
    };
    let task = match unsafe { folder.GetTask(&BSTR::from(task_name)) } {
        Ok(task) => task,
        Err(error) if is_absent(error.code()) => return Ok(None),
        Err(error) => return Err(ReadObservedRegistrationError::OpenTask(error.code())),
    };
    let xml = unsafe { task.Xml() }
        .map_err(|error| ReadObservedRegistrationError::ReadTaskXml(error.code()))?;
    let xml =
        String::try_from(&xml).map_err(|_| ReadObservedRegistrationError::InvalidTaskXmlText)?;
    let observed = parse_observed_registration(&uri, &xml)
        .map_err(ReadObservedRegistrationError::ParseTaskXml)?;

    Ok(Some(observed))
}

/// Makes the planned runtime task registration change and returns its plan.
pub fn ensure_task_registration(
    sid: &str,
    payload_path: impl AsRef<Path>,
    machine_payload_root: impl AsRef<Path>,
    user_payload_root: impl AsRef<Path>,
) -> Result<TaskRegistrationPlan, EnsureTaskRegistrationError> {
    let expected = TaskDefinition::new(sid, payload_path).map_err(|error| match error {
        TaskDefinitionError::InvalidSid => {
            EnsureTaskRegistrationError::InvalidSid(SidError::NotCanonical)
        }
        error => EnsureTaskRegistrationError::InvalidTaskDefinition(error),
    })?;
    let observed = read_observed_registration(sid).map_err(map_read_error)?;
    let plan = plan_task_registration(
        &expected,
        observed.as_ref(),
        machine_payload_root,
        user_payload_root,
    );

    match plan {
        TaskRegistrationPlan::LeaveUnchanged => return Ok(plan),
        TaskRegistrationPlan::Refuse => return Err(EnsureTaskRegistrationError::Refused),
        TaskRegistrationPlan::Register | TaskRegistrationPlan::Update => {}
    }

    let xml = render_task_definition_xml(&expected)
        .map_err(EnsureTaskRegistrationError::RenderTaskXml)?;
    let task_name = expected
        .uri
        .strip_prefix(r"\Muniment\")
        .expect("TaskDefinition::new always returns a task in the Muniment folder");

    let _apartment = ComApartment::initialize_for_write()?;
    let service: ITaskService = unsafe {
        CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| EnsureTaskRegistrationError::CreateTaskService(error.code()))?
    };
    let empty = VARIANT::default();
    unsafe { service.Connect(&empty, &empty, &empty, &empty) }
        .map_err(|error| EnsureTaskRegistrationError::ConnectTaskService(error.code()))?;
    let folder = open_or_create_task_folder(&service, &empty)?;
    let definition = unsafe { service.NewTask(0) }
        .map_err(|error| EnsureTaskRegistrationError::CreateTaskDefinition(error.code()))?;
    unsafe { definition.SetXmlText(&BSTR::from(xml)) }
        .map_err(|error| EnsureTaskRegistrationError::SetTaskXml(error.code()))?;
    register_task_definition(
        &folder,
        task_name,
        &definition,
        TASK_CREATE_OR_UPDATE.0,
        TASK_LOGON_INTERACTIVE_TOKEN,
        &empty,
    )
    .map_err(EnsureTaskRegistrationError::RegisterTaskDefinition)?;

    Ok(plan)
}

/// Makes the planned runtime task removal change and returns its plan.
pub fn apply_task_removal(
    sid: &str,
    scope: &RemovalScope,
) -> Result<TaskRemovalPlan, ApplyTaskRemovalError> {
    let Some(observed) = read_observed_registration(sid).map_err(map_removal_read_error)? else {
        return Ok(TaskRemovalPlan::LeaveUnchanged);
    };
    let plan = plan_task_removal(scope, &observed);
    if plan == TaskRemovalPlan::LeaveUnchanged {
        return Ok(plan);
    }

    let task_name = observed
        .uri
        .strip_prefix(r"\Muniment\")
        .expect("read_observed_registration uses a Muniment task URI");
    let _apartment = ComApartment::initialize_for_removal()?;
    let service: ITaskService = unsafe {
        CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| ApplyTaskRemovalError::CreateTaskService(error.code()))?
    };
    let empty = VARIANT::default();
    unsafe { service.Connect(&empty, &empty, &empty, &empty) }
        .map_err(|error| ApplyTaskRemovalError::ConnectTaskService(error.code()))?;
    let folder = unsafe { service.GetFolder(&BSTR::from(TASK_FOLDER)) }
        .map_err(|error| ApplyTaskRemovalError::OpenTaskFolder(error.code()))?;
    let task = unsafe { folder.GetTask(&BSTR::from(task_name)) }
        .map_err(|error| ApplyTaskRemovalError::OpenTask(error.code()))?;

    match &plan {
        TaskRemovalPlan::StopAndDelete => {
            unsafe { task.Stop(0) }
                .map_err(|error| ApplyTaskRemovalError::StopTask(error.code()))?;
            unsafe { folder.DeleteTask(&BSTR::from(task_name), 0) }
                .map_err(|error| ApplyTaskRemovalError::DeleteTask(error.code()))?;
            let tasks = unsafe { folder.GetTasks(TASK_ENUM_HIDDEN.0) }
                .map_err(|error| ApplyTaskRemovalError::ReadRemainingTasks(error.code()))?;
            let count = unsafe { tasks.Count() }
                .map_err(|error| ApplyTaskRemovalError::CountRemainingTasks(error.code()))?;
            if count == 0 {
                let root = unsafe { service.GetFolder(&BSTR::from(r"\")) }
                    .map_err(|error| ApplyTaskRemovalError::OpenRootFolder(error.code()))?;
                unsafe { root.DeleteFolder(&BSTR::from("Muniment"), 0) }
                    .map_err(|error| ApplyTaskRemovalError::DeleteTaskFolder(error.code()))?;
            }
        }
        TaskRemovalPlan::RepointTo(payload_path) => {
            let definition = unsafe { task.Definition() }
                .map_err(|error| ApplyTaskRemovalError::ReadTaskDefinition(error.code()))?;
            let actions = unsafe { definition.Actions() }
                .map_err(|error| ApplyTaskRemovalError::ReadTaskActions(error.code()))?;
            let action = unsafe { actions.get_Item(1) }
                .map_err(|error| ApplyTaskRemovalError::ReadTaskAction(error.code()))?;
            let action: IExecAction = action
                .cast()
                .map_err(|error| ApplyTaskRemovalError::ReadExecAction(error.code()))?;
            let payload_path: Vec<_> = payload_path.as_os_str().encode_wide().collect();
            unsafe { action.SetPath(&BSTR::from_wide(&payload_path)) }
                .map_err(|error| ApplyTaskRemovalError::SetActionPath(error.code()))?;
            register_task_definition(
                &folder,
                task_name,
                &definition,
                TASK_UPDATE.0,
                TASK_LOGON_NONE,
                &empty,
            )
            .map_err(ApplyTaskRemovalError::RegisterTaskDefinition)?;
        }
        TaskRemovalPlan::LeaveUnchanged => unreachable!(),
    }

    Ok(plan)
}

fn register_task_definition(
    folder: &ITaskFolder,
    task_name: &str,
    definition: &ITaskDefinition,
    flags: i32,
    logon_type: TASK_LOGON_TYPE,
    empty: &VARIANT,
) -> Result<(), HRESULT> {
    unsafe {
        folder.RegisterTaskDefinition(
            &BSTR::from(task_name),
            definition,
            flags,
            empty,
            empty,
            logon_type,
            empty,
        )
    }
    .map(|_| ())
    .map_err(|error| error.code())
}

fn open_or_create_task_folder(
    service: &ITaskService,
    empty: &VARIANT,
) -> Result<ITaskFolder, EnsureTaskRegistrationError> {
    match unsafe { service.GetFolder(&BSTR::from(TASK_FOLDER)) } {
        Ok(folder) => Ok(folder),
        Err(error) if is_absent(error.code()) => {
            let root = unsafe { service.GetFolder(&BSTR::from(r"\")) }
                .map_err(|error| EnsureTaskRegistrationError::OpenRootFolder(error.code()))?;
            match unsafe { root.CreateFolder(&BSTR::from("Muniment"), empty) } {
                Ok(folder) => Ok(folder),
                Err(error) if error.code() == HRESULT_ALREADY_EXISTS => {
                    unsafe { service.GetFolder(&BSTR::from(TASK_FOLDER)) }
                        .map_err(|error| EnsureTaskRegistrationError::OpenTaskFolder(error.code()))
                }
                Err(error) => Err(EnsureTaskRegistrationError::CreateTaskFolder(error.code())),
            }
        }
        Err(error) => Err(EnsureTaskRegistrationError::OpenTaskFolder(error.code())),
    }
}

fn map_read_error(error: ReadObservedRegistrationError) -> EnsureTaskRegistrationError {
    match error {
        ReadObservedRegistrationError::InvalidSid(error) => {
            EnsureTaskRegistrationError::InvalidSid(error)
        }
        ReadObservedRegistrationError::InitializeCom(code) => {
            EnsureTaskRegistrationError::InitializeCom(code)
        }
        ReadObservedRegistrationError::CreateTaskService(code) => {
            EnsureTaskRegistrationError::CreateTaskService(code)
        }
        ReadObservedRegistrationError::ConnectTaskService(code) => {
            EnsureTaskRegistrationError::ConnectTaskService(code)
        }
        ReadObservedRegistrationError::OpenTaskFolder(code) => {
            EnsureTaskRegistrationError::OpenTaskFolder(code)
        }
        ReadObservedRegistrationError::OpenTask(code) => {
            EnsureTaskRegistrationError::OpenTask(code)
        }
        ReadObservedRegistrationError::ReadTaskXml(code) => {
            EnsureTaskRegistrationError::ReadTaskXml(code)
        }
        ReadObservedRegistrationError::InvalidTaskXmlText => {
            EnsureTaskRegistrationError::InvalidTaskXmlText
        }
        ReadObservedRegistrationError::ParseTaskXml(error) => {
            EnsureTaskRegistrationError::ParseTaskXml(error)
        }
    }
}

fn map_removal_read_error(error: ReadObservedRegistrationError) -> ApplyTaskRemovalError {
    match error {
        ReadObservedRegistrationError::InvalidSid(error) => {
            ApplyTaskRemovalError::InvalidSid(error)
        }
        ReadObservedRegistrationError::InitializeCom(code) => {
            ApplyTaskRemovalError::InitializeCom(code)
        }
        ReadObservedRegistrationError::CreateTaskService(code) => {
            ApplyTaskRemovalError::CreateTaskService(code)
        }
        ReadObservedRegistrationError::ConnectTaskService(code) => {
            ApplyTaskRemovalError::ConnectTaskService(code)
        }
        ReadObservedRegistrationError::OpenTaskFolder(code) => {
            ApplyTaskRemovalError::OpenTaskFolder(code)
        }
        ReadObservedRegistrationError::OpenTask(code) => ApplyTaskRemovalError::OpenTask(code),
        ReadObservedRegistrationError::ReadTaskXml(code) => {
            ApplyTaskRemovalError::ReadTaskXml(code)
        }
        ReadObservedRegistrationError::InvalidTaskXmlText => {
            ApplyTaskRemovalError::InvalidTaskXmlText
        }
        ReadObservedRegistrationError::ParseTaskXml(error) => {
            ApplyTaskRemovalError::ParseTaskXml(error)
        }
    }
}

fn is_absent(code: HRESULT) -> bool {
    matches!(
        code,
        HRESULT_FILE_NOT_FOUND | HRESULT_PATH_NOT_FOUND | SCHED_E_TASK_NOT_FOUND
    )
}

struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn initialize() -> Result<Self, ReadObservedRegistrationError> {
        Self::initialize_inner().map_err(ReadObservedRegistrationError::InitializeCom)
    }

    fn initialize_for_write() -> Result<Self, EnsureTaskRegistrationError> {
        Self::initialize_inner().map_err(EnsureTaskRegistrationError::InitializeCom)
    }

    fn initialize_for_removal() -> Result<Self, ApplyTaskRemovalError> {
        Self::initialize_inner().map_err(ApplyTaskRemovalError::InitializeCom)
    }

    fn initialize_inner() -> Result<Self, HRESULT> {
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result.is_ok() {
            Ok(Self { uninitialize: true })
        } else if result == RPC_E_CHANGED_MODE {
            Ok(Self {
                uninitialize: false,
            })
        } else {
            Err(result)
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}
