//! Loopback HTTP client for the wallet command API.
//!
//! The TUI talks to its **own** API exactly like a web UI would: it builds an `argv` request with
//! [`command_to_argv`], POSTs it to `/v1/wallet/command`, and renders the typed
//! [`TuiCommandResponse`]. The wallet itself is only ever touched by the API executor.

use crate::session_client::api_base_url;
use nockchain_wallet::command::{Commands, WatchSubcommand};

use super::TuiCommandResponse;

/// POST a JSON body to an API path and decode the [`TuiCommandResponse`].
///
/// Transport failures (API unreachable) are `Err`; command-level failures are carried inside
/// `Ok(TuiCommandResponse { error: Some(_), .. })` so callers can still show partial output.
async fn post(
    api_listen: &str,
    api_token: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<TuiCommandResponse, String> {
    let url = format!("{}{path}", api_base_url(api_listen));
    // No request timeout: sync-heavy commands (balance, create-tx) can run for minutes.
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(&url)
        .bearer_auth(api_token)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("POST {url}: {e}"))?;
    let status = resp.status();
    // 200 OK and 422 (command error) both carry a `TuiCommandResponse` body.
    if status.is_success() || status == reqwest::StatusCode::UNPROCESSABLE_ENTITY {
        resp.json::<TuiCommandResponse>()
            .await
            .map_err(|e| format!("decode {url}: {e}"))
    } else {
        let body = resp.text().await.unwrap_or_default();
        Err(format!("POST {url}: HTTP {status} {body}"))
    }
}

/// Run a generic wallet command (`argv` tokens, no program name).
pub(crate) async fn run_command(
    api_listen: &str,
    api_token: &str,
    argv: Vec<String>,
) -> Result<TuiCommandResponse, String> {
    post(
        api_listen,
        api_token,
        "/v1/wallet/command",
        serde_json::json!({ "argv": argv }),
    )
    .await
}

/// Planner preview for a simple send (no kernel poke).
pub(crate) async fn plan_simple_send(
    api_listen: &str,
    api_token: &str,
    recipient: &str,
    amount_nicks: u64,
) -> Result<TuiCommandResponse, String> {
    post(
        api_listen,
        api_token,
        "/v1/wallet/tx/plan",
        serde_json::json!({ "recipient": recipient, "amount_nicks": amount_nicks }),
    )
    .await
}

/// Build + broadcast a simple send (create-tx then send-tx).
pub(crate) async fn create_and_send_simple(
    api_listen: &str,
    api_token: &str,
    recipient: &str,
    amount_nicks: u64,
) -> Result<TuiCommandResponse, String> {
    post(
        api_listen,
        api_token,
        "/v1/wallet/tx/create-and-send",
        serde_json::json!({ "recipient": recipient, "amount_nicks": amount_nicks }),
    )
    .await
}

/// Register a `.nock` name (registry-payment create-tx then send-tx).
pub(crate) async fn nns_register(
    api_listen: &str,
    api_token: &str,
    name: &str,
) -> Result<TuiCommandResponse, String> {
    post(
        api_listen,
        api_token,
        "/v1/wallet/nns/register",
        serde_json::json!({ "name": name }),
    )
    .await
}

/// Build the wallet CLI `argv` (no program name) for a command — the wire request the API parses
/// back with clap. Single source of truth, kept in sync with the wallet's subcommand definitions
/// and verified by round-trip tests. Prefers flag forms over positionals to avoid argument-order
/// ambiguity. `create-tx` is intentionally a tag only: composite tx flows use dedicated endpoints
/// (P3), never this generic path.
pub(crate) fn command_to_argv(cmd: &Commands) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    macro_rules! push {
        ($($x:expr),* $(,)?) => {{ $(a.push($x.to_string());)* }};
    }
    match cmd {
        Commands::Keygen => push!("keygen"),
        Commands::DeriveChild {
            index,
            hardened,
            label,
        } => {
            push!("derive-child", index);
            if *hardened {
                push!("--hardened");
            }
            if let Some(l) = label {
                push!("--label", l);
            }
        }
        Commands::DeriveChildBatch {
            start_index,
            count,
            hardened,
            label_prefix,
            out,
        } => {
            push!(
                "derive-child-batch",
                "--start-index",
                start_index,
                "--count",
                count
            );
            if *hardened {
                push!("--hardened");
            }
            if let Some(lp) = label_prefix {
                push!("--label-prefix", lp);
            }
            if let Some(o) = out {
                push!("--out", o);
            }
        }
        Commands::ImportKeys {
            file,
            key,
            seedphrase,
            version,
        } => {
            push!("import-keys");
            if let Some(f) = file {
                push!("--file", f);
            }
            if let Some(k) = key {
                push!("--key", k);
            }
            if let Some(s) = seedphrase {
                push!("--seedphrase", s);
            }
            if let Some(v) = version {
                push!("--version", v);
            }
        }
        Commands::Watch { subcommand } => {
            push!("watch");
            match subcommand {
                WatchSubcommand::Address { address } => push!("address", address),
                WatchSubcommand::Pubkey { pubkey } => push!("pubkey", pubkey),
                WatchSubcommand::Multisig {
                    threshold,
                    participants,
                } => push!(
                    "multisig",
                    "--threshold",
                    threshold,
                    "--participants",
                    participants
                ),
                WatchSubcommand::MultisigBatch {
                    threshold,
                    manifest,
                } => push!(
                    "multisig-batch",
                    "--threshold",
                    threshold,
                    "--manifest",
                    manifest
                ),
            }
        }
        Commands::ExportKeys => push!("export-keys"),
        Commands::ListNotes => push!("list-notes"),
        Commands::ListNotesByAddress { address } => {
            push!("list-notes-by-address");
            if let Some(addr) = address {
                push!(addr);
            }
        }
        Commands::ListNotesByAddressCsv { address } => push!("list-notes-by-address-csv", address),
        Commands::SendTx { transaction } => push!("send-tx", transaction),
        Commands::ShowTx { transaction } => push!("show-tx", transaction),
        Commands::ShowBalance => push!("show-balance"),
        Commands::TxAccepted { tx_id } => push!("tx-accepted", tx_id),
        Commands::MigrateV0Notes { destination } => {
            push!("migrate-v0-notes", "--destination", destination)
        }
        Commands::SignMultisigTx {
            transaction,
            sign_keys,
        } => {
            push!("sign-multisig-tx", transaction);
            if let Some(sk) = sign_keys {
                push!("--sign-keys", sk);
            }
        }
        Commands::ExportMasterPubkey => push!("export-master-pubkey"),
        Commands::ImportMasterPubkey { key_path } => push!("import-master-pubkey", key_path),
        Commands::SetActiveMasterAddress { address_b58 } => {
            push!("set-active-master-address", address_b58)
        }
        Commands::ListActiveAddresses => push!("list-active-addresses"),
        Commands::ListMasterAddresses => push!("list-master-addresses"),
        Commands::ShowSeedphrase => push!("show-seedphrase"),
        Commands::ShowMasterZPub => push!("show-master-zpub"),
        Commands::ShowMasterZPrv => push!("show-master-zprv"),
        Commands::ShowMasterPrv => push!("show-master-prv"),
        Commands::ShowKeyTree { include_values } => {
            push!("show-key-tree");
            if *include_values {
                push!("--include-values");
            }
        }
        Commands::SignMessage {
            message,
            message_file,
            message_pos,
            index,
            hardened,
        } => {
            push!("sign-message");
            if let Some(m) = message.as_ref().or(message_pos.as_ref()) {
                push!("--message", m);
            }
            if let Some(f) = message_file {
                push!("--message-file", f);
            }
            if let Some(i) = index {
                push!("--index", i);
            }
            if *hardened {
                push!("--hardened");
            }
        }
        Commands::SignHash {
            hash_b58,
            index,
            hardened,
        } => {
            push!("sign-hash", hash_b58);
            if let Some(i) = index {
                push!("--index", i);
            }
            if *hardened {
                push!("--hardened");
            }
        }
        Commands::VerifyMessage {
            message,
            message_file,
            message_pos,
            signature_path,
            signature_pos,
            pubkey,
            pubkey_pos,
        } => {
            push!("verify-message");
            if let Some(m) = message.as_ref().or(message_pos.as_ref()) {
                push!("--message", m);
            }
            if let Some(f) = message_file {
                push!("--message-file", f);
            }
            if let Some(s) = signature_path.as_ref().or(signature_pos.as_ref()) {
                push!("--signature", s);
            }
            if let Some(p) = pubkey.as_ref().or(pubkey_pos.as_ref()) {
                push!("--pubkey", p);
            }
        }
        Commands::VerifyHash {
            hash_b58,
            signature_path,
            signature_pos,
            pubkey,
            pubkey_pos,
        } => {
            push!("verify-hash", hash_b58);
            if let Some(s) = signature_path.as_ref().or(signature_pos.as_ref()) {
                push!("--signature", s);
            }
            if let Some(p) = pubkey.as_ref().or(pubkey_pos.as_ref()) {
                push!("--pubkey", p);
            }
        }
        // Composite tx flows use dedicated endpoints (P3); never routed through generic argv.
        Commands::CreateTx { .. } => push!("create-tx"),
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct ApiCli {
        #[command(subcommand)]
        command: Commands,
    }

    /// Parse generated argv back into a `Commands` exactly as the API executor does.
    fn roundtrip(cmd: &Commands) -> Commands {
        let mut full = vec!["nockchain-wallet-tui".to_string()];
        full.extend(command_to_argv(cmd));
        ApiCli::try_parse_from(&full)
            .unwrap_or_else(|e| panic!("argv {full:?} failed to parse: {e}"))
            .command
    }

    #[test]
    fn derive_child_with_flags_roundtrips() {
        let cmd = Commands::DeriveChild {
            index: 7,
            hardened: true,
            label: Some("alpha".into()),
        };
        match roundtrip(&cmd) {
            Commands::DeriveChild {
                index,
                hardened,
                label,
            } => {
                assert_eq!(index, 7);
                assert!(hardened);
                assert_eq!(label.as_deref(), Some("alpha"));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn import_keys_seedphrase_version_roundtrips() {
        let cmd = Commands::ImportKeys {
            file: None,
            key: None,
            seedphrase: Some("a b c".into()),
            version: Some(3),
        };
        match roundtrip(&cmd) {
            Commands::ImportKeys {
                seedphrase,
                version,
                ..
            } => {
                assert_eq!(seedphrase.as_deref(), Some("a b c"));
                assert_eq!(version, Some(3));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn watch_multisig_roundtrips() {
        let cmd = Commands::Watch {
            subcommand: WatchSubcommand::Multisig {
                threshold: 2,
                participants: "a,b,c".into(),
            },
        };
        match roundtrip(&cmd) {
            Commands::Watch {
                subcommand:
                    WatchSubcommand::Multisig {
                        threshold,
                        participants,
                    },
            } => {
                assert_eq!(threshold, 2);
                assert_eq!(participants, "a,b,c");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn verify_message_positional_pubkey_becomes_flag() {
        // TUI builds this with pubkey via the positional slot; argv must use --pubkey to stay
        // unambiguous, and must round-trip to the same pubkey value.
        let cmd = Commands::VerifyMessage {
            message: Some("hello".into()),
            message_file: None,
            message_pos: None,
            signature_path: Some("/tmp/sig".into()),
            signature_pos: None,
            pubkey: None,
            pubkey_pos: Some("PUBKEY58".into()),
        };
        match roundtrip(&cmd) {
            Commands::VerifyMessage {
                message,
                signature_path,
                pubkey,
                ..
            } => {
                assert_eq!(message.as_deref(), Some("hello"));
                assert_eq!(signature_path.as_deref(), Some("/tmp/sig"));
                assert_eq!(pubkey.as_deref(), Some("PUBKEY58"));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn show_key_tree_flag_roundtrips() {
        match roundtrip(&Commands::ShowKeyTree {
            include_values: true,
        }) {
            Commands::ShowKeyTree { include_values } => assert!(include_values),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn simple_subcommands_roundtrip() {
        assert!(matches!(roundtrip(&Commands::Keygen), Commands::Keygen));
        assert!(matches!(
            roundtrip(&Commands::ShowBalance),
            Commands::ShowBalance
        ));
        assert!(matches!(
            roundtrip(&Commands::ListActiveAddresses),
            Commands::ListActiveAddresses
        ));
        assert!(matches!(
            roundtrip(&Commands::ShowMasterZPub),
            Commands::ShowMasterZPub
        ));
    }
}
