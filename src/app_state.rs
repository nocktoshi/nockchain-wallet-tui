//! Aggregates TUI screen plus ephemeral UI state (toast, sync progress watch).
//! Persisted connection/API settings live in [`crate::wallet_api::WalletSessionState`]
//! (`session.json`, GET/POST `/v1/wallet/state`).

use std::time::Instant;

use ratatui::widgets::ListState;
use tokio::sync::watch;

use super::screens::{Overlay, RunningJob, Screen};
use nockchain_wallet::command::Commands;

/// Bottom status/output panel: visible while a command runs or meaningful output exists.
pub(crate) fn status_modal_visible(state: &UiState) -> bool {
    if let Some(job) = &state.job {
        // Receive-address fetch and NNS create-tx draw full-panel; hide the status strip.
        if matches!(state.screen, Screen::Receive { .. })
            && matches!(job.cmd, Commands::ListActiveAddresses)
        {
            return false;
        }
        if matches!(state.screen, Screen::NnsBuy { .. })
            && matches!(job.cmd, Commands::CreateTx { .. })
        {
            return false;
        }
        return true;
    }
    !state.last_command_output.is_empty()
}

/// CoinGecko USD price for the home hero.
#[derive(Debug, Clone, Default)]
pub(crate) struct PriceState {
    pub usd_per_coin: Option<f64>,
    pub loading: bool,
    pub error: Option<String>,
    pub fetched_at: Option<Instant>,
}

/// Presentation-only: animation frame clock, etc. (no wallet semantics).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct UiFx {
    /// Drives braille / create-tx spinners; advanced by [`super::store::UiAction::Tick`].
    pub frame_clock: u64,
}

/// Cached balance markdown for the home wallet tab (from `ShowBalance` / sidebar refresh).
#[derive(Debug, Clone)]
pub(crate) struct BalancePanelState {
    pub text: String,
    pub scroll: u16,
    pub loading: bool,
    pub error: Option<String>,
    /// Latest balance snapshot events (for hero NOCK + USD math).
    pub events: Vec<nockchain_wallet::wallet_outcome::WalletEvent>,
    /// Active receive address + optional primary `.nock` name from the registry API.
    pub identity_loading: bool,
    pub address: Option<String>,
    pub nockname: Option<String>,
}

/// Home wallet picker: the master addresses from `list-master-addresses` and dropdown UI state.
/// Shown as a selectable dropdown only when more than one master address exists.
#[derive(Debug, Clone, Default)]
pub(crate) struct MasterPickerState {
    /// All master addresses with the active one flagged (parsed from list-master-addresses).
    pub addresses: Vec<crate::wallet_api::MasterAddressRow>,
    /// A `list-master-addresses` fetch is in flight.
    pub loading: bool,
    /// The dropdown is expanded.
    pub open: bool,
    /// Highlighted row while the dropdown is open.
    pub sel: usize,
}

impl MasterPickerState {
    /// Index of the active master address, if known.
    pub fn active_index(&self) -> Option<usize> {
        self.addresses.iter().position(|a| a.active)
    }

    /// The active master address string, if known.
    pub fn active_address(&self) -> Option<&str> {
        self.addresses
            .iter()
            .find(|a| a.active)
            .map(|a| a.address_b58.as_str())
    }

    /// Show the dropdown affordance only when there is a real choice to make.
    pub fn has_choice(&self) -> bool {
        self.addresses.len() > 1
    }
}

impl Default for BalancePanelState {
    fn default() -> Self {
        Self {
            text: String::new(),
            scroll: 0,
            loading: false,
            error: None,
            events: Vec::new(),
            identity_loading: false,
            address: None,
            nockname: None,
        }
    }
}

pub(crate) struct UiState {
    /// Active route (activity-panel content). Orthogonal to `job` and `overlay`.
    pub screen: Screen,
    /// A modal (prompt/confirm/exit) drawn over the route, if any.
    pub overlay: Option<Overlay>,
    /// A wallet command in progress, if any — shown as a spinner over the current route.
    pub job: Option<RunningJob>,
    pub toast: Option<String>,
    pub sync_progress: Option<watch::Receiver<(usize, usize)>>,
    /// Terminal text rendered from [`Self::last_command_events`] for the output panel.
    pub last_command_output: String,
    /// Green ✅ success header for the output panel (set only on a command success *with* output;
    /// `None` for info text like help/curl). Folds the success confirmation into the one panel.
    pub last_command_status: Option<String>,
    /// Structured kernel events from the last wallet command (data layer).
    pub last_command_events: Vec<nockchain_wallet::wallet_outcome::WalletEvent>,
    /// Vertical scroll (wrapped lines) for the status/output panel.
    pub output_scroll: u16,
    /// Scroll position for menu [`List`](ratatui::widgets::List) widgets (long menus).
    pub list_state: ListState,
    pub balance_panel: BalancePanelState,
    /// Home wallet (master-address) dropdown picker.
    pub master_picker: MasterPickerState,
    /// Bumped when starting a sidebar balance fetch or any queued wallet command.
    pub balance_job_nonce: u64,
    pub ui_fx: UiFx,
    /// `0` = Wallet tab, `1` = Menu tab on [`Screen::Home`].
    pub home_tab: usize,
    /// Selected row on the Menu tab (`MAIN_MENU`).
    pub menu_sel: usize,
    pub price: PriceState,
}

/// Backwards-compatible alias during migration to [`UIStore`](super::store::UIStore).
pub(crate) type AppState = UiState;

impl UiState {
    pub fn new(screen: Screen) -> Self {
        Self {
            screen,
            overlay: None,
            job: None,
            toast: None,
            sync_progress: None,
            last_command_output: String::new(),
            last_command_status: None,
            last_command_events: Vec::new(),
            output_scroll: 0,
            list_state: ListState::default(),
            balance_panel: BalancePanelState::default(),
            master_picker: MasterPickerState::default(),
            balance_job_nonce: 0,
            ui_fx: UiFx::default(),
            home_tab: 0,
            menu_sel: 0,
            price: PriceState::default(),
        }
    }
}
