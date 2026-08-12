use crate::permission::{
    PermissionObservation, PermissionRegistry, request_id_is_safe_for_response,
};
use jcode_sdk::{ApiEvent, JcodeClient, RunOptions};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use vibecoder_agent_contract::EventHandler;
use vibecoder_domain::{
    AgentEvent, PermissionDecision, Result, SessionId, TokenUsage, ToolCallResult, TurnResult,
    VibeCoderError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveTurn {
    session_id: SessionId,
    connection_generation: u64,
    cancel_requested: bool,
    worker_finished: bool,
}

/// One Jcode bridge connection is attached to one session at a time. Keep exactly one VibeCoder
/// turn active on that connection so a second prompt cannot silently steal the attachment/event
/// stream from the first.
pub(crate) struct TurnRegistry {
    active: Mutex<Option<ActiveTurn>>,
    control_gate: Mutex<()>,
}

impl TurnRegistry {
    pub(crate) fn new() -> Self {
        Self {
            active: Mutex::new(None),
            control_gate: Mutex::new(()),
        }
    }

    pub(crate) fn begin(&self, session_id: &SessionId, generation: u64) -> Result<()> {
        let mut active = self.lock()?;
        if let Some(existing) = active.as_ref() {
            return Err(VibeCoderError::InvalidRequest(format!(
                "a Jcode turn is already running for session {}",
                existing.session_id.0
            )));
        }
        *active = Some(ActiveTurn {
            session_id: session_id.clone(),
            connection_generation: generation,
            cancel_requested: false,
            worker_finished: false,
        });
        Ok(())
    }

    pub(crate) fn has_active(&self) -> Result<bool> {
        Ok(self.lock()?.is_some())
    }

    pub(crate) fn active_generation(&self, session_id: &SessionId) -> Result<Option<u64>> {
        Ok(self.lock()?.as_ref().and_then(|turn| {
            (turn.session_id == *session_id && !turn.worker_finished)
                .then_some(turn.connection_generation)
        }))
    }

    pub(crate) fn mark_worker_finished(
        &self,
        session_id: &SessionId,
        generation: u64,
    ) -> Result<()> {
        let mut active = self.lock()?;
        let Some(turn) = active.as_mut() else {
            // The async caller may have been dropped and its lease already cleared state.
            return Ok(());
        };
        if turn.session_id != *session_id || turn.connection_generation != generation {
            return Err(VibeCoderError::Agent(
                "Jcode worker completion did not match the active turn".into(),
            ));
        }
        turn.worker_finished = true;
        Ok(())
    }

    pub(crate) fn mark_cancel_acknowledged(&self, session_id: &SessionId) -> Result<bool> {
        let mut active = self.lock()?;
        let Some(turn) = active.as_mut() else {
            // The turn may have completed between Jcode acknowledging cancel and this local mark.
            // Completion wins that race; cancel itself is still considered successfully delivered.
            return Ok(false);
        };
        if turn.session_id != *session_id {
            return Err(VibeCoderError::InvalidRequest(
                "the requested session does not own the active Jcode turn".into(),
            ));
        }
        turn.cancel_requested = true;
        Ok(true)
    }

    pub(crate) fn finish(&self, session_id: &SessionId, generation: u64) -> Result<bool> {
        let _control = self.lock_control()?;
        let mut active = self.lock()?;
        let turn = active.as_ref().ok_or_else(|| {
            VibeCoderError::Agent("Jcode active-turn state disappeared before completion".into())
        })?;
        if turn.session_id != *session_id || turn.connection_generation != generation {
            return Err(VibeCoderError::Agent(
                "Jcode active-turn identity changed before completion".into(),
            ));
        }
        let cancelled = turn.cancel_requested;
        *active = None;
        Ok(cancelled)
    }

    pub(crate) fn should_cancel_abandoned(&self, session_id: &SessionId, generation: u64) -> bool {
        self.active
            .lock()
            .ok()
            .and_then(|active| {
                active.as_ref().map(|turn| {
                    turn.session_id == *session_id
                        && turn.connection_generation == generation
                        && !turn.worker_finished
                })
            })
            .unwrap_or(false)
    }

    pub(crate) fn force_clear(&self, session_id: &SessionId, generation: u64) {
        if let Ok(mut active) = self.active.lock()
            && active.as_ref().is_some_and(|turn| {
                turn.session_id == *session_id && turn.connection_generation == generation
            })
        {
            *active = None;
        }
    }

    pub(crate) fn lock_control(&self) -> Result<MutexGuard<'_, ()>> {
        self.control_gate
            .lock()
            .map_err(|_| VibeCoderError::Agent("Jcode turn-control gate poisoned".into()))
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<ActiveTurn>>> {
        self.active
            .lock()
            .map_err(|_| VibeCoderError::Agent("Jcode active-turn lock poisoned".into()))
    }
}

/// Cancels the upstream turn if the async caller drops `run_turn` before its blocking worker
/// finishes. This prevents an abandoned model turn from continuing while the registry claims the
/// connection is idle.
pub(crate) struct ActiveTurnLease {
    registry: Arc<TurnRegistry>,
    permissions: Arc<PermissionRegistry>,
    session_id: SessionId,
    generation: u64,
    cancel_client: JcodeClient,
    completed: bool,
}

impl ActiveTurnLease {
    pub(crate) fn new(
        registry: Arc<TurnRegistry>,
        permissions: Arc<PermissionRegistry>,
        session_id: SessionId,
        generation: u64,
        cancel_client: JcodeClient,
    ) -> Self {
        Self {
            registry,
            permissions,
            session_id,
            generation,
            cancel_client,
            completed: false,
        }
    }

    pub(crate) fn complete(mut self) -> Result<bool> {
        let cancelled = self.registry.finish(&self.session_id, self.generation)?;
        self.permissions
            .finish_turn(&self.session_id, self.generation);
        self.completed = true;
        Ok(cancelled)
    }
}

impl Drop for ActiveTurnLease {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        // Best-effort synchronous cancellation is deliberate in Drop: an async run future can be
        // abandoned at any await point, but the upstream model/tool turn must not be orphaned. The
        // same control gate used by explicit cancel/normal finish prevents contradictory outcomes.
        if let Ok(_control) = self.registry.lock_control() {
            if self
                .registry
                .should_cancel_abandoned(&self.session_id, self.generation)
            {
                let _ = self.cancel_client.cancel(&self.session_id.0);
            }
            self.registry.force_clear(&self.session_id, self.generation);
        }
        self.permissions
            .finish_turn(&self.session_id, self.generation);
    }
}

#[derive(Clone)]
pub(crate) struct TurnSafetyState {
    permission_protocol_failure: Arc<AtomicBool>,
}

impl TurnSafetyState {
    pub(crate) fn permission_protocol_failure(&self) -> bool {
        self.permission_protocol_failure.load(Ordering::Acquire)
    }
}

fn dispatch_event(handler: &Mutex<Option<EventHandler>>, event: AgentEvent) -> bool {
    let Ok(mut slot) = handler.lock() else {
        return false;
    };
    let Some(callback) = slot.as_mut() else {
        return false;
    };
    // A presentation/event consumer must never be able to unwind through Jcode's network
    // reader/turn collector. Disable the callback after a panic. Permission events additionally
    // treat a false return as delivery failure so the upstream turn cannot wait forever.
    if catch_unwind(AssertUnwindSafe(|| callback(event))).is_err() {
        *slot = None;
        return false;
    }
    true
}

fn fail_permission_protocol(
    safety: &TurnSafetyState,
    client: &JcodeClient,
    session_id: &SessionId,
    request_id: &str,
) {
    safety
        .permission_protocol_failure
        .store(true, Ordering::Release);
    // Deny first when the request id is safe to reflect, then cancel. If the server supplied a
    // malformed/oversized id, do not echo attacker-controlled protocol material back upstream;
    // cancellation alone terminates the untrusted turn.
    if request_id_is_safe_for_response(request_id) {
        let _ = client.respond_to_permission(
            &session_id.0,
            request_id,
            jcode_sdk::PermissionDecision::Deny,
        );
    }
    let _ = client.cancel(&session_id.0);
}

pub(crate) fn run_options(
    expected_session: SessionId,
    on_event: Option<EventHandler>,
    safety_client: JcodeClient,
    permissions_supported: bool,
    connection_generation: u64,
    permissions: Arc<PermissionRegistry>,
) -> (RunOptions, TurnSafetyState) {
    let handler = Mutex::new(on_event);
    let safety = TurnSafetyState {
        permission_protocol_failure: Arc::new(AtomicBool::new(false)),
    };
    let callback_safety = safety.clone();
    let callback_session = expected_session.clone();
    let callback_permissions = Arc::clone(&permissions);
    let options = RunOptions {
        images: Vec::new(),
        // VibeCoder never uses the SDK's blanket auto-approve path. Permission responses are
        // mediated by PermissionRegistry and bound to one session + connection generation.
        auto_approve: false,
        on_event: Some(Box::new(move |event| {
            if let ApiEvent::PermissionRequest {
                session_id,
                request_id,
                tool_name,
                description,
            } = event
            {
                if session_id != &callback_session.0 {
                    return;
                }
                if !permissions_supported {
                    // The pinned Jcode 0.73.0 bridge is in this branch if it ever violates its
                    // hello capabilities and emits a permission request unexpectedly.
                    fail_permission_protocol(
                        &callback_safety,
                        &safety_client,
                        &callback_session,
                        request_id,
                    );
                    return;
                }

                match callback_permissions.observe_request(
                    &callback_session,
                    connection_generation,
                    request_id,
                    tool_name,
                    description,
                ) {
                    Ok(PermissionObservation::Prompt(request)) => {
                        if !dispatch_event(&handler, AgentEvent::PermissionRequired(request)) {
                            // A permission-capable bridge must never be allowed to park the turn
                            // forever when there is no live event consumer to answer the prompt.
                            fail_permission_protocol(
                                &callback_safety,
                                &safety_client,
                                &callback_session,
                                request_id,
                            );
                        }
                    }
                    Ok(PermissionObservation::AutoApprove(pending)) => {
                        // `AllowSession` is VibeCoder-local. Each matching future request still
                        // receives one upstream single-use Allow; upstream AllowAlways is never
                        // used because its persistence/scope is not specified by the reviewed API.
                        match safety_client.respond_to_permission(
                            &callback_session.0,
                            request_id,
                            jcode_sdk::PermissionDecision::Allow,
                        ) {
                            Ok(()) => {
                                if callback_permissions
                                    .complete_response(&pending, PermissionDecision::AllowOnce)
                                    .is_err()
                                {
                                    fail_permission_protocol(
                                        &callback_safety,
                                        &safety_client,
                                        &callback_session,
                                        request_id,
                                    );
                                }
                            }
                            Err(_) => {
                                callback_permissions.abort_response(&pending);
                                fail_permission_protocol(
                                    &callback_safety,
                                    &safety_client,
                                    &callback_session,
                                    request_id,
                                );
                            }
                        }
                    }
                    Err(_) => {
                        fail_permission_protocol(
                            &callback_safety,
                            &safety_client,
                            &callback_session,
                            request_id,
                        );
                    }
                }
                return;
            }

            let Some(mapped) = map_stream_event(&expected_session, event) else {
                return;
            };
            let _ = dispatch_event(&handler, mapped);
        })),
    };
    (options, safety)
}

/// Map only application-safe, provider-neutral events. `ReasoningDelta`/`ReasoningDone` are
/// deliberately discarded: VibeCoder never depends on or exposes provider-private chain-of-thought.
pub(crate) fn map_stream_event(
    expected_session: &SessionId,
    event: &ApiEvent,
) -> Option<AgentEvent> {
    match event {
        ApiEvent::TextDelta { session_id, text } if session_id == &expected_session.0 => {
            Some(AgentEvent::TextDelta { text: text.clone() })
        }
        ApiEvent::MessageAccepted { session_id } if session_id == &expected_session.0 => {
            Some(AgentEvent::MessageAccepted)
        }
        ApiEvent::ToolStart {
            session_id,
            call_id,
            name,
        } if session_id == &expected_session.0 => Some(AgentEvent::ToolStarted {
            tool: name.clone(),
            call_id: call_id.clone(),
        }),
        ApiEvent::ToolDone {
            session_id,
            call_id,
            name,
            output,
            error,
        } if session_id == &expected_session.0 => Some(AgentEvent::ToolFinished {
            tool: name.clone(),
            call_id: call_id.clone(),
            ok: error.is_none(),
            output: output.clone(),
            error: error.clone(),
        }),
        ApiEvent::BackgroundProgress {
            session_id,
            task_id,
            label,
            percent,
            summary,
            done,
        } if session_id == &expected_session.0 => Some(AgentEvent::BackgroundProgress {
            task_id: task_id.clone(),
            label: label.clone(),
            percent: *percent,
            summary: summary.clone(),
            done: *done,
        }),
        ApiEvent::SessionStatus { session_id, status } if session_id == &expected_session.0 => {
            Some(AgentEvent::SessionStatus {
                status: status.clone(),
            })
        }
        ApiEvent::TokenUsage {
            session_id,
            input,
            output,
            cache_read_input,
        } if session_id == &expected_session.0 => Some(AgentEvent::TokenUsage(TokenUsage {
            input: *input,
            output: *output,
            cache_read_input: *cache_read_input,
        })),
        ApiEvent::TurnDone { session_id } if session_id == &expected_session.0 => {
            Some(AgentEvent::TurnCompleted)
        }
        // Explicitly skip reasoning, tool-input fragments, model metadata, unknown future events,
        // and events for another session. The reviewed SDK already session-filters its stream; this
        // second check keeps the adapter boundary fail-closed if that behavior ever regresses.
        _ => None,
    }
}

pub(crate) fn map_turn_result(turn: jcode_sdk::TurnResult, cancelled: bool) -> TurnResult {
    TurnResult {
        text: turn.text,
        cancelled,
        tool_calls: turn
            .tool_calls
            .into_iter()
            .map(|call| ToolCallResult {
                call_id: call.call_id,
                tool: call.name,
                output: call.output,
                error: call.error,
            })
            .collect(),
        usage: turn.usage.map(|usage| TokenUsage {
            input: usage.input,
            output: usage.output,
            cache_read_input: usage.cache_read_input,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_is_not_exposed_as_application_event() {
        let session = SessionId("s1".into());
        let event = ApiEvent::ReasoningDelta {
            session_id: "s1".into(),
            text: "private reasoning".into(),
        };
        assert_eq!(map_stream_event(&session, &event), None);
    }

    #[test]
    fn another_sessions_event_is_rejected() {
        let session = SessionId("mine".into());
        let event = ApiEvent::TextDelta {
            session_id: "other".into(),
            text: "wrong stream".into(),
        };
        assert_eq!(map_stream_event(&session, &event), None);
    }
}
