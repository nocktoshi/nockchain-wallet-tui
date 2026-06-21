//! Keyboard dispatch for the wallet TUI TUI.

mod error;
mod home;
mod input;
mod menus;
mod nns_buy;
mod prompts;
mod receive;
mod send_simple;

use std::sync::Arc;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind};
use input::try_output_scroll_keys;
use nockapp::NockAppError;
use tokio::sync::{mpsc, Mutex};

use super::app_state::status_modal_visible;
use super::command_runner::{NnsLookupCompletion, SendSimplePlanCompletion, TuiRuntime};
use super::ct_dispatch;
use super::hooks::terminal::Term;
use super::screens::{Overlay, Screen, TuiControl};
use super::store::{UIStore, UiAction};
use crate::msg::Msg;
use nockchain_wallet::command::Commands;

fn schedule_cmd(
    store: &mut UIStore,
    rt: &TuiRuntime,
    msg_tx: &mpsc::UnboundedSender<Msg>,
    cmd: Commands,
    label: &'static str,
) {
    super::command_runner::schedule_wallet_command(store, rt, msg_tx.clone(), cmd, label);
}

/// Current selection index for any catalog-driven list menu.
fn menu_sel(screen: &Screen) -> usize {
    match screen {
        Screen::Keys { sel }
        | Screen::KeysImport { sel }
        | Screen::Notes { sel }
        | Screen::Transactions { sel }
        | Screen::Watch { sel }
        | Screen::SignVerify { sel } => *sel,
        _ => 0,
    }
}

pub(crate) fn apply_send_simple_plan_result(store: &mut UIStore, result: SendSimplePlanCompletion) {
    send_simple::apply_send_simple_plan_result(store, result);
}

pub(crate) fn apply_nns_lookup_result(store: &mut UIStore, result: NnsLookupCompletion) {
    nns_buy::apply_nns_lookup_result(store, result);
}

/// Route screen transitions through [`super::store::apply_ui_action`].
pub(super) fn replace_screen(store: &mut UIStore, screen: Screen) {
    store.dispatch(UiAction::ReplaceScreen(screen));
}

/// Open (`Some`) or close (`None`) the modal overlay over the current route.
pub(super) fn set_overlay(store: &mut UIStore, overlay: Option<Overlay>) {
    store.state.overlay = overlay;
}

pub(super) async fn dispatch_key(
    rt: &TuiRuntime,
    store: &mut UIStore,
    key: KeyEvent,
    terminal: &Arc<Mutex<Term>>,
    msg_tx: &mpsc::UnboundedSender<Msg>,
) -> Result<TuiControl, NockAppError> {
    if key.kind == KeyEventKind::Release {
        return Ok(TuiControl::Continue);
    }
    if store.state.toast.is_some() {
        store.dispatch(UiAction::TakeToast);
        return Ok(TuiControl::Continue);
    }
    if store.state.job.is_some() {
        return Ok(TuiControl::Continue);
    }
    if matches!(store.state.screen, Screen::Splash) {
        store.dispatch(UiAction::EnterMainFromSplash);
        super::command_runner::schedule_balance_sidebar_refresh(store, rt, msg_tx);
        super::command_runner::schedule_price_fetch(store, msg_tx);
        return Ok(TuiControl::Continue);
    }
    // A modal overlay intercepts all keys before the route handlers.
    match &store.state.overlay {
        Some(Overlay::Prompt { .. }) => {
            return prompts::text_prompt(store, key, rt, terminal, msg_tx).await;
        }
        Some(Overlay::Confirm { .. }) => {
            return prompts::confirm_prompt(store, key, rt, terminal, msg_tx).await;
        }
        Some(Overlay::ExitConfirm { .. }) => {
            return menus::handle_exit_confirm(store, key);
        }
        None => {}
    }
    if status_modal_visible(&store.state) {
        if key.code == KeyCode::Enter {
            store.dispatch(UiAction::DismissStatusOutput);
            return Ok(TuiControl::Continue);
        }
        if try_output_scroll_keys(store, key) {
            return Ok(TuiControl::Continue);
        }
        if !matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q')
        ) {
            return Ok(TuiControl::Continue);
        }
    }
    // Returning to home from any other route should re-derive the wallet view (balance cascades to
    // identity + master-address picker via the Msg::Balance handler).
    let was_home = matches!(store.state.screen, Screen::Home);

    let result = match &mut store.state.screen {
        Screen::Splash => Ok(TuiControl::Continue),
        Screen::Home => home::handle_home(store, key, rt, msg_tx).await,
        Screen::Receive { .. } => receive::handle_receive(store, key).await,
        Screen::NnsBuy { .. } => {
            // Trigger owned names fetch if we haven't loaded them yet
            if let Screen::NnsBuy {
                owned_names,
                owned_names_loading,
                ..
            } = &mut store.state.screen
            {
                if owned_names.is_empty() && !*owned_names_loading {
                    if let Some(addr) = store.state.balance_panel.address.clone() {
                        *owned_names_loading = true;
                        super::command_runner::schedule_nns_verified_names(addr, msg_tx.clone());
                    }
                }
            }
            nns_buy::handle_nns_buy(store, key, rt, msg_tx).await
        }
        Screen::SendSimple { .. } => send_simple::handle_send_simple(store, key, rt, msg_tx).await,
        Screen::Keys { .. } => {
            let s = menu_sel(&store.state.screen);
            Ok(menus::run_menu(
                store,
                rt,
                msg_tx,
                key,
                s,
                crate::actions::KEYS_ITEMS,
                |sel| Screen::Keys { sel },
                Screen::Home,
            ))
        }
        Screen::KeysImport { .. } => {
            let s = menu_sel(&store.state.screen);
            Ok(menus::run_menu(
                store,
                rt,
                msg_tx,
                key,
                s,
                crate::actions::KEYS_IMPORT_ITEMS,
                |sel| Screen::KeysImport { sel },
                Screen::Keys { sel: 2 },
            ))
        }
        Screen::Notes { .. } => {
            let s = menu_sel(&store.state.screen);
            Ok(menus::run_menu(
                store,
                rt,
                msg_tx,
                key,
                s,
                crate::actions::NOTES_ITEMS,
                |sel| Screen::Notes { sel },
                Screen::Home,
            ))
        }
        Screen::Transactions { .. } => {
            let s = menu_sel(&store.state.screen);
            Ok(menus::run_menu(
                store,
                rt,
                msg_tx,
                key,
                s,
                crate::actions::TX_ITEMS,
                |sel| Screen::Transactions { sel },
                Screen::Home,
            ))
        }
        Screen::Watch { .. } => {
            let s = menu_sel(&store.state.screen);
            Ok(menus::run_menu(
                store,
                rt,
                msg_tx,
                key,
                s,
                crate::actions::WATCH_ITEMS,
                |sel| Screen::Watch { sel },
                Screen::Home,
            ))
        }
        Screen::SignVerify { .. } => {
            let s = menu_sel(&store.state.screen);
            Ok(menus::run_menu(
                store,
                rt,
                msg_tx,
                key,
                s,
                crate::actions::SIGN_ITEMS,
                |sel| Screen::SignVerify { sel },
                Screen::Home,
            ))
        }
        Screen::Settings { .. } => menus::handle_settings(store, key, rt),
        Screen::Quick { .. } => menus::handle_quick(store, key, rt, msg_tx),
        Screen::CreateTx { .. } => ct_dispatch::handle_create_tx(store, key, rt, msg_tx).await,
        Screen::ErrorScreen { .. } => error::error_screen(store, key, rt, terminal, msg_tx).await,
    };

    // Just returned to home from elsewhere: refresh the balance (which cascades to the identity and
    // master-address picker). Guards inside the scheduler drop redundant/in-flight fetches.
    if !was_home && matches!(store.state.screen, Screen::Home) {
        super::command_runner::schedule_balance_sidebar_refresh(store, rt, msg_tx);
    }

    // After any handler (including menu navigation), if we are now on the NNS buy screen
    // with no owned names loaded yet, start the fetch immediately so the loading indicator
    // appears without waiting for another key event.
    if let Screen::NnsBuy {
        owned_names,
        owned_names_loading,
        ..
    } = &mut store.state.screen
    {
        if owned_names.is_empty() && !*owned_names_loading {
            if let Some(addr) = store.state.balance_panel.address.clone() {
                *owned_names_loading = true;
                super::command_runner::schedule_nns_verified_names(addr, msg_tx.clone());
            }
        }
    }

    result
}

/// Insert bracketed-paste clipboard text into the focused field.
pub(super) async fn dispatch_paste(
    _connection: &nockchain_wallet::ConnectionCli,
    store: &mut UIStore,
    pasted: String,
    rt: &TuiRuntime,
    msg_tx: &mpsc::UnboundedSender<Msg>,
) -> Result<TuiControl, NockAppError> {
    if matches!(store.state.screen, Screen::Splash) {
        store.dispatch(UiAction::EnterMainFromSplash);
        super::command_runner::schedule_balance_sidebar_refresh(store, rt, msg_tx);
        super::command_runner::schedule_price_fetch(store, msg_tx);
        return Ok(TuiControl::Continue);
    }
    if let Some(Overlay::Prompt { value, then, .. }) = &mut store.state.overlay {
        if super::paste::text_prompt_allows_multiline(then) {
            super::paste::paste_multiline(value, &pasted);
        } else {
            super::paste::paste_single_line(value, &pasted);
        }
        return Ok(TuiControl::Continue);
    }
    match &mut store.state.screen {
        Screen::Quick { line } => {
            super::paste::paste_single_line(line, &pasted);
            Ok(TuiControl::Continue)
        }
        Screen::CreateTx { w } => {
            ct_dispatch::apply_paste_to_wizard(w, &pasted);
            Ok(TuiControl::Continue)
        }
        Screen::NnsBuy {
            value,
            cursor,
            focus,
            verified_name,
            ..
        } => {
            if matches!(focus, crate::screens::NnsBuyFocus::Name) {
                super::paste::paste_single_line(value, &pasted);
                *cursor = value.chars().count();
                *verified_name = None;
            }
            Ok(TuiControl::Continue)
        }
        Screen::SendSimple {
            amount,
            recipient,
            focus,
            ..
        } => {
            match focus {
                crate::screens::SendSimpleFocus::Amount => {
                    super::paste::paste_single_line(amount, &pasted);
                }
                crate::screens::SendSimpleFocus::Recipient => {
                    super::paste::paste_single_line(recipient, &pasted);
                }
                _ => {}
            }
            Ok(TuiControl::Continue)
        }
        _ => Ok(TuiControl::Continue),
    }
}
