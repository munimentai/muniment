//! "This computer" on the Runtimes page: whether this computer's runtime
//! accepts pairing, where it listens, the endpoints its links name, its
//! pairing links and its paired devices. Each command sends one local owner
//! route request over a connection of its own.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use muniment_core::attach::{ClientError, DesktopClient, Operation, DEFAULT_REMOTE_PORT};
use serde_json::{json, Value};

use crate::auth;

fn on_local_runtime<T: Send + 'static>(
    call: impl FnOnce(&mut DesktopClient) -> Result<T, ClientError> + Send + 'static,
) -> impl FnOnce() -> Result<T, String> + Send + 'static {
    move || {
        let mut client = crate::model_service::connect().map_err(auth::desktop_client_error)?;
        call(&mut client)
            .map_err(|error| auth::desktop_request_error(error, client.last_request_error()))
    }
}

async fn run<T: Send + 'static>(
    call: impl FnOnce(&mut DesktopClient) -> Result<T, ClientError> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(on_local_runtime(call))
        .await
        .map_err(|_| auth::background_service_error())?
}

/// The listen address for `port`. A specific address the settings name
/// keeps its address. Otherwise the default port keeps the default, and
/// another port binds every interface: IPv6 with IPv4 where the platform
/// shares them, and IPv4 on Windows, which binds IPv6 sockets to IPv6 only.
pub(crate) fn listen_for(current: Option<SocketAddr>, port: Option<u16>) -> Option<SocketAddr> {
    let Some(port) = port else {
        return current;
    };
    match current {
        Some(address) if !address.ip().is_unspecified() => {
            Some(SocketAddr::new(address.ip(), port))
        }
        _ if port == DEFAULT_REMOTE_PORT => None,
        _ if cfg!(windows) => Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)),
        _ => Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port)),
    }
}

/// Reads this computer's remote access settings and listener state.
#[tauri::command]
pub async fn local_access_settings() -> Result<Value, String> {
    run(|client| client.remote_settings()).await
}

/// Turns pairing on or off, moves the listen port and names the endpoints
/// pairing links carry. An empty list lets the runtime list its addresses.
#[tauri::command]
pub async fn local_access_save(
    enabled: bool,
    port: Option<u16>,
    endpoints: Vec<String>,
) -> Result<Value, String> {
    if port == Some(0) {
        return Err("Enter a port from 1 to 65535.".into());
    }
    let endpoints: Vec<String> = endpoints
        .into_iter()
        .map(|endpoint| endpoint.trim().to_owned())
        .filter(|endpoint| !endpoint.is_empty())
        .collect();
    run(move |client| {
        let current = client.remote_settings()?;
        let listen = current["settings"]["listen"]
            .as_str()
            .and_then(|listen| listen.parse().ok());
        let listen = listen_for(listen, port).map(|address| address.to_string());
        client.save_remote_settings(enabled, listen.as_deref(), &endpoints)
    })
    .await
}

/// Creates a one-time pairing link with its QR code. The link holds the
/// pairing secret, so it goes to the window for display and nowhere else.
#[tauri::command]
pub async fn local_pairing_link(admin: bool) -> Result<Value, String> {
    let scopes = if admin {
        json!(["thread.read", "run.write", "runtime.admin"])
    } else {
        json!(["thread.read", "run.write"])
    };
    let created = run(move |client| {
        client
            .request(
                Operation::RemotePairingCreate,
                None,
                json!({"scopes": scopes}),
            )
            .map(|response| response.body)
    })
    .await?;
    let link = created["link"]
        .as_str()
        .ok_or_else(|| "The runtime returned no pairing link. Try again.".to_owned())?;
    let qr = muniment_desktop_integration::auth::qr_svg(link)
        .map_err(|_| "The pairing link is too long for a QR code. Copy the link.".to_owned())?;
    Ok(json!({
        "link": link,
        "qrSvg": qr,
        "expiresAt": created["expires_at"],
        "scopes": created["scopes"],
    }))
}

/// Lists the devices paired with this computer's runtime. No device
/// carries a secret.
#[tauri::command]
pub async fn local_devices() -> Result<Value, String> {
    run(|client| client.list_remote_devices()).await
}

#[tauri::command]
pub async fn local_device_revoke(device_id: String) -> Result<bool, String> {
    run(move |client| client.revoke_remote_device(&device_id)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_port_keeps_a_named_address_and_the_default_stays_the_default() {
        let named: SocketAddr = "127.0.0.1:7443".parse().unwrap();
        assert_eq!(
            listen_for(Some(named), Some(9000)),
            Some("127.0.0.1:9000".parse().unwrap())
        );
        assert_eq!(listen_for(Some(named), None), Some(named));
        assert_eq!(listen_for(None, None), None);
        assert_eq!(listen_for(None, Some(7443)), None);
        let every = listen_for(None, Some(9000)).unwrap();
        assert!(every.ip().is_unspecified());
        assert_eq!(every.port(), 9000);
        let unspecified: SocketAddr = "[::]:9000".parse().unwrap();
        assert_eq!(listen_for(Some(unspecified), Some(7443)), None);
    }
}
