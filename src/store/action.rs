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
    /// Home balance refresh (receiver only; sender held by spawned task).
    BeginBalanceSidebarFetch {
        progress_rx: watch::Receiver<(usize, usize)>,
    },
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
    BeginPriceFetch,
    PriceFetched {
        usd_per_coin: f64,
    },
    PriceFetchFailed {
        msg: String,
    },
}
