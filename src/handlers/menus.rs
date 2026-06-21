//! Menu routing: one generic `run_menu` over the declarative [`crate::actions`] catalog, plus the
//! bespoke settings / quick-command / exit-confirm handlers.

use crossterm::event::{KeyCode, KeyEvent};
use nockapp::NockAppError;
use tokio::sync::mpsc;

use super::input::{edit_line, esc_back, list_activate};
use crate::actions::{self, MenuAction, MenuItem};
use crate::command_runner::TuiRuntime;
use crate::components::menus::{BOOL, SETTINGS_MENU};
use crate::hooks::logging::{help_text, verbose_text};
use crate::msg::Msg;
use crate::screens::{Overlay, Screen, TextThen, TuiControl};
use crate::session::current_api_listen;
use crate::session_client::api_base_url;
use crate::store::UIStore;
use crate::{normalize_slash_cmd, session};
use nockchain_wallet::command::Commands;

/// Navigate from the home Menu tab (`MAIN_MENU` index).
pub(super) fn navigate_main_menu_item(store: &mut UIStore, i: usize) {
    if i == 7 {
        super::set_overlay(store, Some(Overlay::ExitConfirm { sel: 1 }));
        return;
    }
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
    done_tx: &mpsc::UnboundedSender<Msg>,
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
    done_tx: &mpsc::UnboundedSender<Msg>,
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
            // Keep the menu as the route (the overlay's underlay), then open the prompt.
            super::replace_screen(store, rebuild(sel));
            super::set_overlay(store, Some(Overlay::prompt(*title, "", then.clone())));
        }
        MenuAction::Confirm {
            title,
            sel: csel,
            labels,
            then,
        } => {
            super::replace_screen(store, rebuild(sel));
            super::set_overlay(
                store,
                Some(Overlay::confirm(*title, *csel, labels, then.clone())),
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
                            super::set_overlay(
                                store,
                                Some(Overlay::prompt(
                                    "Public gRPC server (host[:port] or URI)",
                                    session.public_grpc_server_addr,
                                    TextThen::SettingsGrpcEndpoint,
                                )),
                            );
                        }
                        1 => {
                            let session = session::session_config_snapshot(rt);
                            super::set_overlay(
                                store,
                                Some(Overlay::prompt(
                                    "JSON API listen (host:port)",
                                    session.api_listen,
                                    TextThen::SettingsApiListen,
                                )),
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
                                .replace("{token}", rt.api_auth_token.as_ref())
                                .replace("{base}", &base)
                                .replace("{auth}", &auth);
                            store.state.last_command_status = None;
                            store.state.output_scroll = 0;
                            super::replace_screen(store, Screen::Settings { sel });
                        }
                        3 => {
                            store.state.last_command_output = help_text();
                            store.state.output_scroll = 0;
                            super::replace_screen(store, Screen::Settings { sel });
                        }
                        4 => {
                            store.state.last_command_output = verbose_text();
                            store.state.output_scroll = 0;
                            super::replace_screen(store, Screen::Settings { sel });
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

pub(super) fn handle_quick(
    store: &mut UIStore,
    key: KeyEvent,
    rt: &TuiRuntime,
    msg_tx: &mpsc::UnboundedSender<Msg>,
) -> Result<TuiControl, NockAppError> {
    let Screen::Quick { mut line } = store.state.screen.clone() else {
        return Ok(TuiControl::Continue);
    };
    match key.code {
        KeyCode::Esc => super::replace_screen(store, Screen::Home),
        KeyCode::Enter => {
            let input = normalize_slash_cmd(&line).to_string();
            super::replace_screen(
                store,
                Screen::Quick {
                    line: String::new(),
                },
            );
            match input.to_ascii_lowercase().as_str() {
                "exit" | "quit" => return Ok(TuiControl::Quit),
                "menu" => super::replace_screen(store, Screen::Home),
                "" => {}
                "help" => set_output(store, help_text()),
                "verbose" => set_output(store, verbose_text()),
                _ => run_quick_command(store, rt, msg_tx, &input),
            }
        }
        _ => {
            edit_line(&mut line, key);
            super::replace_screen(store, Screen::Quick { line });
        }
    }
    Ok(TuiControl::Continue)
}

fn set_output(store: &mut UIStore, text: String) {
    store.state.last_command_output = text;
    store.state.last_command_status = None; // info text, not a command success — no ✅ header
    store.state.output_scroll = 0;
}

/// Run a free-typed wallet command from the quick command line (same parse as the JSON API).
fn run_quick_command(
    store: &mut UIStore,
    rt: &TuiRuntime,
    msg_tx: &mpsc::UnboundedSender<Msg>,
    input: &str,
) {
    use clap::Parser;
    #[derive(Parser)]
    struct QuickCli {
        #[command(subcommand)]
        command: Commands,
    }
    let mut argv = vec!["nockchain-wallet-tui".to_string()];
    argv.extend(input.split_whitespace().map(String::from));
    match QuickCli::try_parse_from(&argv) {
        Ok(cli) => {
            let label = actions::command_label(&cli.command);
            super::schedule_cmd(store, rt, msg_tx, cli.command, label);
        }
        Err(e) => set_output(store, e.to_string()),
    }
}

pub(super) fn handle_exit_confirm(
    store: &mut UIStore,
    key: KeyEvent,
) -> Result<TuiControl, NockAppError> {
    let Some(Overlay::ExitConfirm { mut sel }) = store.state.overlay.clone() else {
        return Ok(TuiControl::Continue);
    };
    match list_activate(&mut sel, BOOL.len(), key.code) {
        Err(()) => {
            super::set_overlay(store, Some(Overlay::ExitConfirm { sel }));
            Ok(TuiControl::Continue)
        }
        Ok(None) => {
            if esc_back(key.code) {
                super::set_overlay(store, None);
            } else {
                super::set_overlay(store, Some(Overlay::ExitConfirm { sel }));
            }
            Ok(TuiControl::Continue)
        }
        Ok(Some(i)) => {
            if i == 0 {
                return Ok(TuiControl::Quit);
            }
            super::set_overlay(store, None);
            Ok(TuiControl::Continue)
        }
    }
}
