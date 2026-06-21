//! Single transition function for TUI UI state (`apply_ui_action`).

use std::time::{Duration, Instant};

use nockapp::NockAppError;

use super::super::app_state::UiState;
use super::super::components::menus::{CT_ERR_ACTIONS, GENERIC_ERR};
use super::super::screens::{ErrorCtx, RunningJob, Screen};
use super::super::view;
use super::action::UiAction;
use nockchain_wallet::command::Commands;

const PRICE_STALE: Duration = Duration::from_secs(60);

/// Invariants: at most one `Screen::Running`; `balance_job_nonce` monotonic for stale sidebar drops.
pub(crate) fn apply_ui_action(state: &mut UiState, action: UiAction) {
    match action {
        UiAction::Tick => {
            state.ui_fx.frame_clock = state.ui_fx.frame_clock.wrapping_add(1);
        }
        UiAction::TakeToast => {
            state.toast.take();
        }
        UiAction::DismissStatusOutput => {
            state.last_command_output.clear();
            state.last_command_events.clear();
            state.last_command_status = None;
            state.output_scroll = 0;
        }
        UiAction::ReplaceScreen(s) => {
            state.screen = s;
        }
        UiAction::EnterMainFromSplash => {
            state.screen = Screen::Home;
            state.home_tab = 0;
        }
        UiAction::EnterRunningWalletJob {
            cmd,
            label,
            progress_rx,
        } => {
            if state.job.is_some() {
                return;
            }
            state.balance_job_nonce = state.balance_job_nonce.wrapping_add(1);
            state.balance_panel.loading = false;
            // A command launched from a prompt consumes that overlay; the route stays put.
            state.overlay = None;
            state.job = Some(RunningJob { label, cmd });
            state.sync_progress = progress_rx;
        }
        UiAction::BeginBalanceSidebarFetch { progress_rx } => {
            if !matches!(state.screen, Screen::Home) {
                return;
            }
            if state.balance_panel.loading {
                return;
            }
            state.balance_panel.loading = true;
            state.balance_panel.error = None;
            state.balance_job_nonce = state.balance_job_nonce.wrapping_add(1);
            state.sync_progress = Some(progress_rx);
        }
        UiAction::JobCompleted {
            result,
            events,
            output,
        } => {
            apply_job_completed(state, result, events, output);
        }
        UiAction::BalanceSidebarCompleted {
            nonce,
            result,
            events,
        } => {
            apply_balance_sidebar_completed(state, nonce, result, events);
        }
        UiAction::BeginHomeIdentityFetch => {
            if !matches!(state.screen, Screen::Home) {
                return;
            }
            state.balance_panel.identity_loading = true;
        }
        UiAction::HomeIdentityCompleted { address, nockname } => {
            if !matches!(state.screen, Screen::Home) {
                return;
            }
            state.balance_panel.identity_loading = false;
            if let Some(addr) = address {
                state.balance_panel.address = Some(addr);
            }
            state.balance_panel.nockname = nockname;
        }
        UiAction::NnsOwnedNamesLoaded { names } => {
            if let Screen::NnsBuy {
                owned_names,
                owned_names_loading,
                ..
            } = &mut state.screen
            {
                *owned_names = names;
                *owned_names_loading = false;
            }
        }
        UiAction::NudgeOutputScroll { delta } => {
            if delta >= 0 {
                state.output_scroll = state.output_scroll.saturating_add(delta as u16);
            } else {
                state.output_scroll = state.output_scroll.saturating_sub((-delta) as u16);
            }
        }
        UiAction::SetOutputScroll(y) => {
            state.output_scroll = y;
        }
        UiAction::SetHomeTab(tab) => {
            state.home_tab = tab.min(1);
        }
        UiAction::HomeTabNext => {
            state.home_tab = (state.home_tab + 1) % 2;
        }
        UiAction::HomeTabPrev => {
            state.home_tab = (state.home_tab + 2 - 1) % 2;
        }
        UiAction::SetMenuSel(sel) => {
            state.menu_sel = sel;
        }
        UiAction::BeginPriceFetch => {
            if state.price.loading {
                return;
            }
            state.price.loading = true;
            state.price.error = None;
        }
        UiAction::PriceFetched { usd_per_coin } => {
            state.price.loading = false;
            state.price.usd_per_coin = Some(usd_per_coin);
            state.price.fetched_at = Some(Instant::now());
            state.price.error = None;
        }
        UiAction::PriceFetchFailed { msg } => {
            state.price.loading = false;
            state.price.error = Some(msg);
        }
    }
}

pub(crate) fn price_fetch_stale(state: &UiState) -> bool {
    match state.price.fetched_at {
        None => true,
        Some(t) => t.elapsed() > PRICE_STALE,
    }
}

fn apply_balance_sidebar_completed(
    state: &mut UiState,
    nonce: u64,
    result: Result<(), NockAppError>,
    events: Vec<nockchain_wallet::wallet_outcome::WalletEvent>,
) {
    state.sync_progress = None;
    state.balance_panel.loading = false;
    state.last_command_events = events.clone();
    if nonce != state.balance_job_nonce {
        return;
    }
    if state.job.is_some() {
        return;
    }
    let display = view::render_balance_sidebar(&events);
    match result {
        Ok(()) => {
            state.balance_panel.text = display;
            state.balance_panel.events = events;
            state.balance_panel.error = None;
            state.balance_panel.scroll = 0;
        }
        Err(e) => {
            state.balance_panel.error = Some(e.to_string());
            if !display.is_empty() {
                state.balance_panel.text = format!("{display}\n\n--- error ---\n{e}");
            }
            if !events.is_empty() {
                state.balance_panel.events = events;
            }
        }
    }
}

fn apply_job_completed(
    state: &mut UiState,
    result: Result<(), String>,
    events: Vec<nockchain_wallet::wallet_outcome::WalletEvent>,
    output: String,
) {
    state.sync_progress = None;
    state.last_command_events = events.clone();
    state.last_command_status = None;
    let display = output;
    // The route stayed put while the job ran; transition it now based on the command.
    let Some(cmd) = state.job.take().map(|j| j.cmd) else {
        return;
    };
    let receive_fetch = matches!(&cmd, Commands::ListActiveAddresses)
        && matches!(state.screen, Screen::Receive { .. });
    match result {
        Ok(()) => {
            let has_output = !receive_fetch && !display.is_empty();
            if has_output {
                state.last_command_output = display.clone();
                state.output_scroll = 0;
            } else {
                state.last_command_output.clear();
            }

            // Route transition + the success message for this command.
            let mut success_msg = success_line(&cmd);
            if matches!(&cmd, Commands::CreateTx { .. }) {
                state.screen = match &state.screen {
                    Screen::SendSimple { .. } => {
                        success_msg = "Transaction created and sent.".into();
                        Screen::Home
                    }
                    Screen::NnsBuy { .. } => {
                        success_msg = "Name registration sent.".into();
                        Screen::Home
                    }
                    _ => Screen::Transactions { sel: 0 },
                };
            }
            if receive_fetch {
                apply_receive_address_if_needed(state, &cmd, &events);
            }
            if matches!(&cmd, Commands::ShowBalance) {
                state.balance_panel.text = view::render_balance_sidebar(&events);
                state.balance_panel.events = events.clone();
                state.balance_panel.error = None;
                state.balance_panel.scroll = 0;
            }
            // One confirmation: a green ✓ header on the output panel when there's output to scroll,
            // otherwise a floating toast — never both.
            if !receive_fetch {
                if has_output {
                    state.last_command_status = Some(success_msg);
                } else {
                    state.toast = Some(success_msg);
                }
            }
        }
        Err(e) => {
            let out = if !display.is_empty() {
                format!("{display}\n\n--- error ---\n{e}")
            } else {
                e.to_string()
            };
            if receive_fetch || out.is_empty() {
                state.last_command_output.clear();
            } else {
                state.last_command_output = out;
                state.output_scroll = 0;
            }
            if matches!(&cmd, Commands::ShowBalance) {
                state.balance_panel.error = Some(e.to_string());
                if !display.is_empty() {
                    state.balance_panel.text = format!("{display}\n\n--- error ---\n{e}");
                }
            }
            if receive_fetch {
                if let Screen::Receive { error, loading, .. } = &mut state.screen {
                    *loading = false;
                    *error = Some(e.to_string());
                }
            } else {
                state.screen = Screen::ErrorScreen {
                    msg: e.to_string(),
                    sel: 0,
                    actions: error_actions_for_command(&cmd),
                    ctx: error_ctx_for_command(&cmd),
                };
            }
        }
    }
}

fn apply_receive_address_if_needed(
    state: &mut UiState,
    cmd: &Commands,
    events: &[nockchain_wallet::wallet_outcome::WalletEvent],
) {
    if !matches!(cmd, Commands::ListActiveAddresses) {
        return;
    }
    if let Screen::Receive {
        address,
        loading,
        error,
        ..
    } = &mut state.screen
    {
        *loading = false;
        *error = None;
        let resolved = view::first_active_address(events);
        *address = resolved.clone();
        state.balance_panel.address = resolved;
        state.balance_panel.identity_loading = false;
        if address.is_none() {
            *error = Some("No active address found".into());
        }
    }
}

fn error_ctx_for_command(cmd: &Commands) -> ErrorCtx {
    match cmd {
        Commands::CreateTx { .. } => ErrorCtx::CreateTx { cmd: cmd.clone() },
        _ => ErrorCtx::Retry(cmd.clone()),
    }
}

fn error_actions_for_command(cmd: &Commands) -> &'static [&'static str] {
    match cmd {
        Commands::CreateTx { .. } => CT_ERR_ACTIONS,
        _ => GENERIC_ERR,
    }
}

fn success_line(cmd: &Commands) -> String {
    match cmd {
        Commands::ShowBalance => "Balance updated.".into(),
        Commands::Keygen => "New keys generated.".into(),
        Commands::CreateTx { .. } => "Transaction created.".into(),
        Commands::ListNotes => "Notes listed.".into(),
        Commands::DeriveChild { .. } => "Derived child key.".into(),
        Commands::ImportKeys { .. } => "Import completed.".into(),
        Commands::ExportKeys => "Export completed.".into(),
        Commands::MigrateV0Notes { .. } => "Migration step finished.".into(),
        Commands::SendTx { .. } => "Send completed.".into(),
        Commands::ShowTx { .. } => "Transaction shown.".into(),
        Commands::ListActiveAddresses => "Addresses loaded.".into(),
        _ => "Done.".into(),
    }
}
