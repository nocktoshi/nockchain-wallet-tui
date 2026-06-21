//! Ratatui terminal: async event loop and suspend/resume around wallet I/O.
//!
//! **Input:** key events are read on a background thread and sent over an unbounded channel.
//! **Completions:** all background work reports back through one [`Msg`] channel (see [`crate::msg`]).
//! `tokio::select!` merges input, completions, and a spinner tick. Drawing lives in
//! [`super::components`].

use std::io::{self, stdout};
use std::time::Duration;

use crossterm::event::{EnableBracketedPaste, Event, KeyEventKind};
use crossterm::terminal::{enable_raw_mode, EnterAlternateScreen};
use crossterm::ExecutableCommand;
use nockapp::NockAppError;
use tokio::sync::mpsc;
use tokio::task::LocalSet;

use super::command_runner::{self, TuiRuntime};
use super::components::root::draw_ui;
use super::handlers;
use super::hooks::events::spawn_crossterm_channel;
use super::hooks::terminal::{restore_terminal, Term};
use super::msg::Msg;
use super::screens::{Screen, TuiControl};
use super::store::{UIStore, UiAction};
use crate::wallet_api::TuiApiJob;
use nockchain_wallet::command::Commands;

pub(crate) fn io_err(e: io::Error) -> NockAppError {
    NockAppError::OtherError(format!("terminal io: {e}"))
}

pub(super) async fn run(
    rt: TuiRuntime,
    api_job_rx: mpsc::Receiver<TuiApiJob>,
) -> Result<(), NockAppError> {
    LocalSet::new().run_until(run_inner(rt, api_job_rx)).await
}

/// Route one async completion to its reducer (and schedule any follow-up work).
fn handle_msg(store: &mut UIStore, rt: &TuiRuntime, msg: Msg, msg_tx: &mpsc::UnboundedSender<Msg>) {
    match msg {
        Msg::Job((res, events, output)) => {
            // Capture the completing command before the reducer consumes the running job, so we can
            // refresh the home wallet view after switching the active master address.
            let was_set_active = store
                .state
                .job
                .as_ref()
                .is_some_and(|j| matches!(j.cmd, Commands::SetActiveMasterAddress { .. }));
            let ok = res.is_ok();
            command_runner::apply_job_result(store, res, events, output, msg_tx);
            if was_set_active && ok {
                // Re-derive the home view for the new active wallet. Only kick the balance refresh;
                // identity + master-address fetches cascade from its completion (Msg::Balance), so
                // the API-routed master fetch never overlaps this direct balance refresh.
                command_runner::schedule_balance_sidebar_refresh(store, rt, msg_tx);
            }
        }
        Msg::Balance((nonce, res, events)) => {
            let ok = res.is_ok();
            command_runner::apply_balance_sidebar_result(store, nonce, res, events);
            if ok {
                command_runner::schedule_price_fetch(store, msg_tx);
                command_runner::schedule_home_identity_fetch(store, rt, msg_tx);
                command_runner::schedule_master_addresses_fetch(store, rt, msg_tx);
            }
        }
        Msg::Plan(result) => handlers::apply_send_simple_plan_result(store, result),
        Msg::NnsLookup(result) => handlers::apply_nns_lookup_result(store, result),
        Msg::OwnedNnsNames(result) => {
            if let Ok(names) = result {
                store.dispatch(UiAction::NnsOwnedNamesLoaded { names });
                store.dispatch(UiAction::Tick);
            }
        }
        Msg::Identity((address, nockname)) => {
            command_runner::apply_home_identity_result(store, address, nockname);
        }
        Msg::MasterAddresses(rows) => {
            store.dispatch(UiAction::MasterAddressesLoaded { rows });
        }
        Msg::Price(result) => match result {
            Ok(usd) => store.dispatch(UiAction::PriceFetched { usd_per_coin: usd }),
            Err(m) => store.dispatch(UiAction::PriceFetchFailed { msg: m }),
        },
    }
}

async fn run_inner(
    rt: TuiRuntime,
    api_job_rx: mpsc::Receiver<TuiApiJob>,
) -> Result<(), NockAppError> {
    let rt_api = rt.clone();
    let api_task = tokio::task::spawn_local(async move {
        super::wallet_api::run_api_job_loop(rt_api, api_job_rx).await;
    });
    stdout().execute(EnterAlternateScreen).map_err(io_err)?;
    enable_raw_mode().map_err(io_err)?;
    stdout().execute(EnableBracketedPaste).map_err(io_err)?;

    let mut terminal =
        Term::new(ratatui::backend::CrosstermBackend::new(stdout())).map_err(io_err)?;
    terminal.hide_cursor().map_err(io_err)?;
    let terminal = std::sync::Arc::new(tokio::sync::Mutex::new(terminal));

    let mut ev_rx = spawn_crossterm_channel();
    let (msg_tx, mut msg_rx) = mpsc::unbounded_channel::<Msg>();

    let mut store = UIStore::new(Screen::Splash);
    let _ = super::session::refresh_session_from_api(&rt).await;
    store.session_display = super::session::session_config_snapshot(&rt);
    let mut interval = tokio::time::interval(Duration::from_millis(120));

    let result = loop {
        {
            let mut term_guard = terminal.lock().await;
            term_guard
                .draw(|f| draw_ui(f, &mut store))
                .map_err(io_err)?;
        }

        tokio::select! {
            biased;
            maybe_msg = msg_rx.recv() => {
                if let Some(msg) = maybe_msg {
                    handle_msg(&mut store, &rt, msg, &msg_tx);
                }
            }
            _ = interval.tick() => {
                store.dispatch(UiAction::Tick);
            }
            Some(ev) = ev_rx.recv() => {
                match ev {
                    Event::Key(key) => {
                        if key.kind == KeyEventKind::Release {
                            continue;
                        }
                        match handlers::dispatch_key(&rt, &mut store, key, &terminal, &msg_tx).await {
                            Ok(TuiControl::Continue) => {}
                            Ok(TuiControl::Quit) => break Ok(()),
                            Err(e) => {
                                shutdown(&rt, &api_task, &terminal).await;
                                break Err(e);
                            }
                        }
                    }
                    Event::Paste(text) => {
                        match handlers::dispatch_paste(
                            &rt.connection.lock().unwrap(),
                            &mut store,
                            text,
                            &rt,
                            &msg_tx,
                        )
                        .await
                        {
                            Ok(TuiControl::Continue) => {}
                            Ok(TuiControl::Quit) => break Ok(()),
                            Err(e) => {
                                shutdown(&rt, &api_task, &terminal).await;
                                break Err(e);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    };

    shutdown(&rt, &api_task, &terminal).await;
    result
}

/// Stop the background API server, abort the job loop, and restore the terminal.
async fn shutdown(
    rt: &TuiRuntime,
    api_task: &tokio::task::JoinHandle<()>,
    terminal: &std::sync::Arc<tokio::sync::Mutex<Term>>,
) {
    if let Some(h) = rt.api_server.lock().unwrap().take() {
        h.stop();
    }
    api_task.abort();
    let mut term_guard = terminal.lock().await;
    let _ = restore_terminal(&mut term_guard);
}
