#![allow(clippy::unwrap_used)]

use clap::Parser;
use nockapp::kernel::boot::{self, NockStackSize};
use nockapp::NockAppError;
use nockchain_wallet::command::WalletCli;
use nockchain_wallet::open_wallet;

#[tokio::main]
async fn main() -> Result<(), NockAppError> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("default provider already set elsewhere");

    let mut cli = WalletCli::parse();
    cli.boot.stack_size = NockStackSize::Tiny;

    if std::env::var("RUST_LOG").is_err() {
        if cli.verbose {
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

    boot::init_default_tracing(&cli.boot.clone());

    let (wallet, synced_snapshot, data_dir) = open_wallet(&cli).await?;
    nockchain_wallet_tui::run(&cli, wallet, synced_snapshot, data_dir).await
}
