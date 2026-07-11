//! Coordinated acquisition and health-checked activation of a Gemma revision.

use std::path::Path;

use crate::model_install::{
    install_model, AvailableSpace, InstallCancellation, InstallLock, ModelInstallError,
};

use super::acquisition::{
    acquire_gemma_stage, remaining_stage_bytes, GemmaAcquisitionClock, GemmaAcquisitionError,
    GemmaAcquisitionLimits, GemmaAcquisitionRuntime, GemmaCancellation, GemmaDownloadTransport,
    GemmaRetryWait,
};
use super::lifecycle::{
    GemmaActivation, GemmaActivationBoundary, GemmaLifecycleBoundary, GemmaLifecycleError,
    GemmaRevisionDescriptor, GemmaRevisionLifecycle,
};

pub type GemmaInstallError = ModelInstallError<GemmaAcquisitionError, GemmaLifecycleError>;

/// Acquires and activates one pinned Gemma revision under the shared install
/// lock and exact resumable-stage storage checks.
#[allow(clippy::too_many_arguments)]
pub fn install_gemma_revision<T, C, K, W, L, S, B, A>(
    staging_root: &Path,
    install_id: &str,
    descriptor: &'static GemmaRevisionDescriptor,
    limits: GemmaAcquisitionLimits,
    transport: &mut T,
    runtime: GemmaAcquisitionRuntime<'_, K, W>,
    cancellation: &C,
    lock: &mut L,
    space: &mut S,
    lifecycle: &GemmaRevisionLifecycle,
    lifecycle_boundary: &B,
    activation: &mut A,
) -> Result<GemmaActivation, GemmaInstallError>
where
    T: GemmaDownloadTransport,
    C: GemmaCancellation + InstallCancellation,
    K: GemmaAcquisitionClock,
    W: GemmaRetryWait,
    L: InstallLock,
    S: AvailableSpace,
    B: GemmaLifecycleBoundary,
    A: GemmaActivationBoundary,
{
    install_model(
        lock,
        space,
        cancellation,
        || remaining_stage_bytes(staging_root, install_id, descriptor),
        || {
            acquire_gemma_stage(
                staging_root,
                install_id,
                descriptor,
                limits,
                transport,
                runtime,
                cancellation,
            )
        },
        |stage| lifecycle.activate_lock_held(&stage, lifecycle_boundary, activation),
    )
}
