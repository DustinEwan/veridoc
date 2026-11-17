//! Engine-side plugin client: one plugin process, JSON-RPC over its stdio.
//!
//! The rule this file exists to enforce is that a plugin is a fallible peer. A slow answer,
//! EOF, an exit, and an engine kill are all [`Loss`]es, and every one of them is a run
//! failure: the CLI prints `run failed: {kind}: {detail}` on cli stderr and exits 2, and no claim
//! is given an outcome. A refusal is not a loss: the plugin answered, so it is a run failure
//! too, but it says the process is alive. A lost plugin never becomes a claim state.
//!
//! Two deliberate departures from autofuzz, recorded so an implementer does not "fix" them
//! back:
//!
//! 1. The engine checks the protocol version here, in `connect`, rather than only in a probe
//!    command: a run with a mismatched plugin must refuse rather than misread the shapes.
//! 2. One connection, sequential calls. veridoc runs one scenario per claim, not millions of
//!    invocations, so the worker pool and restart watchdog autofuzz exists to manage have
//!    nothing to manage, and the scenario deadline becomes one deterministic timer.
//!
//! Ownership follows autofuzz: the reaper thread owns the `Child`, because only one place can
//! call `wait()`; one mutex guards the loss reason and the in-flight slots together, so a
//! waiter is woken with the reason already recorded.

use std::collections::BTreeMap;
use std::process::ChildStdin;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt as _;
use std::process::ExitStatus;

use serde::Serialize;
use veridoc_protocol::*;

/// How the engine launches the plugin: argv, plus the environment and directory it needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Launch {
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
}

impl Launch {
    pub fn new(argv: Vec<String>) -> Self {
        Launch {
            argv,
            env: BTreeMap::new(),
            cwd: None,
        }
    }

    /// The program name, for messages that name the thing that failed.
    pub fn display(&self) -> String {
        self.argv.first().cloned().unwrap_or_default()
    }
}

/// Whether the plugin is running and version-checked, or it is not. A version mismatch is
/// veridoc's own rule, not a mechanism loss: the plugin answered the handshake correctly and this
/// engine declined it. A handshake refusal is neither of those: the plugin answered that it will
/// not be a veridoc plugin, and its process is alive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// The plugin could not be started, or gave no handshake answer.
    Lost(Loss),
    /// The plugin answered the handshake with an error frame. It is running and it declined to be
    /// version-checked, which is a different thing to tell an operator than a plugin that never
    /// answered.
    Refused { code: i64, message: String },
    /// The plugin answered the handshake with a protocol revision this engine does not speak.
    UnsupportedVersion { reported: String },
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnectError::Lost(loss) => write!(f, "{loss}"),
            ConnectError::Refused { code, message } => {
                write!(f, "plugin refused the handshake ({code}): {message}")
            }
            ConnectError::UnsupportedVersion { reported } => write!(
                f,
                "plugin reported protocol \"{reported}\"; this engine speaks \"{PROTOCOL_VERSION}\""
            ),
        }
    }
}

impl From<Loss> for ConnectError {
    fn from(loss: Loss) -> Self {
        ConnectError::Lost(loss)
    }
}

/// One plugin call as a sentence, for a caller whose own failure channel is a `String` rather
/// than a [`Call`]. A refusal and a loss both mean no payload, but they are different fixes:
/// the plugin answered that it will not do the work, or the plugin did not answer at all. The
/// two must not read the same, because a refusal means the process is alive and nothing that
/// restarts it should fire — so the wording names the plugin's own message for a refusal and
/// the loss reason for a loss, and a caller that branches on liveness uses `Call` directly.
pub fn sentence<T>(what: &str, call: Call<T>) -> Result<T, String> {
    match call {
        Call::Result(value) => Ok(value),
        Call::Refusal(error) => Err(format!(
            "plugin refused {what} ({}): {}",
            error.code, error.message
        )),
        Call::Lost(loss) => Err(format!("plugin did not {what}: {loss}")),
    }
}

/// The engine owns the kill timer; the grace is how long a killed plugin gets to die before
/// the engine stops waiting on it.
pub const KILL_GRACE: Duration = Duration::from_millis(250);

/// Fixed call timeouts. `handshake`, `collect`, and `run` take theirs from the config or the
/// scenario deadline; these do not vary.
pub const SHORT_TIMEOUT: Duration = Duration::from_secs(30);

/// The loss reason plus the requests still in flight, under one lock so a waiter is woken
/// with the reason already recorded.
struct State {
    lost: Option<Loss>,
    slots: BTreeMap<u64, mpsc::Sender<Response>>,
}

impl State {
    /// Record `reason`, keeping the first cause the mechanism's precedence rules allow, then
    /// wake waiters. `Loss::resolve` is what gives the engine's own `Killed` precedence over
    /// the exit status that kill produced, and lets the reaper's exit status replace the
    /// reader's placeholder `Eof`.
    fn lose(&mut self, reason: Loss) {
        self.lost = Some(Loss::resolve(self.lost.take(), reason));
        self.slots.clear();
    }
}

/// One live plugin process plus the pipe machinery around it.
pub struct PluginClient {
    launch: Launch,
    next_id: AtomicU64,
    stdin: Mutex<ChildStdin>,
    state: Arc<Mutex<State>>,
    kill: mpsc::SyncSender<mpsc::Sender<()>>,
    reaper: Mutex<Option<mpsc::Receiver<()>>>,
    pub stderr_lines: Arc<Mutex<Vec<String>>>,
    pid: u32,
}

impl PluginClient {
    /// Spawn, handshake, and check the protocol version. A spawn failure or a handshake that gave
    /// no answer is a [`ConnectError::Lost`]; an error frame on the handshake is a
    /// [`ConnectError::Refused`]; a revision this engine does not speak is
    /// [`ConnectError::UnsupportedVersion`]. The engine never speaks to a plugin it did not
    /// version-check.
    pub fn connect(
        launch: &Launch,
        handshake_timeout: Duration,
    ) -> Result<(PluginClient, Handshake), ConnectError> {
        let client = PluginClient::spawn(launch)?;
        let handshake = match client.handshake(handshake_timeout) {
            Call::Result(h) => h,
            // A plugin that answers the handshake with an error frame is running and has declined
            // to be a veridoc plugin. That is neither a lost connection nor a version the engine
            // declined, and folding it into either would tell the operator the wrong thing.
            Call::Refusal(error) => {
                return Err(ConnectError::Refused {
                    code: error.code,
                    message: error.message,
                })
            }
            Call::Lost(loss) => return Err(ConnectError::Lost(loss)),
        };
        if !SUPPORTED_VERSIONS.contains(&handshake.protocol_version.as_str()) {
            return Err(ConnectError::UnsupportedVersion {
                reported: handshake.protocol_version,
            });
        }
        Ok((client, handshake))
    }

    /// Start the plugin. A spawn failure is reported as `Loss::Spawn`, so the caller
    /// reports it like any other plugin loss.
    pub fn spawn(launch: &Launch) -> Result<PluginClient, Loss> {
        let Some(program) = launch.argv.first() else {
            return Err(Loss::Spawn { detail: "empty plugin.command".to_string() });
        };
        let mut cmd = Command::new(program);
        cmd.args(launch.argv.iter().skip(1))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .envs(&launch.env);
        if let Some(cwd) = &launch.cwd {
            cmd.current_dir(cwd);
        }
        let mut child = match cmd.spawn() {
            Ok(child) => child,
            Err(e) => return Err(Loss::Spawn { detail: format!("{}: {e}", launch.display()) }),
        };
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let pid = child.id();

        let state = Arc::new(Mutex::new(State {
            lost: None,
            slots: BTreeMap::new(),
        }));
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));
        let (kill_tx, kill_rx) = mpsc::sync_channel::<mpsc::Sender<()>>(1);
        let (reaper_done_tx, reaper_done_rx) = mpsc::channel();

        spawn_reader(stdout, Arc::clone(&state));
        spawn_reaper(child, Arc::clone(&state), kill_rx, reaper_done_tx);
        spawn_stderr_drain(stderr, Arc::clone(&stderr_lines));

        Ok(PluginClient {
            launch: launch.clone(),
            next_id: AtomicU64::new(1),
            stdin: Mutex::new(stdin),
            state,
            kill: kill_tx,
            reaper: Mutex::new(Some(reaper_done_rx)),
            stderr_lines,
            pid,
        })
    }

    pub fn launch(&self) -> &Launch {
        &self.launch
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// The recorded loss, if the process is gone. Requests made after this fail immediately.
    pub fn lost(&self) -> Option<Loss> {
        self.state.lock().expect("state lock").lost.clone()
    }

    /// Kill the plugin now and wait for the exit. Any call in flight resolves as
    /// `Loss::Killed`.
    pub fn kill(&self) {
        self.state
            .lock()
            .expect("state lock")
            .lose(Loss::Killed);
        let (ack_tx, ack_rx) = mpsc::channel();
        // A full or closed channel means the reaper already finished: the process is gone.
        if self.kill.send(ack_tx).is_ok() {
            let _ = ack_rx.recv_timeout(KILL_GRACE * 40);
        }
    }

    /// Send one request and wait for the matching response. One `rx.recv_timeout` per
    /// request, not a poll loop: the deadline is the wait.
    pub fn request<T>(&self, method: Method, params: Option<serde_json::Value>, timeout: Duration) -> Call<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let deadline = Instant::now() + timeout;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        {
            let mut state = self.state.lock().expect("state lock");
            if let Some(reason) = state.lost.clone() {
                return Call::Lost(reason);
            }
            state.slots.insert(id, tx);
        }

        let req = match params {
            Some(params) => Request::new(id, method, params),
            None => Request::empty(id, method),
        };
        let write = {
            let mut stdin = self.stdin.lock().expect("stdin lock");
            write_line(&mut *stdin, &req)
        };
        if let Err(e) = write {
            self.drop_slot(id);
            // The write failed because the peer is gone. Give the reaper its poll interval to
            // record the exit status, so the failure says `exited with code 1` rather than
            // the reader's earlier `closed the pipe`.
            let reason = self
                .await_exit_reason(Duration::from_millis(250))
                .unwrap_or_else(|| Loss::Spawn {
                    detail: format!("pipe to {} closed: {e}", self.launch.display()),
                });
            return Call::Lost(reason);
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        let response = match rx.recv_timeout(remaining) {
            Ok(resp) => resp,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.drop_slot(id);
                // A recorded loss is more specific than "slow": the plugin died while we
                // waited, and the caller should classify the death, not the timeout.
                return Call::Lost(self.lost().unwrap_or(Loss::NoAnswer));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.drop_slot(id);
                return Call::Lost(
                    self.await_exit_reason(Duration::from_millis(250))
                        .unwrap_or(Loss::Eof),
                );
            }
        };

        response.into_call::<T>()
    }

    fn drop_slot(&self, id: u64) {
        self.state.lock().expect("state lock").slots.remove(&id);
    }

    /// The reaper records the exit status a poll interval after the reader notices a closed
    /// pipe. Wait that long for the specific reason before settling for a generic one.
    fn await_exit_reason(&self, grace: Duration) -> Option<Loss> {
        let until = Instant::now() + grace;
        loop {
            match self.lost() {
                // EOF is the reader's placeholder; the reaper's exit status is the truth.
                Some(Loss::Eof) if Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                reason => return reason,
            }
        }
    }

    fn call<T: serde::de::DeserializeOwned>(&self, method: Method, req: &impl Serialize, timeout: Duration) -> Call<T> {
        self.request(
            method,
            Some(serde_json::to_value(req).expect("request params are serializable")),
            timeout,
        )
    }

    pub fn handshake(&self, timeout: Duration) -> Call<Handshake> {
        self.request(Method::Handshake, None, timeout)
    }

    pub fn collect(&self, root: &str, timeout: Duration) -> Call<CollectResult> {
        self.call(Method::Collect, &CollectReq { root: root.to_string() }, timeout)
    }

    pub fn resolve(&self, refs: &[String]) -> Call<ResolveResult> {
        self.call(Method::Resolve, &ResolveReq { refs: refs.to_vec() }, SHORT_TIMEOUT)
    }

    pub fn derivable(&self, referent_id: &str) -> Call<DerivableResult> {
        self.call(
            Method::Derivable,
            &DerivableReq {
                referent_id: referent_id.to_string(),
            },
            SHORT_TIMEOUT,
        )
    }

    pub fn bivalence(&self, claim_id: &str) -> Call<BivalenceResult> {
        self.call(
            Method::Bivalence,
            &BivalenceReq {
                claim_id: claim_id.to_string(),
            },
            SHORT_TIMEOUT,
        )
    }

    /// One scenario. The call deadline is the scenario deadline plus the kill grace, so the
    /// engine owns the kill timer and a slow machine is not blamed on the prose.
    pub fn run(&self, req: &RunRequest) -> Call<RunResponse> {
        let timeout = Duration::from_millis(req.deadline_ms) + KILL_GRACE;
        self.call(Method::Run, req, timeout)
    }

    pub fn cite(&self, citations: &[Citation]) -> Call<CiteResult> {
        self.call(
            Method::Cite,
            &CiteReq {
                citations: citations.to_vec(),
            },
            SHORT_TIMEOUT,
        )
    }

    pub fn surface(&self) -> Call<SurfaceResult> {
        self.request(Method::Surface, None, SHORT_TIMEOUT)
    }
}

/// The distinct stderr lines the plugin wrote, capped at 4096 lines; they reach the run
/// failure on stderr as `plugin said: …`.
pub fn plugin_said(lines: &[String]) -> Vec<String> {
    let mut seen = Vec::new();
    for line in lines {
        if !seen.contains(line) {
            seen.push(line.clone());
        }
    }
    seen
}

// The `params` shapes of the four requests the spec writes inline.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectReq {
    root: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResolveReq {
    refs: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DerivableReq {
    referent_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BivalenceReq {
    claim_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CiteReq {
    citations: Vec<Citation>,
}

/// A plugin client is mostly OS handles, so `Debug` reports what identifies it and nothing else.
impl std::fmt::Debug for PluginClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginClient")
            .field("launch", &self.launch.display())
            .field("pid", &self.pid)
            .field("lost", &self.lost())
            .finish_non_exhaustive()
    }
}

impl Drop for PluginClient {
    fn drop(&mut self) {
        self.kill();
        if let Some(rx) = self.reaper.lock().expect("reaper lock").take() {
            let _ = rx.recv_timeout(Duration::from_secs(10));
        }
    }
}

fn spawn_reader(stdout: std::process::ChildStdout, state: Arc<Mutex<State>>) {
    std::thread::Builder::new()
        .name("veridoc-rpc-reader".to_string())
        .spawn(move || {
            let mut reader = buffered(stdout);
            let mut buf = String::new();
            // EOF or a read error ends the loop, and the line below records the loss.
            while let Ok(Some(line)) = read_line(&mut reader, &mut buf) {
                // An unparseable line is dropped so the waiting request times out, and a
                // response whose id matches no in-flight request is discarded.
                let Ok(resp) = parse_response(&line) else {
                    continue;
                };
                let slot = {
                    let mut state = state.lock().expect("state lock");
                    state.slots.remove(&resp.id())
                };
                if let Some(tx) = slot {
                    let _ = tx.send(resp);
                }
            }
            // A closed pipe is how a plugin dies when no exit status is readable yet.
            state.lock().expect("state lock").lose(Loss::Eof);
        })
        .expect("spawn reader");
}

/// Owns the `Child`: waits for exit, records the reason, and answers kill requests.
fn spawn_reaper(
    mut child: Child,
    state: Arc<Mutex<State>>,
    kill_rx: mpsc::Receiver<mpsc::Sender<()>>,
    done_tx: mpsc::Sender<()>,
) {
    std::thread::Builder::new()
        .name("veridoc-rpc-reaper".to_string())
        .spawn(move || {
            let status = loop {
                if let Ok(ack) = kill_rx.try_recv() {
                    let _ = child.kill();
                    let status = child.wait();
                    let _ = ack.send(());
                    break status;
                }
                match child.try_wait() {
                    Ok(Some(status)) => break Ok(status),
                    Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                    Err(e) => {
                        eprintln!("veridoc: cannot wait on plugin: {e}");
                        break Err(e);
                    }
                }
            };
            let reason = match status {
                Ok(status) => Loss::Exited {
                    code: exit_code(&status),
                    signal: exit_signal(&status),
                },
                Err(_) => Loss::Eof,
            };
            state.lock().expect("state lock").lose(reason);
            let _ = done_tx.send(());
        })
        .expect("spawn reaper");
}

/// The signal that ended the plugin, when there was one. Windows has no signal numbers, so
/// the engine classifies an exit there by code alone.
fn exit_signal(status: &ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        status.signal()
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

fn exit_code(status: &ExitStatus) -> Option<i32> {
    status.code()
}

fn spawn_stderr_drain(stderr: std::process::ChildStderr, lines: Arc<Mutex<Vec<String>>>) {
    std::thread::Builder::new()
        .name("veridoc-rpc-stderr".to_string())
        .spawn(move || {
            let mut reader = buffered(stderr);
            let mut buf = String::new();
            while let Ok(Some(line)) = read_line(&mut reader, &mut buf) {
                let mut lines = lines.lock().expect("stderr lock");
                if lines.len() < 4096 {
                    lines.push(line);
                }
            }
        })
        .expect("spawn stderr drain");
}
