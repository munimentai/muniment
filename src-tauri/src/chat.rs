use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{SecondsFormat, Utc};
#[cfg(target_os = "linux")]
use muniment_core::attach::linux::{
    run_authenticated_session_with_service_and_approvals, AttachFilesystem, AttachTransport,
    CompanionProvenance, RunStartAccepted, RunStartRequest as AttachRunStartRequest,
    ThreadListPage, ThreadListRequest, ThreadListService,
};
#[cfg(target_os = "linux")]
use muniment_core::attach::{
    Approval, CommittedResult, ErrorCode, Id, IdempotencyOutcome, IdempotencyStore, Operation,
    Protocol, ProtocolError, Request as AttachRequest, WorkspaceOnboardRequest, WorkspaceOnboarded,
};
use muniment_core::attachment::{ingest_attachment, prepare_pi_images, AttachmentMetadata};
use muniment_core::auth::TokenSet;
use muniment_core::cas::LocalCas;
use muniment_core::journal::reducer::{
    project_chat, reduce, ChatProjection, ChatProjector, PermissionGate, PermissionRequest,
    ProjectedAttachment, RunStatus,
};
use muniment_core::journal::{EventEnvelope, EventPayload, Provenance, RunJournal};
use muniment_core::sidecar::pi_chat::{
    cancel_command, ExtensionUiDialog, ExtensionUiRequest, PiChatEvent, PiImageContent,
    PiRunAdapter, Receipt,
};
use muniment_core::sidecar::pi_install::resolve_current;
use muniment_core::sidecar::{
    pi_sidecar_config, validate_pi_session, PiRpcTransport, PiRpcWiring, PiSessionLocator,
    SidecarStatus, SidecarSupervisor,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(target_os = "linux")]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::PathBuf;
use tauri::{Emitter, Manager};
use uuid::Uuid;

use crate::auth;

const RPC_TIMEOUT: Duration = Duration::from_secs(30);
const QUEUE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ChatGrant {
    workspace: String,
    gateway_url: String,
    virtual_key: String,
    #[serde(default)]
    model: Option<String>,
    receipt_url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResult {
    run_id: String,
    attachments: Vec<ChatAttachment>,
    #[serde(skip)]
    committed_seq: u64,
    #[serde(skip)]
    accepted_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SelectedFile {
    path: PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatDelivery {
    Steer,
    FollowUp,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatQueueRequest {
    run_id: String,
    delivery: ChatDelivery,
    message: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    run_id: String,
    prompt: Option<String>,
    phase: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<Value>,
    tool_activity: Vec<ChatToolActivity>,
    attachments: Vec<ChatAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_permission: Option<ChatPendingPermission>,
    resumable: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatToolActivity {
    effect_id: String,
    display_name: Option<String>,
    status: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatEvent {
    run_id: String,
    phase: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<Value>,
    tool_activity: Vec<ChatToolActivity>,
    attachments: Vec<ChatAttachment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pending_permission: Option<ChatPendingPermission>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChatPendingPermission {
    gate_id: String,
    #[serde(flatten)]
    request: PermissionRequest,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachment {
    display_name: String,
    byte_length: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<String>,
}

fn chat_attachments(attachments: &[ProjectedAttachment]) -> Vec<ChatAttachment> {
    attachments
        .iter()
        .map(|attachment| ChatAttachment {
            display_name: attachment.display_name.clone(),
            byte_length: attachment.byte_length,
            media_type: attachment.media_type.clone(),
        })
        .collect()
}

fn chat_pending_permission(gate: Option<PermissionGate>) -> Option<ChatPendingPermission> {
    gate.map(|gate| ChatPendingPermission {
        gate_id: gate.gate_id,
        request: gate.request,
    })
}

struct ActiveRun {
    id: String,
    cancelled: Arc<AtomicBool>,
    transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
}

struct PiRuntime {
    supervisor: SidecarSupervisor,
    wiring: PiRpcWiring,
}

struct ChatStorage {
    journal: RunJournal,
    cas: LocalCas,
}

type SharedStorage = Arc<Mutex<ChatStorage>>;

pub struct ChatState {
    storage: SharedStorage,
    active: Mutex<Option<ActiveRun>>,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
}

struct RunStartRequest {
    prompt: String,
    files: Vec<SelectedFile>,
    workspace: Option<String>,
    provenance: Option<Provenance>,
}

struct RunStartLaunch {
    run_id: String,
    prompt: String,
    tokens: TokenSet,
    grant: ChatGrant,
    cancelled: Arc<AtomicBool>,
    transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
    prepared: (u64, ChatProjector),
}

trait RunStartBoundaries {
    fn active_run_exists(&self) -> bool;
    fn fresh_tokens(&self) -> Result<TokenSet, RunStartError>;
    fn configure_run(
        &self,
        run_id: &str,
        prompt: &str,
        tokens: &TokenSet,
        requested_workspace: Option<&str>,
    ) -> Result<ChatGrant, RunStartError>;
    fn install_active_run(&self, run: ActiveRun) -> Result<(), RunStartError>;
    fn prepare_run(
        &self,
        run_id: &str,
        grant: &ChatGrant,
        tokens: &TokenSet,
        files: Vec<SelectedFile>,
        provenance: Option<Provenance>,
    ) -> Result<(u64, ChatProjector), RunStartError>;
    fn project_attachments(
        &self,
        projector: &ChatProjector,
    ) -> Result<Vec<ChatAttachment>, RunStartError>;
    fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError>;
    fn clear_active_run(&self, run_id: &str);
    fn launch(&self, launch: RunStartLaunch);
}

#[derive(Debug)]
enum RunStartError {
    Unauthorized(String),
    InvalidRequest(String),
    Persistence(String),
}

impl RunStartError {
    #[cfg(target_os = "linux")]
    fn protocol_error(&self) -> ProtocolError {
        match self {
            Self::Unauthorized(_) => ProtocolError::unauthorized(),
            Self::InvalidRequest(_) => ProtocolError::invalid_request(),
            Self::Persistence(_) => ProtocolError::persistence_failed(),
        }
    }

    fn into_message(self) -> String {
        match self {
            Self::Unauthorized(message)
            | Self::InvalidRequest(message)
            | Self::Persistence(message) => message,
        }
    }
}

/// Shared, channel-neutral acceptance path for starting a desktop-owned run.
/// Channel adapters supply boundaries but cannot bypass validation or ownership.
fn start_desktop_run(
    boundaries: &impl RunStartBoundaries,
    request: RunStartRequest,
) -> Result<SubmitResult, RunStartError> {
    let (result, launch) = prepare_desktop_run(boundaries, request)?;
    boundaries.launch(launch);
    Ok(result)
}

fn prepare_desktop_run(
    boundaries: &impl RunStartBoundaries,
    request: RunStartRequest,
) -> Result<(SubmitResult, RunStartLaunch), RunStartError> {
    let prompt = request.prompt.trim().to_owned();
    if prompt.is_empty() {
        return Err(RunStartError::InvalidRequest(
            "Enter a message before sending.".into(),
        ));
    }
    if boundaries.active_run_exists() {
        return Err(RunStartError::InvalidRequest(
            "A reply is already in progress.".into(),
        ));
    }

    let tokens = boundaries.fresh_tokens()?;
    let run_id = Uuid::now_v7().to_string();
    let grant =
        boundaries.configure_run(&run_id, &prompt, &tokens, request.workspace.as_deref())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    if let Err(error) = boundaries.install_active_run(ActiveRun {
        id: run_id.clone(),
        cancelled: Arc::clone(&cancelled),
        transport: Arc::clone(&transport),
        adapter: Arc::clone(&adapter),
    }) {
        boundaries.clear_active_run(&run_id);
        return Err(error);
    }
    let prepared =
        match boundaries.prepare_run(&run_id, &grant, &tokens, request.files, request.provenance) {
            Ok(prepared) => prepared,
            Err(error) => {
                boundaries.clear_active_run(&run_id);
                return Err(error);
            }
        };
    let attachments = match boundaries.project_attachments(&prepared.1) {
        Ok(attachments) => attachments,
        Err(error) => {
            boundaries.clear_active_run(&run_id);
            return Err(error);
        }
    };
    let result = SubmitResult {
        run_id: run_id.clone(),
        attachments,
        committed_seq: prepared.0,
        accepted_at: Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
    };
    let launch = RunStartLaunch {
        run_id,
        prompt,
        tokens,
        grant,
        cancelled,
        transport,
        adapter,
        prepared,
    };
    Ok((result, launch))
}

struct TauriRunStartBoundaries<R: tauri::Runtime> {
    app: tauri::AppHandle<R>,
}

impl<R: tauri::Runtime> TauriRunStartBoundaries<R> {
    fn state(&self) -> tauri::State<'_, ChatState> {
        self.app.state::<ChatState>()
    }
}

impl<R: tauri::Runtime> RunStartBoundaries for TauriRunStartBoundaries<R> {
    fn active_run_exists(&self) -> bool {
        self.state()
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    fn fresh_tokens(&self) -> Result<TokenSet, RunStartError> {
        auth::fresh_tokens(&self.app.state::<auth::AuthState>())
            .map_err(RunStartError::Unauthorized)
    }

    fn configure_run(
        &self,
        run_id: &str,
        prompt: &str,
        tokens: &TokenSet,
        requested_workspace: Option<&str>,
    ) -> Result<ChatGrant, RunStartError> {
        let grant = fetch_grant(&tokens.access_token).map_err(map_fetch_grant_error)?;
        validate_grant(&grant).map_err(RunStartError::Persistence)?;
        if requested_workspace.is_some_and(|workspace| workspace != grant.workspace) {
            return Err(RunStartError::Unauthorized(
                "The capability is not authorized.".into(),
            ));
        }
        protect_prompt(run_id, prompt, tokens.subject.as_deref())
            .map_err(RunStartError::Persistence)?;
        Ok(grant)
    }

    fn install_active_run(&self, run: ActiveRun) -> Result<(), RunStartError> {
        install_active_run(&self.state().active, run).map_err(RunStartError::InvalidRequest)
    }

    fn prepare_run(
        &self,
        run_id: &str,
        grant: &ChatGrant,
        tokens: &TokenSet,
        files: Vec<SelectedFile>,
        provenance: Option<Provenance>,
    ) -> Result<(u64, ChatProjector), RunStartError> {
        prepare_new_run(
            &self.state().storage,
            run_id,
            &grant.workspace,
            tokens.subject.as_deref(),
            files,
            provenance,
        )
        .map_err(RunStartError::Persistence)
    }

    fn project_attachments(
        &self,
        projector: &ChatProjector,
    ) -> Result<Vec<ChatAttachment>, RunStartError> {
        projector
            .projection()
            .map(|projection| chat_attachments(&projection.attachments))
            .map_err(|_| RunStartError::Persistence(attachment_error()))
    }

    fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError> {
        let failed = event_envelope(
            &launch.run_id,
            launch.prepared.0 + 1,
            "run.failed",
            json!({"reason": "persistence"}),
            launch.tokens.subject.as_deref(),
        );
        self.state()
            .storage
            .lock()
            .map_err(|_| RunStartError::Persistence(attachment_error()))?
            .journal
            .append(launch.prepared.0, &failed)
            .map_err(|_| RunStartError::Persistence(attachment_error()))
    }

    fn clear_active_run(&self, run_id: &str) {
        clear_active_run(&self.state().active, run_id);
    }

    fn launch(&self, launch: RunStartLaunch) {
        let app = self.app.clone();
        let state = self.state();
        let storage = Arc::clone(&state.storage);
        let runtime = Arc::clone(&state.runtime);
        tauri::async_runtime::spawn_blocking(move || {
            coordinate(
                app.clone(),
                storage,
                runtime,
                launch.run_id.clone(),
                launch.prompt,
                launch.tokens.access_token,
                launch.tokens.subject,
                launch.grant,
                launch.cancelled,
                launch.transport,
                launch.adapter,
                None,
                None,
                Some(launch.prepared),
            );
            if let Some(state) = app.try_state::<ChatState>() {
                clear_active_run(&state.active, &launch.run_id);
            }
        });
    }
}

/// Production adapter from the authorized Linux attach seam into the desktop
/// coordinator. The listener lifecycle will own this service in a later slice.
#[cfg(target_os = "linux")]
trait RunStartIdempotency {
    fn execute<A, W>(
        &mut self,
        profile: &str,
        request: &AttachRequest,
        canonical_input: &Value,
        authorize: A,
        work: W,
    ) -> Result<IdempotencyOutcome, ProtocolError>
    where
        A: FnOnce() -> Result<(), ProtocolError>,
        W: FnOnce() -> Result<CommittedResult, ProtocolError>;
}

#[cfg(target_os = "linux")]
impl RunStartIdempotency for IdempotencyStore {
    fn execute<A, W>(
        &mut self,
        profile: &str,
        request: &AttachRequest,
        canonical_input: &Value,
        authorize: A,
        work: W,
    ) -> Result<IdempotencyOutcome, ProtocolError>
    where
        A: FnOnce() -> Result<(), ProtocolError>,
        W: FnOnce() -> Result<CommittedResult, ProtocolError>,
    {
        IdempotencyStore::execute(self, profile, request, canonical_input, authorize, |_| {
            work()
        })
    }
}

#[cfg(target_os = "linux")]
pub struct DesktopAttachService<B, I = IdempotencyStore> {
    boundaries: B,
    idempotency: I,
    home: PathBuf,
    workspace_contexts: Arc<Mutex<HashMap<String, HashMap<PathBuf, Option<String>>>>>,
    client_credentials: Arc<Mutex<HashMap<String, String>>>,
    credential_path: Option<PathBuf>,
    client_identity: Option<String>,
}

#[cfg(target_os = "linux")]
struct AttachListenerState {
    workspace_contexts: Arc<Mutex<HashMap<String, HashMap<PathBuf, Option<String>>>>>,
    client_credentials: Arc<Mutex<HashMap<String, String>>>,
}

#[cfg(target_os = "linux")]
impl AttachListenerState {
    fn load(credential_path: &std::path::Path) -> Result<Self, ProtocolError> {
        Ok(Self {
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(load_client_credentials(credential_path)?)),
        })
    }
}

#[derive(Default)]
pub struct AttachApprovalState {
    pending: Mutex<HashMap<String, std::sync::mpsc::SyncSender<bool>>>,
}

#[tauri::command]
pub fn attach_pairing_decide(
    state: tauri::State<'_, AttachApprovalState>,
    challenge: String,
    approve: bool,
) {
    if let Ok(mut pending) = state.pending.lock() {
        if let Some(sender) = pending.remove(&challenge) {
            let _ = sender.try_send(approve);
        }
    }
}

#[cfg(target_os = "linux")]
impl<R: tauri::Runtime> DesktopAttachService<TauriRunStartBoundaries<R>> {
    pub fn new(
        app: tauri::AppHandle<R>,
        workspace_contexts: Arc<Mutex<HashMap<String, HashMap<PathBuf, Option<String>>>>>,
        client_credentials: Arc<Mutex<HashMap<String, String>>>,
    ) -> Result<Self, ProtocolError> {
        let home = app
            .path()
            .document_dir()
            .map_err(|_| ProtocolError::persistence_failed())?
            .join("Muniment");
        let idempotency = IdempotencyStore::open(
            app.path()
                .app_data_dir()
                .map_err(|_| ProtocolError::persistence_failed())?
                .join("attach-idempotency.sqlite3"),
        )?;
        let credential_path = app
            .path()
            .app_data_dir()
            .map_err(|_| ProtocolError::persistence_failed())?
            .join("attach-client-credentials.json");
        Ok(Self {
            boundaries: TauriRunStartBoundaries { app },
            idempotency,
            home,
            workspace_contexts,
            client_credentials,
            credential_path: Some(credential_path),
            client_identity: None,
        })
    }
}

#[cfg(target_os = "linux")]
fn desktop_attach_approval() -> Option<Approval> {
    Some(Approval {
        profile: "desktop-owner".into(),
        workspace: std::env::current_dir().ok()?.to_string_lossy().into_owned(),
        scopes: BTreeSet::from(["thread.read".into(), "run.write".into()]),
        lifetime: Duration::from_secs(60 * 60),
    })
}

#[cfg(target_os = "linux")]
pub fn start_attach_listener<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    std::thread::spawn(move || {
        let Ok(filesystem) = AttachFilesystem::from_environment() else {
            return;
        };
        let Ok(listener) = AttachTransport::bind(&filesystem) else {
            return;
        };
        let credential_path = match app.path().app_data_dir() {
            Ok(path) => path.join("attach-client-credentials.json"),
            Err(_) => return,
        };
        let Ok(state) = AttachListenerState::load(&credential_path) else {
            return;
        };
        while let Ok((stream, credentials)) = listener.accept() {
            let app = app.clone();
            let workspace_contexts = state.workspace_contexts.clone();
            let client_credentials = state.client_credentials.clone();
            std::thread::spawn(move || {
                let Ok(mut service) =
                    DesktopAttachService::new(app, workspace_contexts, client_credentials)
                else {
                    return;
                };
                let approval_app = service.boundaries.app.clone();
                let _ = run_authenticated_session_with_service_and_approvals(
                    stream,
                    credentials,
                    env!("CARGO_PKG_VERSION"),
                    &mut service,
                    move |challenge: &muniment_core::attach::PairingChallenge,
                          remaining: Duration| {
                        let challenge = challenge.as_str().to_owned();
                        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
                        let state = approval_app.state::<AttachApprovalState>();
                        state.pending.lock().ok()?.insert(challenge.clone(), sender);
                        if approval_app
                            .emit("attach-pairing-requested", &challenge)
                            .is_err()
                        {
                            state.pending.lock().ok()?.remove(&challenge);
                            return Some(muniment_core::attach::linux::ApprovalDecision::Deny);
                        }
                        let approved = receiver.recv_timeout(remaining).unwrap_or(false);
                        state.pending.lock().ok()?.remove(&challenge);
                        if !approved {
                            return Some(muniment_core::attach::linux::ApprovalDecision::Deny);
                        }
                        Some(muniment_core::attach::linux::ApprovalDecision::Approve(
                            desktop_attach_approval()?,
                        ))
                    },
                );
            });
        }
    });
}

#[cfg(target_os = "linux")]
impl<B: RunStartBoundaries, I: RunStartIdempotency> ThreadListService
    for DesktopAttachService<B, I>
{
    fn bind_authorized_client(&mut self, client_identity: &str) {
        self.client_identity = Some(client_identity.to_owned());
    }

    fn reconnect_approval(&self) -> Option<Approval> {
        desktop_attach_approval()
    }

    fn authorize_client(
        &mut self,
        client_identity: &str,
        presented_credential: Option<&str>,
        issued_credential: &str,
    ) -> Result<String, ProtocolError> {
        let mut credentials = self
            .client_credentials
            .lock()
            .map_err(|_| ProtocolError::unauthorized())?;
        let credential = match credentials.get(client_identity) {
            Some(expected) if presented_credential == Some(expected.as_str()) => expected.clone(),
            Some(_) => return Err(ProtocolError::unauthorized()),
            None if presented_credential.is_none() => {
                credentials.insert(client_identity.to_owned(), issued_credential.to_owned());
                if let Some(path) = &self.credential_path {
                    if persist_client_credentials(path, &credentials).is_err() {
                        credentials.remove(client_identity);
                        return Err(ProtocolError::persistence_failed());
                    }
                }
                issued_credential.to_owned()
            }
            None => return Err(ProtocolError::unauthorized()),
        };
        self.client_identity = Some(client_identity.to_owned());
        Ok(credential)
    }

    fn onboard_workspace(
        &mut self,
        request: WorkspaceOnboardRequest,
    ) -> Result<WorkspaceOnboarded, ProtocolError> {
        let opened = PathBuf::from(&request.opened_directory);
        let memory = PathBuf::from(&request.memory_location);
        if !opened.is_absolute() || !memory.is_absolute() {
            return Err(ProtocolError::invalid_request());
        }
        let instructions = muniment_core::onboard_companion_workspace(&opened, &memory)
            .map_err(|_| ProtocolError::persistence_failed())?;
        let opened_canonical = opened
            .canonicalize()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let memory_canonical = memory
            .canonicalize()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let mut contexts = self
            .workspace_contexts
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?;
        let identity = self
            .client_identity
            .as_ref()
            .ok_or_else(ProtocolError::unauthorized)?;
        let contexts = contexts.entry(identity.clone()).or_default();
        contexts.insert(opened_canonical, instructions.clone());
        contexts.insert(memory_canonical, instructions.clone());
        Ok(WorkspaceOnboarded {
            opened_directory: opened.to_string_lossy().into_owned(),
            memory_location: memory.to_string_lossy().into_owned(),
            instructions,
        })
    }

    fn ensure_home(&mut self) -> Result<(), ProtocolError> {
        muniment_core::ensure_cross_project_home(&self.home)
            .map_err(|_| ProtocolError::persistence_failed())
    }

    fn authorized_workspace(&self, workspace: &str) -> Option<String> {
        let Some(identity) = &self.client_identity else {
            return None;
        };
        let canonical = PathBuf::from(workspace).canonicalize().ok()?;
        self.workspace_contexts.lock().ok().and_then(|contexts| {
            contexts.get(identity).and_then(|workspaces| {
                workspaces
                    .contains_key(&canonical)
                    .then(|| canonical.to_string_lossy().into_owned())
            })
        })
    }

    fn list_threads(
        &mut self,
        _workspace: &str,
        _request: ThreadListRequest,
    ) -> Result<ThreadListPage, ProtocolError> {
        Err(ProtocolError::unsupported_operation())
    }

    fn start_run(
        &mut self,
        workspace: &str,
        request: AttachRunStartRequest,
        request_id: &Id,
        idempotency_key: &Id,
        companion: CompanionProvenance,
    ) -> Result<RunStartAccepted, ProtocolError> {
        if request.context.is_some() {
            return Err(ProtocolError::unsupported_operation());
        }
        let canonical_input = json!({
            "workspace": workspace,
            "text": &request.text,
            "context": &request.context,
        });
        let ledger_request = AttachRequest {
            protocol: Protocol,
            request_id: request_id.clone(),
            operation: Operation::RunStart,
            capability: String::new(),
            idempotency_key: Some(idempotency_key.clone()),
            body: canonical_input.clone(),
        };
        let mut extra = BTreeMap::new();
        let profile = companion.profile.clone();
        extra.insert("attach_profile".into(), json!(&companion.profile));
        extra.insert("companion_kind".into(), json!(companion.companion_kind));
        extra.insert(
            "companion_version".into(),
            json!(companion.companion_version),
        );
        extra.insert("peer_uid".into(), json!(companion.peer_uid));
        extra.insert("peer_pid".into(), json!(companion.peer_pid));
        extra.insert("idempotency_key".into(), json!(idempotency_key.as_str()));
        let instructions = self
            .workspace_contexts
            .lock()
            .map_err(|_| ProtocolError::persistence_failed())?
            .get(
                self.client_identity
                    .as_ref()
                    .ok_or_else(ProtocolError::unauthorized)?,
            )
            .and_then(|contexts| contexts.get(&PathBuf::from(workspace)))
            .cloned()
            .flatten();
        if let Some(instructions) = instructions {
            extra.insert("repository_instructions".into(), json!(instructions));
        }
        let provenance = Provenance {
            source: "muniment-attach".into(),
            source_version: env!("CARGO_PKG_VERSION").into(),
            actor_id: None,
            device_id: None,
            rpc_request_id: Some(request_id.as_str().to_owned()),
            capability_versions: None,
            extra,
        };
        let mut pending_launch = None;
        let outcome = self.idempotency.execute(
            &profile,
            &ledger_request,
            &canonical_input,
            || Ok(()),
            || {
                let (result, launch) = prepare_desktop_run(
                    &self.boundaries,
                    RunStartRequest {
                        prompt: request.text,
                        files: Vec::new(),
                        workspace: Some(workspace.to_owned()),
                        provenance: Some(provenance),
                    },
                )
                .map_err(|error| error.protocol_error())?;
                pending_launch = Some(launch);
                Ok(CommittedResult {
                    body: json!({
                        "run_id": result.run_id,
                        "committed_seq": result.committed_seq,
                        "accepted_at": result.accepted_at,
                    }),
                    cursor: None,
                })
            },
        );
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                if let Some(launch) = pending_launch {
                    let _ = self.boundaries.fail_prepared_run(&launch);
                    self.boundaries.clear_active_run(&launch.run_id);
                }
                return Err(error);
            }
        };
        let committed = match outcome {
            IdempotencyOutcome::Committed(result) => {
                let launch = pending_launch.ok_or_else(ProtocolError::persistence_failed)?;
                self.boundaries.launch(launch);
                result
            }
            IdempotencyOutcome::Replayed(result) => result,
        };
        serde_json::from_value(committed.body).map_err(|_| ProtocolError::persistence_failed())
    }
}

#[cfg(target_os = "linux")]
fn load_client_credentials(
    path: &std::path::Path,
) -> Result<HashMap<String, String>, ProtocolError> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true).custom_flags(libc::O_NOFOLLOW);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(_) => return Err(ProtocolError::persistence_failed()),
    };
    let metadata = file
        .metadata()
        .map_err(|_| ProtocolError::persistence_failed())?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o077 != 0 {
        return Err(ProtocolError::persistence_failed());
    }
    let credentials: HashMap<String, String> =
        serde_json::from_reader(file).map_err(|_| ProtocolError::persistence_failed())?;
    if credentials.iter().any(|(identity, credential)| {
        Id::new(identity).is_err()
            || credential.len() != 64
            || !credential.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err(ProtocolError::persistence_failed());
    }
    Ok(credentials)
}

#[cfg(target_os = "linux")]
fn persist_client_credentials(
    path: &std::path::Path,
    credentials: &HashMap<String, String>,
) -> Result<(), ProtocolError> {
    let parent = path
        .parent()
        .ok_or_else(ProtocolError::persistence_failed)?;
    std::fs::create_dir_all(parent).map_err(|_| ProtocolError::persistence_failed())?;
    let temporary = parent.join(format!(".attach-client-credentials-{}.tmp", Uuid::now_v7()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW);
        let file = options
            .open(&temporary)
            .map_err(|_| ProtocolError::persistence_failed())?;
        serde_json::to_writer(&file, credentials)
            .map_err(|_| ProtocolError::persistence_failed())?;
        file.sync_all()
            .map_err(|_| ProtocolError::persistence_failed())?;
        std::fs::rename(&temporary, path).map_err(|_| ProtocolError::persistence_failed())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

impl ChatState {
    pub fn new(app: &tauri::AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let directory = app.path().app_data_dir()?;
        std::fs::create_dir_all(&directory)?;
        std::fs::create_dir_all(directory.join("pi-sessions"))?;
        let mut journal = RunJournal::open(directory.join("runs.sqlite3"))?;
        reconcile_interrupted_runs(&mut journal);
        Ok(Self {
            storage: Arc::new(Mutex::new(ChatStorage {
                journal,
                cas: LocalCas::open(&directory.join("cas"))?,
            })),
            active: Mutex::new(None),
            runtime: Arc::new(Mutex::new(None)),
        })
    }
}

const RESUME_PROMPT: &str =
    "Continue the interrupted response from the existing session. Do not repeat completed work.";

struct ResumeContext {
    events: Vec<EventEnvelope>,
    locator: PiSessionLocator,
}

struct ResumeAttempt {
    result: Option<std::sync::mpsc::Sender<Result<(), String>>>,
}

impl ResumeAttempt {
    fn new(result: Option<std::sync::mpsc::Sender<Result<(), String>>>) -> Self {
        Self { result }
    }

    fn accepted(&mut self) {
        if let Some(result) = self.result.take() {
            let _ = result.send(Ok(()));
        }
    }
}

impl Drop for ResumeAttempt {
    fn drop(&mut self) {
        if let Some(result) = self.result.take() {
            let _ = result.send(Err("This reply could not be resumed. Try again.".into()));
        }
    }
}

fn reconcile_interrupted_runs(journal: &mut RunJournal) {
    let Ok(run_ids) = journal.run_ids() else {
        return;
    };
    for run_id in run_ids {
        let Ok(events) = journal.events(&run_id) else {
            continue;
        };
        let Ok(state) = reduce(&events) else {
            continue;
        };
        if state.is_terminal() {
            continue;
        }
        let envelope = event_envelope(
            &run_id,
            state.last_seq + 1,
            "run.needs_attention",
            json!({"reason": "interrupted"}),
            None,
        );
        let _ = journal.append(state.last_seq, &envelope);
    }
}

const PROMPT_SERVICE: &str = "ai.muniment.desktop.chat";
const PROMPT_USER: &str = "protected-prompts";

fn prompt_user(subject: Option<&str>, run_id: &str) -> String {
    subject.filter(|value| !value.is_empty()).map_or_else(
        || format!("{PROMPT_USER}:{run_id}"),
        |value| format!("{PROMPT_USER}:{value}:{run_id}"),
    )
}

fn load_prompt(run_id: &str, subject: Option<&str>) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(PROMPT_SERVICE, &prompt_user(subject, run_id))
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("Conversation history is unavailable.".to_string()),
    }
}

fn protect_prompt(run_id: &str, prompt: &str, subject: Option<&str>) -> Result<(), String> {
    keyring::Entry::new(PROMPT_SERVICE, &prompt_user(subject, run_id))
        .and_then(|entry| entry.set_password(prompt))
        .map_err(|_| "Conversation history is unavailable.".to_string())
}

fn projection_phase(status: &Option<RunStatus>) -> &'static str {
    match status {
        Some(RunStatus::Streaming) => "streaming",
        Some(RunStatus::Completed) => "complete",
        Some(RunStatus::Cancelled) => "cancelled",
        Some(RunStatus::Failed { .. }) => "failed",
        Some(RunStatus::NeedsAttention(_)) => "interrupted",
        Some(RunStatus::PendingPermission(_)) => "pending-permission",
        _ => "thinking",
    }
}

fn history_entries(
    journal: &mut RunJournal,
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<Vec<HistoryEntry>, String> {
    let mut entries = Vec::new();
    for run_id in journal
        .run_ids()
        .map_err(|_| "Conversation history is unavailable.".to_string())?
    {
        let events = journal
            .events(&run_id)
            .map_err(|_| "Conversation history is unavailable.".to_string())?;
        if matches!(
            events.first().and_then(|event| event.provenance.actor_id.as_deref()),
            Some(owner) if Some(owner) != subject
        ) {
            continue;
        }
        let projection = project_chat(&events)
            .map_err(|_| "Conversation history is unavailable.".to_string())?;
        let resumable = resumable_context(&events, subject, session_root).is_ok();
        entries.push(HistoryEntry {
            prompt: load_prompt(&run_id, subject)?,
            phase: projection_phase(&projection.status).into(),
            text: projection.text,
            receipt: projection.receipt,
            tool_activity: chat_tool_activity(&projection.tool_activity),
            attachments: chat_attachments(&projection.attachments),
            pending_permission: chat_pending_permission(projection.pending_permission),
            resumable,
            run_id,
        });
    }
    Ok(entries)
}

#[tauri::command]
pub async fn chat_history(
    app_handle: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
) -> Result<Vec<HistoryEntry>, String> {
    // History is conversation data and follows the same signed-in gate as send.
    let tokens = auth::fresh_tokens(&auth_state)?;
    let mut storage = state
        .storage
        .lock()
        .map_err(|_| "Conversation history is unavailable.".to_string())?;
    let session_root = state_session_root(&app_handle)?;
    history_entries(
        &mut storage.journal,
        tokens.subject.as_deref(),
        &session_root,
    )
}

fn state_session_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|path| path.join("pi-sessions"))
        .map_err(|_| "Conversation history is unavailable.".to_string())
}

fn resumable_context(
    events: &[EventEnvelope],
    subject: Option<&str>,
    session_root: &std::path::Path,
) -> Result<ResumeContext, String> {
    if events.is_empty()
        || matches!(
            events.first().and_then(|event| event.provenance.actor_id.as_deref()),
            Some(owner) if Some(owner) != subject
        )
    {
        return Err("This reply cannot be resumed.".into());
    }
    let state = reduce(events).map_err(|_| "This reply cannot be resumed.".to_string())?;
    if !matches!(state.status, RunStatus::NeedsAttention(_))
        || state.pending_permission.is_some()
        || !state.running_effects.is_empty()
    {
        return Err("This reply cannot be resumed.".into());
    }
    let binding = state
        .pi_session
        .ok_or_else(|| "This reply cannot be resumed.".to_string())?;
    let (locator, _) = validate_pi_session(session_root, &binding.locator)
        .map_err(|_| "This reply cannot be resumed.".to_string())?;
    Ok(ResumeContext {
        events: events.to_vec(),
        locator,
    })
}

#[tauri::command]
pub async fn chat_submit(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    prompt: String,
    files: Option<Vec<SelectedFile>>,
) -> Result<SubmitResult, String> {
    // Preserve the command argument names while the channel-neutral boundary
    // resolves the same managed values from the owned app handle.
    let _ = (&auth_state, &state);
    tauri::async_runtime::spawn_blocking(move || {
        start_desktop_run(
            &TauriRunStartBoundaries { app },
            RunStartRequest {
                prompt,
                files: files.unwrap_or_default(),
                workspace: None,
                provenance: None,
            },
        )
        .map_err(RunStartError::into_message)
    })
    .await
    .map_err(|_| "Chat configuration is temporarily unavailable.".to_string())?
}

#[tauri::command]
pub async fn chat_resume(
    app: tauri::AppHandle,
    auth_state: tauri::State<'_, auth::AuthState>,
    state: tauri::State<'_, ChatState>,
    run_id: String,
) -> Result<SubmitResult, String> {
    let tokens = auth::fresh_tokens_async(&auth_state).await?;
    let session_root = state_session_root(&app)?;
    let resume = {
        let mut storage = state
            .storage
            .lock()
            .map_err(|_| "This reply cannot be resumed.".to_string())?;
        let events = storage
            .journal
            .events(&run_id)
            .map_err(|_| "This reply cannot be resumed.".to_string())?;
        resumable_context(&events, tokens.subject.as_deref(), &session_root)?
    };
    let attachments = chat_attachments(
        &project_chat(&resume.events)
            .map_err(|_| "This reply could not be resumed.".to_string())?
            .attachments,
    );
    let access_token = tokens.access_token.clone();
    let grant = tauri::async_runtime::spawn_blocking(move || {
        let grant = fetch_grant(&access_token).map_err(FetchGrantError::into_message)?;
        validate_grant(&grant)?;
        Ok::<_, String>(grant)
    })
    .await
    .map_err(|_| "Chat configuration is temporarily unavailable.".to_string())??;
    let cancelled = Arc::new(AtomicBool::new(false));
    let transport = Arc::new(Mutex::new(None));
    let adapter = Arc::new(Mutex::new(None));
    install_active_run(
        &state.active,
        ActiveRun {
            id: run_id.clone(),
            cancelled: Arc::clone(&cancelled),
            transport: Arc::clone(&transport),
            adapter: Arc::clone(&adapter),
        },
    )?;
    let storage = Arc::clone(&state.storage);
    let runtime = Arc::clone(&state.runtime);
    let result_id = run_id.clone();
    let (attempt_sender, attempt_receiver) = std::sync::mpsc::channel();
    tauri::async_runtime::spawn_blocking(move || {
        coordinate(
            app.clone(),
            storage,
            runtime,
            run_id.clone(),
            RESUME_PROMPT.into(),
            tokens.access_token,
            tokens.subject,
            grant,
            cancelled,
            transport,
            adapter,
            Some(resume),
            Some(attempt_sender),
            None,
        );
        if let Some(state) = app.try_state::<ChatState>() {
            let mut active = state
                .active
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if active.as_ref().is_some_and(|current| current.id == run_id) {
                *active = None;
            }
        }
    });
    tauri::async_runtime::spawn_blocking(move || attempt_receiver.recv())
        .await
        .map_err(|_| "This reply could not be resumed. Try again.".to_string())?
        .map_err(|_| "This reply could not be resumed. Try again.".to_string())??;
    Ok(SubmitResult {
        run_id: result_id,
        attachments,
        committed_seq: 0,
        accepted_at: String::new(),
    })
}

#[tauri::command]
pub fn chat_file_metadata(path: PathBuf) -> Result<ChatAttachment, String> {
    let file = std::fs::File::open(&path).map_err(|_| attachment_error())?;
    let metadata = file.metadata().map_err(|_| attachment_error())?;
    if !metadata.is_file() {
        return Err(attachment_error());
    }
    let display_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .ok_or_else(attachment_error)?
        .to_owned();
    Ok(ChatAttachment {
        display_name,
        byte_length: metadata.len(),
        media_type: None,
    })
}

fn install_active_run(active: &Mutex<Option<ActiveRun>>, run: ActiveRun) -> Result<(), String> {
    let mut active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.is_some() {
        return Err("A reply is already in progress.".into());
    }
    *active = Some(run);
    Ok(())
}

fn clear_active_run(active: &Mutex<Option<ActiveRun>>, run_id: &str) {
    let mut active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if active.as_ref().is_some_and(|current| current.id == run_id) {
        *active = None;
    }
}

fn attachment_error() -> String {
    "One or more selected files could not be added. Check the files and try again.".into()
}

fn prepared_pi_images(
    storage: &SharedStorage,
    run_id: &str,
) -> Result<Vec<PiImageContent>, String> {
    let mut storage = storage.lock().map_err(|_| attachment_error())?;
    let events = storage
        .journal
        .events(run_id)
        .map_err(|_| attachment_error())?;
    prepare_pi_images(&storage.cas, &events)
        .map_err(|_| attachment_error())
        .map(|images| {
            images
                .into_iter()
                .map(|image| PiImageContent::new(image.data, image.mime_type))
                .collect()
        })
}

struct OpenSelectedFile {
    file: std::fs::File,
    display_name: String,
    byte_length: u64,
}

fn open_selected_files(files: Vec<SelectedFile>) -> Result<Vec<OpenSelectedFile>, String> {
    files
        .into_iter()
        .map(|selected| {
            // Validate the handle that will actually be read. Checking the path before
            // opening leaves a window where it can be replaced with a different object.
            let file = std::fs::File::open(&selected.path).map_err(|_| attachment_error())?;
            let metadata = file.metadata().map_err(|_| attachment_error())?;
            if !metadata.is_file() {
                return Err(attachment_error());
            }
            let display_name = selected
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(attachment_error)?
                .to_owned();
            Ok(OpenSelectedFile {
                file,
                display_name,
                byte_length: metadata.len(),
            })
        })
        .collect()
}

fn prepare_new_run(
    storage: &SharedStorage,
    run_id: &str,
    workspace: &str,
    subject: Option<&str>,
    files: Vec<SelectedFile>,
    provenance: Option<Provenance>,
) -> Result<(u64, ChatProjector), String> {
    // Open and validate every selection before creating a run, so ordinary
    // selection failures cannot leave a rejected submission in the journal.
    let files = open_selected_files(files)?;
    prepare_opened_run(storage, run_id, workspace, subject, files, provenance)
}

fn prepare_opened_run(
    storage: &SharedStorage,
    run_id: &str,
    workspace: &str,
    subject: Option<&str>,
    files: Vec<OpenSelectedFile>,
    provenance: Option<Provenance>,
) -> Result<(u64, ChatProjector), String> {
    let mut storage = storage.lock().map_err(|_| attachment_error())?;
    let ChatStorage { journal, cas } = &mut *storage;
    let mut projector = ChatProjector::new();
    let mut seq = 1;
    let mut started = event_envelope(run_id, seq, "run.started", json!({}), subject);
    if let Some(provenance) = provenance {
        started.provenance = Provenance {
            actor_id: provenance.actor_id.or_else(|| subject.map(str::to_owned)),
            ..provenance
        };
    }
    projector.apply(&started).map_err(|_| attachment_error())?;
    if !workspace.is_empty() {
        journal
            .append_new_run(workspace, &started)
            .map_err(|_| attachment_error())?;
    } else {
        journal
            .append(0, &started)
            .map_err(|_| attachment_error())?;
    }

    for mut selected in files {
        let next_seq = seq + 1;
        let envelope_subject = subject.map(str::to_owned);
        let attachment = match ingest_attachment(
            cas,
            journal,
            seq,
            &mut selected.file,
            AttachmentMetadata {
                display_name: &selected.display_name,
                byte_length: selected.byte_length,
                media_type: None,
            },
            |attachment| {
                let mut envelope = event_envelope(
                    run_id,
                    next_seq,
                    "chat.attachment.ingested",
                    json!({}),
                    envelope_subject.as_deref(),
                );
                envelope.payload = EventPayload::Attachment { attachment };
                envelope
            },
        ) {
            Ok(attachment) => attachment,
            Err(_) => {
                record_preparation_failure(journal, &mut projector, run_id, seq, subject);
                return Err(attachment_error());
            }
        };
        let attachment_event = match journal
            .events(run_id)
            .ok()
            .and_then(|events| events.last().cloned())
        {
            Some(event) => event,
            None => {
                record_preparation_failure(journal, &mut projector, run_id, seq, subject);
                return Err(attachment_error());
            }
        };
        if projector.apply(&attachment_event).is_err() {
            record_preparation_failure(journal, &mut projector, run_id, seq, subject);
            return Err(attachment_error());
        }
        seq = next_seq;
        if cas.verify(attachment.sha256()).is_err() {
            record_preparation_failure(journal, &mut projector, run_id, seq, subject);
            return Err(attachment_error());
        }
    }
    Ok((seq, projector))
}

fn record_preparation_failure(
    journal: &mut RunJournal,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: u64,
    subject: Option<&str>,
) {
    let failed = event_envelope(
        run_id,
        seq + 1,
        "run.failed",
        json!({"reason": "attachment"}),
        subject,
    );
    if projector.apply(&failed).is_ok() {
        let _ = journal.append(seq, &failed);
    }
}

#[tauri::command]
pub async fn chat_queue(
    state: tauri::State<'_, ChatState>,
    run_id: String,
    delivery: ChatDelivery,
    message: String,
) -> Result<(), String> {
    queue_message(
        &state.active,
        ChatQueueRequest {
            run_id,
            delivery,
            message,
        },
    )
}

fn queue_message(
    active: &Mutex<Option<ActiveRun>>,
    request: ChatQueueRequest,
) -> Result<(), String> {
    let active = active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let run = active
        .as_ref()
        .filter(|run| run.id == request.run_id)
        .ok_or_else(|| "That reply is no longer active.".to_string())?;
    let transport = run
        .transport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let adapter = run
        .adapter
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    drop(active);
    let (transport, adapter) = transport
        .zip(adapter)
        .ok_or_else(|| "The reply is not ready for messages yet.".to_string())?;
    let result = match request.delivery {
        ChatDelivery::Steer => adapter.steer(&transport, &request.message, QUEUE_TIMEOUT),
        ChatDelivery::FollowUp => adapter.follow_up(&transport, &request.message, QUEUE_TIMEOUT),
    };
    result.map_err(|error| {
        if error == "Pi queued message must not be empty" {
            "Enter a message before sending.".to_string()
        } else {
            "The message could not be queued. Try again.".to_string()
        }
    })
}

#[derive(Debug)]
enum PreparedPromptError {
    Start,
    SessionRoot,
    Binding,
    Journal,
}

fn coordinate_prepared_prompt<R: tauri::Runtime, T>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    subject: Option<&str>,
    submit: impl FnOnce() -> Result<(T, PiSessionLocator, Vec<PiChatEvent>), PreparedPromptError>,
) -> Result<(T, Vec<PiChatEvent>), PreparedPromptError> {
    let (handle, locator, buffered_events) = submit()?;
    append_emit(
        app,
        journal,
        projector,
        run_id,
        seq,
        "runtime.pi_session.bound",
        json!({"run_id": run_id, "locator": locator.as_str()}),
        subject,
    )
    .map_err(|_| PreparedPromptError::Journal)?;
    append_emit(
        app,
        journal,
        projector,
        run_id,
        seq,
        "model.prompt.accepted",
        json!({}),
        subject,
    )
    .map_err(|_| PreparedPromptError::Journal)?;
    Ok((handle, buffered_events))
}

#[tauri::command]
pub async fn chat_cancel(state: tauri::State<'_, ChatState>, run_id: String) -> Result<(), String> {
    let active = state
        .active
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let run = active
        .as_ref()
        .filter(|run| run.id == run_id)
        .ok_or_else(|| "That reply is no longer active.".to_string())?;
    let cancelled = Arc::clone(&run.cancelled);
    let transport = run
        .transport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    drop(active);
    cancelled.store(true, Ordering::SeqCst);
    if let Some(transport) = transport {
        transport
            .call(cancel_command(), Duration::from_secs(2))
            .map_err(|_| "The reply could not be stopped yet. Try again.".to_string())?;
    }
    Ok(())
}

fn coordinate<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    journal: SharedStorage,
    runtime: Arc<Mutex<Option<PiRuntime>>>,
    run_id: String,
    prompt: String,
    access_token: String,
    subject: Option<String>,
    grant: ChatGrant,
    cancelled: Arc<AtomicBool>,
    active_transport: Arc<Mutex<Option<Arc<PiRpcTransport>>>>,
    active_adapter: Arc<Mutex<Option<Arc<PiRunAdapter>>>>,
    resume: Option<ResumeContext>,
    resume_result: Option<std::sync::mpsc::Sender<Result<(), String>>>,
    prepared: Option<(u64, ChatProjector)>,
) {
    let mut resume_attempt = ResumeAttempt::new(resume_result);
    let (mut seq, mut projector) = prepared.unwrap_or_else(|| {
        (
            resume.as_ref().map_or(0, |resume| {
                resume.events.last().map_or(0, |event| event.run_seq)
            }),
            ChatProjector::new(),
        )
    });
    if let Some(resume) = &resume {
        for event in &resume.events {
            if projector.apply(event).is_err() {
                return;
            }
        }
    } else if seq == 0
        && append_emit(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "run.started",
            json!({}),
            subject.as_deref(),
        )
        .is_err()
    {
        return;
    }
    if cancelled.load(Ordering::SeqCst) {
        if resume.is_none() {
            let _ = append_emit(
                &app,
                &journal,
                &mut projector,
                &run_id,
                &mut seq,
                "run.cancelled",
                json!({}),
                subject.as_deref(),
            );
        }
        return;
    }
    let images = if resume.is_some() {
        Vec::new()
    } else {
        let images = match prepared_pi_images(&journal, &run_id) {
            Ok(images) => images,
            Err(message) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &message,
                    subject.as_deref(),
                    false,
                );
                return;
            }
        };
        images
    };
    let mut runtime = runtime
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // A local run owns one Pi conversation. Do not carry a previous run's
    // active session into this prompt.
    *runtime = None;
    {
        let root = match std::env::var("MUNIMENT_PI_ROOT") {
            Ok(root) => root,
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is not installed.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        #[cfg(test)]
        let executable = std::env::var_os("MUNIMENT_PI_TEST_EXECUTABLE")
            .map(std::path::PathBuf::from)
            .map(Ok)
            .unwrap_or_else(|| resolve_current(std::path::Path::new(&root)));
        #[cfg(not(test))]
        let executable = resolve_current(std::path::Path::new(&root));
        let executable = match executable {
            Ok(path) => path,
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        let session_root = match app.path().app_data_dir() {
            Ok(path) => path.join("pi-sessions"),
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        let mut config = match pi_sidecar_config(
            executable.to_string_lossy(),
            &session_root,
            resume.as_ref().map(|resume| &resume.locator),
        ) {
            Ok(config) => config,
            Err(_) => {
                fail_start(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The agent runtime is unavailable.",
                    subject.as_deref(),
                    resume.is_some(),
                );
                return;
            }
        };
        // This is a scoped LiteLLM virtual key, never a provider credential. It is
        // inherited by the supervised child only and never serialized or logged.
        config
            .env
            .insert("OPENAI_API_KEY".into(), grant.virtual_key.clone());
        config
            .env
            .insert("OPENAI_BASE_URL".into(), grant.gateway_url.clone());
        if let Some(model) = &grant.model {
            config.env.insert("PI_DEFAULT_MODEL".into(), model.clone());
        }
        let wiring = PiRpcWiring::new();
        let supervisor =
            match SidecarSupervisor::spawn(config, wiring.readiness_probe(Duration::from_secs(10)))
            {
                Ok(value) => value,
                Err(_) => {
                    fail_start(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "The agent runtime could not start.",
                        subject.as_deref(),
                        resume.is_some(),
                    );
                    return;
                }
            };
        *runtime = Some(PiRuntime { supervisor, wiring });
    }
    let runtime = runtime.as_mut().expect("runtime was initialized");
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while runtime.supervisor.status() == SidecarStatus::Starting
        && std::time::Instant::now() < deadline
    {
        if cancelled.load(Ordering::SeqCst) {
            if resume.is_none() {
                let _ = append_emit(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "run.cancelled",
                    json!({}),
                    subject.as_deref(),
                );
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let Some(transport) = runtime.wiring.transport() else {
        fail_start(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "The agent runtime did not become ready.",
            subject.as_deref(),
            resume.is_some(),
        );
        return;
    };
    *active_transport
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&transport));
    if cancelled.load(Ordering::SeqCst) {
        if resume.is_none() {
            let _ = append_emit(
                &app,
                &journal,
                &mut projector,
                &run_id,
                &mut seq,
                "run.cancelled",
                json!({}),
                subject.as_deref(),
            );
        }
        return;
    }
    let (adapter, buffered_events) = if resume.is_some() {
        let (adapter, _) =
            match PiRunAdapter::start(run_id.clone(), &transport, &prompt, RPC_TIMEOUT) {
                Ok(value) => value,
                Err(_) => {
                    fail_start(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "The reply could not be started.",
                        subject.as_deref(),
                        true,
                    );
                    return;
                }
            };
        let adapter = Arc::new(adapter);
        *active_adapter
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&adapter));
        if append_emit(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "run.resumed",
            json!({}),
            subject.as_deref(),
        )
        .is_err()
        {
            if adapter
                .cancel_and_drain(&transport, Duration::from_secs(2))
                .is_err()
            {
                let _ = runtime.supervisor.shutdown();
            }
            return;
        }
        // Prompt acknowledgement only proves that Pi accepted work. Report a
        // successful resume after the transition is durable so callers never
        // observe an active continuation that the journal still calls
        // interrupted.
        resume_attempt.accepted();
        if append_emit(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            "model.prompt.accepted",
            json!({}),
            subject.as_deref(),
        )
        .is_err()
        {
            if adapter
                .cancel_and_drain(&transport, Duration::from_secs(2))
                .is_err()
            {
                let _ = runtime.supervisor.shutdown();
            }
            return;
        }
        (adapter, Vec::new())
    } else {
        let result = coordinate_prepared_prompt(
            &app,
            &journal,
            &mut projector,
            &run_id,
            &mut seq,
            subject.as_deref(),
            || {
                let (adapter, _) = PiRunAdapter::start_with_images(
                    run_id.clone(),
                    &transport,
                    &prompt,
                    images,
                    RPC_TIMEOUT,
                )
                .map_err(|_| PreparedPromptError::Start)?;
                let adapter = Arc::new(adapter);
                *active_adapter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    Some(Arc::clone(&adapter));
                let session_root = app
                    .path()
                    .app_data_dir()
                    .map(|path| path.join("pi-sessions"))
                    .map_err(|_| PreparedPromptError::SessionRoot)?;
                let (locator, events) = adapter
                    .await_session_binding(&transport, &session_root, RPC_TIMEOUT)
                    .map_err(|_| PreparedPromptError::Binding)?;
                Ok((adapter, locator, events))
            },
        );
        match result {
            Ok(value) => value,
            Err(error @ (PreparedPromptError::Journal | PreparedPromptError::SessionRoot)) => {
                let adapter = active_adapter
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                if adapter.is_some_and(|adapter| {
                    adapter
                        .cancel_and_drain(&transport, Duration::from_secs(2))
                        .is_err()
                }) {
                    let _ = runtime.supervisor.shutdown();
                }
                if matches!(error, PreparedPromptError::SessionRoot) {
                    fail(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        "The reply could not be started.",
                        subject.as_deref(),
                    );
                }
                return;
            }
            Err(PreparedPromptError::Binding) => {
                // `await_session_binding` aborts and drains first. Reaping the
                // supervised child is the final containment boundary if Pi did
                // not acknowledge cancellation.
                let _ = runtime.supervisor.shutdown();
                fail(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The reply could not be started.",
                    subject.as_deref(),
                );
                return;
            }
            Err(PreparedPromptError::Start) => {
                fail(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "The reply could not be started.",
                    subject.as_deref(),
                );
                return;
            }
        }
    };
    let mut buffered_events = buffered_events.into_iter();
    let mut aborting = false;
    let mut open_effects = BTreeSet::new();
    loop {
        if cancelled.swap(false, Ordering::SeqCst) {
            aborting = true;
            let _ = transport.call(cancel_command(), Duration::from_secs(2));
        }
        let event = buffered_events
            .next()
            .map(Ok)
            .unwrap_or_else(|| adapter.next(Duration::from_millis(100)));
        match event {
            Ok(PiChatEvent::TextDelta(text)) => {
                if append_emit(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    "model.stream.delta",
                    json!({"text": text}),
                    subject.as_deref(),
                )
                .is_err()
                {
                    break;
                }
            }
            Ok(PiChatEvent::Completed) if aborting => {
                let _ = append_terminal(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "run.cancelled",
                    json!({}),
                    subject.as_deref(),
                );
                break;
            }
            Ok(PiChatEvent::Completed) => {
                match fetch_receipt(&grant.receipt_url, &access_token, &run_id) {
                    Ok(receipt) => {
                        let _ = append_terminal(
                            &app,
                            &journal,
                            &mut projector,
                            &run_id,
                            &mut seq,
                            &mut open_effects,
                            "run.completed",
                            json!({"receipt": receipt}),
                            subject.as_deref(),
                        );
                        break;
                    }
                    Err(_) => {
                        fail_with_open_effects(
                            &app,
                            &journal,
                            &mut projector,
                            &run_id,
                            &mut seq,
                            &mut open_effects,
                            "The reply finished, but its receipt was unavailable.",
                            subject.as_deref(),
                        );
                        break;
                    }
                }
            }
            Ok(PiChatEvent::Cancelled) => {
                let _ = append_terminal(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "run.cancelled",
                    json!({}),
                    subject.as_deref(),
                );
                break;
            }
            Ok(PiChatEvent::Failed) => {
                fail_with_open_effects(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "The model could not complete this reply.",
                    subject.as_deref(),
                );
                break;
            }
            Ok(event @ (PiChatEvent::ToolStarted { .. } | PiChatEvent::ToolFinished { .. })) => {
                if let Some((kind, payload)) = tool_journal_entry(&event, &mut open_effects) {
                    if append_emit(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        kind,
                        payload,
                        subject.as_deref(),
                    )
                    .is_err()
                    {
                        break;
                    }
                }
            }
            Ok(event @ PiChatEvent::ExtensionUiRequest(_)) => {
                if coordinate_extension_ui_request(event, |kind, payload| {
                    append_emit(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        kind,
                        payload,
                        subject.as_deref(),
                    )
                })
                .is_err()
                {
                    break;
                }
            }
            Ok(PiChatEvent::Interleaved | PiChatEvent::PromptAccepted) => {}
            Err(error) if error == "timed out waiting for Pi stream" => {
                if matches!(
                    runtime.supervisor.status(),
                    SidecarStatus::Failed | SidecarStatus::Stopped
                ) {
                    fail_with_open_effects(
                        &app,
                        &journal,
                        &mut projector,
                        &run_id,
                        &mut seq,
                        &mut open_effects,
                        "The agent runtime stopped unexpectedly.",
                        subject.as_deref(),
                    );
                    break;
                }
            }
            Err(_) => {
                fail_with_open_effects(
                    &app,
                    &journal,
                    &mut projector,
                    &run_id,
                    &mut seq,
                    &mut open_effects,
                    "The agent runtime stopped unexpectedly.",
                    subject.as_deref(),
                );
                break;
            }
        }
    }
}

fn coordinate_extension_ui_request(
    event: PiChatEvent,
    append: impl FnOnce(&str, Value) -> Result<(), ()>,
) -> Result<(), ()> {
    let PiChatEvent::ExtensionUiRequest(request) = event else {
        return Ok(());
    };
    append("permission.requested", permission_journal_payload(&request))
}

fn tool_journal_entry(
    event: &PiChatEvent,
    open_effects: &mut BTreeSet<String>,
) -> Option<(&'static str, Value)> {
    match event {
        PiChatEvent::ToolStarted {
            tool_call_id,
            tool_name,
        } if open_effects.insert(tool_call_id.clone()) => Some((
            "tool.effect.started",
            json!({"effect_id": tool_call_id, "display_name": tool_name}),
        )),
        PiChatEvent::ToolFinished {
            tool_call_id,
            failed,
        } if open_effects.remove(tool_call_id) => Some((
            if *failed {
                "tool.effect.failed"
            } else {
                "tool.effect.completed"
            },
            json!({"effect_id": tool_call_id}),
        )),
        _ => None,
    }
}

fn permission_journal_payload(request: &ExtensionUiRequest) -> Value {
    let gate_id = request.id.clone();
    let timeout = request.timeout;
    let request = match &request.dialog {
        ExtensionUiDialog::Select { title, options } => PermissionRequest::Select {
            title: title.clone(),
            options: options.clone(),
            timeout,
        },
        ExtensionUiDialog::Confirm { title, message } => PermissionRequest::Confirm {
            title: title.clone(),
            message: message.clone(),
            timeout,
        },
        ExtensionUiDialog::Input { title, placeholder } => PermissionRequest::Input {
            title: title.clone(),
            placeholder: placeholder.clone(),
            timeout,
        },
        ExtensionUiDialog::Editor { title, prefill } => PermissionRequest::Editor {
            title: title.clone(),
            prefill: prefill.clone(),
            timeout,
        },
    };
    serde_json::to_value(PermissionGate { gate_id, request })
        .expect("permission gate is serializable")
}

fn close_open_effects(
    open_effects: &mut BTreeSet<String>,
    mut append: impl FnMut(&str, Value) -> Result<(), ()>,
) -> Result<(), ()> {
    for effect_id in open_effects.clone() {
        append("tool.effect.failed", json!({"effect_id": effect_id}))?;
        open_effects.remove(&effect_id);
    }
    Ok(())
}

fn append_terminal<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut BTreeSet<String>,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> Result<(), ()> {
    close_open_effects(open_effects, |kind, payload| {
        append_emit(app, journal, projector, run_id, seq, kind, payload, subject)
    })?;
    append_emit(app, journal, projector, run_id, seq, kind, payload, subject)
}

fn fail_with_open_effects<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    open_effects: &mut BTreeSet<String>,
    reason: &str,
    subject: Option<&str>,
) {
    let _ = append_terminal(
        app,
        journal,
        projector,
        run_id,
        seq,
        open_effects,
        "run.failed",
        json!({"reason": reason}),
        subject,
    );
}

fn chat_tool_activity(
    activity: &[muniment_core::journal::reducer::ToolActivity],
) -> Vec<ChatToolActivity> {
    activity
        .iter()
        .map(|activity| ChatToolActivity {
            effect_id: activity.effect_id.clone(),
            display_name: activity.display_name.clone(),
            status: match activity.status {
                muniment_core::journal::reducer::ToolActivityStatus::Running => "running",
                muniment_core::journal::reducer::ToolActivityStatus::Completed => "completed",
                muniment_core::journal::reducer::ToolActivityStatus::Failed => "failed",
            }
            .into(),
        })
        .collect()
}

fn append_emit<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> Result<(), ()> {
    *seq += 1;
    let envelope = event_envelope(run_id, *seq, kind, payload, subject);
    let mut next_projector = projector.clone();
    next_projector.apply(&envelope).map_err(|_| ())?;
    let projection = next_projector.projection().map_err(|_| ())?;
    {
        let mut storage = journal.lock().map_err(|_| ())?;
        storage
            .journal
            .append(*seq - 1, &envelope)
            .map_err(|_| ())?;
    }
    *projector = next_projector;
    app.emit("chat-event", chat_event(run_id, projection))
        .map_err(|_| ())
}

fn chat_event(run_id: &str, projection: ChatProjection) -> ChatEvent {
    let attachments = chat_attachments(&projection.attachments);
    ChatEvent {
        run_id: run_id.into(),
        phase: projection_phase(&projection.status).into(),
        text: projection.text,
        receipt: projection.receipt,
        tool_activity: chat_tool_activity(&projection.tool_activity),
        attachments,
        pending_permission: chat_pending_permission(projection.pending_permission),
    }
}

fn event_envelope(
    run_id: &str,
    run_seq: u64,
    kind: &str,
    payload: Value,
    subject: Option<&str>,
) -> EventEnvelope {
    EventEnvelope {
        event_id: Uuid::now_v7().to_string(),
        run_id: run_id.into(),
        run_seq,
        event_type: kind.into(),
        event_version: 1,
        envelope_version: 1,
        recorded_at: Utc::now().to_rfc3339_opts(SecondsFormat::AutoSi, true),
        occurred_at: None,
        correlation_id: None,
        causation_id: None,
        payload: EventPayload::Inline {
            payload_json: payload,
        },
        provenance: Provenance {
            source: "muniment-desktop".into(),
            source_version: env!("CARGO_PKG_VERSION").into(),
            actor_id: subject.map(str::to_owned),
            device_id: None,
            rpc_request_id: None,
            capability_versions: None,
            extra: BTreeMap::new(),
        },
        extra: BTreeMap::new(),
    }
}

fn fail<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    reason: &str,
    subject: Option<&str>,
) {
    let _ = append_emit(
        app,
        journal,
        projector,
        run_id,
        seq,
        "run.failed",
        json!({"reason": reason}),
        subject,
    );
}

fn fail_start<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    journal: &SharedStorage,
    projector: &mut ChatProjector,
    run_id: &str,
    seq: &mut u64,
    reason: &str,
    subject: Option<&str>,
    resuming: bool,
) {
    if !resuming {
        fail(app, journal, projector, run_id, seq, reason, subject);
    }
}

enum FetchGrantError {
    Unauthorized,
    Internal(String),
}

impl FetchGrantError {
    fn into_message(self) -> String {
        match self {
            Self::Unauthorized => "The capability is not authorized.".into(),
            Self::Internal(message) => message,
        }
    }
}

fn map_fetch_grant_error(error: FetchGrantError) -> RunStartError {
    match error {
        FetchGrantError::Unauthorized => {
            RunStartError::Unauthorized("The capability is not authorized.".into())
        }
        FetchGrantError::Internal(message) => RunStartError::Persistence(message),
    }
}

fn grant_status_error(status: u16) -> FetchGrantError {
    if matches!(status, 401 | 403) {
        FetchGrantError::Unauthorized
    } else {
        FetchGrantError::Internal("Chat configuration is temporarily unavailable.".into())
    }
}

fn fetch_grant(access_token: &str) -> Result<ChatGrant, FetchGrantError> {
    let issuer =
        std::env::var("MUNIMENT_ISSUER").unwrap_or_else(|_| "https://api.muniment.ai".into());
    ureq::post(&format!(
        "{}/v1/desktop/chat/config",
        issuer.trim_end_matches('/')
    ))
    .set("Authorization", &format!("Bearer {access_token}"))
    .call()
    .map_err(|error| match error {
        ureq::Error::Status(status, _) => grant_status_error(status),
        _ => FetchGrantError::Internal("Chat configuration is temporarily unavailable.".into()),
    })?
    .into_json()
    .map_err(|_| FetchGrantError::Internal("The chat configuration response was invalid.".into()))
}

fn validate_grant(grant: &ChatGrant) -> Result<(), String> {
    if !grant.gateway_url.starts_with("https://")
        || !grant.receipt_url.starts_with("https://")
        || grant.virtual_key.trim().is_empty()
        || grant.workspace.trim().is_empty()
    {
        return Err("The chat configuration response was invalid.".into());
    }
    Ok(())
}

fn fetch_receipt(url: &str, access_token: &str, run_id: &str) -> Result<Receipt, ()> {
    ureq::post(url)
        .set("Authorization", &format!("Bearer {access_token}"))
        .send_json(json!({"runId": run_id}))
        .map_err(|_| ())?
        .into_json()
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static PI_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(target_os = "linux")]
    struct FailingFinalization;

    #[cfg(target_os = "linux")]
    impl RunStartIdempotency for FailingFinalization {
        fn execute<A, W>(
            &mut self,
            _profile: &str,
            _request: &AttachRequest,
            _canonical_input: &Value,
            authorize: A,
            work: W,
        ) -> Result<IdempotencyOutcome, ProtocolError>
        where
            A: FnOnce() -> Result<(), ProtocolError>,
            W: FnOnce() -> Result<CommittedResult, ProtocolError>,
        {
            authorize()?;
            let _ = work()?;
            Err(ProtocolError::persistence_failed())
        }
    }

    struct FakeRunStartBoundaries {
        active: bool,
        granted_workspaces: Vec<String>,
        auth_calls: AtomicUsize,
        prompt_protection_calls: AtomicUsize,
        prepare_calls: AtomicUsize,
        launch_calls: AtomicUsize,
        launched_run: Mutex<Option<String>>,
        auth_error: Option<String>,
        configure_error: Option<String>,
        install_error: Option<String>,
        prepare_error: Option<String>,
        projection_error: Option<String>,
        prepared_provenance: Mutex<Option<Provenance>>,
        journaled_events: Mutex<BTreeMap<String, Vec<EventEnvelope>>>,
        clear_calls: AtomicUsize,
    }

    impl FakeRunStartBoundaries {
        fn accepting() -> Self {
            Self {
                active: false,
                granted_workspaces: vec!["workspace-a".into()],
                auth_calls: AtomicUsize::new(0),
                prompt_protection_calls: AtomicUsize::new(0),
                prepare_calls: AtomicUsize::new(0),
                launch_calls: AtomicUsize::new(0),
                launched_run: Mutex::new(None),
                auth_error: None,
                configure_error: None,
                install_error: None,
                prepare_error: None,
                projection_error: None,
                prepared_provenance: Mutex::new(None),
                journaled_events: Mutex::new(BTreeMap::new()),
                clear_calls: AtomicUsize::new(0),
            }
        }
    }

    impl RunStartBoundaries for FakeRunStartBoundaries {
        fn active_run_exists(&self) -> bool {
            self.active
        }

        fn fresh_tokens(&self) -> Result<TokenSet, RunStartError> {
            self.auth_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = &self.auth_error {
                return Err(RunStartError::Unauthorized(error.clone()));
            }
            Ok(TokenSet {
                access_token: "access-token".into(),
                refresh_token: None,
                expires_at: None,
                subject: Some("owner".into()),
            })
        }

        fn configure_run(
            &self,
            _run_id: &str,
            _prompt: &str,
            _tokens: &TokenSet,
            requested_workspace: Option<&str>,
        ) -> Result<ChatGrant, RunStartError> {
            if requested_workspace
                .is_some_and(|workspace| !self.granted_workspaces.iter().any(|g| g == workspace))
            {
                return Err(RunStartError::Unauthorized(
                    "sensitive workspace detail".into(),
                ));
            }
            self.prompt_protection_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(error) = &self.configure_error {
                return Err(RunStartError::Persistence(error.clone()));
            }
            // Model the gateway granting exactly the workspace that was
            // requested (and authorized), so the capability check
            // `requested == grant.workspace` holds for every granted workspace.
            let workspace = requested_workspace
                .map(str::to_owned)
                .or_else(|| self.granted_workspaces.first().cloned())
                .unwrap_or_default();
            Ok(ChatGrant {
                workspace,
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                receipt_url: "https://receipt.invalid".into(),
            })
        }

        fn install_active_run(&self, _run: ActiveRun) -> Result<(), RunStartError> {
            if let Some(error) = &self.install_error {
                return Err(RunStartError::InvalidRequest(error.clone()));
            }
            Ok(())
        }

        fn prepare_run(
            &self,
            run_id: &str,
            _grant: &ChatGrant,
            tokens: &TokenSet,
            _files: Vec<SelectedFile>,
            provenance: Option<Provenance>,
        ) -> Result<(u64, ChatProjector), RunStartError> {
            self.prepare_calls.fetch_add(1, Ordering::SeqCst);
            *self.prepared_provenance.lock().unwrap() = provenance;
            if let Some(error) = &self.prepare_error {
                return Err(RunStartError::Persistence(error.clone()));
            }
            let mut projector = ChatProjector::new();
            let started = event_envelope(
                run_id,
                1,
                "run.started",
                json!({}),
                tokens.subject.as_deref(),
            );
            projector.apply(&started).unwrap();
            self.journaled_events
                .lock()
                .unwrap()
                .insert(run_id.to_owned(), vec![started]);
            Ok((1, projector))
        }

        fn project_attachments(
            &self,
            projector: &ChatProjector,
        ) -> Result<Vec<ChatAttachment>, RunStartError> {
            if let Some(error) = &self.projection_error {
                return Err(RunStartError::Persistence(error.clone()));
            }
            projector
                .projection()
                .map(|projection| chat_attachments(&projection.attachments))
                .map_err(|_| RunStartError::Persistence(attachment_error()))
        }

        fn fail_prepared_run(&self, launch: &RunStartLaunch) -> Result<(), RunStartError> {
            self.journaled_events
                .lock()
                .unwrap()
                .get_mut(&launch.run_id)
                .unwrap()
                .push(event_envelope(
                    &launch.run_id,
                    launch.prepared.0 + 1,
                    "run.failed",
                    json!({"reason": "persistence"}),
                    launch.tokens.subject.as_deref(),
                ));
            Ok(())
        }

        fn clear_active_run(&self, _run_id: &str) {
            self.clear_calls.fetch_add(1, Ordering::SeqCst);
        }

        fn launch(&self, launch: RunStartLaunch) {
            self.launch_calls.fetch_add(1, Ordering::SeqCst);
            *self.launched_run.lock().unwrap() = Some(launch.run_id);
        }
    }

    #[test]
    fn run_start_coordinator_accepts_through_injected_boundaries() {
        let boundaries = FakeRunStartBoundaries::accepting();

        let result = start_desktop_run(
            &boundaries,
            RunStartRequest {
                prompt: "  hello  ".into(),
                files: Vec::new(),
                workspace: None,
                provenance: None,
            },
        )
        .unwrap();

        assert_eq!(boundaries.auth_calls.load(Ordering::SeqCst), 1);
        assert_eq!(result.attachments.len(), 0);
        assert_eq!(
            boundaries.launched_run.lock().unwrap().as_deref(),
            Some(result.run_id.as_str())
        );
    }

    #[test]
    fn run_start_coordinator_rejects_when_another_run_is_active() {
        let boundaries = FakeRunStartBoundaries {
            active: true,
            ..FakeRunStartBoundaries::accepting()
        };

        let error = start_desktop_run(
            &boundaries,
            RunStartRequest {
                prompt: "hello".into(),
                files: Vec::new(),
                workspace: None,
                provenance: None,
            },
        )
        .err()
        .unwrap();

        assert_eq!(error.into_message(), "A reply is already in progress.");
        assert_eq!(boundaries.auth_calls.load(Ordering::SeqCst), 0);
        assert!(boundaries.launched_run.lock().unwrap().is_none());
    }

    #[cfg(target_os = "linux")]
    fn attach_start(
        boundaries: FakeRunStartBoundaries,
        context: Option<Value>,
    ) -> (
        Result<RunStartAccepted, ProtocolError>,
        DesktopAttachService<FakeRunStartBoundaries>,
    ) {
        let mut service = DesktopAttachService {
            boundaries,
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let result = service.start_run(
            "workspace-a",
            AttachRunStartRequest {
                text: "hello".into(),
                context,
            },
            &Id::new("018f0000-0000-7000-8000-000000000001").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000002").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        );
        (result, service)
    }

    #[cfg(target_os = "linux")]
    fn attach_start_on(
        service: &mut DesktopAttachService<FakeRunStartBoundaries>,
        text: &str,
        request_id: &str,
        idempotency_key: &str,
    ) -> Result<RunStartAccepted, ProtocolError> {
        service.start_run(
            "workspace-a",
            AttachRunStartRequest {
                text: text.into(),
                context: None,
            },
            &Id::new(request_id).unwrap(),
            &Id::new(idempotency_key).unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        )
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_returns_receipt_and_records_companion_provenance() {
        let (result, service) = attach_start(FakeRunStartBoundaries::accepting(), None);
        let accepted = result.unwrap();

        assert_eq!(accepted.committed_seq, 1);
        assert!(chrono::DateTime::parse_from_rfc3339(&accepted.accepted_at).is_ok());
        assert_eq!(
            service.boundaries.launched_run.lock().unwrap().as_deref(),
            Some(accepted.run_id.as_str())
        );
        let provenance = service
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap()
            .clone()
            .unwrap();
        assert_eq!(
            provenance.rpc_request_id.as_deref(),
            Some("018f0000-0000-7000-8000-000000000001")
        );
        assert_eq!(
            provenance.extra["idempotency_key"],
            "018f0000-0000-7000-8000-000000000002"
        );
        assert_eq!(provenance.extra["companion_kind"], "cli");
        assert_eq!(provenance.extra["peer_uid"], 1000);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_onboarding_context_survives_connections_and_keys_distinct_external_roots() {
        let root = std::env::temp_dir().join(format!("muniment-attach-context-{}", Uuid::now_v7()));
        let first = root.join("repo-one");
        let second = root.join("repo-two");
        let first_memory = root.join("memory-one");
        let second_memory = root.join("memory-two");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(first.join("AGENTS.md"), "first instructions").unwrap();
        std::fs::write(second.join("AGENTS.md"), "second instructions").unwrap();
        let contexts = Arc::new(Mutex::new(HashMap::new()));
        let mut first_connection = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        first_connection
            .onboard_workspace(WorkspaceOnboardRequest {
                opened_directory: first.to_string_lossy().into_owned(),
                memory_location: first_memory.to_string_lossy().into_owned(),
            })
            .unwrap();
        drop(first_connection);

        // The fake coordinator grants exactly the workspace the gateway would
        // hand back for this client -- here the canonical repository the run
        // targets. A run requesting any other workspace is rejected by
        // `configure_run`, mirroring the real capability check.
        let first_canonical = first.canonicalize().unwrap().to_string_lossy().into_owned();
        let mut second_connection = DesktopAttachService {
            boundaries: FakeRunStartBoundaries {
                granted_workspaces: vec![first_canonical.clone()],
                ..FakeRunStartBoundaries::accepting()
            },
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        second_connection
            .onboard_workspace(WorkspaceOnboardRequest {
                opened_directory: second.to_string_lossy().into_owned(),
                memory_location: second_memory.to_string_lossy().into_owned(),
            })
            .unwrap();

        let guard = contexts.lock().unwrap();
        let stored = guard.get("default").unwrap();
        assert_eq!(
            stored
                .get(&first.canonicalize().unwrap())
                .unwrap()
                .as_deref(),
            Some("first instructions")
        );
        assert_eq!(
            stored
                .get(&second.canonicalize().unwrap())
                .unwrap()
                .as_deref(),
            Some("second instructions")
        );
        assert_eq!(
            stored
                .get(&first_memory.canonicalize().unwrap())
                .unwrap()
                .as_deref(),
            Some("first instructions")
        );
        // Release the shared `contexts` lock before the client-b/second/third
        // connections call back into the service -- those methods re-lock the
        // same mutex, so holding the guard here would deadlock (dropping the
        // former `stored` reference was a no-op that left the guard live).
        drop(guard);
        let client_b = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("client-b".into()),
        };
        // Grants are bound to the authorizing client: a second identity cannot
        // borrow another client's onboarded workspaces. `authorized_workspace`
        // is the gate the dispatcher applies before a run is ever started, so it
        // resolves nothing for client-b even though the contexts are shared.
        for workspace in [&first, &first_memory] {
            assert!(client_b
                .authorized_workspace(&workspace.to_string_lossy())
                .is_none());
        }
        assert!(client_b
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap()
            .is_none());
        // The onboarding client resolves its own repository through the same
        // gate (canonicalizing the request) before starting the run.
        let first_authorized = second_connection
            .authorized_workspace(&first.to_string_lossy())
            .unwrap();
        assert_eq!(first_authorized, first_canonical);
        let run = second_connection
            .start_run(
                &first_authorized,
                AttachRunStartRequest {
                    text: "use repository context".into(),
                    context: None,
                },
                &Id::new("018f0000-0000-7000-8000-000000000011").unwrap(),
                &Id::new("018f0000-0000-7000-8000-000000000012").unwrap(),
                CompanionProvenance {
                    profile: "default".into(),
                    companion_kind: "editor-extension".into(),
                    companion_version: "1.2.3".into(),
                    peer_uid: 1000,
                    peer_pid: 42,
                },
            )
            .unwrap();
        assert!(!run.run_id.is_empty());
        let provenance = second_connection
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap();
        assert_eq!(
            provenance.as_ref().unwrap().extra["repository_instructions"],
            "first instructions"
        );
        drop(provenance);
        let second_memory_canonical = second_memory
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut third_connection = DesktopAttachService {
            boundaries: FakeRunStartBoundaries {
                granted_workspaces: vec![second_memory_canonical.clone()],
                ..FakeRunStartBoundaries::accepting()
            },
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let second_memory_authorized = third_connection
            .authorized_workspace(&second_memory.to_string_lossy())
            .unwrap();
        assert_eq!(second_memory_authorized, second_memory_canonical);
        third_connection
            .start_run(
                &second_memory_authorized,
                AttachRunStartRequest {
                    text: "second context".into(),
                    context: None,
                },
                &Id::new("018f0000-0000-7000-8000-000000000021").unwrap(),
                &Id::new("018f0000-0000-7000-8000-000000000022").unwrap(),
                CompanionProvenance {
                    profile: "default".into(),
                    companion_kind: "editor-extension".into(),
                    companion_version: "1.2.3".into(),
                    peer_uid: 1000,
                    peer_pid: 42,
                },
            )
            .unwrap();
        assert_eq!(
            third_connection
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .extra["repository_instructions"],
            "second instructions"
        );
        assert!(!root.join("home").exists());
        second_connection.ensure_home().unwrap();
        for child in ["memory", "agents", "projects", "sessions"] {
            assert!(root.join("home").join(child).join("README.md").is_file());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_credentials_reject_impersonation_and_canonical_grants_reject_retargeting() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("muniment-attach-grants-{}", Uuid::now_v7()));
        let first = root.join("first");
        let second = root.join("second");
        let memory = root.join("memory");
        let alias = root.join("alias");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        symlink(&first, &alias).unwrap();
        let state = AttachListenerState::load(&root.join("credentials.json")).unwrap();
        let contexts = state.workspace_contexts;
        let credentials = state.client_credentials;
        let make_service = || DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: contexts.clone(),
            client_credentials: credentials.clone(),
            credential_path: None,
            client_identity: None,
        };

        let mut client_a = make_service();
        assert_eq!(
            client_a
                .authorize_client("client-a", None, &"aa".repeat(32))
                .unwrap(),
            "aa".repeat(32)
        );
        client_a
            .onboard_workspace(WorkspaceOnboardRequest {
                opened_directory: alias.to_string_lossy().into_owned(),
                memory_location: memory.to_string_lossy().into_owned(),
            })
            .unwrap();
        assert_eq!(
            client_a.authorized_workspace(&alias.to_string_lossy()),
            Some(first.to_string_lossy().into_owned())
        );
        assert!(client_a
            .authorized_workspace(&root.join("missing").to_string_lossy())
            .is_none());
        std::fs::remove_file(&alias).unwrap();
        symlink(&second, &alias).unwrap();
        assert!(client_a
            .authorized_workspace(&alias.to_string_lossy())
            .is_none());

        let mut impersonator = make_service();
        for credential in [None, Some("cc".repeat(32))] {
            assert_eq!(
                impersonator
                    .authorize_client("client-a", credential.as_deref(), &"bb".repeat(32))
                    .unwrap_err()
                    .code(),
                ErrorCode::Unauthorized
            );
        }
        assert!(impersonator.client_identity.is_none());
        assert!(impersonator
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap()
            .is_none());

        let mut reconnect = make_service();
        assert_eq!(
            reconnect
                .authorize_client("client-a", Some(&"aa".repeat(32)), &"dd".repeat(32))
                .unwrap(),
            "aa".repeat(32)
        );
        assert_eq!(
            reconnect.authorized_workspace(&first.to_string_lossy()),
            Some(first.to_string_lossy().into_owned())
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_client_credentials_survive_restart_and_unsafe_state_fails_closed() {
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root =
            std::env::temp_dir().join(format!("muniment-attach-credentials-{}", Uuid::now_v7()));
        let path = root.join("credentials.json");
        let identity = "018f0000-0000-7000-8000-000000000099";
        let credential = "ab".repeat(32);
        let make_service = |credentials| DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: root.join("home"),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(credentials)),
            credential_path: Some(path.clone()),
            client_identity: None,
        };

        let mut initial = make_service(HashMap::new());
        assert_eq!(
            initial
                .authorize_client(identity, None, &credential)
                .unwrap(),
            credential
        );
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        drop(initial);

        let loaded = load_client_credentials(&path).unwrap();
        let mut restarted = make_service(loaded);
        assert_eq!(
            restarted
                .authorize_client(identity, Some(&credential), &"cd".repeat(32))
                .unwrap(),
            credential
        );
        for presented in [None, Some("00".repeat(32))] {
            let mut rejected = make_service(load_client_credentials(&path).unwrap());
            assert_eq!(
                rejected
                    .authorize_client(identity, presented.as_deref(), &"ef".repeat(32))
                    .unwrap_err()
                    .code(),
                ErrorCode::Unauthorized
            );
            assert!(rejected.client_identity.is_none());
        }
        let mut unknown = make_service(load_client_credentials(&path).unwrap());
        assert_eq!(
            unknown
                .authorize_client(
                    "018f0000-0000-7000-8000-000000000100",
                    Some(&credential),
                    &"ef".repeat(32)
                )
                .unwrap_err()
                .code(),
            ErrorCode::Unauthorized
        );

        std::fs::write(&path, b"not-json").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load_client_credentials(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        let target = root.join("target");
        std::fs::write(&target, b"{}").unwrap();
        symlink(&target, &path).unwrap();
        assert!(load_client_credentials(&path).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn production_listener_state_isolates_workspace_grants_across_transport_connections() {
        use muniment_attach::{handshake_stream_with_credential, ClientError};
        use muniment_core::attach::linux::ApprovalDecision;
        use std::os::unix::fs::{symlink, PermissionsExt};

        let root =
            std::env::temp_dir().join(format!("muniment-listener-isolation-{}", Uuid::now_v7()));
        let opened = root.join("opened");
        let memory = root.join("memory");
        let other = root.join("other");
        let alias = root.join("alias");
        std::fs::create_dir_all(&opened).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(opened.join("AGENTS.md"), "private client A context").unwrap();
        symlink(&opened, &alias).unwrap();
        let contexts = Arc::new(Mutex::new(HashMap::new()));
        let credentials = Arc::new(Mutex::new(HashMap::new()));
        let identity_a = "018f0000-0000-7000-8000-0000000000a1";
        let identity_b = "018f0000-0000-7000-8000-0000000000b1";
        // The attach socket is bound at `<runtime>/muniment/attach-v1.sock`; a
        // runtime path nested under the descriptive `root` overruns the AF_UNIX
        // `sun_path` limit (108 bytes) when the client connects. Bind each
        // connection under a short, dedicated temp directory -- created 0700 to
        // satisfy the runtime-directory secrecy check the same way a real
        // XDG_RUNTIME_DIR is -- and remove them when the test finishes.
        let runtime_dirs = std::cell::RefCell::new(Vec::new());
        let connect = |identity: &'static str,
                       credential: Option<String>,
                       boundaries: FakeRunStartBoundaries| {
            let runtime =
                std::env::temp_dir().join(format!("mt-attach-{}", Uuid::now_v7().simple()));
            std::fs::create_dir(&runtime).unwrap();
            std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
            runtime_dirs.borrow_mut().push(runtime.clone());
            let filesystem = AttachFilesystem::from_runtime_directory(&runtime).unwrap();
            let transport = AttachTransport::bind(&filesystem).unwrap();
            let client_stream =
                std::os::unix::net::UnixStream::connect(transport.local_path()).unwrap();
            let (server_stream, peer) = transport.accept().unwrap();
            let contexts = contexts.clone();
            let credentials = credentials.clone();
            let service_root = root.clone();
            let approval_workspace = opened.clone();
            let worker = std::thread::spawn(move || {
                let mut service = DesktopAttachService {
                    boundaries,
                    idempotency: IdempotencyStore::open(":memory:").unwrap(),
                    home: service_root.join("home"),
                    workspace_contexts: contexts,
                    client_credentials: credentials,
                    credential_path: None,
                    client_identity: None,
                };
                let result = run_authenticated_session_with_service_and_approvals(
                    server_stream,
                    peer,
                    "0.0.1",
                    &mut service,
                    |_: &muniment_core::attach::PairingChallenge, _: Duration| {
                        Some(ApprovalDecision::Approve(Approval {
                            profile: "desktop-owner".into(),
                            workspace: approval_workspace.to_string_lossy().into_owned(),
                            scopes: BTreeSet::from(["thread.read".into(), "run.write".into()]),
                            lifetime: Duration::from_secs(3600),
                        }))
                    },
                );
                (result, service)
            });
            let client = handshake_stream_with_credential(
                client_stream,
                "0.0.1",
                identity,
                credential.as_deref(),
                Duration::from_secs(1),
                Duration::from_secs(1),
                || {},
            );
            (client, worker)
        };

        let (client_a, worker) = connect(identity_a, None, FakeRunStartBoundaries::accepting());
        let mut client_a = client_a.unwrap();
        let credential_a = client_a.authorized_client_credential().to_owned();
        client_a
            .onboard_workspace(&alias.to_string_lossy(), &memory.to_string_lossy())
            .unwrap();
        drop(client_a);
        assert!(worker.join().unwrap().0.is_ok());

        // Client A is authorized for both the opened repository and its memory
        // root, so the coordinator grants either when a run requests it.
        let a_dispatch = FakeRunStartBoundaries {
            granted_workspaces: vec![
                opened
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                memory
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ],
            ..FakeRunStartBoundaries::accepting()
        };
        let (client_a, worker) = connect(identity_a, Some(credential_a.clone()), a_dispatch);
        let mut client_a = client_a.unwrap();
        client_a
            .start_run_in_workspace("opened", None, Some(&opened.to_string_lossy()))
            .unwrap();
        client_a
            .start_run_in_workspace("override", None, Some(&memory.to_string_lossy()))
            .unwrap();
        drop(client_a);
        let (_, service) = worker.join().unwrap();
        assert_eq!(
            service
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .extra["repository_instructions"],
            "private client A context"
        );

        let (client_b, worker) = connect(identity_b, None, FakeRunStartBoundaries::accepting());
        let mut client_b = client_b.unwrap();
        let credential_b = client_b.authorized_client_credential().to_owned();
        client_b
            .onboard_workspace(&other.to_string_lossy(), &other.to_string_lossy())
            .unwrap();
        drop(client_b);
        assert!(worker.join().unwrap().0.is_ok());
        for workspace in [&opened, &memory] {
            let b_dispatch = FakeRunStartBoundaries::accepting();
            let (client_b, worker) = connect(identity_b, Some(credential_b.clone()), b_dispatch);
            let mut client_b = client_b.unwrap();
            // The workspace is not among client-b's grants, so the dispatcher
            // resolves it to an `Unauthorized` protocol error, which the client
            // surfaces as `AuthorizationExpired` (see `map_protocol_error`).
            assert_eq!(
                client_b.start_run_in_workspace("borrow", None, Some(&workspace.to_string_lossy())),
                Err(ClientError::AuthorizationExpired)
            );
            drop(client_b);
            let (_, service) = worker.join().unwrap();
            assert!(service
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .is_none());
            assert!(service.boundaries.launched_run.lock().unwrap().is_none());
        }

        for (identity, credential) in [
            (identity_a, None),
            (identity_a, Some("malformed".into())),
            (identity_a, Some("00".repeat(32))),
            (
                "018f0000-0000-7000-8000-0000000000ff",
                Some(credential_a.clone()),
            ),
        ] {
            let denied = FakeRunStartBoundaries::accepting();
            let (client, worker) = connect(identity, credential, denied);
            assert!(client.is_err());
            let (_, service) = worker.join().unwrap();
            assert!(service
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .is_none());
            assert!(service.client_identity.is_none());
        }

        for workspace in [root.join("missing"), alias.clone()] {
            if workspace == alias {
                std::fs::remove_file(&alias).unwrap();
                symlink(&other, &alias).unwrap();
            }
            let denied = FakeRunStartBoundaries::accepting();
            let (client, worker) = connect(identity_a, Some(credential_a.clone()), denied);
            let mut client = client.unwrap();
            // A missing directory or a workspace retargeted through a symlink
            // resolves to nothing authorized, so the run is denied the same way.
            assert_eq!(
                client.start_run_in_workspace("invalid", None, Some(&workspace.to_string_lossy())),
                Err(ClientError::AuthorizationExpired)
            );
            drop(client);
            let (_, service) = worker.join().unwrap();
            assert!(service
                .boundaries
                .prepared_provenance
                .lock()
                .unwrap()
                .is_none());
            assert!(service.boundaries.launched_run.lock().unwrap().is_none());
        }
        std::fs::remove_dir_all(root).unwrap();
        for runtime in runtime_dirs.borrow().iter() {
            let _ = std::fs::remove_dir_all(runtime);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_replays_exact_retry_without_second_coordinator_run() {
        let mut service = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let key = "018f0000-0000-7000-8000-000000000002";
        let first = attach_start_on(
            &mut service,
            "hello",
            "018f0000-0000-7000-8000-000000000001",
            key,
        )
        .unwrap();
        let replay = attach_start_on(
            &mut service,
            "hello",
            "018f0000-0000-7000-8000-000000000003",
            key,
        )
        .unwrap();

        assert_eq!(replay, first);
        assert_eq!(
            service
                .boundaries
                .prompt_protection_calls
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(service.boundaries.prepare_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service.boundaries.launch_calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_rejects_conflicting_key_without_second_coordinator_run() {
        let mut service = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let key = "018f0000-0000-7000-8000-000000000002";
        attach_start_on(
            &mut service,
            "hello",
            "018f0000-0000-7000-8000-000000000001",
            key,
        )
        .unwrap();
        let conflict = attach_start_on(
            &mut service,
            "different",
            "018f0000-0000-7000-8000-000000000003",
            key,
        )
        .unwrap_err();

        assert_eq!(
            serde_json::to_value(conflict).unwrap()["code"],
            "idempotency_conflict"
        );
        assert_eq!(
            service
                .boundaries
                .prompt_protection_calls
                .load(Ordering::SeqCst),
            1
        );
        assert_eq!(service.boundaries.prepare_calls.load(Ordering::SeqCst), 1);
        assert_eq!(service.boundaries.launch_calls.load(Ordering::SeqCst), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_rejects_in_flight_and_unsupported_context() {
        let boundaries = FakeRunStartBoundaries {
            active: true,
            ..FakeRunStartBoundaries::accepting()
        };
        let (in_flight, _) = attach_start(boundaries, None);
        assert_eq!(
            serde_json::to_value(in_flight.unwrap_err()).unwrap()["code"],
            "invalid_request"
        );

        let (context, service) = attach_start(
            FakeRunStartBoundaries::accepting(),
            Some(json!({"cwd": "/private/path"})),
        );
        assert_eq!(
            serde_json::to_value(context.unwrap_err()).unwrap()["code"],
            "unsupported_operation"
        );
        assert_eq!(service.boundaries.auth_calls.load(Ordering::SeqCst), 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_rejects_a_grant_for_another_workspace() {
        let mut service = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: IdempotencyStore::open(":memory:").unwrap(),
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let result = service.start_run(
            "workspace-b",
            AttachRunStartRequest {
                text: "hello".into(),
                context: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000001").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000002").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        );

        assert_eq!(
            serde_json::to_value(result.unwrap_err()).unwrap()["code"],
            "unauthorized"
        );
        assert_eq!(
            service
                .boundaries
                .prompt_protection_calls
                .load(Ordering::SeqCst),
            0
        );
        assert!(service.boundaries.launched_run.lock().unwrap().is_none());
        assert!(service
            .boundaries
            .prepared_provenance
            .lock()
            .unwrap()
            .is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_redacts_coordinator_failure_and_clears_active_run() {
        let boundaries = FakeRunStartBoundaries {
            prepare_error: Some(
                "token secret-token path /private/work sidecar socket unavailable".into(),
            ),
            ..FakeRunStartBoundaries::accepting()
        };
        let (result, service) = attach_start(boundaries, None);
        let encoded = serde_json::to_string(&result.unwrap_err()).unwrap();

        assert!(encoded.contains("persistence_failed"));
        assert!(!encoded.contains("secret-token"));
        assert!(!encoded.contains("/private/work"));
        assert!(!encoded.contains("sidecar"));
        assert_eq!(service.boundaries.clear_calls.load(Ordering::SeqCst), 1);
        assert!(service.boundaries.launched_run.lock().unwrap().is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_does_not_launch_when_receipt_finalization_fails() {
        let mut service = DesktopAttachService {
            boundaries: FakeRunStartBoundaries::accepting(),
            idempotency: FailingFinalization,
            home: PathBuf::new(),
            workspace_contexts: Arc::new(Mutex::new(HashMap::new())),
            client_credentials: Arc::new(Mutex::new(HashMap::new())),
            credential_path: None,
            client_identity: Some("default".into()),
        };
        let result = service.start_run(
            "workspace-a",
            AttachRunStartRequest {
                text: "private prompt".into(),
                context: None,
            },
            &Id::new("018f0000-0000-7000-8000-000000000001").unwrap(),
            &Id::new("018f0000-0000-7000-8000-000000000002").unwrap(),
            CompanionProvenance {
                profile: "default".into(),
                companion_kind: "cli".into(),
                companion_version: "1.2.3".into(),
                peer_uid: 1000,
                peer_pid: 42,
            },
        );

        let encoded = serde_json::to_string(&result.unwrap_err()).unwrap();
        assert!(encoded.contains("persistence_failed"));
        assert!(!encoded.contains("private prompt"));
        assert_eq!(service.boundaries.launch_calls.load(Ordering::SeqCst), 0);
        assert_eq!(service.boundaries.clear_calls.load(Ordering::SeqCst), 1);
        let journaled_events = service.boundaries.journaled_events.lock().unwrap();
        let events = journaled_events.values().next().unwrap();
        assert_eq!(events.last().unwrap().event_type, "run.failed");
        assert!(reduce(events).unwrap().is_terminal());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn attach_adapter_classifies_and_redacts_every_coordinator_failure_stage() {
        const DETAIL: &str =
            "prompt private-prompt token secret-token path /private/work sidecar socket";
        let cases = [
            (
                "unauthorized",
                FakeRunStartBoundaries {
                    auth_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
            (
                "persistence_failed",
                FakeRunStartBoundaries {
                    configure_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
            (
                "invalid_request",
                FakeRunStartBoundaries {
                    install_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
            (
                "persistence_failed",
                FakeRunStartBoundaries {
                    prepare_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
            (
                "persistence_failed",
                FakeRunStartBoundaries {
                    projection_error: Some(DETAIL.into()),
                    ..FakeRunStartBoundaries::accepting()
                },
            ),
        ];

        for (expected_code, boundaries) in cases {
            let (result, _) = attach_start(boundaries, None);
            let encoded = serde_json::to_string(&result.unwrap_err()).unwrap();
            assert!(encoded.contains(expected_code), "{encoded}");
            for secret in ["private-prompt", "secret-token", "/private/work", "sidecar"] {
                assert!(!encoded.contains(secret), "leaked {secret}: {encoded}");
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn grant_http_authentication_statuses_are_unauthorized_and_redacted() {
        for status in [401, 403] {
            let error = map_fetch_grant_error(grant_status_error(status)).protocol_error();
            let encoded = serde_json::to_string(&error).unwrap();
            assert!(encoded.contains("unauthorized"), "{encoded}");
            for secret in ["private-prompt", "secret-token", "/private/work", "sidecar"] {
                assert!(!encoded.contains(secret), "leaked {secret}: {encoded}");
            }
        }

        let internal = map_fetch_grant_error(grant_status_error(500)).protocol_error();
        assert_eq!(
            serde_json::to_value(internal).unwrap()["code"],
            "persistence_failed"
        );
    }

    fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        match mutex.lock() {
            Ok(guard) => guard,
            Err(error) => error.into_inner(),
        }
    }

    // Acquire the shared PI-environment lock without propagating poisoning: if a
    // prior test panicked while holding the guard (e.g. a slow CI VM tripped the
    // receipt-handshake deadline), recover the guard instead of turning that one
    // flake into cascaded PoisonError failures across every sibling test.
    fn lock_pi_environment() -> std::sync::MutexGuard<'static, ()> {
        lock_unpoisoned(&PI_ENV_LOCK)
    }

    #[test]
    fn lock_unpoisoned_recovers_guard_after_panic() {
        let mutex = Mutex::new(0);

        std::thread::scope(|scope| {
            let result = scope.spawn(|| {
                let mut value = mutex.lock().unwrap();
                *value = 42;
                panic!("poison mutex");
            });
            assert!(result.join().is_err());
        });

        assert!(mutex.is_poisoned());
        let value = lock_unpoisoned(&mutex);
        assert_eq!(*value, 42);
    }

    fn accept_receipt_request(listener: std::net::TcpListener) -> std::net::TcpStream {
        listener.set_nonblocking(true).unwrap();
        // 60s (not 10s): on the contended single linux CI VM the coordinator can
        // take well over 10s to open its receipt connection. A healthy run still
        // connects in milliseconds; this is only a generous ceiling before we
        // declare the handshake genuinely broken.
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            match listener.accept() {
                Ok((stream, _)) => return stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "coordinator did not request its receipt"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("receipt listener failed: {error}"),
            }
        }
    }

    fn append_test_event(
        journal: &mut RunJournal,
        run_id: &str,
        seq: u64,
        kind: &str,
        payload: Value,
        subject: Option<&str>,
    ) {
        journal
            .append(
                seq - 1,
                &event_envelope(run_id, seq, kind, payload, subject),
            )
            .unwrap();
    }

    fn inactive_transport_run(id: &str) -> ActiveRun {
        ActiveRun {
            id: id.into(),
            cancelled: Arc::new(AtomicBool::new(false)),
            transport: Arc::new(Mutex::new(None)),
            adapter: Arc::new(Mutex::new(None)),
        }
    }

    #[test]
    fn new_run_ingests_multiple_files_into_one_sequence_and_projector() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory =
            std::env::temp_dir().join(format!("muniment-attachments-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.txt");
        let second = directory.join("second.bin");
        std::fs::write(&first, b"first attachment").unwrap();
        std::fs::write(&second, b"second attachment").unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();

        let (seq, mut projector) = prepare_new_run(
            &storage,
            &run_id,
            "workspace-a",
            Some("owner"),
            vec![SelectedFile { path: first }, SelectedFile { path: second }],
            None,
        )
        .unwrap();
        assert_eq!(seq, 3);
        let mut storage = storage.lock().unwrap();
        assert!(storage
            .journal
            .run_belongs_to_workspace(&run_id, "workspace-a")
            .unwrap());
        assert!(!storage
            .journal
            .run_belongs_to_workspace(&run_id, "owner")
            .unwrap());
        assert_eq!(
            storage
                .journal
                .workspace_run_summaries("workspace-a", 10, None)
                .unwrap()
                .summaries
                .len(),
            1
        );
        assert!(storage
            .journal
            .projected_thread_entries("workspace-a", &run_id, seq, 0, 10)
            .is_ok());
        assert!(storage
            .journal
            .projected_thread_entries("owner", &run_id, seq, 0, 10)
            .is_err());
        let events = storage.journal.events(&run_id).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            [
                "run.started",
                "chat.attachment.ingested",
                "chat.attachment.ingested",
            ]
        );
        for event in &events[1..] {
            let EventPayload::Attachment { attachment } = &event.payload else {
                panic!("attachment payload")
            };
            storage.cas.verify(attachment.sha256()).unwrap();
        }
        let projection = projector.projection().unwrap();
        assert_eq!(
            projection
                .attachments
                .iter()
                .map(|attachment| attachment.display_name.as_str())
                .collect::<Vec<_>>(),
            ["first.txt", "second.bin"]
        );
        let public = serde_json::to_string(&chat_attachments(&projection.attachments)).unwrap();
        assert!(!public.contains("sha256"));
        assert!(!public.contains(directory.to_string_lossy().as_ref()));
        let history = history_entries(&mut storage.journal, Some("owner"), &directory).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].attachments.len(), 2);
        let restored = serde_json::to_string(&history).unwrap();
        assert!(!restored.contains("sha256"));
        assert!(!restored.contains(directory.to_string_lossy().as_ref()));
        let serialized = serde_json::to_string(&events).unwrap();
        assert!(!serialized.contains(directory.to_string_lossy().as_ref()));
        let next = event_envelope(
            &run_id,
            4,
            "assistant.delta",
            json!({"text":"ready"}),
            Some("owner"),
        );
        projector.apply(&next).unwrap();
        storage.journal.append(3, &next).unwrap();
        assert_eq!(
            storage
                .journal
                .events(&run_id)
                .unwrap()
                .last()
                .unwrap()
                .run_seq,
            4
        );
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn missing_attachment_returns_non_path_leaking_copy_before_pi_can_start() {
        let _environment = lock_pi_environment();
        let directory = std::env::temp_dir().join(format!("muniment-missing-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let prompt_log = directory.join("prompt.txt");
        std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let missing = directory.join("private-name.txt");
        let error = match prepare_new_run(
            &storage,
            &Uuid::now_v7().to_string(),
            "workspace-a",
            Some("owner"),
            vec![SelectedFile {
                path: missing.clone(),
            }],
            None,
        ) {
            Ok(_) => panic!("missing attachment must fail"),
            Err(error) => error,
        };
        assert_eq!(error, attachment_error());
        assert!(!error.contains(missing.to_string_lossy().as_ref()));
        assert!(!prompt_log.exists(), "Pi must not receive a prompt");
        std::env::remove_var("PI_RESUME_STUB_PROMPTS");
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn non_regular_attachment_is_rejected_before_a_run_is_created() {
        let directory =
            std::env::temp_dir().join(format!("muniment-non-regular-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();

        assert!(prepare_new_run(
            &storage,
            &run_id,
            "workspace-a",
            Some("owner"),
            vec![SelectedFile {
                path: directory.clone(),
            }],
            None,
        )
        .is_err());
        assert!(storage
            .lock()
            .unwrap()
            .journal
            .events(&run_id)
            .unwrap()
            .is_empty());

        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn changed_second_attachment_fails_the_run_without_prompting_pi() {
        let _environment = lock_pi_environment();
        let directory = std::env::temp_dir().join(format!("muniment-changed-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.txt");
        let second = directory.join("private-second.txt");
        let prompt_log = directory.join("prompt.txt");
        std::fs::write(&first, b"first attachment").unwrap();
        std::fs::write(&second, b"second attachment").unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();
        let opened = open_selected_files(vec![
            SelectedFile { path: first },
            SelectedFile {
                path: second.clone(),
            },
        ])
        .unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .open(&second)
            .unwrap()
            .set_len(1)
            .unwrap();
        std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);

        let error = match prepare_opened_run(
            &storage,
            &run_id,
            "workspace-a",
            Some("owner"),
            opened,
            None,
        ) {
            Ok(_) => panic!("changed attachment length must fail"),
            Err(error) => error,
        };
        assert_eq!(error, attachment_error());
        assert!(!error.contains(second.to_string_lossy().as_ref()));
        assert!(!prompt_log.exists(), "Pi must receive zero prompts");
        let events = storage.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            ["run.started", "chat.attachment.ingested", "run.failed"]
        );
        assert!(reduce(&events).unwrap().is_terminal());

        std::env::remove_var("PI_RESUME_STUB_PROMPTS");
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn prepared_attachments_reach_pi_before_coordinator_events_continue() {
        let app = tauri::test::mock_app();
        let directory =
            std::env::temp_dir().join(format!("muniment-coordinate-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let first = directory.join("first.txt");
        let second = directory.join("second.txt");
        std::fs::write(&first, b"first attachment").unwrap();
        std::fs::write(&second, b"second attachment").unwrap();
        std::fs::write(directory.join("session.jsonl"), "{}\n").unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();
        let prepared = prepare_new_run(
            &storage,
            &run_id,
            "workspace-a",
            Some("owner"),
            vec![SelectedFile { path: first }, SelectedFile { path: second }],
            None,
        )
        .unwrap();
        let (locator, _) = validate_pi_session(&directory, "session.jsonl").unwrap();

        assert_eq!(prepared.0, 3);
        let (mut seq, mut projector) = prepared;
        let prompt_submissions = AtomicUsize::new(0);
        coordinate_prepared_prompt(
            app.handle(),
            &storage,
            &mut projector,
            &run_id,
            &mut seq,
            Some("owner"),
            || {
                prompt_submissions.fetch_add(1, Ordering::SeqCst);
                let mut storage = storage.lock().unwrap();
                let events = storage.journal.events(&run_id).unwrap();
                assert_eq!(
                    events
                        .iter()
                        .map(|event| event.event_type.as_str())
                        .collect::<Vec<_>>(),
                    [
                        "run.started",
                        "chat.attachment.ingested",
                        "chat.attachment.ingested",
                    ]
                );
                for event in &events[1..] {
                    let EventPayload::Attachment { attachment } = &event.payload else {
                        panic!("attachment payload")
                    };
                    storage.cas.verify(attachment.sha256()).unwrap();
                }
                Ok(((), locator, Vec::new()))
            },
        )
        .unwrap();

        assert_eq!(prompt_submissions.load(Ordering::SeqCst), 1);
        let events = storage.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(
            events.iter().map(|event| event.run_seq).collect::<Vec<_>>(),
            (1..=events.len() as u64).collect::<Vec<_>>()
        );
        assert_eq!(events.len(), 5);
        assert_eq!(events[1].event_type, "chat.attachment.ingested");
        assert_eq!(events[2].event_type, "chat.attachment.ingested");
        assert_eq!(events[3].run_seq, 4);
        assert_eq!(events[3].event_type, "runtime.pi_session.bound");
        assert_eq!(events[4].event_type, "model.prompt.accepted");
        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn coordinator_prepares_supported_images_in_journal_order_and_omits_unsupported_files() {
        let directory = std::env::temp_dir().join(format!("muniment-images-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let png = directory.join("renamed.bin");
        let unsupported = directory.join("notes.png");
        let jpeg = directory.join("second.dat");
        std::fs::write(
            &png,
            b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0IEND\xaeB`\x82",
        )
        .unwrap();
        std::fs::write(&unsupported, b"durable but not an image").unwrap();
        std::fs::write(&jpeg, b"\xff\xd8payload\xff\xd9").unwrap();
        let storage = Arc::new(Mutex::new(ChatStorage {
            journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let run_id = Uuid::now_v7().to_string();
        prepare_new_run(
            &storage,
            &run_id,
            "workspace-a",
            Some("owner"),
            vec![
                SelectedFile { path: png },
                SelectedFile { path: unsupported },
                SelectedFile { path: jpeg },
            ],
            None,
        )
        .unwrap();

        assert_eq!(
            prepared_pi_images(&storage, &run_id).unwrap(),
            vec![
                PiImageContent::new(
                    "iVBORw0KGgoAAAANSUhEUgAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAElFTkSuQmCC",
                    "image/png"
                ),
                PiImageContent::new("/9hwYXlsb2Fk/9k=", "image/jpeg"),
            ]
        );
        assert_eq!(
            storage
                .lock()
                .unwrap()
                .journal
                .events(&run_id)
                .unwrap()
                .iter()
                .filter(|event| matches!(event.payload, EventPayload::Attachment { .. }))
                .count(),
            3
        );

        drop(storage);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn missing_cas_image_and_oversized_image_fail_with_non_leaking_copy() {
        for (name, bytes, remove_object) in [
            ("private-missing.png", valid_test_png(), true),
            (
                "private-large.jpg",
                {
                    let mut bytes =
                        vec![
                            0_u8;
                            usize::try_from(muniment_core::attachment::MAX_PI_IMAGE_BYTES + 1)
                                .unwrap()
                        ];
                    bytes[..2].copy_from_slice(b"\xff\xd8");
                    let length = bytes.len();
                    bytes[length - 2..].copy_from_slice(b"\xff\xd9");
                    bytes
                },
                false,
            ),
        ] {
            let directory =
                std::env::temp_dir().join(format!("muniment-image-fail-{}", Uuid::now_v7()));
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join(name);
            std::fs::write(&path, &bytes).unwrap();
            let storage = Arc::new(Mutex::new(ChatStorage {
                journal: RunJournal::open(directory.join("runs.sqlite3")).unwrap(),
                cas: LocalCas::open(&directory.join("cas")).unwrap(),
            }));
            let run_id = Uuid::now_v7().to_string();
            prepare_new_run(
                &storage,
                &run_id,
                "workspace-a",
                Some("owner"),
                vec![SelectedFile { path: path.clone() }],
                None,
            )
            .unwrap();
            let attachment_hash = {
                let mut storage = storage.lock().unwrap();
                let events = storage.journal.events(&run_id).unwrap();
                let EventPayload::Attachment { attachment } = &events[1].payload else {
                    panic!("attachment event")
                };
                attachment.sha256().clone()
            };
            if remove_object {
                storage
                    .lock()
                    .unwrap()
                    .cas
                    .remove(&attachment_hash)
                    .unwrap();
            }

            let error = prepared_pi_images(&storage, &run_id).unwrap_err();
            assert_eq!(error, attachment_error());
            for secret in [
                path.to_string_lossy().as_ref(),
                attachment_hash.as_str(),
                "payload",
            ] {
                assert!(!error.contains(secret));
            }

            drop(storage);
            std::fs::remove_dir_all(directory).unwrap();
        }
    }

    fn valid_test_png() -> Vec<u8> {
        b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0IEND\xaeB`\x82"
            .to_vec()
    }

    #[test]
    fn queue_request_is_closed_and_typed() {
        for value in [
            json!({"runId":"run-1", "delivery":"later", "message":"hello"}),
            json!({"runId":"run-1", "delivery":"steer", "message":"hello", "extra":true}),
        ] {
            assert!(serde_json::from_value::<ChatQueueRequest>(value).is_err());
        }
        assert!(serde_json::from_value::<ChatQueueRequest>(json!({
            "runId":"run-1", "delivery":"followUp", "message":"hello"
        }))
        .is_ok());
    }

    #[test]
    fn event_envelope_records_the_owning_subject() {
        let owned = event_envelope("run-1", 1, "run.started", json!({}), Some("sub-a"));
        assert_eq!(owned.provenance.actor_id.as_deref(), Some("sub-a"));

        let unowned = event_envelope("run-2", 1, "run.started", json!({}), None);
        assert_eq!(unowned.provenance.actor_id, None);
    }

    #[test]
    fn queue_rejects_mismatched_and_not_ready_runs_safely() {
        let active = Mutex::new(Some(inactive_transport_run("run-1")));
        let request = |run_id: &str| ChatQueueRequest {
            run_id: run_id.into(),
            delivery: ChatDelivery::Steer,
            message: "hello".into(),
        };
        assert_eq!(
            queue_message(&active, request("stale-run")).unwrap_err(),
            "That reply is no longer active."
        );
        assert_eq!(
            queue_message(&active, request("run-1")).unwrap_err(),
            "The reply is not ready for messages yet."
        );
    }

    #[test]
    fn concurrent_active_run_installs_allow_exactly_one_run() {
        let active = Arc::new(Mutex::new(None));
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|index| {
                let active = Arc::clone(&active);
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    install_active_run(&active, inactive_transport_run(&format!("run-{index}")))
                })
            })
            .collect();

        let results: Vec<_> = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let errors: Vec<_> = results
            .iter()
            .filter_map(|result| result.as_ref().err().map(String::as_str))
            .collect();
        assert_eq!(errors, ["A reply is already in progress."]);
    }

    #[test]
    fn resume_attempt_reports_acceptance_and_every_early_return() {
        let (sender, receiver) = std::sync::mpsc::channel();
        drop(ResumeAttempt::new(Some(sender)));
        assert_eq!(
            receiver.recv().unwrap().unwrap_err(),
            "This reply could not be resumed. Try again."
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        let mut attempt = ResumeAttempt::new(Some(sender));
        attempt.accepted();
        drop(attempt);
        assert_eq!(receiver.recv().unwrap(), Ok(()));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn resume_validation_fails_closed_for_every_unsafe_projection() {
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        let sessions = directory.join("pi-sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("session.jsonl"), "{}\n").unwrap();
        let run_id = Uuid::now_v7().to_string();
        let events = |tail: Vec<(&str, Value)>| {
            let mut values = vec![event_envelope(
                &run_id,
                1,
                "run.started",
                json!({}),
                Some("owner"),
            )];
            values.extend(
                tail.into_iter()
                    .enumerate()
                    .map(|(index, (kind, payload))| {
                        event_envelope(&run_id, index as u64 + 2, kind, payload, Some("owner"))
                    }),
            );
            values
        };
        let eligible = events(vec![
            (
                "runtime.pi_session.bound",
                json!({"run_id":run_id, "locator":"session.jsonl"}),
            ),
            ("run.needs_attention", json!({"reason":"interrupted"})),
        ]);
        assert!(resumable_context(&eligible, Some("owner"), &sessions).is_ok());
        assert!(resumable_context(&eligible, Some("another-subject"), &sessions).is_err());

        for unsafe_events in [
            events(vec![(
                "run.needs_attention",
                json!({"reason":"interrupted"}),
            )]),
            events(vec![
                (
                    "runtime.pi_session.bound",
                    json!({"run_id":run_id, "locator":"missing.jsonl"}),
                ),
                ("run.needs_attention", json!({"reason":"interrupted"})),
            ]),
            events(vec![("run.completed", json!({}))]),
            events(vec![
                (
                    "runtime.pi_session.bound",
                    json!({"run_id":run_id, "locator":"session.jsonl"}),
                ),
                (
                    "permission.requested",
                    json!({"gate_id":"gate", "kind":"confirm", "title":"Allow?", "message":"Proceed?"}),
                ),
                ("run.needs_attention", json!({"reason":"interrupted"})),
            ]),
            events(vec![
                (
                    "runtime.pi_session.bound",
                    json!({"run_id":run_id, "locator":"session.jsonl"}),
                ),
                (
                    "tool.effect.started",
                    json!({"effect_id":"effect", "display_name":"Command"}),
                ),
                ("run.needs_attention", json!({"reason":"interrupted"})),
            ]),
            Vec::new(),
        ] {
            assert!(resumable_context(&unsafe_events, Some("owner"), &sessions).is_err());
        }
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resume_runtime_failure_leaves_the_existing_journal_event_for_event_unchanged() {
        let _environment = lock_pi_environment();
        let app = tauri::test::mock_app();
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let run_id = Uuid::now_v7().to_string();
        let mut journal = RunJournal::open(&path).unwrap();
        append_test_event(
            &mut journal,
            &run_id,
            1,
            "run.started",
            json!({}),
            Some("owner"),
        );
        append_test_event(
            &mut journal,
            &run_id,
            2,
            "runtime.pi_session.bound",
            json!({"run_id":run_id, "locator":"session.jsonl"}),
            Some("owner"),
        );
        append_test_event(
            &mut journal,
            &run_id,
            3,
            "run.needs_attention",
            json!({"reason":"interrupted"}),
            Some("owner"),
        );
        let before = journal.events(&run_id).unwrap();
        let sessions = directory.join("pi-sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("session.jsonl"), "{}\n").unwrap();
        let (locator, _) = validate_pi_session(&sessions, "session.jsonl").unwrap();
        let shared = Arc::new(Mutex::new(ChatStorage {
            journal,
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));
        let previous_root = std::env::var_os("MUNIMENT_PI_ROOT");
        std::env::remove_var("MUNIMENT_PI_ROOT");
        let (sender, receiver) = std::sync::mpsc::channel();
        coordinate(
            app.handle().clone(),
            Arc::clone(&shared),
            Arc::new(Mutex::new(None)),
            run_id.clone(),
            RESUME_PROMPT.into(),
            "token".into(),
            Some("owner".into()),
            ChatGrant {
                workspace: "workspace-a".into(),
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                receipt_url: "https://receipt.invalid".into(),
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Some(ResumeContext {
                events: before.clone(),
                locator,
            }),
            Some(sender),
            None,
        );
        if let Some(root) = previous_root {
            std::env::set_var("MUNIMENT_PI_ROOT", root);
        }
        assert!(receiver.recv().unwrap().is_err());
        assert_eq!(
            shared.lock().unwrap().journal.events(&run_id).unwrap(),
            before
        );
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn resume_reopens_the_stub_session_and_completes_the_same_contiguous_run() {
        let _environment = lock_pi_environment();
        let app = tauri::test::mock_app();
        let directory = std::env::temp_dir().join(format!("muniment-resume-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let args_log = directory.join("args.txt");
        let prompt_log = directory.join("prompt.txt");
        let session_name = format!("{}.jsonl", Uuid::now_v7());
        let sessions = app.path().app_data_dir().unwrap().join("pi-sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join(&session_name), "persisted Pi data\n").unwrap();

        let executable_name = if cfg!(windows) {
            "sidecar-test-stub.exe"
        } else {
            "sidecar-test-stub"
        };
        let test_executable = std::env::current_exe().unwrap();
        let target_dir = test_executable.parent().unwrap().parent().unwrap();
        let stub = target_dir.join("examples").join(executable_name);
        assert!(stub.is_file(), "sidecar test stub was not built");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let receipt_url = format!("http://{}/receipt", listener.local_addr().unwrap());
        let receipt_server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            let mut stream = accept_receipt_request(listener);
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request).unwrap();
            let body = r#"{"route":"resume-stub","model":"test"}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });

        let run_id = Uuid::now_v7().to_string();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        append_test_event(
            &mut journal,
            &run_id,
            1,
            "run.started",
            json!({}),
            Some("owner"),
        );
        append_test_event(
            &mut journal,
            &run_id,
            2,
            "runtime.pi_session.bound",
            json!({"run_id":run_id, "locator":session_name}),
            Some("owner"),
        );
        append_test_event(
            &mut journal,
            &run_id,
            3,
            "run.needs_attention",
            json!({"reason":"interrupted"}),
            Some("owner"),
        );
        let existing = journal.events(&run_id).unwrap();
        let (locator, _) = validate_pi_session(&sessions, &session_name).unwrap();
        let shared = Arc::new(Mutex::new(ChatStorage {
            journal,
            cas: LocalCas::open(&directory.join("cas")).unwrap(),
        }));

        std::env::set_var("MUNIMENT_PI_ROOT", &directory);
        std::env::set_var("MUNIMENT_PI_TEST_EXECUTABLE", &stub);
        std::env::set_var("PI_RESUME_STUB_ARGS", &args_log);
        std::env::set_var("PI_RESUME_STUB_PROMPTS", &prompt_log);
        let (sender, receiver) = std::sync::mpsc::channel();
        coordinate(
            app.handle().clone(),
            Arc::clone(&shared),
            Arc::new(Mutex::new(None)),
            run_id.clone(),
            RESUME_PROMPT.into(),
            "token".into(),
            Some("owner".into()),
            ChatGrant {
                workspace: "workspace-a".into(),
                gateway_url: "https://gateway.invalid".into(),
                virtual_key: "virtual-key".into(),
                model: None,
                receipt_url,
            },
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(None)),
            Arc::new(Mutex::new(None)),
            Some(ResumeContext {
                events: existing,
                locator,
            }),
            Some(sender),
            None,
        );
        assert_eq!(receiver.recv().unwrap(), Ok(()));
        receipt_server.join().unwrap();
        for key in [
            "MUNIMENT_PI_ROOT",
            "MUNIMENT_PI_TEST_EXECUTABLE",
            "PI_RESUME_STUB_ARGS",
            "PI_RESUME_STUB_PROMPTS",
        ] {
            std::env::remove_var(key);
        }

        let events = shared.lock().unwrap().journal.events(&run_id).unwrap();
        assert_eq!(
            events.iter().map(|event| event.run_seq).collect::<Vec<_>>(),
            (1..=events.len() as u64).collect::<Vec<_>>()
        );
        assert_eq!(events.last().unwrap().event_type, "run.completed");
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "run.started")
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| event.event_type == "runtime.pi_session.bound")
                .count(),
            1
        );
        let args = std::fs::read_to_string(args_log).unwrap();
        assert!(args.lines().any(|argument| argument == "--session"));
        assert!(args.contains(&session_name));
        let sent_prompt = std::fs::read_to_string(prompt_log).unwrap();
        assert_eq!(sent_prompt, format!("{RESUME_PROMPT}\n"));
        assert!(!sent_prompt.contains("ORIGINAL PROTECTED PROMPT"));

        std::fs::remove_file(sessions.join(session_name)).unwrap();
        drop(shared);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn tool_frames_ignore_duplicate_starts_and_unmatched_finishes() {
        let mut open_effects = BTreeSet::new();
        let started = PiChatEvent::ToolStarted {
            tool_call_id: "tool-1".into(),
            tool_name: "Read file".into(),
        };
        let unmatched = PiChatEvent::ToolFinished {
            tool_call_id: "missing".into(),
            failed: false,
        };

        assert_eq!(
            tool_journal_entry(&started, &mut open_effects),
            Some((
                "tool.effect.started",
                json!({"effect_id": "tool-1", "display_name": "Read file"})
            ))
        );
        assert!(tool_journal_entry(&started, &mut open_effects).is_none());
        assert!(tool_journal_entry(&unmatched, &mut open_effects).is_none());

        let finished = PiChatEvent::ToolFinished {
            tool_call_id: "tool-1".into(),
            failed: false,
        };
        assert_eq!(
            tool_journal_entry(&finished, &mut open_effects),
            Some(("tool.effect.completed", json!({"effect_id": "tool-1"})))
        );
        assert!(tool_journal_entry(&finished, &mut open_effects).is_none());

        let failed = PiChatEvent::ToolFinished {
            tool_call_id: "tool-2".into(),
            failed: true,
        };
        assert!(open_effects.insert("tool-2".into()));
        assert_eq!(
            tool_journal_entry(&failed, &mut open_effects),
            Some(("tool.effect.failed", json!({"effect_id": "tool-2"})))
        );
    }

    #[test]
    fn blocking_dialogs_translate_to_permission_journal_payloads() {
        let cases = [
            (
                ExtensionUiDialog::Select {
                    title: "Choose".into(),
                    options: vec!["A".into(), "B".into()],
                },
                json!({"gate_id":"gate","kind":"select","title":"Choose","options":["A","B"],"timeout":5000}),
            ),
            (
                ExtensionUiDialog::Confirm {
                    title: "Allow?".into(),
                    message: "Proceed?".into(),
                },
                json!({"gate_id":"gate","kind":"confirm","title":"Allow?","message":"Proceed?","timeout":5000}),
            ),
            (
                ExtensionUiDialog::Input {
                    title: "Value".into(),
                    placeholder: Some("Type".into()),
                },
                json!({"gate_id":"gate","kind":"input","title":"Value","placeholder":"Type","timeout":5000}),
            ),
            (
                ExtensionUiDialog::Editor {
                    title: "Edit".into(),
                    prefill: Some("draft".into()),
                },
                json!({"gate_id":"gate","kind":"editor","title":"Edit","prefill":"draft","timeout":5000}),
            ),
        ];
        for (dialog, expected) in cases {
            assert_eq!(
                permission_journal_payload(&ExtensionUiRequest {
                    id: "gate".into(),
                    dialog,
                    timeout: Some(5000),
                }),
                expected
            );
        }
    }

    #[test]
    fn coordinator_journals_extension_ui_before_projecting_and_stops_on_failure() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_id = Uuid::now_v7().to_string();
        append_test_event(&mut journal, &run_id, 1, "run.started", json!({}), None);
        let mut projector = ChatProjector::new();
        projector
            .apply(&journal.events(&run_id).unwrap()[0])
            .unwrap();
        let mut emitted = Vec::new();

        coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-1".into(),
                dialog: ExtensionUiDialog::Confirm {
                    title: "Allow?".into(),
                    message: "Proceed?".into(),
                },
                timeout: Some(5_000),
            }),
            |kind, payload| {
                let envelope = event_envelope(&run_id, 2, kind, payload, None);
                journal.append(1, &envelope).map_err(|_| ())?;
                assert_eq!(journal.events(&run_id).unwrap().len(), 2);
                projector.apply(&envelope).map_err(|_| ())?;
                emitted.push(chat_event(&run_id, projector.projection().map_err(|_| ())?));
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            journal.events(&run_id).unwrap()[1].event_type,
            "permission.requested"
        );
        let projection = emitted.pop().unwrap();
        assert_eq!(projection.phase, "pending-permission");
        assert_eq!(
            projection.pending_permission.unwrap().gate_id,
            "pi-request-1"
        );

        let mut append_attempts = 0;
        assert!(coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-2".into(),
                dialog: ExtensionUiDialog::Input {
                    title: "Secret".into(),
                    placeholder: None,
                },
                timeout: None,
            }),
            |_kind, _payload| {
                append_attempts += 1;
                Err(())
            },
        )
        .is_err());
        assert_eq!(append_attempts, 1);

        let mut emitted_after_projection_failure = false;
        assert!(coordinate_extension_ui_request(
            PiChatEvent::ExtensionUiRequest(ExtensionUiRequest {
                id: "pi-request-3".into(),
                dialog: ExtensionUiDialog::Select {
                    title: "Choose".into(),
                    options: vec!["A".into(), "B".into()],
                },
                timeout: None,
            }),
            |kind, payload| {
                let envelope = event_envelope(&run_id, 3, kind, payload, None);
                let mut next_projector = projector.clone();
                next_projector.apply(&envelope).map_err(|_| ())?;
                next_projector.projection().map_err(|_| ())?;
                journal.append(2, &envelope).map_err(|_| ())?;
                emitted_after_projection_failure = true;
                Ok(())
            },
        )
        .is_err());
        assert!(!emitted_after_projection_failure);
        // The coordinator handler has no response transport and therefore cannot
        // synthesize an allow/deny (or any other Pi extension-UI response).
        let events = journal.events(&run_id).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "run.started");
        assert_eq!(events[1].event_type, "permission.requested");
        let replayed = muniment_core::journal::reducer::project_chat(&events).unwrap();
        assert_eq!(replayed.pending_permission.unwrap().gate_id, "pi-request-1");

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn history_projects_and_clears_a_typed_pending_permission() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_id = Uuid::now_v7().to_string();
        append_test_event(&mut journal, &run_id, 1, "run.started", json!({}), None);
        append_test_event(
            &mut journal,
            &run_id,
            2,
            "permission.requested",
            json!({
                "gate_id": "pi-request-1",
                "kind": "editor",
                "title": "Review command",
                "prefill": "cargo test",
                "timeout": 30_000
            }),
            None,
        );

        let entries = history_entries(&mut journal, None, std::path::Path::new(".")).unwrap();
        let entry = entries.iter().find(|entry| entry.run_id == run_id).unwrap();
        assert_eq!(entry.phase, "pending-permission");
        let pending = entry.pending_permission.as_ref().unwrap();
        assert_eq!(pending.gate_id, "pi-request-1");
        assert!(matches!(
            &pending.request,
            PermissionRequest::Editor { title, prefill, timeout }
                if title == "Review command"
                    && prefill.as_deref() == Some("cargo test")
                    && *timeout == Some(30_000)
        ));

        append_test_event(
            &mut journal,
            &run_id,
            3,
            "permission.resolved",
            json!({"gate_id": "pi-request-1", "decision": "cancelled"}),
            None,
        );
        let entries = history_entries(&mut journal, None, std::path::Path::new(".")).unwrap();
        let entry = entries.iter().find(|entry| entry.run_id == run_id).unwrap();
        assert!(entry.pending_permission.is_none());
        assert_eq!(entry.phase, "thinking");

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn terminal_failure_closes_a_tool_before_projecting_the_run() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_id = Uuid::now_v7().to_string();
        let mut seq = 1;
        append_test_event(&mut journal, &run_id, seq, "run.started", json!({}), None);
        seq += 1;
        append_test_event(
            &mut journal,
            &run_id,
            seq,
            "tool.effect.started",
            json!({"effect_id": "tool-1", "display_name": "Read file"}),
            None,
        );
        let mut open_effects = BTreeSet::from(["tool-1".to_string()]);

        close_open_effects(&mut open_effects, |kind, payload| {
            seq += 1;
            append_test_event(&mut journal, &run_id, seq, kind, payload, None);
            Ok(())
        })
        .unwrap();
        seq += 1;
        append_test_event(
            &mut journal,
            &run_id,
            seq,
            "run.failed",
            json!({"reason": "runtime stopped"}),
            None,
        );

        let events = journal.events(&run_id).unwrap();
        let projection = project_chat(&events).unwrap();
        assert!(open_effects.is_empty());
        assert!(matches!(projection.status, Some(RunStatus::Failed { .. })));
        assert_eq!(projection.tool_activity.len(), 1);
        assert_eq!(
            projection.tool_activity[0].status,
            muniment_core::journal::reducer::ToolActivityStatus::Failed
        );

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn history_is_scoped_by_the_first_events_actor() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut journal = RunJournal::open(directory.join("runs.sqlite3")).unwrap();
        let run_a = "01900000-0000-7000-8000-000000000001";
        let run_a_reconciled = "01900000-0000-7000-8000-000000000002";
        let run_b = "01900000-0000-7000-8000-000000000003";
        let run_legacy = "01900000-0000-7000-8000-000000000004";

        append_test_event(
            &mut journal,
            run_a,
            1,
            "run.started",
            json!({}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            run_a,
            2,
            "model.stream.delta",
            json!({"text": "private-a"}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            run_a,
            3,
            "run.completed",
            json!({"receipt": {"owner": "sub-a"}}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            run_a_reconciled,
            1,
            "run.started",
            json!({}),
            Some("sub-a"),
        );
        append_test_event(
            &mut journal,
            run_a_reconciled,
            2,
            "run.needs_attention",
            json!({"reason": "interrupted"}),
            None,
        );

        append_test_event(
            &mut journal,
            run_b,
            1,
            "run.started",
            json!({}),
            Some("sub-b"),
        );
        append_test_event(
            &mut journal,
            run_b,
            2,
            "model.stream.delta",
            json!({"text": "private-b"}),
            Some("sub-b"),
        );
        append_test_event(
            &mut journal,
            run_b,
            3,
            "run.completed",
            json!({"receipt": {"owner": "sub-b"}}),
            Some("sub-b"),
        );

        append_test_event(&mut journal, run_legacy, 1, "run.started", json!({}), None);
        append_test_event(
            &mut journal,
            run_legacy,
            2,
            "run.completed",
            json!({"receipt": {"legacy": true}}),
            None,
        );

        let sub_b =
            history_entries(&mut journal, Some("sub-b"), std::path::Path::new(".")).unwrap();
        assert_eq!(
            sub_b
                .iter()
                .map(|entry| entry.run_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([run_b, run_legacy])
        );
        assert!(sub_b.iter().all(|entry| !entry.text.contains("private-a")));
        assert!(sub_b.iter().all(|entry| {
            entry
                .receipt
                .as_ref()
                .and_then(|receipt| receipt.get("owner"))
                != Some(&json!("sub-a"))
        }));

        let sub_a =
            history_entries(&mut journal, Some("sub-a"), std::path::Path::new(".")).unwrap();
        assert_eq!(
            sub_a
                .iter()
                .map(|entry| entry.run_id.as_str())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([run_a, run_a_reconciled, run_legacy])
        );
        assert_eq!(
            sub_a
                .iter()
                .find(|entry| entry.run_id == run_a)
                .unwrap()
                .text,
            "private-a"
        );

        drop(journal);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn startup_reconciles_only_non_terminal_runs_once() {
        let directory = std::env::temp_dir().join(format!("muniment-chat-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("runs.sqlite3");
        let interrupted = Uuid::now_v7().to_string();
        let completed = Uuid::now_v7().to_string();

        {
            let mut journal = RunJournal::open(&path).unwrap();
            append_test_event(
                &mut journal,
                &interrupted,
                1,
                "run.started",
                json!({}),
                None,
            );
            append_test_event(
                &mut journal,
                &interrupted,
                2,
                "runtime.pi_session.bound",
                json!({"run_id": interrupted, "locator": "session.jsonl"}),
                None,
            );
            append_test_event(
                &mut journal,
                &interrupted,
                3,
                "permission.requested",
                json!({"gate_id":"gate-1","kind":"confirm","title":"Allow?","message":"Proceed?"}),
                None,
            );
            let pending = project_chat(&journal.events(&interrupted).unwrap()).unwrap();
            assert_eq!(projection_phase(&pending.status), "pending-permission");
            assert_eq!(pending.pending_permission.unwrap().gate_id, "gate-1");
            append_test_event(&mut journal, &completed, 1, "run.started", json!({}), None);
            append_test_event(
                &mut journal,
                &completed,
                2,
                "run.completed",
                json!({}),
                None,
            );
        }

        {
            let mut reopened = RunJournal::open(&path).unwrap();
            reconcile_interrupted_runs(&mut reopened);
            let interrupted_events = reopened.events(&interrupted).unwrap();
            assert_eq!(interrupted_events.len(), 4);
            assert_eq!(interrupted_events[3].event_type, "run.needs_attention");
            assert_eq!(interrupted_events[3].provenance.actor_id, None);
            let state = reduce(&interrupted_events).unwrap();
            assert!(matches!(state.status, RunStatus::NeedsAttention(_)));
            assert_eq!(state.pi_session.unwrap().locator, "session.jsonl");
            assert_eq!(reopened.events(&completed).unwrap().len(), 2);

            reconcile_interrupted_runs(&mut reopened);
            assert_eq!(reopened.events(&interrupted).unwrap().len(), 4);
            assert_eq!(reopened.events(&completed).unwrap().len(), 2);
        }

        std::fs::remove_dir_all(directory).unwrap();
    }
}
