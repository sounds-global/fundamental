use std::str::FromStr;
use std::sync::Arc;

use fundamental_server::{Method, ServerResponse};
use serde::Serialize;
use tauri::State;
use tracing::error;

use crate::{ActionBridge, BridgeRequest, BridgeResponse};

pub(crate) struct BridgeState(pub(crate) Arc<dyn ActionBridge>);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InvokeResponse {
    /// HTTP status of the bridged call.
    pub(crate) status: u16,
    /// Content type of `body`.
    pub(crate) content_type: String,
    /// Response body (action responses are JSON text).
    pub(crate) body: String,
}

impl From<BridgeResponse> for InvokeResponse {
    fn from(response: BridgeResponse) -> Self {
        Self {
            status: response.status,
            content_type: response.content_type,
            body: String::from_utf8_lossy(&response.body).into_owned(),
        }
    }
}

/// Runs one action call from the webview through the managed bridge.
///
/// Never rejects: failures render as the standard JSON error envelope. The `Err` arm Tauri
/// requires is unreachable.
#[tauri::command]
pub(crate) async fn fundamental_invoke(
    app: tauri::AppHandle,
    state: State<'_, BridgeState>,
    method: String,
    path: String,
    body: String,
) -> Result<InvokeResponse, String> {
    // The reserved live paths are answered by the hosted database, never the router.
    #[cfg(feature = "sync-host")]
    if let Some(op) = path
        .strip_prefix("/_fundamental/live/")
        .filter(|op| matches!(*op, "bind" | "unbind" | "rows"))
    {
        return Ok(live_dispatch(&app, op, &body).await);
    }
    #[cfg(not(feature = "sync-host"))]
    let _ = &app;

    let method = match Method::from_str(&method) {
        Ok(method) => method,
        Err(unsupported) => {
            let response: BridgeResponse = ServerResponse::from_error(&unsupported).into();
            return Ok(response.into());
        }
    };

    let request = bridge_request(method, &path, body);
    match state.0.call(request).await {
        Ok(response) => Ok(response.into()),
        Err(bridge_error) => {
            // Logged here, once, where the failure is rendered to the webview.
            error!(error = ?bridge_error, "invoked action failed at the bridge");
            let response: BridgeResponse = ServerResponse::from_error(&bridge_error).into();
            Ok(response.into())
        }
    }
}

/// The bridged request for one invoke, its path's query given apart.
fn bridge_request(method: Method, target: &str, body: String) -> BridgeRequest {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    BridgeRequest::new(method, path)
        .with_query(query)
        .with_body(body.into_bytes())
}

#[cfg(feature = "sync-host")]
#[derive(Debug, serde::Deserialize)]
struct LiveRequest {
    slice: String,
    #[serde(default = "empty_params")]
    params: serde_json::Value,
}

#[cfg(feature = "sync-host")]
fn empty_params() -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::new())
}

/// Serves one reserved live operation from the managed [`SyncHost`](crate::SyncHost).
///
/// Failures render as the standard JSON error envelope and are logged here, once.
#[cfg(feature = "sync-host")]
async fn live_dispatch(app: &tauri::AppHandle, op: &str, body: &str) -> InvokeResponse {
    use tauri::Manager;

    use crate::NativeError;
    use crate::sync_host::SyncHostState;

    let envelope = |error: &NativeError| -> InvokeResponse {
        error!(error = ?error, op, "live request failed");
        let response: BridgeResponse = ServerResponse::from_error(error).into();
        response.into()
    };

    let Some(host) = app.try_state::<SyncHostState>() else {
        // Configured webview, unconfigured host: say so rather than 404.
        return envelope(&NativeError::SyncHostUnavailable);
    };
    let host = host.0.clone();

    let request: LiveRequest = match serde_json::from_str(body) {
        Ok(request) => request,
        Err(source) => {
            return envelope(&NativeError::MalformedLiveRequest {
                detail: source.to_string(),
            });
        }
    };

    let result = match op {
        "bind" => host
            .bind(request.slice, request.params)
            .await
            .map(|()| "{}".to_owned()),
        "unbind" => host
            .unbind(request.slice, request.params)
            .await
            .map(|()| "{}".to_owned()),
        _ => {
            let slice = request.slice;
            let rows = host.rows(slice.clone()).await;
            rows.and_then(|rows| {
                serde_json::to_string(&rows).map_err(|source| NativeError::LiveEncoding {
                    slice,
                    detail: source.to_string(),
                })
            })
        }
    };

    match result {
        Ok(body) => InvokeResponse {
            status: 200,
            content_type: "application/json".to_owned(),
            body,
        },
        Err(error) => envelope(&error),
    }
}

#[cfg(test)]
mod tests {
    use super::{InvokeResponse, bridge_request};
    use crate::BridgeResponse;
    use fundamental_server::Method;

    #[test]
    fn an_invoked_query_is_given_apart_from_the_path() {
        let request = bridge_request(Method::Get, "/sessions?active=true", String::new());
        assert_eq!(request.path, "/sessions");
        assert_eq!(request.query, "active=true");
        assert!(request.body.is_empty());
    }

    #[test]
    fn an_invoke_without_a_query_keeps_its_path_and_body() {
        let request = bridge_request(Method::Post, "/teams", "{}".to_owned());
        assert_eq!(request.path, "/teams");
        assert_eq!(request.query, "");
        assert_eq!(request.body, b"{}");
    }

    #[test]
    fn invoke_response_serializes_camel_case_for_the_webview() {
        let response: InvokeResponse = BridgeResponse {
            status: 200,
            content_type: "application/json".into(),
            headers: Vec::new(),
            body: b"{\"ok\":true}".to_vec(),
        }
        .into();
        let json = serde_json::to_value(&response).expect("serializes");
        assert_eq!(json["status"], 200);
        assert_eq!(json["contentType"], "application/json");
        assert_eq!(json["body"], "{\"ok\":true}");
    }
}
