//! JSON HTTP API for wallet commands and persisted TUI session settings.

mod auth;
mod client;
mod executor;
mod normalize;
mod report;
mod server;
mod state;

pub(crate) use auth::generate_api_token;
pub(crate) use client::{command_to_argv, run_command as run_command_http};
pub(crate) use executor::{restart_api_server_if_listen_changed, run_api_job_loop};
pub(crate) use normalize::normalize;
pub(crate) use report::{reports_to_text, TuiCommandResponse, TUI_OUTCOME_SCHEMA};
pub(crate) use server::{spawn_http_server, ApiServerHandle, TuiApiJob};
pub(crate) use state::{
    load_session_state, save_session_state, WalletSessionState, SESSION_FILE_NAME,
    WALLET_SESSION_SCHEMA,
};
