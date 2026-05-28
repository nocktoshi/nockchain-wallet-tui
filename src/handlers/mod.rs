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
use super::command_runner::{
    BalanceRefreshCompletion, JobCompletion, NnsLookupCompletion,
    OwnedNnsNamesCompletion, TuiRuntime, SendSimplePlanCompletion,
};
use super::ct_dispatch;
use super::hooks::terminal::Term;
use super::prompt_overlay::has_prompt_overlay;
use super::screens::{TuiControl, Screen};
use super::store::{UIStore, UiAction};
use nockchain_wallet::command::Commands;

fn schedule_cmd(
    store: &mut UIStore,
    rt: &TuiRuntime,
    done_tx: &mpsc::UnboundedSender<JobCompletion>,
    cmd: Commands,
    label: &'static str,
) {
    super::command_runner::schedule_wallet_command(store, rt, done_tx.clone(), cmd, label);
}

pub(crate) fn apply_send_simple_plan_result(
    store: &mut UIStore,
    result: SendSimplePlanCompletion,
) {
    send_simple::apply_send_simple_plan_result(store, result);
}

pub(crate) fn apply_nns_lookup_result(store: &mut UIStore, result: NnsLookupCompletion) {
    nns_buy::apply_nns_lookup_result(store, result);
}

/// Route screen transitions through [`super::store::apply_ui_action`].
pub(super) fn replace_screen(store: &mut UIStore, screen: Screen) {
    store.dispatch(UiAction::ReplaceScreen(screen));
}

pub(super) async fn dispatch_key(
    rt: &TuiRuntime,
    store: &mut UIStore,
    key: KeyEvent,
    terminal: &Arc<Mutex<Term>>,
    done_tx: &mpsc::UnboundedSender<JobCompletion>,
    balance_done_tx: &mpsc::UnboundedSender<BalanceRefreshCompletion>,
    price_done_tx: &mpsc::UnboundedSender<Result<f64, String>>,
    plan_done_tx: &mpsc::UnboundedSender<SendSimplePlanCompletion>,
    nns_lookup_done_tx: &mpsc::UnboundedSender<NnsLookupCompletion>,
    owned_nns_names_done_tx: &mpsc::UnboundedSender<OwnedNnsNamesCompletion>,
) -> Result<TuiControl, NockAppError> {
    if key.kind == KeyEventKind::Release {
        return Ok(TuiControl::Continue);
    }
    if store.state.toast.is_some() {
        store.dispatch(UiAction::TakeToast);
        return Ok(TuiControl::Continue);
    }
    if matches!(store.state.screen, Screen::Running { .. }) {
        return Ok(TuiControl::Continue);
    }
    if matches!(store.state.screen, Screen::Splash) {
        store.dispatch(UiAction::EnterMainFromSplash);
        super::command_runner::schedule_balance_sidebar_refresh(store, rt, balance_done_tx);
        super::command_runner::schedule_price_fetch(store, price_done_tx);
        return Ok(TuiControl::Continue);
    }
    if status_modal_visible(&store.state)
        && !has_prompt_overlay(&store.state.screen)
        && !matches!(store.state.screen, Screen::Running { .. })
    {
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
    let result = match &mut store.state.screen {
        Screen::Splash => Ok(TuiControl::Continue),
        Screen::Home => {
            home::handle_home(store, key, rt, done_tx, balance_done_tx, price_done_tx).await
        }
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
                        super::command_runner::schedule_nns_verified_names(
                            addr,
                            owned_nns_names_done_tx.clone(),
                        );
                    }
                }
            }
            nns_buy::handle_nns_buy(store, key, rt, done_tx, nns_lookup_done_tx).await
        }
        Screen::SendSimple { .. } => {
            send_simple::handle_send_simple(store, key, rt, done_tx, plan_done_tx).await
        }
        Screen::Keys { .. } => menus::handle_keys(store, key, rt, terminal, done_tx).await,
        Screen::KeysImport { .. } => menus::handle_keys_import(store, key).await,
        Screen::Notes { .. } => menus::handle_notes(store, key, rt, terminal, done_tx).await,
        Screen::Transactions { .. } => menus::handle_transactions(store, key).await,
        Screen::Watch { .. } => menus::handle_watch(store, key).await,
        Screen::SignVerify { .. } => menus::handle_sign(store, key).await,
        Screen::Settings { .. } => menus::handle_settings(store, key, rt),
        Screen::Quick { .. } => menus::handle_quick(store, key),
        Screen::TextPrompt { .. } => {
            prompts::text_prompt(store, key, rt, terminal, done_tx).await
        }
        Screen::Confirm { .. } => {
            prompts::confirm_prompt(store, key, rt, terminal, done_tx).await
        }
        Screen::CreateTx { .. } => {
            ct_dispatch::handle_create_tx(store, key, rt, done_tx).await
        }
        Screen::ExitConfirm { .. } => menus::handle_exit_confirm(store, key),
        Screen::ErrorScreen { .. } => {
            error::error_screen(store, key, rt, terminal, done_tx).await
        }
        Screen::Running { .. } => Ok(TuiControl::Continue),
    };

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
                super::command_runner::schedule_nns_verified_names(
                    addr,
                    owned_nns_names_done_tx.clone(),
                );
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
    balance_done_tx: &mpsc::UnboundedSender<BalanceRefreshCompletion>,
    price_done_tx: &mpsc::UnboundedSender<Result<f64, String>>,
) -> Result<TuiControl, NockAppError> {
    if matches!(store.state.screen, Screen::Splash) {
        store.dispatch(UiAction::EnterMainFromSplash);
        super::command_runner::schedule_balance_sidebar_refresh(store, rt, balance_done_tx);
        super::command_runner::schedule_price_fetch(store, price_done_tx);
        return Ok(TuiControl::Continue);
    }
    match &mut store.state.screen {
        Screen::TextPrompt { value, then, .. } => {
            if super::paste::text_prompt_allows_multiline(then) {
                super::paste::paste_multiline(value, &pasted);
            } else {
                super::paste::paste_single_line(value, &pasted);
            }
            Ok(TuiControl::Continue)
        }
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
