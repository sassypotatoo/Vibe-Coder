//! Jcode adapter boundary for VibeCoder.
//!
//! Parts 2-6 own transport lifecycle, safe session mapping, real turn/event streaming,
//! capability-gated permission mediation, and verified session-scoped model selection.

mod config;
mod error;
mod lifecycle;
mod model;
mod permission;
mod runtime;
mod session;
mod turn;

pub use config::{
    JcodeConnectionConfig, JcodeConnectionMode, JcodeModelGatewayBridge,
    VIBECODER_BRIDGED_FILE_TOOLS, VIBECODER_BRIDGED_MAX_TOOL_CALLS_PER_TURN,
};
pub use error::{JcodeConnectionFailure, JcodeFailureClass};
pub use lifecycle::{
    JcodeConnectionManager, JcodeConnectionSnapshot, JcodeConnectionState, JcodeServerIdentity,
};
pub use runtime::JcodeAgentRuntime;
