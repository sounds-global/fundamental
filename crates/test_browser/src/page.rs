use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, instrument, warn};

use crate::connection::Connection;
use crate::{AxNode, ConsoleLine, ImageFormat, SeenRequest, TestBrowserError};

/// How long a page may take to load.
const LOAD_BUDGET: Duration = Duration::from_secs(30);

/// What the page made while it existed.
#[derive(Default)]
struct Record {
    requests: Vec<SeenRequest>,
    console: Vec<ConsoleLine>,
}

/// One page, in a browser context of its own, recording every request and console line.
pub struct Page {
    connection: Connection,
    session: String,
    context: String,
    record: Arc<Mutex<Record>>,
}

impl Page {
    pub(crate) async fn open(connection: Connection) -> Result<Self, TestBrowserError> {
        let context = connection
            .call(
                None,
                "Target.createBrowserContext",
                json!({ "disposeOnDetach": true }),
            )
            .await?["browserContextId"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let target = connection
            .call(
                None,
                "Target.createTarget",
                json!({ "url": "about:blank", "browserContextId": context }),
            )
            .await?["targetId"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let session = connection
            .call(
                None,
                "Target.attachToTarget",
                json!({ "targetId": target, "flatten": true }),
            )
            .await?["sessionId"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        let record = Arc::new(Mutex::new(Record::default()));
        listen(&connection, session.clone(), Arc::clone(&record));
        let page = Self {
            connection,
            session,
            context,
            record,
        };
        for domain in [
            "Page.enable",
            "Runtime.enable",
            "Network.enable",
            "Log.enable",
        ] {
            page.call(domain, json!({})).await?;
        }
        Ok(page)
    }

    /// Sends a DevTools command on this page's session.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses it.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, TestBrowserError> {
        self.connection
            .call(Some(&self.session), method, params)
            .await
    }

    /// Loads `url` and waits for its `load` event.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when navigation fails, and
    /// [`TestBrowserError::Timeout`] when the page never loads.
    pub async fn goto(&self, url: &str) -> Result<(), TestBrowserError> {
        let mut events = self.connection.events();
        let answer = self.call("Page.navigate", json!({ "url": url })).await?;
        if let Some(error) = answer["errorText"].as_str() {
            return Err(TestBrowserError::Protocol {
                method: "Page.navigate".to_owned(),
                detail: error.to_owned(),
            });
        }
        self.until(&mut events, "Page.loadEventFired", LOAD_BUDGET)
            .await
    }

    /// Waits for the next `load` — the page a form or a script navigated to.
    ///
    /// # Errors
    /// [`TestBrowserError::Timeout`] when no page loads.
    pub async fn next_load(
        &self,
        after: impl AsyncFnOnce(&Self) -> Result<(), TestBrowserError>,
    ) -> Result<(), TestBrowserError> {
        let mut events = self.connection.events();
        after(self).await?;
        self.until(&mut events, "Page.loadEventFired", LOAD_BUDGET)
            .await
    }

    /// Evaluates `expression` in the page, awaiting a promise, and reads its value back.
    ///
    /// # Errors
    /// [`TestBrowserError::Script`] when it throws or answers something not `T`.
    pub async fn eval<T: DeserializeOwned>(&self, expression: &str) -> Result<T, TestBrowserError> {
        let answer = self
            .call(
                "Runtime.evaluate",
                json!({ "expression": expression, "returnByValue": true, "awaitPromise": true }),
            )
            .await?;
        if let Some(exception) = answer.get("exceptionDetails") {
            return Err(TestBrowserError::Script {
                detail: exception["exception"]["description"]
                    .as_str()
                    .or_else(|| exception["text"].as_str())
                    .unwrap_or("an exception")
                    .to_owned(),
            });
        }
        serde_json::from_value(answer["result"]["value"].clone()).map_err(|error| {
            TestBrowserError::Script {
                detail: format!("{expression}: {error}"),
            }
        })
    }

    /// Waits until the script expression `condition` is truthy, asking again across a navigation.
    ///
    /// # Errors
    /// [`TestBrowserError::Timeout`] naming the condition when it never holds.
    pub async fn wait_for(
        &self,
        condition: &str,
        budget: Duration,
    ) -> Result<(), TestBrowserError> {
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            let left = deadline.saturating_duration_since(tokio::time::Instant::now());
            let left_ms = u64::try_from(left.as_millis()).unwrap_or(u64::MAX);
            let poll = format!(
                "new Promise((resolve) => {{ const end = Date.now() + {left_ms}; const tick = () => {{ \
                 let held = false; try {{ held = Boolean({condition}); }} catch (_) {{}} \
                 if (held) resolve(true); else if (Date.now() > end) resolve(false); else setTimeout(tick, 25); }}; tick(); }})"
            );
            match self.eval::<bool>(&poll).await {
                Ok(true) => return Ok(()),
                Ok(false) => break,
                Err(
                    TestBrowserError::Protocol { detail, .. } | TestBrowserError::Script { detail },
                ) if navigated(&detail) && !left.is_zero() => {
                    debug!("the page navigated while a wait was asked; asking the new one");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(TestBrowserError::Timeout {
            what: condition.to_owned(),
            waited_ms: u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
        })
    }

    /// Clicks `selector` with a real mouse press at its centre, one a WebAuthn call may follow.
    ///
    /// # Errors
    /// [`TestBrowserError::Script`] when nothing matches, or it has no size.
    pub async fn click(&self, selector: &str) -> Result<(), TestBrowserError> {
        let (x, y) = self.centre(selector).await?;
        for kind in ["mousePressed", "mouseReleased"] {
            self.call(
                "Input.dispatchMouseEvent",
                json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 }),
            )
            .await?;
        }
        Ok(())
    }

    /// Focuses `selector` and types `text` into it, as a keyboard would.
    ///
    /// # Errors
    /// [`TestBrowserError::Script`] when nothing matches.
    pub async fn type_into(&self, selector: &str, text: &str) -> Result<(), TestBrowserError> {
        let selector_json = serde_json::to_string(selector).unwrap_or_default();
        let found: bool = self
            .eval(&format!(
                "(() => {{ const el = document.querySelector({selector_json}); if (!el) return false; el.focus(); return true; }})()"
            ))
            .await?;
        if !found {
            return Err(TestBrowserError::Script {
                detail: format!("nothing matches {selector}"),
            });
        }
        self.call("Input.insertText", json!({ "text": text }))
            .await?;
        Ok(())
    }

    /// Presses `key` — `Tab`, `Enter`, `Space`, `Escape` — as a keyboard would.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses the key.
    pub async fn press(&self, key: &str) -> Result<(), TestBrowserError> {
        let (code, virtual_key, text) = match key {
            "Tab" => ("Tab", 9, ""),
            "Enter" => ("Enter", 13, "\r"),
            "Space" => ("Space", 32, " "),
            "Escape" => ("Escape", 27, ""),
            other => (other, 0, ""),
        };
        let typed = if key == "Space" { " " } else { key };
        self.call(
            "Input.dispatchKeyEvent",
            json!({ "type": "keyDown", "key": typed, "code": code, "windowsVirtualKeyCode": virtual_key, "text": text }),
        )
        .await?;
        self.call(
            "Input.dispatchKeyEvent",
            json!({ "type": "keyUp", "key": typed, "code": code, "windowsVirtualKeyCode": virtual_key }),
        )
        .await?;
        Ok(())
    }

    /// Turns the page's scripting off or on; it applies from the next load.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses.
    pub async fn scripting(&self, enabled: bool) -> Result<(), TestBrowserError> {
        self.call(
            "Emulation.setScriptExecutionDisabled",
            json!({ "value": !enabled }),
        )
        .await?;
        Ok(())
    }

    /// Lays the page out at `width` × `height` CSS pixels, in `dark` or light.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses.
    pub async fn view(&self, width: u32, height: u32, dark: bool) -> Result<(), TestBrowserError> {
        self.call(
            "Emulation.setDeviceMetricsOverride",
            json!({ "width": width, "height": height, "deviceScaleFactor": 1, "mobile": width < 600 }),
        )
        .await?;
        self.call(
            "Emulation.setEmulatedMedia",
            json!({ "features": [{ "name": "prefers-color-scheme", "value": if dark { "dark" } else { "light" } }] }),
        )
        .await?;
        Ok(())
    }

    /// Runs `source` in every document this page loads from now on, before the page's own script.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses.
    pub async fn before_every_load(&self, source: &str) -> Result<(), TestBrowserError> {
        self.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": source }),
        )
        .await?;
        Ok(())
    }

    /// Adds a virtual platform authenticator and answers its id.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses.
    pub async fn virtual_authenticator(&self) -> Result<String, TestBrowserError> {
        self.call("WebAuthn.enable", json!({ "enableUI": false }))
            .await?;
        let added = self
            .call(
                "WebAuthn.addVirtualAuthenticator",
                json!({ "options": {
                    "protocol": "ctap2",
                    "transport": "internal",
                    "hasResidentKey": true,
                    "hasUserVerification": true,
                    "isUserVerified": true,
                    "automaticPresenceSimulation": true,
                } }),
            )
            .await?;
        Ok(added["authenticatorId"]
            .as_str()
            .unwrap_or_default()
            .to_owned())
    }

    /// Whether `authenticator` gives the person's presence by itself; off, a request waits.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses.
    pub async fn presence(
        &self,
        authenticator: &str,
        automatic: bool,
    ) -> Result<(), TestBrowserError> {
        self.call(
            "WebAuthn.setAutomaticPresenceSimulation",
            json!({ "authenticatorId": authenticator, "enabled": automatic }),
        )
        .await?;
        Ok(())
    }

    /// The credentials `authenticator` holds.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses.
    pub async fn credentials(&self, authenticator: &str) -> Result<Vec<Value>, TestBrowserError> {
        let held = self
            .call(
                "WebAuthn.getCredentials",
                json!({ "authenticatorId": authenticator }),
            )
            .await?;
        Ok(held["credentials"].as_array().cloned().unwrap_or_default())
    }

    /// The cookies the page's context holds, as `name=value` pairs.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses.
    pub async fn cookies(&self) -> Result<Vec<(String, String)>, TestBrowserError> {
        let held = self.call("Network.getCookies", json!({})).await?;
        Ok(held["cookies"]
            .as_array()
            .map(|cookies| {
                cookies
                    .iter()
                    .map(|cookie| {
                        (
                            cookie["name"].as_str().unwrap_or_default().to_owned(),
                            cookie["value"].as_str().unwrap_or_default().to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// The accessibility tree as a screen reader is given it.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses.
    pub async fn accessibility_tree(&self) -> Result<Vec<AxNode>, TestBrowserError> {
        let tree = self.call("Accessibility.getFullAXTree", json!({})).await?;
        Ok(tree["nodes"]
            .as_array()
            .map(|nodes| nodes.iter().map(ax_node).collect())
            .unwrap_or_default())
    }

    /// What the page shows in its viewport now, encoded as `format`.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses or answers no readable image.
    #[instrument(skip(self))]
    pub async fn screenshot(&self, format: ImageFormat) -> Result<Vec<u8>, TestBrowserError> {
        let (name, quality) = format.devtools();
        let mut params = json!({ "format": name, "captureBeyondViewport": false });
        if let Some(quality) = quality {
            params["quality"] = json!(quality);
        }
        let answer = self.call("Page.captureScreenshot", params).await?;
        let refused = |detail: String| TestBrowserError::Protocol {
            method: "Page.captureScreenshot".to_owned(),
            detail,
        };
        let data = answer["data"]
            .as_str()
            .ok_or_else(|| refused("the answer carried no image".to_owned()))?;
        let bytes = STANDARD
            .decode(data)
            .map_err(|error| refused(format!("the image is not base64: {error}")))?;
        debug!(format = name, bytes = bytes.len(), "screenshot captured");
        Ok(bytes)
    }

    /// Every request the page made so far.
    #[must_use]
    pub fn requests(&self) -> Vec<SeenRequest> {
        self.record
            .lock()
            .map(|record| record.requests.clone())
            .unwrap_or_default()
    }

    /// Every console line and uncaught exception so far.
    #[must_use]
    pub fn console(&self) -> Vec<ConsoleLine> {
        self.record
            .lock()
            .map(|record| record.console.clone())
            .unwrap_or_default()
    }

    /// Ends the page's browser context, with its cookies and authenticators.
    ///
    /// # Errors
    /// [`TestBrowserError::Protocol`] when the browser refuses.
    pub async fn close(self) -> Result<(), TestBrowserError> {
        self.connection
            .call(
                None,
                "Target.disposeBrowserContext",
                json!({ "browserContextId": self.context }),
            )
            .await?;
        Ok(())
    }

    async fn centre(&self, selector: &str) -> Result<(f64, f64), TestBrowserError> {
        let selector_json = serde_json::to_string(selector).unwrap_or_default();
        let rect: Option<(f64, f64)> = self
            .eval(&format!(
                "(() => {{ const el = document.querySelector({selector_json}); if (!el) return null; \
                 el.scrollIntoView({{ block: 'center' }}); const r = el.getBoundingClientRect(); \
                 return (r.width && r.height) ? [r.left + r.width / 2, r.top + r.height / 2] : null; }})()"
            ))
            .await?;
        rect.ok_or_else(|| TestBrowserError::Script {
            detail: format!("nothing visible matches {selector}"),
        })
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    async fn until(
        &self,
        events: &mut tokio::sync::broadcast::Receiver<crate::CdpEvent>,
        method: &str,
        budget: Duration,
    ) -> Result<(), TestBrowserError> {
        let wait = async {
            loop {
                match events.recv().await {
                    Ok(event)
                        if event.method == method
                            && event.session.as_deref() == Some(self.session.as_str()) =>
                    {
                        return Ok(());
                    }
                    Ok(_) => {}
                    Err(RecvError::Lagged(missed)) => {
                        warn!(missed, "DevTools: events were missed while waiting");
                    }
                    Err(RecvError::Closed) => {
                        return Err(TestBrowserError::Unavailable {
                            detail: "the connection closed".to_owned(),
                        });
                    }
                }
            }
        };
        tokio::time::timeout(budget, wait)
            .await
            .map_err(|_| TestBrowserError::Timeout {
                what: method.to_owned(),
                waited_ms: u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
            })?
    }
}

#[expect(
    clippy::too_many_lines,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
fn listen(connection: &Connection, session: String, record: Arc<Mutex<Record>>) {
    let mut events = connection.events();
    tokio::spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(RecvError::Lagged(missed)) => {
                    warn!(missed, "DevTools: the page's record missed events");
                    continue;
                }
                Err(RecvError::Closed) => break,
            };
            if event.session.as_deref() != Some(session.as_str()) {
                continue;
            }
            let Ok(mut record) = record.lock() else {
                break;
            };
            match event.method.as_str() {
                "Network.requestWillBeSent" => {
                    let request = &event.params["request"];
                    record.requests.push(SeenRequest {
                        method: request["method"].as_str().unwrap_or_default().to_owned(),
                        url: request["url"].as_str().unwrap_or_default().to_owned(),
                        body: request["postData"].as_str().map(str::to_owned),
                    });
                }
                "Runtime.consoleAPICalled" => record.console.push(ConsoleLine {
                    level: event.params["type"].as_str().unwrap_or_default().to_owned(),
                    text: event.params["args"]
                        .as_array()
                        .map(|args| {
                            args.iter()
                                .map(|arg| {
                                    arg["value"]
                                        .as_str()
                                        .map(str::to_owned)
                                        .unwrap_or_else(|| arg["description"].to_string())
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default(),
                }),
                "Runtime.exceptionThrown" => record.console.push(ConsoleLine {
                    level: "exception".to_owned(),
                    text: event.params["exceptionDetails"]["exception"]["description"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                }),
                "Log.entryAdded" => record.console.push(ConsoleLine {
                    level: event.params["entry"]["level"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    text: event.params["entry"]["text"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                }),
                _ => {}
            }
        }
        debug!("DevTools: the page's record ended");
    });
}

fn ax_node(node: &Value) -> AxNode {
    let text = |value: &Value| value["value"].as_str().unwrap_or_default().to_owned();
    AxNode {
        role: text(&node["role"]),
        name: text(&node["name"]),
        ignored: node["ignored"].as_bool().unwrap_or(false),
        properties: node["properties"]
            .as_array()
            .map(|properties| {
                properties
                    .iter()
                    .map(|property| {
                        (
                            property["name"].as_str().unwrap_or_default().to_owned(),
                            property["value"]["value"].clone(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn navigated(detail: &str) -> bool {
    [
        "navigated or closed",
        "Execution context was destroyed",
        "Cannot find context",
    ]
    .iter()
    .any(|sign| detail.contains(sign))
}
