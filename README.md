# Nockchain Wallet TUI

<img width="656" height="608" alt="telegram-cloud-photo-size-1-4940823604891421713-y" src="https://github.com/user-attachments/assets/c45fdd7b-902a-4a26-bc53-220620dab5bd" />
<br />
<br />

Interactive full-screen terminal UI for [`nockchain-wallet`](../nockchain-wallet/), built with [ratatui](https://github.com/ratatui-org/ratatui) and [crossterm](https://github.com/crossterm-rs/crossterm). The TUI exposes the same wallet kernel commands as the CLI, plus a session-scoped JSON HTTP API for automation.

## Launch

```bash
# Monorepo
cargo run -p nockchain-wallet-tui

# Installed binary (requires matching nockchain-wallet lib version)
nockchain-wallet-tui
```

`main.rs` boots the wallet via `nockchain_wallet::open_wallet`, then calls `nockchain_wallet_tui::run`. The `nockchain-wallet` CLI binary has **no** `tui` subcommand.

## Version coupling

Pin `nockchain-wallet-tui` to the same git commit (or crates.io release) as `nockchain-wallet`. The shared contract is documented on `nockchain_wallet::wallet_outcome::WALLET_OUTCOME_SCHEMA` (`wallet-outcome-v1` today) and kernel effects `[%wallet <kind> [%v1 …]]`. Mismatched versions may fail to decode structured events at runtime.

```toml
# External repo / Cargo.toml
nockchain-wallet = { git = "https://github.com/nocktoshi/nockchain", branch = "dev" }
```

## Architecture

Wallet work always goes through `nockchain_wallet::dispatch::execute_wallet_command`; the TUI never pokes the kernel directly.

```
┌─────────────────────────────────────────────────────────────┐
│  event_loop.rs   async event loop (keys, ticks, jobs)       │
│    ├─ handlers/  keyboard + paste routing per Screen        │
│    ├─ components/ ratatui widgets (draw only)             │
│    └─ store/       UIStore + UiAction → apply_ui_action     │
├─────────────────────────────────────────────────────────────┤
│  command_runner.rs   TuiRuntime, background wallet jobs     │
│    └─ nockchain_wallet::dispatch::execute_wallet_command    │
├─────────────────────────────────────────────────────────────┤
│  view/ + nockchain_wallet::wallet_outcome   WalletEvent     │
├─────────────────────────────────────────────────────────────┤
│  wallet_api/     JSON HTTP API (same runtime, same wallet)  │
└─────────────────────────────────────────────────────────────┘
```

### Output layer (`view/` + `wallet_outcome`)

The TUI prefers structured **`WalletEvent`** values from kernel `[%wallet <kind> [%v1 …]]` effects (decoded in `nockchain-wallet`). `view::render_command_output` formats them for the status panel. When no structured event exists, it falls back to captured kernel `%markdown`.

## JSON HTTP API (`wallet_api/`)

Session-scoped (token required on every request). Listen address and token shown at TUI startup. Session schema: `wallet-session-v1` in `wallet_api/state.rs`.

```bash
# Session state
curl -sS '{base}/v1/wallet/state' \
  -H '{auth}'

# Show balance
curl -sS '{base}/v1/wallet/command' \
  -H '{auth}' \
  -H 'Content-Type: application/json' \
  -d '{"argv":["show"]}'

# List notes
curl -sS '{base}/v1/wallet/command' \
  -H '{auth}' \
  -H 'Content-Type: application/json' \
  -d '{"argv":["list-notes"]}'

# Health check
curl -sS '{base}/health' \
  -H '{auth}'
```

## Module map

```
src/
├── lib.rs              Entry: nockchain_wallet_tui::run
├── main.rs             Binary entry
├── event_loop.rs       Terminal + async loop
├── command_runner.rs   TuiRuntime + job scheduling
├── view/               WalletEvent → display text
├── wallet_api/         axum server, auth, executor
├── create_tx.rs        Create-tx wizard UI state (not the planner)
└── …                   handlers, components, store, session, nns
```

Planner and tx file snapshots live in `nockchain_wallet::create_tx`.

## Adding a feature

1. **Screen** — variant in `screens.rs` if needed.
2. **Actions** — `UiAction` + `store/apply.rs`.
3. **Handler** — `handlers/`.
4. **Draw** — `components/`, wired from `components/root.rs`.
5. **Wallet I/O** — `command_runner` + dispatch hooks.
6. **Output** — extend `WalletEvent` in `nockchain-wallet` and render in `view/mod.rs`.

## Testing

```bash
cargo test -p nockchain-wallet-tui
cargo check -p nockchain-wallet-tui
```
