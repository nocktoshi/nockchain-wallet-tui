//! TUI-only orchestration around [`nockchain_wallet::dispatch::execute_wallet_command`].
//! CLI entry continues to call dispatch directly with owned [`nockchain_wallet::Wallet`] — unaffected by this module.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use nockapp::NockAppError;
use tokio::sync::{mpsc, watch, Mutex};

use super::screens::Screen;
use super::store::{UIStore, UiAction};
use crate::msg::Msg;
use crate::wallet_api::{
    command_to_argv, create_and_send_simple, nns_register, plan_simple_send, reports_to_text,
    run_command_http, TuiApiJob, TuiCommandResponse, WalletSessionState,
};
use nockchain_wallet::command::Commands;
use nockchain_wallet::dispatch::execute_wallet_command;
use nockchain_wallet::wallet_outcome::{WalletCommandData, WalletEvent};
use nockchain_wallet::WrittenTxSnapshot;
use nockchain_wallet::{ConnectionCli, DispatchHooks, NormalizedSnapshot, Wallet};

const TX_DIR: &str = "txs";

/// [`NockApp::run`] can return before the structured effect driver has polled; yield briefly.
async fn snapshot_wallet_events(
    sink: &Arc<std::sync::Mutex<Vec<WalletEvent>>>,
) -> Vec<WalletEvent> {
    let mut events = sink.lock().unwrap().clone();
    if !events.is_empty() {
        return events;
    }
    for _ in 0..96 {
        tokio::task::yield_now().await;
        events = sink.lock().unwrap().clone();
        if !events.is_empty() {
            return events;
        }
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    sink.lock().unwrap().clone()
}

/// Job completion: command result (error string), structured events, and the **rendered report
/// text** for the output panel (normalized server-side; never raw kernel markdown).
pub(crate) type JobCompletion = (Result<(), String>, Vec<WalletEvent>, String);

/// Background balance sidebar refresh (same `ShowBalance` path as the menu; does not use [`Screen::Running`]).
pub(crate) type BalanceRefreshCompletion = (u64, Result<(), NockAppError>, Vec<WalletEvent>);

/// Simple-send planner preview text (no kernel poke), from the `/tx/plan` endpoint.
pub(crate) type SendSimplePlanCompletion = Result<String, String>;

/// NNS name availability lookup (HTTP).
pub(crate) type NnsLookupCompletion = Result<crate::nns::NnsLookupOk, String>;

/// Owned `.nock` names for the active address (from /verified).
pub(crate) type OwnedNnsNamesCompletion = Result<Vec<String>, String>;

/// Home identity: active address + optional primary `.nock` name.
pub(crate) type HomeIdentityCompletion = (Option<String>, Option<String>);

/// Master addresses for the home wallet picker (full list with the active one flagged).
pub(crate) type MasterAddressesCompletion = Vec<crate::wallet_api::MasterAddressRow>;

/// Shared wallet + snapshot for spawned TUI jobs (`tui::run` wraps with [`Arc`]).
#[derive(Clone)]
pub(crate) struct TuiRuntime {
    pub wallet: Arc<Mutex<Wallet>>,
    pub snapshot: Arc<Mutex<Option<NormalizedSnapshot>>>,
    /// Session connection config.
    pub connection: Arc<std::sync::Mutex<ConnectionCli>>,
    /// Structured kernel/API events from `[%raw …]` effects. Shared across calls on purpose: the
    /// upstream IO drivers accumulate on the long-lived wallet app, so a per-call sink would let an
    /// old command's driver write to a dropped sink. Concurrency is handled by routing data fetches
    /// through the serialized HTTP API rather than by isolating sinks.
    pub wallet_event_sink: Arc<std::sync::Mutex<Vec<WalletEvent>>>,
    /// Captured kernel `%markdown` (create-tx, send-tx, …) for server-side normalization.
    pub tui_markdown_sink: Arc<std::sync::Mutex<String>>,
    /// Session settings persisted in `session.json` and exposed via GET/POST `/v1/wallet/state`.
    pub session_config: Arc<RwLock<WalletSessionState>>,
    pub session_path: PathBuf,
    /// Secret bearer token for this TUI session only (never written to disk).
    pub api_auth_token: Arc<str>,
    /// Channel to the background HTTP server (jobs executed on this TUI [`LocalSet`]).
    pub api_job_tx: mpsc::Sender<TuiApiJob>,
    /// Background HTTP listener (restarted when session `api_listen` changes).
    pub api_server: Arc<std::sync::Mutex<Option<crate::wallet_api::ApiServerHandle>>>,
}

/// Run a wallet command on the shared TUI runtime (TUI jobs and JSON API).
pub(crate) async fn run_command_on_runtime(
    rt: &TuiRuntime,
    command: Commands,
    sync_attempt: Option<watch::Sender<(usize, usize)>>,
    tx_snapshot_before: Option<WrittenTxSnapshot>,
) -> Result<WalletCommandData, NockAppError> {
    rt.tui_markdown_sink.lock().unwrap().clear();
    let mut hooks = DispatchHooks::structured_with_markdown(
        Arc::clone(&rt.wallet_event_sink),
        Arc::clone(&rt.tui_markdown_sink),
    );
    if let Some(tx) = sync_attempt {
        hooks = hooks.with_sync_attempt(tx);
    }
    let is_create_tx = matches!(command, Commands::CreateTx { .. });
    let outcome = {
        let mut w = rt.wallet.lock().await;
        let mut s = rt.snapshot.lock().await;
        execute_wallet_command(
            &rt.connection.lock().unwrap(),
            &mut w,
            &command,
            &mut s,
            false,
            hooks,
        )
        .await
    };
    match finalize_outcome(outcome, &rt.wallet_event_sink).await {
        Ok(mut data) => {
            if is_create_tx {
                append_create_tx_event(
                    &mut data.events,
                    &rt.tui_markdown_sink.lock().unwrap(),
                    tx_snapshot_before,
                )
                .await?;
            }
            Ok(data)
        }
        Err(e) => Err(e),
    }
}

async fn finalize_outcome(
    outcome: Result<WalletCommandData, NockAppError>,
    wallet_event_sink: &Arc<std::sync::Mutex<Vec<WalletEvent>>>,
) -> Result<WalletCommandData, NockAppError> {
    match outcome {
        Ok(mut data) => {
            if data.events.is_empty() {
                data.events = snapshot_wallet_events(wallet_event_sink).await;
            }
            Ok(data)
        }
        Err(e) => Err(e),
    }
}

/// Queue a wallet command: [`Screen::Running`] + in-TUI progress; work runs without leaving the alternate screen.
pub(crate) fn schedule_wallet_command(
    store: &mut UIStore,
    rt: &TuiRuntime,
    done_tx: mpsc::UnboundedSender<Msg>,
    cmd: Commands,
    label: impl Into<String>,
) {
    if store.state.job.is_some() {
        return;
    }
    let label_s = label.into();
    store.dispatch(UiAction::EnterRunningWalletJob {
        cmd: cmd.clone(),
        label: label_s,
        progress_rx: None,
    });

    let rt = rt.clone();
    tokio::task::spawn_local(async move {
        let _ = done_tx.send(Msg::Job(run_command_via_api(&rt, &cmd).await));
    });
}

/// Execute a simple command through the loopback HTTP API — the TUI is a client of its own API,
/// exactly like a web UI. The wallet itself is only touched by the API executor.
async fn run_command_via_api(rt: &TuiRuntime, cmd: &Commands) -> JobCompletion {
    let listen = crate::session::current_api_listen(rt);
    let argv = command_to_argv(cmd);
    http_completion(run_command_http(&listen, rt.api_auth_token.as_ref(), argv).await)
}

/// Map an HTTP API response into a [`JobCompletion`]: rendered report text + events, with command
/// or transport errors surfaced as `Err`.
fn http_completion(resp: Result<TuiCommandResponse, String>) -> JobCompletion {
    match resp {
        Ok(r) => {
            let output = reports_to_text(&r.reports);
            let result = match r.error {
                Some(e) => Err(e),
                None => Ok(()),
            };
            (result, r.events, output)
        }
        Err(transport) => (Err(transport), Vec::new(), String::new()),
    }
}

/// Refresh balance for the home hero through the loopback HTTP API — same client path as a web UI
/// and as the identity / master-address fetches, so the `api_job_loop` serializes them all. The
/// in-process sync-progress channel is gone (it can't cross HTTP); the hero shows a plain spinner.
pub(crate) fn schedule_balance_sidebar_refresh(
    store: &mut UIStore,
    rt: &TuiRuntime,
    done_tx: &mpsc::UnboundedSender<Msg>,
) {
    if !matches!(store.state.screen, Screen::Home) {
        return;
    }
    if store.state.balance_panel.loading {
        return;
    }
    store.dispatch(UiAction::BeginBalanceSidebarFetch);

    let nonce = store.state.balance_job_nonce;
    let rt = rt.clone();
    let tx = done_tx.clone();
    tokio::task::spawn_local(async move {
        let listen = crate::session::current_api_listen(&rt);
        let resp = run_command_http(
            &listen,
            rt.api_auth_token.as_ref(),
            vec!["show-balance".to_string()],
        )
        .await;
        let (result, events) = match resp {
            Ok(r) => {
                let result = match r.error {
                    Some(e) => Err(NockAppError::OtherError(e)),
                    None => Ok(()),
                };
                (result, r.events)
            }
            Err(transport) => (Err(NockAppError::OtherError(transport)), Vec::new()),
        };
        let _ = tx.send(Msg::Balance((nonce, result, events)));
    });
}

pub(crate) fn apply_balance_sidebar_result(
    store: &mut UIStore,
    nonce: u64,
    result: Result<(), NockAppError>,
    events: Vec<WalletEvent>,
) {
    store.dispatch(UiAction::BalanceSidebarCompleted {
        nonce,
        result,
        events,
    });
}

/// Resolve primary `.nock` name for an address already shown on home/receive.
pub(crate) fn schedule_nockname_resolve(address: String, done_tx: &mpsc::UnboundedSender<Msg>) {
    let tx = done_tx.clone();
    tokio::task::spawn_local(async move {
        let nockname = crate::nns::resolve_primary_name(&address)
            .await
            .ok()
            .flatten();
        let _ = tx.send(Msg::Identity((Some(address), nockname)));
    });
}

/// Load active address (same path as Receive) and resolve primary `.nock` name for home.
pub(crate) fn schedule_home_identity_fetch(
    store: &mut UIStore,
    rt: &TuiRuntime,
    done_tx: &mpsc::UnboundedSender<Msg>,
) {
    if !matches!(store.state.screen, Screen::Home) {
        return;
    }
    if store.state.balance_panel.identity_loading {
        return;
    }

    if let Some(addr) = store.state.balance_panel.address.clone() {
        store.dispatch(UiAction::BeginHomeIdentityFetch);
        schedule_nockname_resolve(addr, done_tx);
        return;
    }

    store.dispatch(UiAction::BeginHomeIdentityFetch);

    let rt = rt.clone();
    let tx = done_tx.clone();
    tokio::task::spawn_local(async move {
        let listen = crate::session::current_api_listen(&rt);
        let resp = run_command_http(
            &listen,
            rt.api_auth_token.as_ref(),
            vec!["list-active-addresses".to_string()],
        )
        .await;
        let address = resp
            .ok()
            .and_then(|r| super::view::first_active_address(&r.events));
        let nockname = match address.as_deref() {
            Some(a) => crate::nns::resolve_primary_name(a).await.ok().flatten(),
            None => None,
        };
        let _ = tx.send(Msg::Identity((address, nockname)));
    });
}

pub(crate) fn apply_home_identity_result(
    store: &mut UIStore,
    address: Option<String>,
    nockname: Option<String>,
) {
    store.dispatch(UiAction::HomeIdentityCompleted { address, nockname });
}

/// Fetch `list-master-addresses` for the home wallet dropdown through the loopback HTTP API — the
/// same client path a web UI would use. Going through the API means the `api_job_loop` serializes
/// this with the identity fetch (no shared-sink contention), and the active-wallet flag rides the
/// normalized report (`**(active)**` preserved server-side). Triggered only after balance completes,
/// so it never overlaps the direct balance refresh.
pub(crate) fn schedule_master_addresses_fetch(
    store: &mut UIStore,
    rt: &TuiRuntime,
    done_tx: &mpsc::UnboundedSender<Msg>,
) {
    if !matches!(store.state.screen, Screen::Home) {
        return;
    }
    if store.state.master_picker.loading {
        return;
    }
    store.dispatch(UiAction::BeginMasterAddressesFetch);

    let rt = rt.clone();
    let tx = done_tx.clone();
    tokio::task::spawn_local(async move {
        let listen = crate::session::current_api_listen(&rt);
        let resp = run_command_http(
            &listen,
            rt.api_auth_token.as_ref(),
            vec!["list-master-addresses".to_string()],
        )
        .await;
        let rows = match resp {
            Ok(r) => crate::wallet_api::parse_master_addresses(&reports_to_text(&r.reports)),
            Err(_) => Vec::new(),
        };
        let _ = tx.send(Msg::MasterAddresses(rows));
    });
}

pub(crate) fn apply_job_result(
    store: &mut UIStore,
    result: Result<(), String>,
    events: Vec<WalletEvent>,
    output: String,
    identity_done_tx: &mpsc::UnboundedSender<Msg>,
) {
    let receive_fetch = store
        .state
        .job
        .as_ref()
        .is_some_and(|j| matches!(j.cmd, Commands::ListActiveAddresses))
        && matches!(store.state.screen, Screen::Receive { .. });
    let ok = result.is_ok();
    store.dispatch(UiAction::JobCompleted {
        result,
        events,
        output,
    });
    if receive_fetch && ok {
        if let Some(addr) = store.state.balance_panel.address.clone() {
            schedule_nockname_resolve(addr, identity_done_tx);
        }
    }
}

/// Query NNS registry for name availability (background HTTP).
pub(crate) fn schedule_nns_lookup(raw: String, done_tx: mpsc::UnboundedSender<Msg>) {
    tokio::task::spawn_local(async move {
        let result = crate::nns::lookup_name(&raw).await;
        let _ = done_tx.send(Msg::NnsLookup(result));
    });
}

/// Query NNS registry for all verified names owned by an address (background HTTP).
pub(crate) fn schedule_nns_verified_names(address: String, done_tx: mpsc::UnboundedSender<Msg>) {
    tokio::task::spawn_local(async move {
        let result = crate::nns::list_verified_names(&address).await;
        let _ = done_tx.send(Msg::OwnedNnsNames(result));
    });
}

/// Plan a simple-send transaction via the `/tx/plan` endpoint; preview text for the review screen.
pub(crate) fn schedule_send_simple_plan(
    rt: TuiRuntime,
    recipient: String,
    amount_nicks: u64,
    done_tx: mpsc::UnboundedSender<Msg>,
) {
    tokio::task::spawn_local(async move {
        let listen = crate::session::current_api_listen(&rt);
        let resp = plan_simple_send(
            &listen,
            rt.api_auth_token.as_ref(),
            &recipient,
            amount_nicks,
        )
        .await;
        let result = match resp {
            Ok(r) if r.error.is_none() => Ok(reports_to_text(&r.reports)),
            Ok(r) => Err(r.error.unwrap_or_else(|| "Planning failed".into())),
            Err(e) => Err(e),
        };
        let _ = done_tx.send(Msg::Plan(result));
    });
}

/// Build the tx file then broadcast each with `send-tx`. Wallet-touching orchestration run by the
/// API executor (on the `LocalSet`); returns raw events + captured markdown for normalization.
pub(crate) async fn execute_create_and_send(
    rt: &TuiRuntime,
    create_cmd: Commands,
) -> (Result<(), NockAppError>, Vec<WalletEvent>, String) {
    rt.wallet_event_sink.lock().unwrap().clear();
    rt.tui_markdown_sink.lock().unwrap().clear();

    let before = match Wallet::snapshot_written_txs(Path::new(TX_DIR)).await {
        Ok(s) => s,
        Err(e) => return (Err(e), Vec::new(), String::new()),
    };

    let create_outcome = run_command_on_runtime(rt, create_cmd, None, Some(before.clone())).await;
    let mut events = create_outcome
        .as_ref()
        .map(|d| d.events.clone())
        .unwrap_or_default();

    if create_outcome.is_err() {
        let markdown = rt.tui_markdown_sink.lock().unwrap().clone();
        return (create_outcome.map(|_| ()), events, markdown);
    }

    let tx_paths = create_tx_paths_from_events_or_disk(&events, &before).await;
    if tx_paths.is_empty() {
        let markdown = rt.tui_markdown_sink.lock().unwrap().clone();
        return (
            Err(NockAppError::OtherError(
                "create-tx finished but no transaction file was written under ./txs/".into(),
            )),
            events,
            markdown,
        );
    }

    let mut combined = Ok(());
    for path in tx_paths {
        let send_outcome =
            run_command_on_runtime(rt, Commands::SendTx { transaction: path }, None, None).await;
        match send_outcome {
            Ok(data) => events.extend(data.events),
            Err(e) => {
                combined = Err(e);
                break;
            }
        }
    }

    let markdown = rt.tui_markdown_sink.lock().unwrap().clone();
    refresh_create_tx_summary(&mut events, &markdown);
    (combined, events, markdown)
}

/// Simple-send review **Send** — build + broadcast via the `/tx/create-and-send` endpoint. The
/// locally-built command only drives the Running restore/toast discriminant; the wallet work runs
/// server-side in the executor.
pub(crate) fn schedule_send_simple_create_and_send(
    store: &mut UIStore,
    rt: &TuiRuntime,
    done_tx: mpsc::UnboundedSender<Msg>,
    recipient: String,
    amount_nicks: u64,
) {
    if store.state.job.is_some() {
        return;
    }
    let cmd = match crate::send_simple::build_simple_send_tx(&recipient, amount_nicks) {
        Ok(c) => c,
        Err(e) => {
            let _ = done_tx.send(Msg::Job((Err(e), Vec::new(), String::new())));
            return;
        }
    };
    store.dispatch(UiAction::EnterRunningWalletJob {
        cmd,
        label: "Create & send".into(),
        progress_rx: None,
    });

    let rt = rt.clone();
    tokio::task::spawn_local(async move {
        let listen = crate::session::current_api_listen(&rt);
        let resp = create_and_send_simple(
            &listen,
            rt.api_auth_token.as_ref(),
            &recipient,
            amount_nicks,
        )
        .await;
        let _ = done_tx.send(Msg::Job(http_completion(resp)));
    });
}

/// NNS **Register** — registry-payment create-tx + send via the `/nns/register` endpoint.
pub(crate) fn schedule_nns_register(
    store: &mut UIStore,
    rt: &TuiRuntime,
    done_tx: mpsc::UnboundedSender<Msg>,
    canonical_name: &str,
) -> Result<(), String> {
    if store.state.job.is_some() {
        return Ok(());
    }
    let recipient = crate::nns::build_registry_recipient(canonical_name)?;
    let cmd = crate::nns::schedule_create_tx_command(recipient);
    let name = canonical_name.to_string();
    store.dispatch(UiAction::EnterRunningWalletJob {
        cmd,
        label: "Register & send".into(),
        progress_rx: None,
    });

    let rt = rt.clone();
    tokio::task::spawn_local(async move {
        let listen = crate::session::current_api_listen(&rt);
        let resp = nns_register(&listen, rt.api_auth_token.as_ref(), &name).await;
        let _ = done_tx.send(Msg::Job(http_completion(resp)));
    });
    Ok(())
}

async fn append_create_tx_event(
    events: &mut Vec<WalletEvent>,
    markdown: &str,
    before: Option<WrittenTxSnapshot>,
) -> Result<(), NockAppError> {
    if events
        .iter()
        .any(|e| matches!(e, WalletEvent::CreateTxV1 { .. }))
    {
        return Ok(());
    }
    let tx_paths = if let Some(before) = before {
        let after = Wallet::snapshot_written_txs(Path::new(TX_DIR)).await?;
        Wallet::detect_written_tx_paths(&before, &after).ok()
    } else {
        None
    };
    let tx_paths = tx_paths.unwrap_or_else(|| parse_tx_paths_from_markdown(markdown));
    if markdown.trim().is_empty() && tx_paths.is_empty() {
        return Ok(());
    }
    events.push(WalletEvent::CreateTxV1 {
        tx_paths,
        summary: markdown.to_string(),
    });
    Ok(())
}

fn refresh_create_tx_summary(events: &mut Vec<WalletEvent>, markdown: &str) {
    for event in events.iter_mut() {
        if let WalletEvent::CreateTxV1 { summary, .. } = event {
            *summary = markdown.to_string();
            return;
        }
    }
}

async fn create_tx_paths_from_events_or_disk(
    events: &[WalletEvent],
    before: &WrittenTxSnapshot,
) -> Vec<String> {
    if let Some(WalletEvent::CreateTxV1 { tx_paths, .. }) = events
        .iter()
        .find(|e| matches!(e, WalletEvent::CreateTxV1 { .. }))
    {
        if !tx_paths.is_empty() {
            return tx_paths.clone();
        }
    }
    if let Ok(after) = Wallet::snapshot_written_txs(Path::new(TX_DIR)).await {
        if let Ok(paths) = Wallet::detect_written_tx_paths(before, &after) {
            return paths;
        }
    }
    Vec::new()
}

fn parse_tx_paths_from_markdown(markdown: &str) -> Vec<String> {
    markdown
        .lines()
        .filter_map(|line| {
            line.split("Saved transaction to ").nth(1).map(|rest| {
                rest.trim()
                    .trim_start_matches('`')
                    .trim_end_matches('`')
                    .to_string()
            })
        })
        .collect()
}

/// Spawn CoinGecko price fetch (result delivered as [`Msg::Price`]).
pub(crate) fn schedule_price_fetch(
    store: &mut UIStore,
    price_done_tx: &mpsc::UnboundedSender<Msg>,
) {
    use super::store::price_fetch_stale;

    if store.state.price.loading {
        return;
    }
    if !price_fetch_stale(&store.state) {
        return;
    }
    store.dispatch(UiAction::BeginPriceFetch);
    let tx = price_done_tx.clone();
    tokio::task::spawn_local(async move {
        let result = super::components::price::fetch_nock_usd().await;
        let _ = tx.send(Msg::Price(result));
    });
}
