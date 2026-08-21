//! Where a profile's browser actually is, and how to talk to it.
//!
//! Every automation tool answers "where is this browser?" by reading a local
//! debugging port out of the local profile directory. There is exactly one
//! resolver here, [`resolve`], and one connection type, [`CdpConnection`]:
//! tools ask for a target and get a page socket on this machine, and nothing
//! above this module has to know how it was reached.

use crate::profile::types::BrowserProfile;
use serde_json::Value;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Request as WsRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// How long the WebSocket handshake may take before the connect is abandoned.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long one CDP command may wait for its reply.
///
/// Without a cap, a browser that never answers holds the caller until the
/// socket dies. An automation client that hangs is worse than one that fails.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// Attempts at establishing a connection before giving up.
const CONNECT_ATTEMPTS: u32 = 3;

/// Delay before the second connect attempt; doubles for the third.
const CONNECT_RETRY_BASE: Duration = Duration::from_millis(400);

/// Where a profile's browser is, and what is needed to reach it.
#[derive(Debug, Clone)]
pub enum CdpTarget {
  /// A browser on this machine. The URL is a PAGE-level socket, so commands
  /// carry no CDP session id.
  Local { ws_url: String },
}

impl CdpTarget {
  /// No browser is remote any more; every target is local.
  pub fn is_remote(&self) -> bool {
    false
  }

  /// A short label for logs and errors. Never carries the credential.
  pub fn describe(&self) -> String {
    match self {
      Self::Local { .. } => "local browser".to_string(),
    }
  }
}

/// Why a target could not be reached, or a command could not be run.
///
/// The variants exist so a caller can tell "come back when it is up" from "that
/// credential is no good" from "the socket broke". Collapsing them into one
/// string is how a session that is merely still provisioning gets reported as a
/// broken one.
#[derive(Debug)]
pub enum CdpError {
  /// Nothing is listening, or the relay could not reach the browser.
  Unreachable(String),
  /// The relay refused the credential.
  Unauthorized(String),
  /// The session exists but is not in a state that can be driven.
  NotDrivable(String),
  /// The socket broke, or a reply never arrived.
  Transport(String),
  /// The browser answered with a CDP `error` object.
  Protocol(String),
}

impl std::fmt::Display for CdpError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Unreachable(m) => write!(f, "browser unreachable: {m}"),
      Self::Unauthorized(m) => write!(f, "not authorised to drive this browser: {m}"),
      Self::NotDrivable(m) => write!(f, "browser is not drivable yet: {m}"),
      Self::Transport(m) => write!(f, "CDP transport failed: {m}"),
      Self::Protocol(m) => write!(f, "CDP error: {m}"),
    }
  }
}

impl CdpError {
  /// Whether a fresh connection attempt could plausibly succeed.
  ///
  /// A refused credential and a session that is still provisioning are answers,
  /// not failures. Retrying either spends the caller's time and, on the relay,
  /// burns one of the four attachments a session is allowed — so the retry can
  /// make the next honest attempt fail too.
  fn is_retryable(&self) -> bool {
    matches!(self, Self::Unreachable(_) | Self::Transport(_))
  }
}

/// Why a profile could not be resolved to a browser at all.
#[derive(Debug)]
pub enum ResolveError {
  /// The profile is not one this app can drive.
  Unsupported(String),
  /// Neither a local process nor a live remote session.
  NotRunning(String),
  /// A remote session exists but its endpoint could not be read.
  Endpoint(String),
}

impl std::fmt::Display for ResolveError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Unsupported(m) | Self::NotRunning(m) | Self::Endpoint(m) => write!(f, "{m}"),
    }
  }
}

/// Whether the caller is willing to wait for a browser that is still coming up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Patience {
  /// One attempt, used to decide local-vs-remote without stalling.
  Immediate,
  /// The full retry budget, used once the answer is known to be local.
  WaitForLaunch,
}

impl Patience {
  fn attempts(self, waiting: u32) -> u32 {
    match self {
      Self::Immediate => 1,
      Self::WaitForLaunch => waiting,
    }
  }
}

/// Find the local browser for `profile`.
///
/// The check is split in two: one cheap probe first, then — only if that misses
/// — a patient probe that spends its full budget waiting for a browser that is
/// still starting. The split also covers a stale `process_id` left by a crash,
/// where nothing answers on the recorded port.
pub async fn resolve(profile: &BrowserProfile) -> Result<CdpTarget, ResolveError> {
  if profile.browser != "wayfern" {
    return Err(ResolveError::Unsupported(format!(
      "Profile '{}' runs {}, which cannot be driven over CDP",
      profile.name, profile.browser
    )));
  }

  let has_local_process = profile.process_id.is_some();

  if has_local_process {
    if let Some(ws_url) = local_page_ws_url(profile, Patience::Immediate).await {
      return Ok(CdpTarget::Local { ws_url });
    }
  }

  if has_local_process {
    return match local_page_ws_url(profile, Patience::WaitForLaunch).await {
      Some(ws_url) => Ok(CdpTarget::Local { ws_url }),
      None => Err(ResolveError::NotRunning(format!(
        "No CDP connection available for profile '{}'. Make sure the browser is running.",
        profile.name
      ))),
    };
  }

  Err(ResolveError::NotRunning(format!(
    "Profile '{}' is not running",
    profile.name
  )))
}

/// The debugging port a locally launched browser registered for this profile.
async fn local_cdp_port(profile: &BrowserProfile, patience: Patience) -> Option<u16> {
  let profiles_dir = crate::profile::manager::ProfileManager::instance().get_profiles_dir();
  let profile_path = profile.get_profile_data_path(&profiles_dir);
  let profile_path_str = profile_path.to_string_lossy().to_string();

  // Port info is written once the process is up, so a tool called straight
  // after a launch has to wait for it.
  for attempt in 0..patience.attempts(10) {
    if attempt > 0 {
      tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if let Some(port) = crate::wayfern_manager::WayfernManager::instance()
      .get_cdp_port(&profile_path_str)
      .await
    {
      return Some(port);
    }
  }
  None
}

/// A page-level socket on a locally running browser.
///
/// Returns `None` rather than an error: a miss is how the resolver decides the
/// browser is not local, and a port whose process has since died answers
/// nothing, which is exactly the signal that decision needs.
async fn local_page_ws_url(profile: &BrowserProfile, patience: Patience) -> Option<String> {
  let port = local_cdp_port(profile, patience).await?;
  let listing = format!("http://127.0.0.1:{port}/json");
  let client = reqwest::Client::new();

  let mut last_err = String::new();
  for attempt in 0..patience.attempts(15) {
    if attempt > 0 {
      tokio::time::sleep(Duration::from_secs(1)).await;
    }
    match client
      .get(&listing)
      .timeout(Duration::from_secs(3))
      .send()
      .await
    {
      Ok(response) => match response.json::<Vec<Value>>().await {
        Ok(targets) => {
          if let Some(ws_url) = pick_local_page_socket(&targets) {
            return Some(ws_url);
          }
          last_err = "no page target found in browser".to_string();
        }
        Err(e) => last_err = format!("failed to parse CDP targets: {e}"),
      },
      Err(e) => last_err = format!("failed to reach the browser's CDP endpoint: {e}"),
    }
  }

  if patience == Patience::WaitForLaunch {
    log::warn!("Local CDP discovery on port {port} gave up: {last_err}");
  }
  None
}

/// Pick a drivable page from what `/json` lists on a local browser.
pub fn pick_local_page_socket(targets: &[Value]) -> Option<String> {
  targets
    .iter()
    .find(|t| t.get("type").and_then(Value::as_str) == Some("page"))
    .and_then(|t| t.get("webSocketDebuggerUrl"))
    .and_then(Value::as_str)
    .map(str::to_string)
}

/// One outgoing CDP message, addressed to a page when a session id is in play.
pub fn cdp_frame(session: Option<&str>, id: u64, method: &str, params: Value) -> Value {
  let mut message = serde_json::json!({ "id": id, "method": method, "params": params });
  if let Some(session) = session {
    message["sessionId"] = Value::String(session.to_string());
  }
  message
}

/// Why the peer hung up.
#[derive(Debug, Clone)]
struct CloseInfo {
  code: u16,
  reason: String,
}

/// An open CDP conversation with one browser.
pub struct CdpConnection {
  stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
  closed: Option<CloseInfo>,
}

impl CdpConnection {
  fn new(stream: WebSocketStream<MaybeTlsStream<TcpStream>>) -> Self {
    Self {
      stream,
      closed: None,
    }
  }

  /// Send `method` as command `id`.
  pub async fn send_command(
    &mut self,
    id: u64,
    method: &str,
    params: Value,
  ) -> Result<(), CdpError> {
    use futures_util::sink::SinkExt;
    let frame = cdp_frame(None, id, method, params);
    self
      .stream
      .send(Message::Text(frame.to_string().into()))
      .await
      .map_err(|e| CdpError::Transport(format!("failed to send CDP command: {e}")))
  }

  /// The next text message, or `None` once the peer has gone.
  ///
  /// Binary frames, pings and pongs are consumed silently; a close is recorded
  /// so its reason survives into whatever error the caller builds.
  pub async fn next_text(&mut self) -> Option<Result<String, CdpError>> {
    use futures_util::stream::StreamExt;
    loop {
      match self.stream.next().await? {
        Ok(Message::Text(text)) => return Some(Ok(text.to_string())),
        Ok(Message::Close(frame)) => {
          self.closed = frame.map(|f| CloseInfo {
            code: u16::from(f.code),
            reason: f.reason.to_string(),
          });
          return None;
        }
        Ok(_) => continue,
        Err(e) => {
          return Some(Err(CdpError::Transport(format!(
            "CDP WebSocket error: {e}"
          ))))
        }
      }
    }
  }

  /// Turn a hang-up into the error it means.
  ///
  /// The relay's close codes are its whole vocabulary: 1008 is "that credential
  /// is no good", 1013 is "come back when the session is up". Reporting either
  /// as a generic transport failure throws away the only actionable thing the
  /// server said.
  pub fn closed_error(&self, context: &str) -> CdpError {
    match &self.closed {
      Some(info) if info.reason.is_empty() => {
        classify_close(info.code, format!("{context} (close {})", info.code))
      }
      Some(info) => classify_close(
        info.code,
        format!("{context} ({}: {})", info.code, info.reason),
      ),
      None => CdpError::Transport(format!("{context} (connection closed)")),
    }
  }

  /// Send a command and read its reply, bounded by [`COMMAND_TIMEOUT`].
  pub async fn call(&mut self, id: u64, method: &str, params: Value) -> Result<Value, CdpError> {
    self.send_command(id, method, params).await?;
    self.await_reply(id, COMMAND_TIMEOUT).await
  }

  /// Read until the reply to `id` arrives, discarding events on the way.
  pub async fn await_reply(&mut self, id: u64, timeout: Duration) -> Result<Value, CdpError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
      let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
      if remaining.is_zero() {
        return Err(CdpError::Transport(
          "timed out waiting for a CDP response".to_string(),
        ));
      }
      let text = match tokio::time::timeout(remaining, self.next_text()).await {
        Err(_) => {
          return Err(CdpError::Transport(
            "timed out waiting for a CDP response".to_string(),
          ))
        }
        Ok(None) => return Err(self.closed_error("no response received from CDP")),
        Ok(Some(result)) => result?,
      };

      let response: Value = serde_json::from_str(&text)
        .map_err(|e| CdpError::Protocol(format!("failed to parse CDP response: {e}")))?;
      if response.get("id") != Some(&Value::from(id)) {
        continue;
      }
      if let Some(error) = response.get("error") {
        return Err(CdpError::Protocol(error.to_string()));
      }
      return Ok(
        response
          .get("result")
          .cloned()
          .unwrap_or_else(|| serde_json::json!({})),
      );
    }
  }

  /// Hang up politely so the peer releases its side immediately, rather than
  /// dropping the TCP connection and letting it time out.
  pub async fn close(mut self) {
    let _ = self.stream.close(None).await;
  }
}

/// Ids used by the one-shot command runners below.
///
/// A caller never picks these, so they are stated once here rather than being
/// re-derived at each call site.
const RUN_PAGE_ENABLE_ID: u64 = 1;
const RUN_COMMAND_ID: u64 = 2;
const RUN_PAGE_DISABLE_ID: u64 = 3;

/// Run one command on a fresh connection and hand back its result.
pub async fn run_command(
  target: &CdpTarget,
  method: &str,
  params: Value,
) -> Result<Value, CdpError> {
  let mut connection = target.connect().await?;
  let result = connection.call(RUN_COMMAND_ID, method, params).await;
  connection.close().await;
  result
}

/// Run one command, then wait for the page to finish loading.
///
/// Used for anything that might navigate: `Page.navigate` obviously, but also a
/// click or a script that turns out to follow a link. When nothing navigates,
/// the wait simply expires and the command's own result is returned — that is
/// the intended path, not a failure.
pub async fn run_command_awaiting_load(
  target: &CdpTarget,
  method: &str,
  params: Value,
  timeout_secs: u64,
) -> Result<Value, CdpError> {
  let mut connection = target.connect().await?;

  // Page events have to be on before the command runs, or `loadEventFired`
  // for a fast navigation is missed and the wait runs to its full timeout.
  connection
    .call(RUN_PAGE_ENABLE_ID, "Page.enable", serde_json::json!({}))
    .await?;
  connection
    .send_command(RUN_COMMAND_ID, method, params)
    .await?;

  let mut command_result = None;
  let mut failure = None;
  let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);

  loop {
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    if remaining.is_zero() {
      break;
    }
    let text = match tokio::time::timeout(remaining, connection.next_text()).await {
      Ok(Some(Ok(text))) => text,
      Ok(Some(Err(e))) => {
        failure = Some(e);
        break;
      }
      // The peer hung up, or the wait expired. Either way whatever the command
      // already answered is the best result available.
      Ok(None) | Err(_) => break,
    };

    let response: Value = serde_json::from_str(&text).unwrap_or_default();

    if response.get("id") == Some(&Value::from(RUN_COMMAND_ID)) {
      if let Some(error) = response.get("error") {
        failure = Some(CdpError::Protocol(error.to_string()));
        break;
      }
      command_result = Some(
        response
          .get("result")
          .cloned()
          .unwrap_or_else(|| serde_json::json!({})),
      );
    }

    // The load event carries `method` at the top level.
    if response.get("method") == Some(&Value::from("Page.loadEventFired")) {
      break;
    }
  }

  let _ = connection
    .send_command(RUN_PAGE_DISABLE_ID, "Page.disable", serde_json::json!({}))
    .await;
  let closed = connection.closed_error("no response received from CDP");
  connection.close().await;

  if let Some(error) = failure {
    return Err(error);
  }
  command_result.ok_or(closed)
}

/// Point a browser at a URL and wait for it to settle.
///
/// This is what "open a URL in that profile" means once the browser is already
/// up: it navigates the existing page rather than opening a new tab.
pub async fn navigate(target: &CdpTarget, url: &str, timeout_secs: u64) -> Result<(), CdpError> {
  run_command_awaiting_load(
    target,
    "Page.navigate",
    serde_json::json!({ "url": url }),
    timeout_secs,
  )
  .await
  .map(|_| ())
}

/// Map a WebSocket close code onto what the caller should do about it.
pub fn classify_close(code: u16, detail: String) -> CdpError {
  match code {
    1008 => CdpError::Unauthorized(detail),
    1013 => CdpError::NotDrivable(detail),
    1009 => CdpError::Transport(format!("{detail} — message too large")),
    1011 => CdpError::Unreachable(detail),
    _ => CdpError::Transport(detail),
  }
}

impl CdpTarget {
  /// Open a conversation with this browser.
  ///
  /// Retries a connection that failed for a reason a retry could fix, and never
  /// one that failed because the answer was no.
  pub async fn connect(&self) -> Result<CdpConnection, CdpError> {
    let mut attempt = 0u32;
    loop {
      let error = match self.connect_once().await {
        Ok(connection) => return Ok(connection),
        Err(e) => e,
      };
      attempt += 1;
      if attempt >= CONNECT_ATTEMPTS || !error.is_retryable() {
        return Err(error);
      }
      let delay = CONNECT_RETRY_BASE * 2u32.pow(attempt - 1);
      log::warn!(
        "CDP connect to {} failed ({error}); retrying in {}ms",
        self.describe(),
        delay.as_millis()
      );
      tokio::time::sleep(delay).await;
    }
  }

  async fn connect_once(&self) -> Result<CdpConnection, CdpError> {
    match self {
      Self::Local { ws_url } => {
        let request = ws_url
          .as_str()
          .into_client_request()
          .map_err(|e| CdpError::Unreachable(format!("invalid CDP endpoint: {e}")))?;
        Ok(CdpConnection::new(dial(request, None).await?))
      }
    }
  }
}

/// Perform the handshake, bounded by [`CONNECT_TIMEOUT`].
async fn dial(
  request: WsRequest,
  config: Option<WebSocketConfig>,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, CdpError> {
  let connect = tokio_tungstenite::connect_async_with_config(request, config, false);
  match tokio::time::timeout(CONNECT_TIMEOUT, connect).await {
    Err(_) => Err(CdpError::Unreachable(format!(
      "the CDP endpoint did not answer within {}s",
      CONNECT_TIMEOUT.as_secs()
    ))),
    Ok(Ok((stream, _response))) => Ok(stream),
    Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) => {
      Err(classify_handshake_status(response.status().as_u16()))
    }
    Ok(Err(e)) => Err(CdpError::Unreachable(e.to_string())),
  }
}

/// Map a refused upgrade onto the error it means.
///
/// A 401 is a credential problem the caller can fix by signing in again; a 404
/// means the session is not theirs or no longer exists. Surfacing either as
/// "connection failed" is what makes an automation client retry forever.
pub fn classify_handshake_status(status: u16) -> CdpError {
  match status {
    401 | 403 => {
      CdpError::Unauthorized(format!("the relay refused the credential (HTTP {status})"))
    }
    404 => CdpError::NotDrivable(
      "no such remote session, or it does not belong to this account".to_string(),
    ),
    409 => CdpError::NotDrivable("the remote session is not drivable yet".to_string()),
    429 => CdpError::NotDrivable("too many attachments to this remote session".to_string()),
    other => CdpError::Unreachable(format!("the relay answered HTTP {other}")),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn the_local_page_socket_is_read_from_the_json_listing() {
    let targets = vec![
      serde_json::json!({ "type": "background_page", "webSocketDebuggerUrl": "ws://x/bg" }),
      serde_json::json!({ "type": "page", "webSocketDebuggerUrl": "ws://127.0.0.1:1/devtools/page/A" }),
    ];
    assert_eq!(
      pick_local_page_socket(&targets).as_deref(),
      Some("ws://127.0.0.1:1/devtools/page/A")
    );
    assert!(pick_local_page_socket(&[]).is_none());
  }

  #[test]
  fn a_remote_frame_addresses_the_page_and_a_local_one_does_not() {
    // A page-level CDP command must carry `sessionId` only when a session is
    // in play; a missing or spurious id silently misroutes a single tool.
    let remote = cdp_frame(
      Some("SESSION-42"),
      7,
      "Page.navigate",
      serde_json::json!({ "url": "https://example.com" }),
    );
    assert_eq!(remote["sessionId"], "SESSION-42");
    assert_eq!(remote["id"], 7);
    assert_eq!(remote["method"], "Page.navigate");
    assert_eq!(remote["params"]["url"], "https://example.com");

    let local = cdp_frame(None, 7, "Page.navigate", serde_json::json!({}));
    assert!(local.get("sessionId").is_none());
    assert_eq!(local["id"], 7);
  }

  #[test]
  fn a_relay_close_says_what_the_caller_should_do_about_it() {
    // These codes are the relay's entire vocabulary. Collapsing them into one
    // transport failure is how "your session is still provisioning" and "you
    // are signed out" both become "something went wrong".
    assert!(matches!(
      classify_close(1008, "x".into()),
      CdpError::Unauthorized(_)
    ));
    assert!(matches!(
      classify_close(1013, "x".into()),
      CdpError::NotDrivable(_)
    ));
    assert!(matches!(
      classify_close(1011, "x".into()),
      CdpError::Unreachable(_)
    ));
    assert!(matches!(
      classify_close(1009, "x".into()),
      CdpError::Transport(_)
    ));
    assert!(matches!(
      classify_close(1000, "x".into()),
      CdpError::Transport(_)
    ));
  }

  #[test]
  fn only_the_failures_a_retry_could_fix_are_retried() {
    assert!(CdpError::Unreachable("x".into()).is_retryable());
    assert!(CdpError::Transport("x".into()).is_retryable());
    assert!(!CdpError::Unauthorized("x".into()).is_retryable());
    assert!(!CdpError::NotDrivable("x".into()).is_retryable());
    assert!(!CdpError::Protocol("x".into()).is_retryable());
  }

  #[test]
  fn a_refused_upgrade_is_not_reported_as_an_unreachable_browser() {
    assert!(matches!(
      classify_handshake_status(401),
      CdpError::Unauthorized(_)
    ));
    assert!(matches!(
      classify_handshake_status(404),
      CdpError::NotDrivable(_)
    ));
    assert!(matches!(
      classify_handshake_status(409),
      CdpError::NotDrivable(_)
    ));
    assert!(matches!(
      classify_handshake_status(429),
      CdpError::NotDrivable(_)
    ));
    assert!(matches!(
      classify_handshake_status(502),
      CdpError::Unreachable(_)
    ));
  }

  #[test]
  fn a_hasty_probe_tries_once_and_a_patient_one_waits() {
    // A quick probe tries once; a patient one waits out a browser that is
    // still starting.
    assert_eq!(Patience::Immediate.attempts(10), 1);
    assert_eq!(Patience::WaitForLaunch.attempts(10), 10);
  }

  // --- Against a real socket -----------------------------------------------
  //
  // Everything above is pure. This drives the local arm against a WebSocket
  // server and reads back the exact frames it put on the wire: a page command
  // must carry no sessionId and must not be preceded by an attach handshake.

  /// What the fake endpoint observed.
  #[derive(Debug, Default)]
  struct RelayLog {
    /// Every message the client sent, in order.
    received: Vec<Value>,
  }

  /// A stand-in CDP endpoint that echoes each command back so the test can read
  /// what was actually on the wire.
  async fn fake_relay() -> (String, tokio::task::JoinHandle<RelayLog>) {
    use futures_util::sink::SinkExt;
    use futures_util::stream::StreamExt;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
      .await
      .expect("the fake endpoint must bind");
    let port = listener.local_addr().expect("a bound port").port();

    let handle = tokio::spawn(async move {
      let mut log = RelayLog::default();
      let Ok((socket, _)) = listener.accept().await else {
        return log;
      };
      let Ok(mut stream) = tokio_tungstenite::accept_async(socket).await else {
        return log;
      };

      while let Some(Ok(message)) = stream.next().await {
        let Message::Text(text) = message else {
          continue;
        };
        let Ok(request) = serde_json::from_str::<Value>(&text) else {
          continue;
        };
        log.received.push(request.clone());

        // Everything is handed straight back, so the test can assert on the
        // exact frame the client put on the wire.
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let reply = serde_json::json!({
          "id": id,
          "result": { "echo": request }
        });
        if stream
          .send(Message::Text(reply.to_string().into()))
          .await
          .is_err()
        {
          break;
        }
      }
      log
    });

    (format!("ws://127.0.0.1:{port}"), handle)
  }

  #[tokio::test]
  async fn a_local_command_skips_the_attach_and_carries_no_session() {
    // The local arm talks to a PAGE socket. Sending it a sessionId, or making
    // it pay for an attach handshake it does not need, would be a regression
    // in the path that already worked.
    let (ws_url, server) = fake_relay().await;
    let target = CdpTarget::Local { ws_url };

    let result = run_command(
      &target,
      "Runtime.evaluate",
      serde_json::json!({ "expression": "1" }),
    )
    .await
    .expect("a local command must succeed");
    assert_eq!(result["echo"]["method"], "Runtime.evaluate");

    let log = server.await.expect("the fake relay must finish");
    let methods: Vec<&str> = log
      .received
      .iter()
      .filter_map(|m| m.get("method").and_then(Value::as_str))
      .collect();
    assert_eq!(methods, vec!["Runtime.evaluate"]);
    assert!(log.received[0].get("sessionId").is_none());
  }
}
