//! Normalize wallet output (structured `events` + captured kernel `%markdown`) into [`Report`]s.
//!
//! This is the **single** place kernel markdown is parsed; it runs server-side in the API executor
//! so markdown never reaches a client. Structured events map to typed sections directly; commands
//! that still only emit `%markdown` go through the generic [`parse_markdown_to_sections`] (and may
//! gain a bespoke parser later). As upstream grafts `[%wallet …]` effects, the markdown path for a
//! command disappears.

use nockchain_wallet::command::Commands;
use nockchain_wallet::wallet_outcome::{
    WalletAddressRowV1, WalletEvent, WalletKeyTreeNodeV1, WalletKeygenV1, WalletMigrateSignerRowV1,
    WalletNoteRowV1,
};

use super::report::{Report, Section};
use crate::format::format_nock_from_nicks;

const NICKS_PER_NOCK: f64 = 65_536.0;

/// Build reports for a command's output. Prefer structured events; fall back to parsing captured
/// kernel markdown. Empty output → no reports (caller shows a neutral "no output" state).
pub(crate) fn normalize(events: &[WalletEvent], markdown: &str, cmd: &Commands) -> Vec<Report> {
    if !events.is_empty() {
        let name = command_name(cmd);
        return events.iter().map(|e| report_from_event(name, e)).collect();
    }
    let md = markdown.trim();
    if md.is_empty() {
        return Vec::new();
    }
    vec![report_from_markdown(cmd, markdown)]
}

/// Kebab CLI id for a command (matches the wallet's clap subcommand names; the P2 catalog reuses it).
pub(crate) fn command_name(cmd: &Commands) -> &'static str {
    match cmd {
        Commands::Keygen => "keygen",
        Commands::DeriveChild { .. } => "derive-child",
        Commands::DeriveChildBatch { .. } => "derive-child-batch",
        Commands::ImportKeys { .. } => "import-keys",
        Commands::Watch { .. } => "watch",
        Commands::ExportKeys => "export-keys",
        Commands::ListNotes => "list-notes",
        Commands::ListNotesByAddress { .. } => "list-notes-by-address",
        Commands::ListNotesByAddressCsv { .. } => "list-notes-by-address-csv",
        Commands::SendTx { .. } => "send-tx",
        Commands::ShowTx { .. } => "show-tx",
        Commands::ShowBalance => "show-balance",
        Commands::TxAccepted { .. } => "tx-accepted",
        Commands::CreateTx { .. } => "create-tx",
        Commands::MigrateV0Notes { .. } => "migrate-v0-notes",
        Commands::SignMultisigTx { .. } => "sign-multisig-tx",
        Commands::ExportMasterPubkey => "export-master-pubkey",
        Commands::ImportMasterPubkey { .. } => "import-master-pubkey",
        Commands::SetActiveMasterAddress { .. } => "set-active-master-address",
        Commands::ListActiveAddresses => "list-active-addresses",
        Commands::ListMasterAddresses => "list-master-addresses",
        Commands::ShowSeedphrase => "show-seedphrase",
        Commands::ShowMasterZPub => "show-master-zpub",
        Commands::ShowMasterZPrv => "show-master-zprv",
        Commands::ShowMasterPrv => "show-master-prv",
        Commands::ShowKeyTree { .. } => "show-key-tree",
        Commands::SignMessage { .. } => "sign-message",
        Commands::SignHash { .. } => "sign-hash",
        Commands::VerifyMessage { .. } => "verify-message",
        Commands::VerifyHash { .. } => "verify-hash",
    }
}

fn nicks_with_nock(nicks: u64) -> String {
    format!(
        "{} ({nicks} nicks)",
        format_nock_from_nicks(nicks as u128)
    )
}

fn report_from_event(command: &str, event: &WalletEvent) -> Report {
    match event {
        WalletEvent::BalanceSnapshotV1 {
            wallet_version,
            block_id_b58,
            height,
            note_count,
            total_assets,
        } => Report::new(
            command,
            "Balance",
            vec![
                Section::kv("wallet version", wallet_version.to_string()),
                Section::kv("block", block_id_b58.clone()),
                Section::kv("height", height.to_string()),
                Section::kv("notes", note_count.to_string()),
                Section::kv("total", nicks_with_nock(*total_assets)),
            ],
        ),
        WalletEvent::NotesListV1 {
            height,
            block_id_b58,
            filter_address,
            rows,
        } => report_notes(command, *height, block_id_b58, filter_address.as_deref(), rows),
        WalletEvent::AddressListV1 { list_kind, rows } => report_addresses(command, list_kind, rows),
        WalletEvent::KeyTreeV1 {
            include_values,
            nodes,
        } => report_key_tree(command, *include_values, nodes),
        WalletEvent::KeygenV1(k) => report_keygen(command, k),
        WalletEvent::MigrateSummaryV1 {
            destination,
            block_id,
            height,
            examined_signers,
            created_count,
            skipped_count,
            signers,
        } => report_migrate(
            command,
            destination,
            block_id,
            *height,
            *examined_signers,
            *created_count,
            *skipped_count,
            signers,
        ),
        WalletEvent::TxAcceptedV1 { tx_id, accepted } => Report::new(
            command,
            "Transaction acceptance",
            vec![
                Section::kv("tx id", tx_id.clone()),
                Section::kv(
                    "status",
                    if *accepted {
                        "accepted by node"
                    } else {
                        "not yet accepted"
                    },
                ),
            ],
        ),
        WalletEvent::NnsRegistrationV1 {
            name,
            fee_nicks,
            blob,
            tx_paths,
        } => report_nns(command, name, *fee_nicks, blob, tx_paths),
        WalletEvent::CreateTxV1 { tx_paths, summary } => report_create_tx(command, tx_paths, summary),
    }
}

fn report_notes(
    command: &str,
    height: u64,
    block_id_b58: &str,
    filter_address: Option<&str>,
    rows: &[WalletNoteRowV1],
) -> Report {
    let mut sections = vec![
        Section::kv("height", height.to_string()),
        Section::kv("block", block_id_b58.to_string()),
    ];
    if let Some(addr) = filter_address {
        sections.push(Section::kv("filter address", addr.to_string()));
    }
    sections.push(Section::kv("count", rows.len().to_string()));
    let table_rows = rows
        .iter()
        .map(|r| {
            vec![
                r.version.to_string(),
                format_nock_from_nicks(r.assets as u128),
                r.name_first_b58.clone(),
                r.name_last_b58.clone(),
            ]
        })
        .collect();
    sections.push(Section::Table {
        headers: vec![
            "version".into(),
            "assets (NOCK)".into(),
            "name first".into(),
            "name last".into(),
        ],
        rows: table_rows,
    });
    Report::new(command, "Notes", sections)
}

fn report_addresses(command: &str, list_kind: &str, rows: &[WalletAddressRowV1]) -> Report {
    let mut sections = vec![Section::kv("count", rows.len().to_string())];
    for row in rows {
        sections.push(Section::kv(format!("v{}", row.version), row.address_b58.clone()));
    }
    Report::new(command, format!("Addresses ({list_kind})"), sections)
}

fn report_key_tree(command: &str, include_values: bool, nodes: &[WalletKeyTreeNodeV1]) -> Report {
    let mut sections = vec![Section::kv("include values", include_values.to_string())];
    for node in nodes {
        let value = match &node.pubkey_b58 {
            Some(pk) => format!("{} → {pk}", node.label),
            None => node.label.clone(),
        };
        sections.push(Section::kv(node.path.clone(), value));
    }
    Report::new(command, "Key tree", sections)
}

fn report_keygen(command: &str, k: &WalletKeygenV1) -> Report {
    let mut sections = Vec::with_capacity(k.paths.len());
    for (path, pk) in k.paths.iter().zip(k.pubkeys_b58.iter()) {
        sections.push(Section::kv(path.clone(), pk.clone()));
    }
    Report::new(command, k.message.clone(), sections)
}

#[allow(clippy::too_many_arguments)]
fn report_migrate(
    command: &str,
    destination: &str,
    block_id: &str,
    height: u64,
    examined_signers: usize,
    created_count: usize,
    skipped_count: usize,
    signers: &[WalletMigrateSignerRowV1],
) -> Report {
    let mut sections = vec![
        Section::kv("destination", destination.to_string()),
        Section::kv("block id", block_id.to_string()),
        Section::kv("height", height.to_string()),
        Section::kv("active signing keys examined", examined_signers.to_string()),
        Section::kv("migration txs created", created_count.to_string()),
        Section::kv("signing keys skipped", skipped_count.to_string()),
    ];
    for signer in signers {
        sections.push(Section::heading(signer.label.clone()));
        sections.push(Section::kv("signer address", signer.address_b58.clone()));
        sections.push(Section::kv("signer version", signer.version.to_string()));
        sections.push(Section::kv("selected notes", signer.note_count.to_string()));
        sections.push(Section::kv(
            "selected total",
            format_nock_from_nicks(signer.selected_total as u128),
        ));
        match (&signer.migrated_amount, &signer.tx_path) {
            (Some(amount), Some(tx_path)) => {
                sections.push(Section::kv("result", "created"));
                if let Some(fee) = signer.fee {
                    sections.push(Section::kv("fee", format_nock_from_nicks(fee as u128)));
                }
                sections.push(Section::kv(
                    "migrated amount",
                    format_nock_from_nicks(*amount as u128),
                ));
                sections.push(Section::kv("tx path", tx_path.clone()));
            }
            _ => {
                sections.push(Section::kv("result", "skipped"));
                if let Some(fee) = signer.fee {
                    sections.push(Section::kv("fee estimate", format_nock_from_nicks(fee as u128)));
                }
                if let Some(reason) = &signer.skip_reason {
                    sections.push(Section::kv("skip reason", reason.clone()));
                }
            }
        }
    }
    Report::new(command, "V0 migration sweep", sections)
}

fn report_nns(command: &str, name: &str, fee_nicks: u64, blob: &str, tx_paths: &[String]) -> Report {
    let fee_nocks = fee_nicks as f64 / NICKS_PER_NOCK;
    let mut sections = vec![
        Section::kv("name", name.to_string()),
        Section::kv("fee", format!("{fee_nicks} nicks (~{fee_nocks:.4} NOCK)")),
        Section::kv("blob", blob.to_string()),
    ];
    if tx_paths.is_empty() {
        sections.push(Section::kv("tx", "(pending create-tx)"));
    } else {
        for path in tx_paths {
            sections.push(Section::kv("tx path", path.clone()));
        }
    }
    Report::new(command, "NNS name registration", sections)
}

fn report_create_tx(command: &str, tx_paths: &[String], summary: &str) -> Report {
    let mut sections = Vec::new();
    let summary = summary.trim();
    if !summary.is_empty() {
        sections.extend(parse_markdown_to_sections(summary));
    }
    if !tx_paths.is_empty() {
        sections.push(Section::heading("Transaction files"));
        for path in tx_paths {
            sections.push(Section::kv("path", path.clone()));
        }
    }
    Report::new(command, "Create transaction", sections)
}

fn report_from_markdown(cmd: &Commands, markdown: &str) -> Report {
    let title = title_for_command(cmd);
    Report::new(command_name(cmd), title, parse_markdown_to_sections(markdown))
}

fn title_for_command(cmd: &Commands) -> String {
    // Title-case the kebab command id, e.g. "show-seedphrase" -> "Show Seedphrase".
    let name = command_name(cmd);
    let mut out = String::new();
    for (i, word) in name.split('-').enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Generic kernel-markdown → sections parser. Recognizes `#`/`##`/`###` headings, `- key: value`
/// and `- item` bullets, and `| table | rows |`; everything else becomes `Text`. Robust to most
/// wallet kernel markdown without per-command code.
pub(crate) fn parse_markdown_to_sections(md: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut text_buf: Vec<String> = Vec::new();
    let mut table_buf: Vec<Vec<String>> = Vec::new();

    fn flush_text(buf: &mut Vec<String>, sections: &mut Vec<Section>) {
        if !buf.is_empty() {
            sections.push(Section::text(buf.join("\n")));
            buf.clear();
        }
    }
    fn flush_table(buf: &mut Vec<Vec<String>>, sections: &mut Vec<Section>) {
        if buf.is_empty() {
            return;
        }
        let headers = buf.first().cloned().unwrap_or_default();
        let rows = buf.iter().skip(1).cloned().collect();
        sections.push(Section::Table { headers, rows });
        buf.clear();
    }

    for raw in md.lines() {
        let t = raw.trim();

        // Table rows: `| a | b |`
        if t.starts_with('|') && t.ends_with('|') && t.len() >= 2 {
            flush_text(&mut text_buf, &mut sections);
            let cells: Vec<String> = t
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect();
            // Skip separator rows like `| --- | :--: |`.
            let is_separator = cells
                .iter()
                .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'));
            if !is_separator {
                table_buf.push(cells);
            }
            continue;
        }
        flush_table(&mut table_buf, &mut sections);

        if t.is_empty() {
            flush_text(&mut text_buf, &mut sections);
            continue;
        }

        if let Some(rest) = t
            .strip_prefix("### ")
            .or_else(|| t.strip_prefix("## "))
            .or_else(|| t.strip_prefix("# "))
        {
            flush_text(&mut text_buf, &mut sections);
            sections.push(Section::heading(rest.trim()));
            continue;
        }

        if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            flush_text(&mut text_buf, &mut sections);
            match rest.split_once(": ") {
                Some((k, v)) => sections.push(Section::kv(k.trim(), v.trim())),
                None => sections.push(Section::text(rest.trim())),
            }
            continue;
        }

        text_buf.push(t.to_string());
    }

    flush_text(&mut text_buf, &mut sections);
    flush_table(&mut table_buf, &mut sections);
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_headings_bullets_tables() {
        let md = "## Balance\n- height: 42\n- total: 5 NOCK\n\n| a | b |\n| --- | --- |\n| 1 | 2 |";
        let sections = parse_markdown_to_sections(md);
        assert_eq!(sections[0], Section::heading("Balance"));
        assert_eq!(sections[1], Section::kv("height", "42"));
        assert_eq!(sections[2], Section::kv("total", "5 NOCK"));
        match &sections[3] {
            Section::Table { headers, rows } => {
                assert_eq!(headers, &vec!["a".to_string(), "b".to_string()]);
                assert_eq!(rows, &vec![vec!["1".to_string(), "2".to_string()]]);
            }
            other => panic!("expected table, got {other:?}"),
        }
    }

    #[test]
    fn empty_output_no_reports() {
        assert!(normalize(&[], "   ", &Commands::ShowBalance).is_empty());
    }

    #[test]
    fn markdown_only_command_produces_report() {
        let reports = normalize(&[], "## Seed phrase\n- words: alpha bravo", &Commands::ShowSeedphrase);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].command, "show-seedphrase");
        assert_eq!(reports[0].title, "Show Seedphrase");
        assert_eq!(reports[0].sections[0], Section::heading("Seed phrase"));
    }
}
