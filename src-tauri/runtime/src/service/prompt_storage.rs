use muniment_core::chat_prompt::store_prompt;
use muniment_core::journal::reducer::ChatProjector;
use muniment_core::journal::run_append::append_run_event;
use muniment_core::run_events::SharedStorage;
use muniment_core::run_preparation::{event_envelope, record_persistence_failure};
use muniment_core::runtime_eprintln as eprintln;

/// Leaves the execution copy in memory when the keyring refuses prompt history.
pub(crate) fn store_prompt_or_notice(
    run_id: &str,
    prompt: &str,
    subject: Option<&str>,
) -> Option<String> {
    store_prompt(run_id, prompt, subject).err().map(|error| {
        let notice = format!(
            "Prompt text stays in runtime memory for this run. The keyring refused prompt history: {error:?}."
        );
        eprintln!("muniment-runtime: run {run_id}: {notice}");
        notice
    })
}

/// Records the storage choice without writing prompt text to the journal.
pub(crate) fn record_prompt_storage_notice(
    storage: &SharedStorage,
    run_id: &str,
    subject: Option<&str>,
    prepared: Result<(u64, ChatProjector), String>,
    notice: Option<String>,
) -> Result<(u64, ChatProjector), String> {
    let mut prepared = prepared.map_err(|error| match &notice {
        Some(notice) => format!("{notice} {error}"),
        None => error,
    })?;
    let Some(notice) = notice else {
        return Ok(prepared);
    };
    let mut storage = match storage.lock() {
        Ok(storage) => storage,
        Err(poisoned) => {
            let mut storage = poisoned.into_inner();
            record_persistence_failure(
                &mut storage.journal,
                &mut prepared.1,
                run_id,
                prepared.0,
                subject,
                "muniment-runtime",
                env!("CARGO_PKG_VERSION"),
            );
            return Err(format!(
                "{notice} The runtime could not record the prompt storage notice."
            ));
        }
    };
    let event = event_envelope(
        run_id,
        prepared.0 + 1,
        "chat.prompt.storage_notice",
        serde_json::json!({"notice": notice}),
        subject,
        "muniment-runtime",
        env!("CARGO_PKG_VERSION"),
    );
    if let Err(error) = append_run_event(&mut storage.journal, &mut prepared.1, &event) {
        record_persistence_failure(
            &mut storage.journal,
            &mut prepared.1,
            run_id,
            prepared.0,
            subject,
            "muniment-runtime",
            env!("CARGO_PKG_VERSION"),
        );
        return Err(format!(
            "{notice} The runtime could not record the prompt storage notice: {error:?}."
        ));
    }
    prepared.0 += 1;
    Ok(prepared)
}
