//! TUI JSON API: HTTP listener lifecycle and command execution.

use std::sync::Arc;

use clap::Parser;
use tokio::sync::mpsc;

use super::{normalize, spawn_http_server, ApiServerHandle, TuiApiJob, TuiCommandResponse};
use crate::command_runner::{self, TuiRuntime};
use crate::session::current_api_listen;

/// Run a wallet command from the TUI JSON API (`argv` tokens, no binary name).
async fn execute_tui_api_command(rt: &TuiRuntime, argv: Vec<String>) -> TuiCommandResponse {
    rt.wallet_event_sink.lock().unwrap().clear();

    // clap expects argv[0] to be the program name; provide a dummy.
    let mut full_argv = vec!["nockchain-wallet-tui".to_string()];
    full_argv.extend(argv);

    #[derive(Parser)]
    struct ApiCli {
        #[command(subcommand)]
        command: nockchain_wallet::command::Commands,
    }

    let command = match ApiCli::try_parse_from(full_argv) {
        Ok(cli) => cli.command,
        Err(e) => return TuiCommandResponse::failure(e.to_string()),
    };

    tracing::debug!(?command, "api: executing command");
    let outcome = command_runner::run_command_on_runtime(rt, command.clone(), None, None).await;
    let resp = match outcome {
        Ok(data) => {
            let markdown = rt.tui_markdown_sink.lock().unwrap().clone();
            let reports = normalize(&data.events, &markdown, &command);
            TuiCommandResponse::ok(data.events, reports)
        }
        Err(e) => TuiCommandResponse::failure(e.to_string()),
    };
    tracing::debug!(success = resp.is_success(), error = ?resp.error, "api: command result");
    resp
}

/// Start (or restart) the JSON API listener using session `api_listen`.
pub(crate) fn restart_api_server(rt: &TuiRuntime, handle_slot: &mut Option<ApiServerHandle>) {
    if let Some(prev) = handle_slot.take() {
        prev.stop();
    }
    let listen = current_api_listen(rt);
    match spawn_http_server(
        listen,
        rt.api_job_tx.clone(),
        rt.session_path.clone(),
        Arc::clone(&rt.session_config),
        Arc::clone(&rt.api_auth_token),
    ) {
        Ok(h) => {
            *handle_slot = Some(h);
        }
        Err(e) => {
            tracing::warn!("wallet API not listening: {e}");
        }
    }
}

/// Process HTTP API jobs on the TUI [`LocalSet`] (same wallet + capture sinks as the TUI).
pub(crate) async fn run_api_job_loop(rt: TuiRuntime, mut job_rx: mpsc::Receiver<TuiApiJob>) {
    let mut server = rt.api_server.lock().unwrap().take();
    restart_api_server(&rt, &mut server);
    *rt.api_server.lock().unwrap() = server;

    while let Some(job) = job_rx.recv().await {
        tracing::debug!(argv = ?job.argv, "api: job received");
        let resp = execute_tui_api_command(&rt, job.argv).await;
        let _ = job.resp.send(resp);
    }

    if let Some(h) = rt.api_server.lock().unwrap().take() {
        h.stop();
    }
}

/// After POST changes `api_listen`, rebind the HTTP listener.
pub(crate) fn restart_api_server_if_listen_changed(rt: &TuiRuntime, previous_listen: &str) {
    let now = current_api_listen(rt);
    if now != previous_listen {
        let mut server = rt.api_server.lock().unwrap().take();
        restart_api_server(rt, &mut server);
        *rt.api_server.lock().unwrap() = server;
    }
}
