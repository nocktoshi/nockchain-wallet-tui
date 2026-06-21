//! Menu routing: one generic `run_menu` over the declarative [`crate::actions`] catalog, plus the
//! bespoke settings / quick-command / exit-confirm handlers.

use crossterm::event::{KeyCode, KeyEvent};
use nockapp::NockAppError;
use tokio::sync::mpsc;
use tracing::warn;

use super::input::{edit_line, esc_back, list_activate};
use crate::actions::{self, MenuAction, MenuItem};
use crate::command_runner::{JobCompletion, TuiRuntime};
use crate::components::menus::{BOOL, SETTINGS_MENU};
use crate::hooks::logging::{log_help, log_verbose_info};
use crate::screens::{Screen, TextThen, TuiControl};
use crate::session::current_api_listen;
use crate::session_client::api_base_url;
use crate::store::UIStore;
use crate::{normalize_slash_cmd, session};

/// Navigate from the home Menu tab (`MAIN_MENU` index).
pub(super) fn navigate_main_menu_item(store: &mut UIStore, i: usize) {
    let next = match i {
        0 => Screen::Keys { sel: 0 },
        1 => Screen::Notes { sel: 0 },
        2 => Screen::Transactions { sel: 0 },
        3 => Screen::Watch { sel: 0 },
        4 => Screen::SignVerify { sel: 0 },
        5 => Screen::Settings { sel: 0 },
        6 => Screen::Quick {
            line: String::new(),
        },
        7 => crate::prompt_overlay::exit_confirm(Screen::Home, 1),
        _ => Screen::Home,
    };
    super::replace_screen(store, next);
}

/// Drive any list menu defined in [`crate::actions`]: `rebuild` reconstructs the current
/// [`Screen`] variant at a new selection; `back` is where Esc / the trailing item returns.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_menu(
    store: &mut UIStore,
    rt: &TuiRuntime,
    done_tx: &mpsc::UnboundedSender<JobCompletion>,
    key: KeyEvent,
    cur_sel: usize,
    items: &[MenuItem],
    rebuild: fn(usize) -> Screen,
    back: Screen,
) -> TuiControl {
    if esc_back(key.code) {
        super::replace_screen(store, back);
        return TuiControl::Continue;
    }
    let mut sel = cur_sel;
    match list_activate(&mut sel, items.len(), key.code) {
        Err(()) | Ok(None) => super::replace_screen(store, rebuild(sel)),
        Ok(Some(i)) => apply_menu_action(store, rt, done_tx, &items[i].action, rebuild, sel),
    }
    TuiControl::Continue
}

fn apply_menu_action(
    store: &mut UIStore,
    rt: &TuiRuntime,
    done_tx: &mpsc::UnboundedSender<JobCompletion>,
    action: &MenuAction,
    rebuild: fn(usize) -> Screen,
    sel: usize,
) {
    match action {
        MenuAction::Run(cmd) => {
            // Keep the menu position so the command's `restore` screen returns here.
            super::replace_screen(store, rebuild(sel));
            super::schedule_cmd(store, rt, done_tx, cmd.clone(), actions::command_label(cmd));
        }
        MenuAction::Prompt { title, then } => {
            let underlay = crate::prompt_overlay::prompt_underlay(&rebuild(sel));
            super::replace_screen(
                store,
                crate::prompt_overlay::text_prompt_screen(
                    underlay,
                    *title,
                    String::new(),
                    then.clone(),
                ),
            );
        }
        MenuAction::Confirm {
            title,
            sel: csel,
            labels,
            then,
        } => {
            let underlay = crate::prompt_overlay::prompt_underlay(&rebuild(sel));
            super::replace_screen(
                store,
                crate::prompt_overlay::confirm_prompt_screen(
                    underlay,
                    *title,
                    *csel,
                    labels,
                    then.clone(),
                ),
            );
        }
        MenuAction::Goto(target) => super::replace_screen(store, target.build()),
    }
}

pub(super) fn handle_settings(
    store: &mut UIStore,
    key: KeyEvent,
    rt: &TuiRuntime,
) -> Result<TuiControl, NockAppError> {
    let taken = store.state.screen.clone();
    super::replace_screen(store, Screen::Settings { sel: 0 });
    match taken {
        Screen::Settings { mut sel } => {
            if esc_back(key.code) {
                super::replace_screen(store, Screen::Home);
                return Ok(TuiControl::Continue);
            }
            match list_activate(&mut sel, SETTINGS_MENU.len(), key.code) {
                Err(()) => {
                    super::replace_screen(store, Screen::Settings { sel });
                    Ok(TuiControl::Continue)
                }
                Ok(None) => {
                    super::replace_screen(store, Screen::Settings { sel });
                    Ok(TuiControl::Continue)
                }
                Ok(Some(i)) => {
                    match i {
                        0 => {
                            let session = session::session_config_snapshot(rt);
                            super::replace_screen(
                                store,
                                crate::prompt_overlay::text_prompt_screen(
                                    crate::prompt_overlay::prompt_underlay(&taken),
                                    "Public gRPC server (host[:port] or URI)",
                                    session.public_grpc_server_addr,
                                    TextThen::SettingsGrpcEndpoint,
                                ),
                            );
                        }
                        1 => {
                            let session = session::session_config_snapshot(rt);
                            super::replace_screen(
                                store,
                                crate::prompt_overlay::text_prompt_screen(
                                    crate::prompt_overlay::prompt_underlay(&taken),
                                    "JSON API listen (host:port)",
                                    session.api_listen,
                                    TextThen::SettingsApiListen,
                                ),
                            );
                        }
                        2 => {
                            const API_CURL_TEMPLATE: &str = r#"Wallet JSON API — this TUI session only
Listen: {listen}
Token:  {token}

Copy any block below (token is required on every request).

── Session state ──
curl -sS '{base}/v1/wallet/state' \
  -H '{auth}'

── Show balance ──
curl -sS '{base}/v1/wallet/command' \
  -H '{auth}' \
  -H 'Content-Type: application/json' \
  -d '{"argv":["show-balance"]}'

── List notes ──
curl -sS '{base}/v1/wallet/command' \
  -H '{auth}' \
  -H 'Content-Type: application/json' \
  -d '{"argv":["list-notes"]}'

── Health check ──
curl -sS '{base}/health' \
  -H '{auth}'
"#;
                            let base = api_base_url(&current_api_listen(rt));
                            let token = rt.api_auth_token.as_ref();
                            let auth = format!("Authorization: Bearer {token}");

                            store.state.last_command_output = API_CURL_TEMPLATE
                                .replace("{listen}", &current_api_listen(rt))
                                .replace("{token}", &rt.api_auth_token.as_ref())
                                .replace("{base}", &base)
                                .replace("{auth}", &auth);
                            store.state.output_scroll = 0;
                            super::replace_screen(store, Screen::Settings { sel });
                        }
                        3 => {
                            log_help(false); // TODO: verbose moved to TuiOptions / runtime_cli if still needed
                        }
                        4 => {
                            log_verbose_info();
                        }
                        5 => super::replace_screen(store, Screen::Home),
                        _ => {
                            super::replace_screen(store, Screen::Settings { sel });
                        }
                    }
                    Ok(TuiControl::Continue)
                }
            }
        }
        other => {
            super::replace_screen(store, other);
            Ok(TuiControl::Continue)
        }
    }
}

pub(super) fn handle_quick(store: &mut UIStore, key: KeyEvent) -> Result<TuiControl, NockAppError> {
    let taken = store.state.screen.clone();
    super::replace_screen(
        store,
        Screen::Quick {
            line: String::new(),
        },
    );
    match taken {
        Screen::Quick { mut line } => {
            match key.code {
                KeyCode::Esc => {
                    super::replace_screen(store, Screen::Home);
                }
                KeyCode::Enter => {
                    let cmd = normalize_slash_cmd(&line);
                    match cmd.to_ascii_lowercase().as_str() {
                        "exit" | "quit" => return Ok(TuiControl::Quit),
                        "help" => {
                            log_help(false); // TODO: verbose moved to TuiOptions / runtime_cli if still needed
                        }
                        "verbose" => {
                            log_verbose_info();
                        }
                        "menu" => {
                            super::replace_screen(store, Screen::Home);
                            return Ok(TuiControl::Continue);
                        }
                        "" => {}
                        other => {
                            warn!(
                                "Unknown command {:?}; type `help` or open the Wallet menu.",
                                other
                            );
                        }
                    }
                    line.clear();
                    super::replace_screen(store, Screen::Quick { line });
                }
                _ => {
                    edit_line(&mut line, key);
                    super::replace_screen(store, Screen::Quick { line });
                }
            }
            Ok(TuiControl::Continue)
        }
        other => {
            super::replace_screen(store, other);
            Ok(TuiControl::Continue)
        }
    }
}

pub(super) fn handle_exit_confirm(
    store: &mut UIStore,
    key: KeyEvent,
) -> Result<TuiControl, NockAppError> {
    let taken = store.state.screen.clone();
    match taken {
        Screen::ExitConfirm { underlay, mut sel } => {
            match list_activate(&mut sel, BOOL.len(), key.code) {
                Err(()) => {
                    super::replace_screen(
                        store,
                        crate::prompt_overlay::exit_confirm((*underlay).clone(), sel),
                    );
                    Ok(TuiControl::Continue)
                }
                Ok(None) => {
                    if esc_back(key.code) {
                        super::replace_screen(store, *underlay);
                    } else {
                        super::replace_screen(
                            store,
                            crate::prompt_overlay::exit_confirm((*underlay).clone(), sel),
                        );
                    }
                    Ok(TuiControl::Continue)
                }
                Ok(Some(i)) => {
                    if i == 0 {
                        return Ok(TuiControl::Quit);
                    }
                    super::replace_screen(store, *underlay);
                    Ok(TuiControl::Continue)
                }
            }
        }
        other => {
            super::replace_screen(store, other);
            Ok(TuiControl::Continue)
        }
    }
}
