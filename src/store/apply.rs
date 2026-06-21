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
        UiAction::BeginBalanceSidebarFetch => {
            if !matches!(state.screen, Screen::Home) {
                return;
            }
            if state.balance_panel.loading {
                return;
            }
            state.balance_panel.loading = true;
            state.balance_panel.error = None;
            state.balance_job_nonce = state.balance_job_nonce.wrapping_add(1);
            state.sync_progress = None;
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
        UiAction::BeginMasterAddressesFetch => {
            state.master_picker.loading = true;
        }
        UiAction::MasterAddressesLoaded { rows } => {
            state.master_picker.loading = false;
            state.master_picker.addresses = rows;
            // Park the dropdown highlight on the active row (clamped if the list shrank).
            let active = state.master_picker.active_index().unwrap_or(0);
            let len = state.master_picker.addresses.len();
            state.master_picker.sel = active.min(len.saturating_sub(1));
            if !state.master_picker.has_choice() {
                state.master_picker.open = false;
            }
        }
        UiAction::ToggleMasterPicker => {
            if !state.master_picker.has_choice() {
                state.master_picker.open = false;
                return;
            }
            state.master_picker.open = !state.master_picker.open;
            if state.master_picker.open {
                state.master_picker.sel = state.master_picker.active_index().unwrap_or(0);
            }
        }
        UiAction::MoveMasterPickerSel { delta } => {
            let len = state.master_picker.addresses.len();
            if len == 0 {
                return;
            }
            let cur = state.master_picker.sel.min(len - 1) as i32;
            let next = (cur + delta).rem_euclid(len as i32);
            state.master_picker.sel = next as usize;
        }
        UiAction::CloseMasterPicker => {
            state.master_picker.open = false;
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
    // A new completion supersedes any prior floating toast (e.g. "Address copied"); the ✓ panel
    // header and the toast are mutually exclusive, set fresh below.
    state.toast = None;
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

#[cfg(test)]
mod single_confirmation_tests {
    use super::*;
    use crate::app_state::{status_modal_visible, UiState};
    use crate::screens::{RunningJob, Screen};

    fn running(cmd: Commands) -> UiState {
        let mut s = UiState::new(Screen::Home);
        s.job = Some(RunningJob {
            label: "running".into(),
            cmd,
        });
        s
    }

    /// Success *with* output: one scrollable panel carrying the green ✓ header, no floating toast,
    /// so a single Enter (DismissStatusOutput) clears it. A stray earlier toast must not survive.
    #[test]
    fn success_with_output_uses_panel_not_toast() {
        let mut s = running(Commands::ListNotes);
        s.toast = Some("Address copied to clipboard".into());
        apply_job_completed(&mut s, Ok(()), vec![], "## Notes\n- count: 0".into());

        assert!(s.toast.is_none(), "stray/old toast must be cleared");
        assert_eq!(s.last_command_status.as_deref(), Some("Notes listed."));
        assert!(!s.last_command_output.is_empty());
        assert!(s.job.is_none());
        // Panel visible -> the single-key dismiss path is DismissStatusOutput, not TakeToast.
        assert!(status_modal_visible(&s));
    }

    /// Success *without* output: only a floating toast, dismissed by the single TakeToast keypress;
    /// no status panel and no ✓ header competing for a second dismiss.
    #[test]
    fn success_without_output_uses_toast_not_panel() {
        let mut s = running(Commands::ExportKeys);
        apply_job_completed(&mut s, Ok(()), vec![], String::new());

        assert_eq!(s.toast.as_deref(), Some("Export completed."));
        assert!(s.last_command_status.is_none());
        assert!(s.last_command_output.is_empty());
        // No panel (output empty, no job) -> nothing left to dismiss after the toast.
        assert!(!status_modal_visible(&s));
    }
}

#[cfg(test)]
mod master_picker_tests {
    use super::*;
    use crate::app_state::UiState;
    use crate::screens::Screen;
    use crate::wallet_api::MasterAddressRow;

    fn row(addr: &str, active: bool) -> MasterAddressRow {
        MasterAddressRow {
            address_b58: addr.into(),
            version: 1,
            active,
        }
    }

    /// Loading addresses parks the highlight on the active row and only enables the dropdown when
    /// there is more than one wallet.
    #[test]
    fn loaded_parks_selection_on_active_and_gates_choice() {
        let mut s = UiState::new(Screen::Home);
        apply_ui_action(
            &mut s,
            UiAction::MasterAddressesLoaded {
                rows: vec![row("aaa", false), row("bbb", true), row("ccc", false)],
            },
        );
        assert_eq!(s.master_picker.sel, 1);
        assert!(s.master_picker.has_choice());
        assert_eq!(s.master_picker.active_address(), Some("bbb"));

        // A single wallet -> no dropdown affordance, force-closed.
        s.master_picker.open = true;
        apply_ui_action(
            &mut s,
            UiAction::MasterAddressesLoaded {
                rows: vec![row("only", true)],
            },
        );
        assert!(!s.master_picker.has_choice());
        assert!(!s.master_picker.open);
    }

    /// Toggle opens (onto the active row) only with a real choice; movement wraps.
    #[test]
    fn toggle_and_move_wrap() {
        let mut s = UiState::new(Screen::Home);
        apply_ui_action(
            &mut s,
            UiAction::MasterAddressesLoaded {
                rows: vec![row("aaa", false), row("bbb", true)],
            },
        );
        apply_ui_action(&mut s, UiAction::ToggleMasterPicker);
        assert!(s.master_picker.open);
        assert_eq!(s.master_picker.sel, 1, "opens on the active row");

        apply_ui_action(&mut s, UiAction::MoveMasterPickerSel { delta: 1 });
        assert_eq!(s.master_picker.sel, 0, "wraps past the end");
        apply_ui_action(&mut s, UiAction::MoveMasterPickerSel { delta: -1 });
        assert_eq!(s.master_picker.sel, 1, "wraps before the start");

        apply_ui_action(&mut s, UiAction::CloseMasterPicker);
        assert!(!s.master_picker.open);
    }

    /// A single-wallet picker never opens.
    #[test]
    fn toggle_noop_without_choice() {
        let mut s = UiState::new(Screen::Home);
        apply_ui_action(
            &mut s,
            UiAction::MasterAddressesLoaded {
                rows: vec![row("solo", true)],
            },
        );
        apply_ui_action(&mut s, UiAction::ToggleMasterPicker);
        assert!(!s.master_picker.open);
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
