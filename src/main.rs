#![allow(clippy::unwrap_used)]

use clap::Parser;
use nockapp::kernel::boot::{self, Cli, NockStackSize};
use nockapp::NockAppError;
use nockchain_wallet::boot_wallet;
use nockchain_wallet::ConnectionCli;
use nockchain_wallet_tui::TuiOptions;

/// Lightweight CLI options struct for the TUI binary.
/// Contains only what the TUI needs (no Commands subcommand).
#[derive(Parser)]
struct TuiCli {
    #[command(flatten)]
    boot: Cli,

    /// More detailed logs (info/debug). When unset, the wallet TUI uses a quiet default unless `RUST_LOG` is set.
    #[arg(short, long, global = true)]
    verbose: bool,

    #[arg(long, default_value = "false")]
    fakenet: bool,

    #[command(flatten)]
    connection: ConnectionCli,
}

#[tokio::main]
async fn main() -> Result<(), NockAppError> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("default provider already set elsewhere");

    let mut tui = TuiCli::parse();
    tui.boot.stack_size = NockStackSize::Tiny;

    if std::env::var("RUST_LOG").is_err() {
        if tui.verbose {
            std::env::set_var(
                "RUST_LOG",
                "info,nockapp=info,nockchain_wallet=info,nockchain_wallet_tui=info,opentelemetry_sdk=off",
            );
        } else {
            std::env::set_var(
                "RUST_LOG",
                "warn,nockapp=warn,nockchain_wallet=warn,nockchain_wallet_tui=warn,tonic=warn,h2=warn,tower=warn,hyper=warn,rustls=warn,opentelemetry_sdk=off",
            );
        }
    }

    boot::init_default_tracing(&tui.boot);

    let (wallet, synced_snapshot, data_dir) = boot_wallet(tui.boot.clone(), tui.fakenet).await?;

    let opts = TuiOptions {
        boot: tui.boot,
        verbose: tui.verbose,
        fakenet: tui.fakenet,
        connection: tui.connection,
    };

    nockchain_wallet_tui::run_with_options(opts, wallet, synced_snapshot, data_dir).await
}
