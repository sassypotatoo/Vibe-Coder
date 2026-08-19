//! OmniRoute OpenAI-compatible gateway adapter.
//!
//! Part 8 adds authenticated model-catalog semantics on top of the hardened Part 7 transport.
//! Health truth comes from the same bounded `GET /v1/models` response used for discovery; the
//! unauthenticated upstream `HEAD` route remains availability-only and is never promoted to health.

mod auth;
mod bootstrap;
mod catalog;
mod chat;
mod client;
mod config;
mod gateway;
mod profile;

pub use auth::RequestAuth;
pub use bootstrap::{
    FreeProviderBootstrapOutcome, OmniRouteProviderBootstrap, VIBECODER_FREE_PROVIDER_ID,
    VIBECODER_FREE_PROVIDER_NAME,
};
pub use client::OmniRouteClient;
pub use config::OmniRouteConfig;
