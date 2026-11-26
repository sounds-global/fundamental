use std::borrow::Cow;

use fundamental_core::AnyError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tracing::instrument;

/// The transport used by functions generated in `actions` by `mount_actions!`.
#[derive(Debug, Clone, Copy, Default)]
pub struct WebActionTransport;

#[cfg(not(target_arch = "wasm32"))]
type WebCallFuture<O> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<O, AnyError>> + Send + 'static>>;

#[cfg(target_arch = "wasm32")]
type WebCallFuture<O> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<O, AnyError>> + 'static>>;

impl fundamental_core::action::ActionTransport for WebActionTransport {
    type Future<O>
        = WebCallFuture<O>
    where
        O: 'static;

    fn call<I, O>(method: &'static str, path: Cow<'static, str>, input: I) -> Self::Future<O>
    where
        I: Serialize + 'static,
        O: DeserializeOwned + 'static,
    {
        #[cfg(target_arch = "wasm32")]
        {
            Box::pin(async move { call_action(method, &path, &input).await })
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::web_client_error::WebClientError;

            let _ = (method, input);
            tracing::warn!(%path, "web action stub invoked during server rendering; returning a transport error");
            Box::pin(async move {
                Err(WebClientError::Transport(
                    "a web action cannot be called during server rendering; it runs in the browser"
                        .to_owned(),
                )
                .into())
            })
        }
    }
}

/// Calls the web action served at `path` with `method`, returning its deserialized output.
///
/// The `#[action]` macro's browser stub calls it; on the server it is never dispatched.
///
/// # Errors
///
/// Returns [`WebClientError::Transport`](crate::WebClientError::Transport) when the
/// call never yields a usable response (a network or `fetch` failure, a redirect — which
/// is refused rather than followed — a body that will not (de)serialize, a path that still
/// holds a `{capture}`, or being invoked during server rendering), and
/// [`WebClientError::Remote`](crate::WebClientError::Remote) when the server answers
/// with its error envelope. Both convert into the returned [`AnyError`].
#[instrument(skip(input), fields(method = %method))]
pub async fn call_action<I, O>(method: &str, path: &str, input: &I) -> Result<O, AnyError>
where
    I: Serialize,
    O: DeserializeOwned,
{
    // A Tauri webview reaches the server through IPC `invoke` instead of `fetch`.
    #[cfg(all(target_arch = "wasm32", not(feature = "tauri")))]
    {
        wasm::call(method, path, input).await
    }
    #[cfg(all(target_arch = "wasm32", feature = "tauri"))]
    {
        tauri::call(method, path, input).await
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use crate::web_client_error::WebClientError;
        use tracing::warn;

        let _ = input;
        warn!(
            %path,
            "web action stub invoked during server rendering; returning a transport error"
        );
        Err(WebClientError::Transport(
            "a web action cannot be called during server rendering; it runs in the browser"
                .to_owned(),
        )
        .into())
    }
}

/// The browser `fetch` transport.
#[cfg(all(target_arch = "wasm32", not(feature = "tauri")))]
mod wasm {
    use fundamental_core::AnyError;
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Headers, Request, RequestCredentials, RequestInit, Response, window};

    use crate::action_request::ActionRequest;
    use crate::web_client_error::WebClientError;

    pub(super) async fn call<I, O>(method: &str, path: &str, input: &I) -> Result<O, AnyError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let response = fetch(method, path, input).await?;
        let status = response.status();
        let body = response_text(&response).await?;

        if (200..300).contains(&status) {
            return serde_json::from_str::<O>(&body).map_err(|error| {
                WebClientError::Transport(format!("response body: {error}")).into()
            });
        }
        Err(WebClientError::from_error_response(status, &body).into())
    }

    async fn fetch<I: Serialize>(
        method: &str,
        path: &str,
        input: &I,
    ) -> Result<Response, WebClientError> {
        let shaped = ActionRequest::new(method, path, input)?;
        let init = RequestInit::new();
        init.set_method(method);
        init.set_credentials(RequestCredentials::SameOrigin);
        // A followed redirect would read another origin's body as the action's answer.
        // `RequestInit::set_redirect` needs a web-sys feature not enabled here.
        js_sys::Reflect::set(
            &init,
            &JsValue::from_str("redirect"),
            &JsValue::from_str("error"),
        )
        .map_err(js_transport)?;

        if let Some(json) = shaped.body() {
            init.set_body(&JsValue::from_str(json));

            let headers = Headers::new().map_err(js_transport)?;
            headers
                .append("Content-Type", "application/json")
                .map_err(js_transport)?;
            init.set_headers(headers.as_ref());
        }

        let request =
            Request::new_with_str_and_init(&shaped.target(), &init).map_err(js_transport)?;
        let window =
            window().ok_or_else(|| WebClientError::Transport("no browser window".into()))?;
        let resolved = JsFuture::from(window.fetch_with_request(&request))
            .await
            .map_err(js_transport)?;
        resolved
            .dyn_into::<Response>()
            .map_err(|_| WebClientError::Transport("fetch did not resolve to a Response".into()))
    }

    async fn response_text(response: &Response) -> Result<String, WebClientError> {
        let promise = response.text().map_err(js_transport)?;
        let value = JsFuture::from(promise).await.map_err(js_transport)?;
        value
            .as_string()
            .ok_or_else(|| WebClientError::Transport("response body was not text".into()))
    }

    fn js_transport(value: JsValue) -> WebClientError {
        let detail = value
            .as_string()
            .or_else(|| js_sys::Error::from(value).message().as_string())
            .unwrap_or_else(|| "JavaScript error".into());
        WebClientError::Transport(detail)
    }
}

/// The Tauri webview transport, through the native surface's one `fundamental_invoke` command.
#[cfg(all(target_arch = "wasm32", feature = "tauri"))]
mod tauri {
    use fundamental_core::AnyError;
    use serde::Deserialize;
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use wasm_bindgen::JsValue;
    use wasm_bindgen::prelude::wasm_bindgen;

    use crate::action_request::ActionRequest;
    use crate::web_client_error::WebClientError;

    const INVOKE_COMMAND: &str = "fundamental_invoke";

    #[wasm_bindgen]
    extern "C" {
        /// Tauri's raw IPC entry point, present in every Tauri webview.
        #[wasm_bindgen(js_namespace = ["window", "__TAURI_INTERNALS__"], js_name = invoke, catch)]
        async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;
    }

    /// The wire shape `fundamental_invoke` returns (see `fundamental_native`).
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct InvokeResponse {
        status: u16,
        body: String,
    }

    /// Invokes the action over Tauri IPC and decodes the typed result or the error envelope.
    pub(super) async fn call<I, O>(method: &str, path: &str, input: &I) -> Result<O, AnyError>
    where
        I: Serialize,
        O: DeserializeOwned,
    {
        let shaped = ActionRequest::new(method, path, input)?;

        let args = js_sys::Object::new();
        set(&args, "method", method)?;
        set(&args, "path", &shaped.target())?;
        set(&args, "body", shaped.body().unwrap_or_default())?;

        let value = tauri_invoke(INVOKE_COMMAND, args.into())
            .await
            .map_err(js_transport)?;
        let json = js_sys::JSON::stringify(&value)
            .map_err(js_transport)?
            .as_string()
            .ok_or_else(|| WebClientError::Transport("invoke reply was not JSON".into()))?;
        let response: InvokeResponse = serde_json::from_str(&json)
            .map_err(|error| WebClientError::Transport(format!("invoke reply: {error}")))?;

        if (200..300).contains(&response.status) {
            return serde_json::from_str::<O>(&response.body).map_err(|error| {
                WebClientError::Transport(format!("response body: {error}")).into()
            });
        }
        Err(WebClientError::from_error_response(response.status, &response.body).into())
    }

    fn set(args: &js_sys::Object, key: &str, value: &str) -> Result<(), WebClientError> {
        js_sys::Reflect::set(args, &JsValue::from_str(key), &JsValue::from_str(value))
            .map(|_| ())
            .map_err(js_transport)
    }

    fn js_transport(value: JsValue) -> WebClientError {
        let detail = value
            .as_string()
            .or_else(|| js_sys::Error::from(value).message().as_string())
            .unwrap_or_else(|| "JavaScript error".into());
        WebClientError::Transport(detail)
    }
}
