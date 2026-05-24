# Nockchain Wallet TUI

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
nockchain-wallet = { git = "https://github.com/nockchain/nockchain", branch = "dev" }
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

See [`src/README.md`](src/README.md) for module-level detail (event loop, screens, API routes).

## Testing

```bash
cargo test -p nockchain-wallet-tui
cargo check -p nockchain-wallet-tui
```
