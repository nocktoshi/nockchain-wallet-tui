//! Redux-style actions for the wallet TUI UI.

use tokio::sync::watch;

use crate::screens::Screen;
use nockchain_wallet::command::Commands;

/// All UI state transitions flow through [`super::apply_ui_action`](fn@super::apply_ui_action).
#[derive(Debug)]
pub(crate) enum UiAction {
    /// Advance spinner / animation clock (presentation-only).
    Tick,
    /// Dismiss toast on any key (consumes toast field).
    TakeToast,
    /// Clear command output and hide the status modal.
    DismissStatusOutput,
    /// Full screen swap (routes through [`super::apply_ui_action`]).
    ReplaceScreen(Screen),
    /// Leave splash for home + activity focus (balance refresh scheduled by caller).
    EnterMainFromSplash,
    /// Swap to [`Screen::Running`]. `progress_rx` is `None` for HTTP-routed commands (no granular
    /// sync progress over the API boundary; spinner only) and `Some` for in-process flows.
    EnterRunningWalletJob {
        cmd: Commands,
        label: String,
        progress_rx: Option<watch::Receiver<(usize, usize)>>,
    },
    /// Home balance refresh start. Routed through the HTTP API, so there is no in-process sync
    /// progress channel — the hero shows a plain spinner.
    BeginBalanceSidebarFetch,
    JobCompleted {
        result: Result<(), String>,
        events: Vec<nockchain_wallet::wallet_outcome::WalletEvent>,
        /// Rendered report text for the output panel (normalized; never raw kernel markdown).
        output: String,
    },
    BalanceSidebarCompleted {
        nonce: u64,
        result: Result<(), nockapp::NockAppError>,
        events: Vec<nockchain_wallet::wallet_outcome::WalletEvent>,
    },
    BeginHomeIdentityFetch,
    HomeIdentityCompleted {
        address: Option<String>,
        nockname: Option<String>,
    },
    NudgeOutputScroll {
        delta: i32,
    },
    NnsOwnedNamesLoaded {
        names: Vec<String>,
    },
    SetOutputScroll(u16),
    SetHomeTab(usize),
    HomeTabNext,
    HomeTabPrev,
    SetMenuSel(usize),
    /// A `list-master-addresses` fetch for the home wallet picker started.
    BeginMasterAddressesFetch,
    /// Master addresses loaded for the home wallet picker.
    MasterAddressesLoaded {
        rows: Vec<crate::wallet_api::MasterAddressRow>,
    },
    /// Expand/collapse the home wallet dropdown (resets the highlight to the active row on open).
    ToggleMasterPicker,
    /// Move the highlighted row in the open wallet dropdown.
    MoveMasterPickerSel {
        delta: i32,
    },
    /// Collapse the home wallet dropdown without selecting.
    CloseMasterPicker,
    BeginPriceFetch,
    PriceFetched {
        usd_per_coin: f64,
    },
    PriceFetchFailed {
        msg: String,
    },
}
