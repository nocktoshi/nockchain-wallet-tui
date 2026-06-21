//! TUI JSON API: HTTP listener lifecycle and command execution.

use std::sync::Arc;

use clap::Parser;
use tokio::sync::mpsc;

use super::{
    normalize, parse_markdown_to_sections, spawn_http_server, ApiServerHandle, Report, TuiApiJob,
    TuiApiRequest, TuiCommandResponse,
};
use crate::command_runner::{self, TuiRuntime};
use crate::session::current_api_listen;
use nockchain_wallet::command::Commands;

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
            let mut events = data.events;
            super::augment_events_from_markdown(&mut events, &markdown, &command);
            let reports = normalize(&events, &markdown, &command);
            TuiCommandResponse::ok(events, reports)
        }
        Err(e) => TuiCommandResponse::failure(e.to_string()),
    };
    tracing::debug!(success = resp.is_success(), error = ?resp.error, "api: command result");
    resp
}

/// Route a typed API request to the right wallet operation (all on the TUI `LocalSet`).
async fn execute_request(rt: &TuiRuntime, request: TuiApiRequest) -> TuiCommandResponse {
    match request {
        TuiApiRequest::Command(argv) => execute_tui_api_command(rt, argv).await,
        TuiApiRequest::PlanSimpleSend {
            recipient,
            amount_nicks,
        } => execute_plan(rt, &recipient, amount_nicks).await,
        TuiApiRequest::CreateAndSendSimple {
            recipient,
            amount_nicks,
        } => match crate::send_simple::build_simple_send_tx(&recipient, amount_nicks) {
            Ok(cmd) => execute_create_and_send_request(rt, cmd).await,
            Err(e) => TuiCommandResponse::failure(e),
        },
        TuiApiRequest::NnsRegister { name } => execute_nns_register(rt, &name).await,
    }
}

/// Planner preview for a simple send: build the create-tx, run the planner (no kernel poke), and
/// return the preview as a structured report.
async fn execute_plan(rt: &TuiRuntime, recipient: &str, amount_nicks: u64) -> TuiCommandResponse {
    let cmd = match crate::send_simple::build_simple_send_tx(recipient, amount_nicks) {
        Ok(c) => c,
        Err(e) => return TuiCommandResponse::failure(e),
    };
    let preview = {
        let snap = rt.snapshot.lock().await.clone();
        let mut wallet = rt.wallet.lock().await;
        crate::send_simple::plan_send_preview(&mut wallet, snap, &cmd).await
    };
    match preview {
        Ok(text) => TuiCommandResponse::ok(
            Vec::new(),
            vec![Report::new(
                "create-tx-plan",
                "Review transaction",
                parse_markdown_to_sections(&text),
            )],
        ),
        Err(e) => TuiCommandResponse::failure(e.to_string()),
    }
}

async fn execute_create_and_send_request(rt: &TuiRuntime, cmd: Commands) -> TuiCommandResponse {
    let (res, events, markdown) = command_runner::execute_create_and_send(rt, cmd.clone()).await;
    let reports = normalize(&events, &markdown, &cmd);
    match res {
        Ok(()) => TuiCommandResponse::ok(events, reports),
        Err(e) => TuiCommandResponse::from_parts(events, reports, Some(e.to_string())),
    }
}

async fn execute_nns_register(rt: &TuiRuntime, name: &str) -> TuiCommandResponse {
    let recipient = match crate::nns::build_registry_recipient(name) {
        Ok(r) => r,
        Err(e) => return TuiCommandResponse::failure(e),
    };
    let cmd = crate::nns::schedule_create_tx_command(recipient);
    execute_create_and_send_request(rt, cmd).await
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
        tracing::debug!(request = ?job.request, "api: job received");
        let resp = execute_request(&rt, job.request).await;
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
