use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard};
use vibecoder_domain::{PermissionDecision, PermissionRequest, Result, SessionId, VibeCoderError};

const MAX_REQUEST_ID_BYTES: usize = 256;
const MAX_ACTION_BYTES: usize = 256;
const MAX_DESCRIPTION_BYTES: usize = 16 * 1024;
const MAX_PERMISSION_REQUESTS_PER_TURN: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PermissionIdentity {
    session_id: String,
    request_id: String,
    connection_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SessionGrantKey {
    session_id: String,
    action: String,
    description: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingPermission {
    pub(crate) request: PermissionRequest,
    pub(crate) connection_generation: u64,
    scope_key: SessionGrantKey,
    responding: bool,
}

impl PendingPermission {
    fn identity(&self) -> PermissionIdentity {
        PermissionIdentity {
            session_id: self.request.session_id.0.clone(),
            request_id: self.request.request_id.clone(),
            connection_generation: self.connection_generation,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum PermissionObservation {
    Prompt(PermissionRequest),
    AutoApprove(PendingPermission),
}

#[derive(Debug, Default)]
struct PermissionState {
    pending_by_request_id: HashMap<String, PendingPermission>,
    resolved_this_turn: HashSet<PermissionIdentity>,
    session_grants: HashSet<SessionGrantKey>,
}

/// In-memory broker for permission prompts emitted by the active Jcode turn.
///
/// Request ids are bound to the exact session and transport generation that emitted them. A
/// response therefore cannot be replayed after reconnect, against another session, or after the
/// turn has completed. `AllowSession` is implemented locally as an exact action+description grant;
/// VibeCoder deliberately sends Jcode's single-use `Allow` decision for each request instead of
/// mapping to the broader/underspecified upstream `AllowAlways` variant.
pub(crate) struct PermissionRegistry {
    state: Mutex<PermissionState>,
}

impl PermissionRegistry {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(PermissionState::default()),
        }
    }

    pub(crate) fn observe_request(
        &self,
        session_id: &SessionId,
        connection_generation: u64,
        request_id: &str,
        action: &str,
        description: &str,
    ) -> Result<PermissionObservation> {
        validate_request_id(request_id)?;
        validate_action(action)?;
        validate_description(description)?;

        let request = PermissionRequest {
            request_id: request_id.to_string(),
            session_id: session_id.clone(),
            action: action.to_string(),
            reason: (!description.trim().is_empty()).then(|| description.to_string()),
        };
        let scope_key = SessionGrantKey {
            session_id: session_id.0.clone(),
            action: action.to_string(),
            description: description.to_string(),
        };
        let identity = PermissionIdentity {
            session_id: session_id.0.clone(),
            request_id: request_id.to_string(),
            connection_generation,
        };

        let mut state = self.lock()?;
        if state.pending_by_request_id.contains_key(request_id)
            || state.resolved_this_turn.contains(&identity)
        {
            return Err(VibeCoderError::Agent(
                "Jcode repeated a permission request id within the active turn".into(),
            ));
        }
        if state.pending_by_request_id.len() + state.resolved_this_turn.len()
            >= MAX_PERMISSION_REQUESTS_PER_TURN
        {
            return Err(VibeCoderError::Agent(
                "Jcode exceeded the permission-request budget for one turn".into(),
            ));
        }

        if state.session_grants.contains(&scope_key) {
            let pending = PendingPermission {
                request,
                connection_generation,
                scope_key,
                responding: true,
            };
            state
                .pending_by_request_id
                .insert(request_id.to_string(), pending.clone());
            return Ok(PermissionObservation::AutoApprove(pending));
        }

        state.pending_by_request_id.insert(
            request_id.to_string(),
            PendingPermission {
                request: request.clone(),
                connection_generation,
                scope_key,
                responding: false,
            },
        );
        Ok(PermissionObservation::Prompt(request))
    }

    pub(crate) fn begin_response(
        &self,
        session_id: &SessionId,
        request_id: &str,
        connection_generation: u64,
    ) -> Result<PendingPermission> {
        validate_request_id(request_id)?;
        let mut state = self.lock()?;
        let pending = state
            .pending_by_request_id
            .get_mut(request_id)
            .ok_or_else(|| {
                VibeCoderError::InvalidRequest(
                    "permission request is unknown, already resolved, or no longer active".into(),
                )
            })?;
        if pending.request.session_id != *session_id
            || pending.connection_generation != connection_generation
        {
            return Err(VibeCoderError::InvalidRequest(
                "permission request does not belong to this active session/connection".into(),
            ));
        }
        if pending.responding {
            return Err(VibeCoderError::InvalidRequest(
                "permission request already has a response in progress".into(),
            ));
        }
        pending.responding = true;
        Ok(pending.clone())
    }

    pub(crate) fn complete_response(
        &self,
        pending: &PendingPermission,
        decision: PermissionDecision,
    ) -> Result<()> {
        let mut state = self.lock()?;
        let current = state
            .pending_by_request_id
            .get(&pending.request.request_id)
            .ok_or_else(|| {
                VibeCoderError::Agent(
                    "permission response acknowledgement lost its pending request".into(),
                )
            })?;
        if current.identity() != pending.identity() || !current.responding {
            return Err(VibeCoderError::Agent(
                "permission response acknowledgement no longer matches the active request".into(),
            ));
        }

        let removed = state
            .pending_by_request_id
            .remove(&pending.request.request_id)
            .ok_or_else(|| {
                VibeCoderError::Agent(
                    "permission response acknowledgement lost its pending request".into(),
                )
            })?;
        state.resolved_this_turn.insert(removed.identity());
        if decision == PermissionDecision::AllowSession {
            state.session_grants.insert(removed.scope_key);
        }
        Ok(())
    }

    pub(crate) fn abort_response(&self, pending: &PendingPermission) {
        if let Ok(mut state) = self.state.lock()
            && let Some(current) = state
                .pending_by_request_id
                .get_mut(&pending.request.request_id)
            && current.identity() == pending.identity()
        {
            current.responding = false;
        }
    }

    pub(crate) fn finish_turn(&self, session_id: &SessionId, generation: u64) {
        if let Ok(mut state) = self.state.lock() {
            state.pending_by_request_id.retain(|_, pending| {
                pending.request.session_id != *session_id
                    || pending.connection_generation != generation
            });
            state.resolved_this_turn.retain(|identity| {
                identity.session_id != session_id.0 || identity.connection_generation != generation
            });
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, PermissionState>> {
        self.state
            .lock()
            .map_err(|_| VibeCoderError::Agent("Jcode permission broker lock poisoned".into()))
    }
}

pub(crate) fn request_id_is_safe_for_response(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_REQUEST_ID_BYTES
        && !value.chars().any(char::is_control)
}

fn validate_request_id(value: &str) -> Result<()> {
    if !request_id_is_safe_for_response(value) {
        return Err(VibeCoderError::InvalidRequest(
            "permission request id has an invalid format".into(),
        ));
    }
    Ok(())
}

fn validate_action(value: &str) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > MAX_ACTION_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(VibeCoderError::Agent(
            "Jcode permission request reported an invalid action".into(),
        ));
    }
    Ok(())
}

fn validate_description(value: &str) -> Result<()> {
    if value.len() > MAX_DESCRIPTION_BYTES
        || value
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(VibeCoderError::Agent(
            "Jcode permission request description is invalid or too large".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_request_id_is_rejected() {
        let broker = PermissionRegistry::new();
        let session = SessionId("session_1".into());
        broker
            .observe_request(&session, 7, "req-1", "bash", "run tests")
            .unwrap();
        assert!(
            broker
                .observe_request(&session, 7, "req-1", "bash", "run tests")
                .is_err()
        );
    }

    #[test]
    fn stale_generation_response_is_rejected() {
        let broker = PermissionRegistry::new();
        let session = SessionId("session_1".into());
        broker
            .observe_request(&session, 7, "req-1", "bash", "npm test")
            .unwrap();
        assert!(broker.begin_response(&session, "req-1", 8).is_err());
    }

    #[test]
    fn finish_turn_invalidates_pending_requests() {
        let broker = PermissionRegistry::new();
        let session = SessionId("session_1".into());
        broker
            .observe_request(&session, 7, "req-1", "bash", "npm test")
            .unwrap();
        broker.finish_turn(&session, 7);
        assert!(broker.begin_response(&session, "req-1", 7).is_err());
    }

    #[test]
    fn unsafe_request_ids_are_not_reflected() {
        assert!(!request_id_is_safe_for_response(""));
        assert!(!request_id_is_safe_for_response("bad\nrequest"));
        assert!(!request_id_is_safe_for_response(
            &"x".repeat(MAX_REQUEST_ID_BYTES + 1)
        ));
        assert!(request_id_is_safe_for_response("req-123_safe"));
    }

    #[test]
    fn allow_session_is_scoped_to_exact_action_and_description() {
        let broker = PermissionRegistry::new();
        let session = SessionId("session_1".into());
        broker
            .observe_request(&session, 7, "req-1", "bash", "npm test")
            .unwrap();
        let pending = broker.begin_response(&session, "req-1", 7).unwrap();
        broker
            .complete_response(&pending, PermissionDecision::AllowSession)
            .unwrap();

        assert!(matches!(
            broker
                .observe_request(&session, 7, "req-2", "bash", "npm test")
                .unwrap(),
            PermissionObservation::AutoApprove(_)
        ));
        assert!(matches!(
            broker
                .observe_request(&session, 7, "req-3", "bash", "npm publish")
                .unwrap(),
            PermissionObservation::Prompt(_)
        ));
    }
}
