# Wallet TUI (implementation)

Crate-level overview and install: [`../README.md`](../README.md).

## Launch

```bash
cargo run -p nockchain-wallet-tui
```

## Architecture

The TUI is intentionally isolated from dispatch and the Hoon kernel. Wallet work always goes through `nockchain_wallet::dispatch::execute_wallet_command`; the TUI never pokes the kernel directly.

```
┌─────────────────────────────────────────────────────────────┐
│  event_loop.rs   async event loop (keys, ticks, jobs)       │
│    ├─ handlers/  keyboard + paste routing per Screen        │
│    ├─ components/ ratatui widgets (draw only)             │
│    └─ store/       UIStore + UiAction → apply_ui_action     │
├─────────────────────────────────────────────────────────────┤
│  command_runner.rs   TuiRuntime, background wallet jobs     │
│    └─ nockchain_wallet::dispatch::execute_wallet_command  │
├─────────────────────────────────────────────────────────────┤
│  view/ + nockchain_wallet::wallet_outcome   WalletEvent   │
├─────────────────────────────────────────────────────────────┤
│  wallet_api/     JSON HTTP API (same runtime, same wallet)  │
└─────────────────────────────────────────────────────────────┘
```

### Output layer (`view/` + `wallet_outcome`)

The TUI prefers structured **`WalletEvent`** values from kernel `[%wallet <kind> [%v1 …]]` effects (decoded in `nockchain-wallet`). `view::render_command_output` formats them for the status panel. When no structured event exists, it falls back to captured kernel `%markdown`.

## JSON HTTP API (`wallet_api/`)

Example curls: [`docs/api-curl.txt`](docs/api-curl.txt). Session schema: `wallet-session-v1` in `wallet_api/state.rs`.

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
```
