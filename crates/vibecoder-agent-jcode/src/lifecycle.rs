use crate::{JcodeConnectionConfig, JcodeConnectionFailure, JcodeConnectionMode};
use jcode_sdk::{ConnectOptions, JcodeClient, LaunchOptions};
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, RwLock};
use std::time::Duration;
use vibecoder_domain::{Result, VibeCoderError};

/// Server identity learned from the mandatory Jcode harness handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JcodeServerIdentity {
    pub server: String,
    pub protocol_major: u32,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JcodeConnectionState {
    Disconnected,
    Connecting,
    Connected { identity: JcodeServerIdentity },
    Faulted { failure: JcodeConnectionFailure },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JcodeConnectionSnapshot {
    pub generation: u64,
    pub state: JcodeConnectionState,
}

struct LifecycleState {
    generation: u64,
    state: JcodeConnectionState,
}

/// Owns one long-lived Jcode SDK client connection and its lifecycle state.
///
/// Lifecycle-changing operations are serialized by `lifecycle_gate`. The owner SDK client is
/// deliberately not exposed publicly. Adapter internals normally access it through `with_client`;
/// narrowly-scoped Part 6 model probes may open short-lived secondary API connections to the same
/// socket without transferring or dropping ownership of the private runtime.
pub struct JcodeConnectionManager {
    config: JcodeConnectionConfig,
    client: Mutex<Option<JcodeClient>>,
    lifecycle: RwLock<LifecycleState>,
    lifecycle_gate: Mutex<()>,
}

impl JcodeConnectionManager {
    pub fn new(config: JcodeConnectionConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            client: Mutex::new(None),
            lifecycle: RwLock::new(LifecycleState {
                generation: 0,
                state: JcodeConnectionState::Disconnected,
            }),
            lifecycle_gate: Mutex::new(()),
        })
    }

    pub fn config(&self) -> &JcodeConnectionConfig {
        &self.config
    }

    /// Connect once, or return the current healthy snapshot when already connected.
    ///
    /// A failed attempt records a structured `Faulted` state. A later call may retry; successful
    /// reconnect increments the generation so pending work can detect that it belongs to an old
    /// transport generation.
    pub fn connect(&self) -> Result<JcodeConnectionSnapshot> {
        let _gate = self.lock_lifecycle_gate()?;
        self.connect_locked()
    }

    /// Explicitly close the connection. Dropping the last SDK client handle closes its native
    /// socket; for an SDK-owned private instance, Jcode's `Drop` also tears down the daemon/bridge.
    pub fn disconnect(&self) -> Result<JcodeConnectionSnapshot> {
        let _gate = self.lock_lifecycle_gate()?;
        self.disconnect_locked()
    }

    pub fn reconnect(&self) -> Result<JcodeConnectionSnapshot> {
        let _gate = self.lock_lifecycle_gate()?;
        self.disconnect_locked()?;
        self.connect_locked()
    }

    /// Snapshot lifecycle state after detecting a socket that the Jcode reader already marked
    /// closed. This keeps orchestration code from seeing stale `Connected` status.
    pub fn status(&self) -> Result<JcodeConnectionSnapshot> {
        let _gate = self.lock_lifecycle_gate()?;
        self.refresh_closed_connection_locked()?;
        let state = self.read_lifecycle()?;
        Ok(snapshot(&state))
    }

    pub fn is_connected(&self) -> bool {
        self.status()
            .map(|snapshot| matches!(snapshot.state, JcodeConnectionState::Connected { .. }))
            .unwrap_or(false)
    }

    /// Execute an adapter-internal operation against the active SDK connection.
    ///
    /// The client mutex stays held for the operation, so an explicit disconnect waits instead of
    /// dropping the transport halfway through a session request.
    pub(crate) fn with_client<T>(
        &self,
        operation: impl FnOnce(&JcodeClient) -> Result<T>,
    ) -> Result<T> {
        {
            let _gate = self.lock_lifecycle_gate()?;
            self.refresh_closed_connection_locked()?;
        }
        let slot = self.lock_client()?;
        let client = slot.as_ref().ok_or_else(|| {
            VibeCoderError::Agent("Jcode operation requires an active connection".into())
        })?;
        operation(client)
    }

    /// Clone an adapter-internal SDK handle for an in-flight turn.
    ///
    /// The clone intentionally never crosses the crate boundary. Unlike `with_client`, this does
    /// not hold the manager's client mutex while a model turn may run for minutes, which keeps a
    /// second SDK handle available for cancellation. Runtime-level turn tracking prevents explicit
    /// disconnect/reconnect from racing this borrowed transport lifetime.
    pub(crate) fn clone_client_for_inflight(&self) -> Result<JcodeClient> {
        {
            let _gate = self.lock_lifecycle_gate()?;
            self.refresh_closed_connection_locked()?;
        }
        self.lock_client()?.as_ref().cloned().ok_or_else(|| {
            VibeCoderError::Agent("Jcode operation requires an active connection".into())
        })
    }

    /// Open a second API connection to the already-running Jcode runtime.
    ///
    /// This is deliberately not `Clone`: a clone shares one API connection and therefore one
    /// bridge cache. A new socket connection receives a fresh server-side BridgeState while the
    /// manager keeps owning the parent private instance (including an ephemeral JCODE_HOME).
    pub(crate) fn open_clean_model_client(&self) -> Result<JcodeClient> {
        let socket_path = {
            let _gate = self.lock_lifecycle_gate()?;
            self.refresh_closed_connection_locked()?;
            let slot = self.lock_client()?;
            let client = slot.as_ref().ok_or_else(|| {
                VibeCoderError::Agent("Jcode model probe requires an active connection".into())
            })?;
            client.socket_path().to_path_buf()
        };

        JcodeClient::connect(ConnectOptions {
            socket_path: Some(socket_path),
            client_name: format!("vibecoder-model-sidecar/{}", env!("CARGO_PKG_VERSION")),
            request_timeout: Some(self.config.request_timeout()),
            ensure_runtime: false,
        })
        .map_err(|error| JcodeConnectionFailure::from_sdk(error).into_domain_error())
    }

    fn connect_locked(&self) -> Result<JcodeConnectionSnapshot> {
        self.refresh_closed_connection_locked()?;
        {
            let state = self.read_lifecycle()?;
            match &state.state {
                JcodeConnectionState::Connected { .. } => return Ok(snapshot(&state)),
                JcodeConnectionState::Connecting => {
                    return Err(VibeCoderError::Agent(
                        "Jcode connection attempt is already in progress".into(),
                    ));
                }
                JcodeConnectionState::Disconnected | JcodeConnectionState::Faulted { .. } => {}
            }
        }

        self.set_state(JcodeConnectionState::Connecting, false)?;
        match open_client(&self.config) {
            Ok(client) => {
                let identity = identity_from(&client);
                {
                    let mut slot = self.lock_client()?;
                    let old = slot.replace(client);
                    drop(old);
                }
                self.set_state(JcodeConnectionState::Connected { identity }, true)
            }
            Err(error) => {
                let failure = JcodeConnectionFailure::from_sdk(error);
                self.set_state(
                    JcodeConnectionState::Faulted {
                        failure: failure.clone(),
                    },
                    false,
                )?;
                Err(failure.into_domain_error())
            }
        }
    }

    fn disconnect_locked(&self) -> Result<JcodeConnectionSnapshot> {
        let client = self.lock_client()?.take();
        drop(client);
        self.set_state(JcodeConnectionState::Disconnected, false)
    }

    fn refresh_closed_connection_locked(&self) -> Result<()> {
        let closed = self
            .lock_client()?
            .as_ref()
            .map(JcodeClient::is_closed)
            .unwrap_or(false);
        if !closed {
            return Ok(());
        }

        let stale = self.lock_client()?.take();
        drop(stale);
        let failure = JcodeConnectionFailure {
            code: "disconnected".into(),
            message: "Jcode harness transport closed".into(),
            class: crate::JcodeFailureClass::RetryableTransport,
            retryable: true,
        };
        self.set_state(JcodeConnectionState::Faulted { failure }, false)?;
        Ok(())
    }

    fn set_state(
        &self,
        state: JcodeConnectionState,
        increment_generation: bool,
    ) -> Result<JcodeConnectionSnapshot> {
        let mut lifecycle = self.write_lifecycle()?;
        if increment_generation {
            lifecycle.generation = lifecycle.generation.saturating_add(1);
        }
        lifecycle.state = state;
        Ok(snapshot(&lifecycle))
    }

    fn lock_client(&self) -> Result<std::sync::MutexGuard<'_, Option<JcodeClient>>> {
        self.client
            .lock()
            .map_err(|_| VibeCoderError::Agent("Jcode client lock poisoned".into()))
    }

    fn lock_lifecycle_gate(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.lifecycle_gate
            .lock()
            .map_err(|_| VibeCoderError::Agent("Jcode lifecycle gate poisoned".into()))
    }

    fn read_lifecycle(&self) -> Result<std::sync::RwLockReadGuard<'_, LifecycleState>> {
        self.lifecycle
            .read()
            .map_err(|_| VibeCoderError::Agent("Jcode lifecycle lock poisoned".into()))
    }

    fn write_lifecycle(&self) -> Result<std::sync::RwLockWriteGuard<'_, LifecycleState>> {
        self.lifecycle
            .write()
            .map_err(|_| VibeCoderError::Agent("Jcode lifecycle lock poisoned".into()))
    }
}

impl Drop for JcodeConnectionManager {
    fn drop(&mut self) {
        if let Ok(slot) = self.client.get_mut() {
            let client = slot.take();
            drop(client);
        }
    }
}

fn open_client(config: &JcodeConnectionConfig) -> jcode_sdk::Result<JcodeClient> {
    match &config.connection {
        JcodeConnectionMode::Shared {
            socket_path,
            ensure_runtime,
        } => JcodeClient::connect(ConnectOptions {
            socket_path: socket_path.clone(),
            client_name: config.client_name.clone(),
            request_timeout: Some(config.request_timeout()),
            ensure_runtime: *ensure_runtime,
        }),
        JcodeConnectionMode::Private {
            jcode_home,
            binary,
            inherit_logins,
            startup_timeout_ms,
            cleanup_timeout_ms,
        } => JcodeClient::launch(LaunchOptions {
            jcode_home: jcode_home.clone(),
            working_dir: None,
            inherit_logins: *inherit_logins,
            binary: binary.clone(),
            startup_timeout: Duration::from_millis(*startup_timeout_ms),
            inherit_stderr: false,
            cleanup_timeout: Duration::from_millis(*cleanup_timeout_ms),
            client_name: config.client_name.clone(),
            request_timeout: Some(config.request_timeout()),
            api_socket: None,
            start_timeout: Duration::from_millis(*startup_timeout_ms),
            ..LaunchOptions::default()
        }),
    }
}

fn identity_from(client: &JcodeClient) -> JcodeServerIdentity {
    let mut capabilities = client.capabilities.clone();
    capabilities.sort();
    capabilities.dedup();
    JcodeServerIdentity {
        server: client.server.clone(),
        protocol_major: jcode_sdk::api::API_VERSION_MAJOR,
        capabilities,
    }
}

fn snapshot(state: &LifecycleState) -> JcodeConnectionSnapshot {
    JcodeConnectionSnapshot {
        generation: state.generation,
        state: state.state.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manager_starts_disconnected_without_touching_runtime() {
        let manager = JcodeConnectionManager::new(JcodeConnectionConfig::default()).unwrap();
        let snapshot = manager.status().unwrap();
        assert_eq!(snapshot.generation, 0);
        assert_eq!(snapshot.state, JcodeConnectionState::Disconnected);
    }
}
