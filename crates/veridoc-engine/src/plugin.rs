//! Engine-side plugin client: one plugin process, JSON-RPC over its stdio.
//!
//! The rule this file exists to enforce is that a plugin is a fallible peer. A slow answer,
//! EOF, an exit, and an engine kill are all [`Loss`]es, and every one of them is a run
//! failure: the CLI prints `run failed: {kind}: {detail}` on cli stderr and exits 2, and no claim
//! is given an outcome. A refusal is not a loss: the plugin answered, so it is a run failure
//! too, but it says the process is alive. A lost plugin never becomes a claim state.
//!
//! The mechanism — spawn, the stdout reader, the exit watcher, the stderr drain, the in-flight
//! slots, the id allocator, the handshake-first rule, the kill and drop rules, and which loss
//! outranks which — is [`Plugin`], the stdio-process transport of the `plugin_rpc` crate. This
//! file drives it and restates none of it. What is left here is the profile and veridoc's own
//! reporting: the eight methods with their params, the protocol revisions this engine speaks,
//! the fixed call timeouts, and the way a loss or a refusal becomes a run-failure sentence.
//!
//! Two deliberate departures, recorded so an implementer does not "fix" them back:
//!
//! 1. The engine checks the protocol version here, in `connect`, rather than only in a probe
//!    command: a run with a mismatched plugin must refuse rather than misread the shapes. The
//!    mechanism hands the handshake back for exactly this reason — which `protocolVersion`
//!    strings are acceptable is the profile's rule, never the mechanism's.
//! 2. One connection, sequential calls. veridoc runs one scenario per claim, not millions of
//!    invocations, so the worker pool and restart watchdog autofuzz exists to manage have
//!    nothing to manage, and the scenario deadline becomes one deterministic timer.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
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

    /// The same launch in the mechanism's spelling. The handshake wait is the caller's because it
    /// comes from the project config; the request wait is the engine's fixed [`SHORT_TIMEOUT`],
    /// and each call passes its own wait to [`Plugin::request`] besides.
    fn config(&self, handshake_timeout: Duration) -> PluginConfig {
        PluginConfig {
            command: self.argv.clone(),
            env: self.env.clone(),
            cwd: self.cwd.as_ref().map(PathBuf::from),
            timeouts: Timeouts {
                handshake_timeout,
                request_timeout: SHORT_TIMEOUT,
            },
        }
    }
}

/// Whether the plugin is running and version-checked, or it is not. A version mismatch is
/// veridoc's own rule, not a mechanism loss: the plugin answered the handshake correctly and this
/// engine declined it. A handshake refusal is neither of those: the plugin answered that it will
/// not be a veridoc plugin, and its process is alive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// The plugin could not be started, gave no handshake answer, or answered the handshake in a
    /// shape this profile cannot read.
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

/// Fixed call timeouts. `handshake` and `run` take theirs from the config or the scenario
/// deadline; these do not vary.
pub const SHORT_TIMEOUT: Duration = Duration::from_secs(30);

/// One live plugin process, driven through the mechanism's transport. The handle owns the
/// process: dropping it ends the plugin, so nothing here kills or waits on a child by hand.
pub struct PluginClient {
    launch: Launch,
    plugin: Plugin,
}

impl PluginClient {
    /// Start the plugin, shake hands, and check the protocol version. The mechanism does the
    /// exchange — the handshake is the first request on the connection, by rule and not by
    /// arrangement here — and returns the answer for this engine to judge, the whole `result`
    /// object included. A spawn failure or a handshake that gave no answer is a
    /// [`ConnectError::Lost`], as is an answer whose shape this profile cannot read; an error frame
    /// on the handshake is a [`ConnectError::Refused`]; a revision this engine does not speak is
    /// [`ConnectError::UnsupportedVersion`]. The engine never speaks to a plugin it did not
    /// version-check.
    pub fn connect(
        launch: &Launch,
        handshake_timeout: Duration,
    ) -> Result<(PluginClient, Handshake), ConnectError> {
        let (plugin, declared) = match Plugin::start(&launch.config(handshake_timeout)) {
            Ok(started) => started,
            // The mechanism reports a handshake answered with an error frame as a refusal
            // (`StartError::Refused`), separately from a lost process: the plugin is alive and it
            // declined. veridoc agrees with that split and carries the frame's own code and
            // message through, so `PLUGIN_ERROR` and a `-32601` stay distinguishable to whoever
            // reads the report.
            Err(StartError::Refused(error)) => {
                return Err(ConnectError::Refused {
                    code: error.code,
                    message: error.message,
                })
            }
            Err(StartError::Lost(loss)) => return Err(ConnectError::Lost(loss)),
        };
        if !SUPPORTED_VERSIONS.contains(&declared.protocol_version.as_str()) {
            return Err(ConnectError::UnsupportedVersion {
                reported: declared.protocol_version,
            });
        }
        // The mechanism's `Handshake` reads the four core fields and hands the rest of the object
        // through untouched (`Handshake::result`: the envelope is open, and `handshake` is one
        // request). veridoc's handshake also declares `languages` and typed `capabilities`, so the
        // profile's own type is read from that object rather than asked for a second time.
        let handshake: Handshake = match serde_json::from_value(declared.result.clone()) {
            Ok(handshake) => handshake,
            // The plugin declared a `protocolVersion` this engine speaks and answered in a shape
            // that revision does not have. That is `Loss::Profile { UNUSABLE_RESULT }`, the same
            // verdict every other method gets for an answer the host cannot read, and it carries
            // what was wrong. `UnsupportedVersion` is the wrong home for it: that variant asserts
            // a revision this engine does not speak, and here the declared string is one it does
            // speak, so the report would read "plugin reported protocol "vd/1"; this engine speaks
            // "vd/1"" and send an operator looking for a version mismatch that is not the problem.
            Err(e) => {
                return Err(ConnectError::Lost(Loss::Profile {
                    reason: UNUSABLE_RESULT.to_string(),
                    detail: e.to_string(),
                }))
            }
        };
        Ok((
            PluginClient {
                launch: launch.clone(),
                plugin,
            },
            handshake,
        ))
    }

    pub fn launch(&self) -> &Launch {
        &self.launch
    }

    pub fn pid(&self) -> u32 {
        self.plugin.pid()
    }

    /// The recorded loss, if the process is gone. Requests made after this fail immediately.
    pub fn lost(&self) -> Option<Loss> {
        self.plugin.lost()
    }

    /// The most recent refusal the plugin could not attach to a request (spec §4). It answers no
    /// particular call, so it cannot travel with one; [`PluginClient::with_fault`] puts it where
    /// the engine reports a connection problem.
    pub fn connection_fault(&self) -> Option<ErrorObject> {
        self.plugin.connection_fault()
    }

    /// A plugin problem as the engine states it, with the connection fault beside it when the
    /// connection carries one (spec §4). A refusal with the uncorrelated `id` is the plugin
    /// reporting a fault on the connection itself, and a host must never discard it.
    pub fn with_fault(&self, problem: &str) -> String {
        match self.connection_fault() {
            Some(fault) => format!(
                "{problem}; the plugin also reported a fault on the connection ({}): {}",
                fault.code, fault.message
            ),
            None => problem.to_string(),
        }
    }

    /// The distinct stderr lines the plugin wrote, for the run failure on stderr. The mechanism
    /// drains stderr (spec §3); this only reads what it collected.
    pub fn stderr_lines(&self) -> Vec<String> {
        self.plugin
            .stderr_lines
            .lock()
            .expect("the stderr drain is never poisoned")
            .clone()
    }

    /// End the plugin and wait for it to be gone. The mechanism records `Loss::Killed` before the
    /// kill, so a call in flight is blamed on the engine and not on the exit status the kill
    /// produced.
    pub fn kill(&self) {
        self.plugin.kill();
    }

    /// Send one request and wait for the matching response.
    pub fn request<T>(&self, method: Method, params: Option<Value>, timeout: Duration) -> Call<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.plugin.request(method, params, timeout)
    }

    fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        req: &impl Serialize,
        timeout: Duration,
    ) -> Call<T> {
        self.plugin.call(method, req, timeout)
    }

    pub fn collect(&self, root: &str, timeout: Duration) -> Call<CollectResult> {
        self.call(
            Method::Collect,
            &CollectReq {
                root: root.to_string(),
            },
            timeout,
        )
    }

    pub fn resolve(&self, refs: &[String]) -> Call<ResolveResult> {
        self.call(
            Method::Resolve,
            &ResolveReq {
                refs: refs.to_vec(),
            },
            SHORT_TIMEOUT,
        )
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

    /// One scenario. `host_timeout` is the engine's own wait for it; the plugin's `deadlineMs` is
    /// already derived from that same wait by [`Timeouts::plugin_deadline`] where the request was
    /// built, so this only keeps the host's side of the one number (spec §9).
    pub fn run(&self, req: &RunRequest, host_timeout: Duration) -> Call<RunResponse> {
        self.call(Method::Run, req, host_timeout)
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

/// The distinct stderr lines the plugin wrote, capped at 4096 of them; they reach the run
/// failure on stderr as `plugin said: …`.
pub fn plugin_said(lines: &[String]) -> Vec<String> {
    let mut seen = Vec::new();
    for line in lines {
        if !seen.contains(line) {
            seen.push(line.clone());
            if seen.len() >= 4096 {
                break;
            }
        }
    }
    seen
}

/// A plugin client is mostly OS handles, so `Debug` reports what identifies it and nothing else.
impl std::fmt::Debug for PluginClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginClient")
            .field("launch", &self.launch.display())
            .field("pid", &self.plugin.pid())
            .field("lost", &self.lost())
            .finish_non_exhaustive()
    }
}
