//! Home screen: tabs, wallet CTAs, menu list.

use crossterm::event::{KeyCode, KeyEvent};
use nockapp::NockAppError;

use super::input::{esc_back, list_activate};
use super::{replace_screen, schedule_cmd};
use crate::command_runner::TuiRuntime;
use crate::components::home::cta_key_to_index;
use crate::components::menus::MAIN_MENU;
use crate::msg::Msg;
use crate::screens::{Screen, TuiControl};
use crate::store::{UIStore, UiAction};
use nockchain_wallet::command::Commands;
use tokio::sync::mpsc;

pub(super) async fn handle_home(
    store: &mut UIStore,
    key: KeyEvent,
    rt: &TuiRuntime,
    msg_tx: &mpsc::UnboundedSender<Msg>,
) -> Result<TuiControl, NockAppError> {
    // The wallet dropdown, while open, captures navigation keys before anything else.
    if store.state.master_picker.open {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                store.dispatch(UiAction::MoveMasterPickerSel { delta: -1 })
            }
            KeyCode::Down | KeyCode::Char('j') => {
                store.dispatch(UiAction::MoveMasterPickerSel { delta: 1 })
            }
            KeyCode::Esc => store.dispatch(UiAction::CloseMasterPicker),
            KeyCode::Enter => select_active_master(store, rt, msg_tx),
            _ => {}
        }
        return Ok(TuiControl::Continue);
    }
    // `w` opens the wallet picker (wallet tab only, and only when there's more than one wallet).
    if store.state.home_tab == 0
        && store.state.master_picker.has_choice()
        && matches!(key.code, KeyCode::Char('w'))
    {
        store.dispatch(UiAction::ToggleMasterPicker);
        return Ok(TuiControl::Continue);
    }

    match key.code {
        KeyCode::Left | KeyCode::Char('h') => {
            store.dispatch(UiAction::HomeTabPrev);
            return Ok(TuiControl::Continue);
        }
        KeyCode::Right | KeyCode::Char('l') => {
            store.dispatch(UiAction::HomeTabNext);
            return Ok(TuiControl::Continue);
        }
        KeyCode::Char('1') => {
            store.dispatch(UiAction::SetHomeTab(0));
            return Ok(TuiControl::Continue);
        }
        KeyCode::Char('2') => {
            store.dispatch(UiAction::SetHomeTab(1));
            return Ok(TuiControl::Continue);
        }
        KeyCode::Char('r') if store.state.home_tab == 0 => {
            super::super::command_runner::schedule_price_fetch(store, msg_tx);
            replace_screen(store, Screen::receive_new(true));
            schedule_cmd(
                store,
                rt,
                msg_tx,
                Commands::ListActiveAddresses,
                "ListActiveAddresses",
            );
            return Ok(TuiControl::Continue);
        }
        _ => {}
    }

    if store.state.home_tab == 0 {
        if let KeyCode::Char(c) = key.code {
            if esc_back(key.code) {
                return Ok(TuiControl::Quit);
            }
            match cta_key_to_index(c) {
                Some(0) => {
                    replace_screen(store, crate::send_simple::new_screen());
                    return Ok(TuiControl::Continue);
                }
                Some(1) => {
                    super::super::command_runner::schedule_price_fetch(store, msg_tx);
                    replace_screen(store, Screen::receive_new(true));
                    schedule_cmd(
                        store,
                        rt,
                        msg_tx,
                        Commands::ListActiveAddresses,
                        "ListActiveAddresses",
                    );
                    return Ok(TuiControl::Continue);
                }
                Some(2) => {
                    replace_screen(store, Screen::nns_buy_new());
                    return Ok(TuiControl::Continue);
                }
                _ => {}
            }
        }
        if key.code == KeyCode::Char('r') {
            return Ok(TuiControl::Continue);
        }
        if esc_back(key.code) {
            return Ok(TuiControl::Quit);
        }
        return Ok(TuiControl::Continue);
    }

    // Menu tab
    let mut sel = store.state.menu_sel;
    match list_activate(&mut sel, MAIN_MENU.len(), key.code) {
        Err(()) => {
            store.dispatch(UiAction::SetMenuSel(sel));
            Ok(TuiControl::Continue)
        }
        Ok(None) => {
            if key.code == KeyCode::Esc {
                store.dispatch(UiAction::SetHomeTab(0));
                return Ok(TuiControl::Continue);
            }
            if esc_back(key.code) {
                return Ok(TuiControl::Quit);
            }
            store.dispatch(UiAction::SetMenuSel(sel));
            Ok(TuiControl::Continue)
        }
        Ok(Some(i)) => {
            store.dispatch(UiAction::SetMenuSel(sel));
            super::menus::navigate_main_menu_item(store, i);
            Ok(TuiControl::Continue)
        }
    }
}

/// Confirm the highlighted wallet in the dropdown: if it isn't already active, switch to it via
/// `set-active-master-address`. The completion refreshes the home view (see the `Msg::Job` handler).
fn select_active_master(store: &mut UIStore, rt: &TuiRuntime, msg_tx: &mpsc::UnboundedSender<Msg>) {
    let chosen = store
        .state
        .master_picker
        .addresses
        .get(store.state.master_picker.sel)
        .cloned();
    store.dispatch(UiAction::CloseMasterPicker);
    if let Some(row) = chosen {
        if !row.active {
            schedule_cmd(
                store,
                rt,
                msg_tx,
                Commands::SetActiveMasterAddress {
                    address_b58: row.address_b58,
                },
                "Switch wallet",
            );
        }
    }
}
