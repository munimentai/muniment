//! Links a machine this app just paired into the fleet of this computer's
//! runtime, so it syncs like any member and the owner never links by hand.
//! Core issues and redeems the invitation. This module only carries it
//! between the two runtimes, because only the app holds a credential for both.

use muniment_core::attach::{
    connect_remote_runtime, DesktopClient, ErrorDetails, Operation, RemoteRuntimeRecord,
    SCOPE_RUNTIME_ADMIN,
};
use serde_json::{json, Value};

use super::connection::client_options;

/// Why a machine added with limited access does not sync: only a machine
/// that holds admin access joins this computer's machines.
const LIMITED_MACHINE: &str = "This machine was added with limited access, so it does not sync with this computer. Remove it in Settings, Machines, then add it again with a code from Add machine.";

/// Links the paired runtime `record` names with this computer's runtime.
/// The paired runtime accepts connections, since this app just reached it,
/// so it invites and this computer dials it. When this computer cannot dial
/// it, a runtime here that accepts connections invites it instead. Answers
/// the sentence that says why neither worked.
pub(crate) fn link_into_fleet(record: &RemoteRuntimeRecord) -> Result<(), String> {
    if !record.scopes.contains(SCOPE_RUNTIME_ADMIN) {
        return Err(LIMITED_MACHINE.into());
    }
    let mut remote = connect_remote_runtime(record, &client_options(None))
        .map_err(|_| "The machine stopped answering before it could be linked.".to_owned())?;
    let mut local =
        crate::model_service::connect().map_err(|_| crate::auth::background_service_error())?;
    let members = swarm(&mut local, Operation::SwarmMembers, json!({}))?;
    if linked(&members, record.runtime_id.as_str()) {
        return Ok(());
    }
    let dialed = invite_and_join(&mut remote, &mut local);
    if dialed.is_ok() {
        return dialed;
    }
    invite_and_join(&mut local, &mut remote).map_err(|_| {
        format!(
            "The machine is paired but not linked: {}",
            dialed.unwrap_err()
        )
    })
}

/// Whether this computer's member list already links `runtime_id`.
fn linked(members: &Value, runtime_id: &str) -> bool {
    members["members"].as_array().is_some_and(|members| {
        members
            .iter()
            .any(|member| member["runtime_id"] == runtime_id && !member["link"].is_null())
    })
}

/// `from` issues an invitation and `to` redeems it.
fn invite_and_join(from: &mut DesktopClient, to: &mut DesktopClient) -> Result<(), String> {
    let invitation = swarm(from, Operation::SwarmInvite, json!({}))?;
    let link = invitation["link"]
        .as_str()
        .ok_or("The invitation came back empty.")?;
    swarm(to, Operation::SwarmJoin, json!({ "link": link })).map(drop)
}

/// One swarm request, refused with the runtime's own sentence.
fn swarm(client: &mut DesktopClient, operation: Operation, body: Value) -> Result<Value, String> {
    client.swarm_request(operation, body).map_err(|_| {
        match client.last_request_error().and_then(|error| error.details()) {
            Some(ErrorDetails::RequestReason { reason }) if !reason.trim().is_empty() => {
                reason.trim().to_owned()
            }
            _ => "The machines could not be linked.".to_owned(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_member_counts_as_linked_only_with_a_link() {
        let members = json!({"members": [
            {"runtime_id": "a", "link": {"state": "connected"}},
            {"runtime_id": "b", "link": null},
        ]});
        assert!(linked(&members, "a"));
        assert!(!linked(&members, "b"));
        assert!(!linked(&members, "c"));
        assert!(!linked(&json!({}), "a"));
    }
}
