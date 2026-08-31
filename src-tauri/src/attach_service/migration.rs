use super::*;

#[cfg(unix)]
pub(crate) fn runtime_upgrade_pending(client: &DesktopClientHolder) -> bool {
    runtime_version_upgrade_pending(client.runtime_version().as_deref())
}

#[cfg(unix)]
pub(crate) fn runtime_version_compatible(version: &str) -> bool {
    !runtime_version_upgrade_pending(Some(version))
}

#[cfg(unix)]
pub(super) fn runtime_version_upgrade_pending(connected_version: Option<&str>) -> bool {
    let minimum = semver::Version::parse(MINIMUM_COMPATIBLE_RUNTIME_VERSION)
        .expect("minimum compatible runtime version must be valid");
    connected_version
        .and_then(|version| semver::Version::parse(version).ok())
        .map_or(true, |version| version < minimum)
}

#[cfg(target_os = "linux")]
pub(super) fn decide_migration_control(
    peer_result: Result<(), PeerAuthorityError>,
    activity: RuntimeActivity,
    slot: &mut PreparedHandoffSlot,
    nonce: String,
    deadline_ms: u64,
    now: Instant,
) -> Result<(), ProtocolError> {
    peer_result.map_err(|_| ProtocolError::unauthorized())?;
    evaluate_quiesce(activity).map_err(|_| ProtocolError::migration_not_ready())?;
    slot.prepare(nonce, deadline_ms, now)
        .map_err(|_| ProtocolError::migration_not_ready())
}

#[cfg(target_os = "linux")]
pub(crate) fn control_desktop_migration<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    request: muniment_core::attach::linux::MigrationControlRequest,
    peer_pid: u32,
) -> Result<(), ProtocolError> {
    let nonce = request.handoff_nonce;
    let deadline_ms = request.deadline_ms;
    let prepared_at = Instant::now();
    let expected_executable = app
        .path()
        .resource_dir()
        .map_err(|_| ProtocolError::unauthorized())?
        .join("muniment-runtime");
    let peer_result = verify_migration_control_peer(peer_pid, &expected_executable).map(|_| ());
    let activity = app.state::<RuntimeActivityRegistry>().snapshot();
    let handoff_state = app.state::<Mutex<PreparedHandoffSlot>>();
    let mut slot = handoff_state
        .lock()
        .map_err(|_| ProtocolError::migration_not_ready())?;
    decide_migration_control(
        peer_result,
        activity,
        &mut slot,
        nonce.clone(),
        deadline_ms,
        prepared_at,
    )?;
    drop(slot);

    let handoff_app = app.clone();
    std::thread::spawn(move || {
        let Ok(filesystem) = AttachFilesystem::from_environment() else {
            stop_attach_listener(&handoff_app);
            cancel_handoff_and_restart(
                &handoff_app,
                &nonce,
                "runtime service readiness filesystem lookup failed",
                || start_attach_listener(handoff_app.clone()),
            );
            return;
        };
        let _ = release_prepared_handoff(
            &handoff_app,
            filesystem.endpoint_path(),
            &nonce,
            prepared_at + Duration::from_millis(deadline_ms),
            || start_attach_listener(handoff_app.clone()),
        );
    });
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn release_prepared_handoff<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    endpoint: &std::path::Path,
    nonce: &str,
    deadline: Instant,
    restart: impl FnOnce(),
) -> Result<ConfirmedHandoff, HandoffProbeError> {
    stop_attach_listener(app);
    match probe_handoff(endpoint, nonce, deadline) {
        Ok(confirmed) => {
            eprintln!("runtime service handoff confirmed");
            start_approval_presenter(app);
            start_desktop_client(app);
            Ok(confirmed)
        }
        Err(error) => {
            cancel_handoff_and_restart(app, nonce, &error.to_string(), restart);
            Err(error)
        }
    }
}

#[cfg(target_os = "linux")]
pub(super) fn cancel_handoff_and_restart<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    nonce: &str,
    reason: &str,
    restart: impl FnOnce(),
) {
    let cancelled = app
        .state::<Mutex<PreparedHandoffSlot>>()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .cancel_if_matches(nonce);
    if cancelled {
        restart();
    }
    eprintln!("runtime service handoff failed: {reason}");
}
