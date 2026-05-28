//! NNS buy screen keyboard handling.

use crossterm::event::{KeyCode, KeyEvent};
use nockapp::NockAppError;
use tokio::sync::mpsc;

use super::input::esc_back;
use super::replace_screen;
use crate::command_runner::{
    schedule_nns_lookup, schedule_nns_register, JobCompletion, NnsLookupCompletion, TuiRuntime,
};
use crate::nns;
use crate::screens::{NnsBuyFocus, Screen, TuiControl};
use crate::store::UIStore;

pub(crate) fn apply_nns_lookup_result(store: &mut UIStore, result: NnsLookupCompletion) {
    let Screen::NnsBuy {
        value,
        cursor,
        focus,
        owned_names,
        ..
    } = store.state.screen.clone()
    else {
        return;
    };

    match result {
        Ok(ok) => {
            let status = nns::availability_message(&ok, store.state.price.usd_per_coin);
            store.dispatch(crate::store::UiAction::ReplaceScreen(make_nns_buy(
                value,
                cursor,
                focus,
                Some(status),
                false,
                Some(ok.canonical_name),
                owned_names,
                false,
            )));
        }
        Err(msg) => {
            store.dispatch(crate::store::UiAction::ReplaceScreen(make_nns_buy(
                value,
                cursor,
                focus,
                Some(format!("Error: {msg}")),
                false,
                None,
                owned_names,
                false,
            )));
        }
    }
}

pub(super) async fn handle_nns_buy(
    store: &mut UIStore,
    key: KeyEvent,
    rt: &TuiRuntime,
    done_tx: &mpsc::UnboundedSender<JobCompletion>,
    lookup_done_tx: &mpsc::UnboundedSender<NnsLookupCompletion>,
) -> Result<TuiControl, NockAppError> {
    let taken = store.state.screen.clone();
    let Screen::NnsBuy {
        mut value,
        mut cursor,
        mut focus,
        mut status,
        lookup_busy,
        verified_name,
        owned_names,
        owned_names_loading: _,
    } = taken
    else {
        return Ok(TuiControl::Continue);
    };

    if lookup_busy {
        return Ok(TuiControl::Continue);
    }

    if esc_back(key.code) {
        replace_screen(store, Screen::Home);
        return Ok(TuiControl::Continue);
    }

    let mut clear_verify = false;

    match key.code {
        KeyCode::Tab => focus = next_focus(focus),
        KeyCode::BackTab => focus = prev_focus(focus),
        KeyCode::Left if matches!(focus, NnsBuyFocus::Cancel | NnsBuyFocus::Register) => {
            focus = NnsBuyFocus::Cancel;
        }
        KeyCode::Right if matches!(focus, NnsBuyFocus::Cancel | NnsBuyFocus::Register) => {
            focus = NnsBuyFocus::Register;
        }
        KeyCode::Up | KeyCode::Down => {
            focus = if key.code == KeyCode::Up {
                prev_focus(focus)
            } else {
                next_focus(focus)
            };
            clear_verify = false;
        }
        KeyCode::Enter => match focus {
            NnsBuyFocus::Search => {
                start_lookup(store, &value, lookup_done_tx);
                return Ok(TuiControl::Continue);
            }
            NnsBuyFocus::Cancel => {
                replace_screen(store, Screen::Home);
                return Ok(TuiControl::Continue);
            }
            NnsBuyFocus::Register => {
                let canonical = match nns::normalize_nns_name(&value) {
                    Ok(n) => n,
                    Err(e) => {
                        status = Some(format!("Error: {e}"));
                        replace_screen(
                            store,
                            make_nns_buy(value, cursor, focus, status, false, None, owned_names, false),
                        );
                        return Ok(TuiControl::Continue);
                    }
                };
                if verified_name.as_deref() != Some(canonical.as_str()) {
                    status =
                        Some("Error: Search for the name first to confirm it is available".into());
                } else {
                    replace_screen(
                        store,
                        make_nns_buy(
                            value.clone(),
                            cursor,
                            focus,
                            None,
                            true,
                            Some(canonical.clone()),
                            owned_names.clone(),
                            false,
                        ),
                    );
                    if let Err(e) = schedule_nns_register(store, rt, done_tx.clone(), &canonical) {
                        replace_screen(
                            store,
                            make_nns_buy(
                                value,
                                cursor,
                                focus,
                                Some(format!("Error: {e}")),
                                false,
                                Some(canonical),
                                owned_names,
                                false,
                            ),
                        );
                    }
                    return Ok(TuiControl::Continue);
                }
            }
            NnsBuyFocus::Name => {
                start_lookup(store, &value, lookup_done_tx);
                return Ok(TuiControl::Continue);
            }
        },
        KeyCode::Char(c) if focus == NnsBuyFocus::Name => {
            insert_char(&mut value, &mut cursor, c);
            clear_verify = true;
        }
        KeyCode::Backspace if focus == NnsBuyFocus::Name => {
            delete_char(&mut value, &mut cursor);
            clear_verify = true;
        }
        KeyCode::Left if focus == NnsBuyFocus::Name => {
            cursor = cursor.saturating_sub(1);
        }
        KeyCode::Right if focus == NnsBuyFocus::Name => {
            cursor = (cursor + 1).min(value.chars().count());
        }
        _ => {}
    }

    replace_screen(
        store,
        make_nns_buy(
            value,
            cursor,
            focus,
            if clear_verify { None } else { status },
            false,
            if clear_verify { None } else { verified_name },
            owned_names,
            false,
        ),
    );
    Ok(TuiControl::Continue)
}

fn start_lookup(
    store: &mut UIStore,
    value: &str,
    lookup_done_tx: &mpsc::UnboundedSender<NnsLookupCompletion>,
) {
    let Screen::NnsBuy {
        cursor,
        focus,
        owned_names,
        ..
    } = store.state.screen.clone()
    else {
        return;
    };
    replace_screen(
        store,
        make_nns_buy(
            value.to_string(),
            cursor,
            focus,
            None,
            true,
            None,
            owned_names,
            false,
        ),
    );
    schedule_nns_lookup(value.to_string(), lookup_done_tx.clone());
}

/// Centralized constructor so adding fields only touches one place.
fn make_nns_buy(
    value: String,
    cursor: usize,
    focus: NnsBuyFocus,
    status: Option<String>,
    lookup_busy: bool,
    verified_name: Option<String>,
    owned_names: Vec<String>,
    owned_names_loading: bool,
) -> Screen {
    Screen::NnsBuy {
        value,
        cursor,
        focus,
        status,
        lookup_busy,
        verified_name,
        owned_names,
        owned_names_loading,
    }
}

fn next_focus(f: NnsBuyFocus) -> NnsBuyFocus {
    match f {
        NnsBuyFocus::Name => NnsBuyFocus::Search,
        NnsBuyFocus::Search => NnsBuyFocus::Cancel,
        NnsBuyFocus::Cancel => NnsBuyFocus::Register,
        NnsBuyFocus::Register => NnsBuyFocus::Name,
    }
}

fn prev_focus(f: NnsBuyFocus) -> NnsBuyFocus {
    match f {
        NnsBuyFocus::Name => NnsBuyFocus::Register,
        NnsBuyFocus::Search => NnsBuyFocus::Name,
        NnsBuyFocus::Cancel => NnsBuyFocus::Search,
        NnsBuyFocus::Register => NnsBuyFocus::Cancel,
    }
}

fn insert_char(line: &mut String, cursor: &mut usize, c: char) {
    if c.is_ascii_uppercase() {
        return;
    }
    let byte_index = line
        .char_indices()
        .map(|(i, _)| i)
        .nth(*cursor)
        .unwrap_or(line.len());
    line.insert(byte_index, c.to_ascii_lowercase());
    *cursor = cursor.saturating_add(1);
}

fn delete_char(line: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let from = cursor.saturating_sub(1);
    let before = line.chars().take(from);
    let after = line.chars().skip(*cursor);
    *line = before.chain(after).collect();
    *cursor = from;
}
