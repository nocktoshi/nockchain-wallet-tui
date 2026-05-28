//! Interactive wallet TUI (ratatui + crossterm) for [`nockchain-wallet`].

mod app_state;
mod clipboard;
mod command_runner;
mod components;
mod create_tx;
mod ct_dispatch;
mod event_loop;
mod format;
mod handlers;
mod hooks;
mod nns;
mod paste;
mod prompt_overlay;
mod screens;
mod send_simple;
mod session;
mod session_client;
mod store;
mod view;
mod wallet_api;

use std::path::PathBuf;
use std::sync::Arc;

use command_runner::TuiRuntime;
use nockapp::NockAppError;
use session::init_session_config;
use tokio::sync::{mpsc, Mutex};
pub(crate) use wallet_api::TuiApiJob;
use wallet_api::{generate_api_token, SESSION_FILE_NAME};
use wallet_tx_builder::adapter::NormalizedSnapshot;

use nockapp::kernel::boot::Cli as BootCli;
use nockchain_wallet::ConnectionCli;
use nockchain_wallet::Wallet;

/// Options the TUI binary needs. Decouples the TUI from the full WalletCli (which requires a Commands subcommand).
#[derive(Clone, Debug)]
pub struct TuiOptions {
    pub boot: BootCli,
    pub verbose: bool,
    pub fakenet: bool,
    pub connection: ConnectionCli,
}

pub(crate) fn normalize_slash_cmd(line: &str) -> &str {
    let t = line.trim();
    t.strip_prefix('/').unwrap_or(t).trim()
}

/// Main TUI entry using the decoupled options (preferred for the TUI binary).
pub async fn run_with_options(
    opts: TuiOptions,
    wallet: Wallet,
    synced_snapshot_for_planner: Option<NormalizedSnapshot>,
    wallet_data_dir: PathBuf,
) -> Result<(), NockAppError> {
    let session_path = wallet_data_dir.join(SESSION_FILE_NAME);
    let session_config = init_session_config(session_path.clone(), &opts.connection);
    let api_auth_token = Arc::from(generate_api_token());
    let wallet = Arc::new(Mutex::new(wallet));
    let snapshot = Arc::new(Mutex::new(synced_snapshot_for_planner));
    let (api_job_tx, api_job_rx) = mpsc::channel::<TuiApiJob>(32);
    let (price_done_tx, price_done_rx) = mpsc::unbounded_channel();
    let rt = TuiRuntime {
        wallet: Arc::clone(&wallet),
        snapshot: Arc::clone(&snapshot),
        connection: Arc::new(std::sync::Mutex::new(opts.connection.clone())),
        wallet_event_sink: Arc::new(std::sync::Mutex::new(Vec::new())),
        tui_markdown_sink: Arc::new(std::sync::Mutex::new(String::new())),
        session_config,
        session_path,
        api_auth_token,
        api_job_tx,
        api_server: Arc::new(std::sync::Mutex::new(None)),
    };
    session::apply_session_to_cli(&rt);
    event_loop::run(rt, api_job_rx, price_done_tx, price_done_rx).await
}

#[cfg(test)]
mod tests {
    use super::normalize_slash_cmd;

    #[test]
    fn slash_normalization() {
        assert_eq!(normalize_slash_cmd("/help"), "help");
        assert_eq!(normalize_slash_cmd("  /exit  "), "exit");
        assert_eq!(normalize_slash_cmd("verbose"), "verbose");
    }
}
