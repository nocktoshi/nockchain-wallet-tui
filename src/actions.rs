//! Declarative menu catalog — the single source of truth for list menus.
//!
//! Each [`MenuItem`] couples a **label** (drawn by the UI) with an **action** (run by the keyboard
//! router), so a menu's contents and behavior live in one place and stay index-aligned by
//! construction. The generic `run_menu` handler walks these tables; there is no per-menu handler.

use crate::components::menus::BOOL;
use crate::create_tx::CreateTxWizard;
use crate::screens::{ConfirmThen, Screen, TextThen};
use nockchain_wallet::command::Commands;

/// What activating a menu item does.
pub(crate) enum MenuAction {
    /// Schedule a wallet command over the API (spinner label from [`command_label`]).
    Run(Commands),
    /// Open a text prompt that continues via [`TextThen`].
    Prompt {
        title: &'static str,
        then: TextThen,
    },
    /// Open a yes/no (or labelled) confirm that continues via [`ConfirmThen`].
    Confirm {
        title: &'static str,
        sel: usize,
        labels: &'static [&'static str],
        then: ConfirmThen,
    },
    /// Navigate to another screen.
    Goto(ScreenTarget),
}

/// Navigation targets reachable from menus (kept as a const-friendly enum, not a fn pointer).
pub(crate) enum ScreenTarget {
    Home,
    /// Back to the Keys menu at a given selection.
    Keys(usize),
    KeysImport,
    NnsBuy,
    CreateTx,
}

impl ScreenTarget {
    pub(crate) fn build(&self) -> Screen {
        match self {
            ScreenTarget::Home => Screen::Home,
            ScreenTarget::Keys(sel) => Screen::Keys { sel: *sel },
            ScreenTarget::KeysImport => Screen::KeysImport { sel: 0 },
            ScreenTarget::NnsBuy => Screen::nns_buy_new(),
            ScreenTarget::CreateTx => Screen::CreateTx {
                w: CreateTxWizard::new(),
            },
        }
    }
}

pub(crate) struct MenuItem {
    pub label: &'static str,
    pub action: MenuAction,
}

const fn item(label: &'static str, action: MenuAction) -> MenuItem {
    MenuItem { label, action }
}

const fn run(label: &'static str, cmd: Commands) -> MenuItem {
    item(label, MenuAction::Run(cmd))
}

const fn prompt(label: &'static str, title: &'static str, then: TextThen) -> MenuItem {
    item(label, MenuAction::Prompt { title, then })
}

const fn goto(label: &'static str, target: ScreenTarget) -> MenuItem {
    item(label, MenuAction::Goto(target))
}

/// Labels for drawing (the UI list widgets take `&[&str]`).
pub(crate) fn labels(items: &[MenuItem]) -> Vec<&'static str> {
    items.iter().map(|i| i.label).collect()
}

/// Human label for a command's in-flight spinner / running screen.
pub(crate) fn command_label(cmd: &Commands) -> &'static str {
    match cmd {
        Commands::Keygen => "Keygen",
        Commands::DeriveChild { .. } => "Derive child key",
        Commands::DeriveChildBatch { .. } => "Derive child keys",
        Commands::ImportKeys { .. } => "Import keys",
        Commands::Watch { .. } => "Add watch-only",
        Commands::ExportKeys => "Export keys",
        Commands::ListNotes => "List notes",
        Commands::ListNotesByAddress { .. } => "List notes by address",
        Commands::ListNotesByAddressCsv { .. } => "List notes (CSV)",
        Commands::SendTx { .. } => "Send transaction",
        Commands::ShowTx { .. } => "Show transaction",
        Commands::ShowBalance => "Show balance",
        Commands::TxAccepted { .. } => "Check acceptance",
        Commands::CreateTx { .. } => "Create transaction",
        Commands::MigrateV0Notes { .. } => "Migrate v0 notes",
        Commands::SignMultisigTx { .. } => "Sign multisig tx",
        Commands::ExportMasterPubkey => "Export master pubkey",
        Commands::ImportMasterPubkey { .. } => "Import master pubkey",
        Commands::SetActiveMasterAddress { .. } => "Set active address",
        Commands::ListActiveAddresses => "List active addresses",
        Commands::ListMasterAddresses => "List master addresses",
        Commands::ShowSeedphrase => "Show seed phrase",
        Commands::ShowMasterZPub => "Show master zpub",
        Commands::ShowMasterZPrv => "Show master zprv",
        Commands::ShowMasterPrv => "Show master prv",
        Commands::ShowKeyTree { .. } => "Show key tree",
        Commands::SignMessage { .. } => "Sign message",
        Commands::SignHash { .. } => "Sign hash",
        Commands::VerifyMessage { .. } => "Verify message",
        Commands::VerifyHash { .. } => "Verify hash",
    }
}

pub(crate) const KEYS_ITEMS: &[MenuItem] = &[
    run("Keygen", Commands::Keygen),
    prompt(
        "Derive child key",
        "Child index (u64)",
        TextThen::KeysDeriveIndex,
    ),
    goto(
        "Import keys (file / extended key / seed)",
        ScreenTarget::KeysImport,
    ),
    run("Export keys", Commands::ExportKeys),
    run("Show seed phrase", Commands::ShowSeedphrase),
    run("Show master zpub", Commands::ShowMasterZPub),
    run("Show master zprv", Commands::ShowMasterZPrv),
    item(
        "Show key tree",
        MenuAction::Confirm {
            title: "Include values at each path?",
            sel: 1,
            labels: BOOL,
            then: ConfirmThen::KeysKeyTree,
        },
    ),
    run("List active addresses", Commands::ListActiveAddresses),
    run("List master addresses", Commands::ListMasterAddresses),
    prompt(
        "Set active master address",
        "Address (base58)",
        TextThen::KeysSetActive,
    ),
    prompt(
        "Import master pubkey",
        "Path to exported master pubkey file",
        TextThen::KeysImportMaster,
    ),
    run("Export master pubkey", Commands::ExportMasterPubkey),
    goto("Back", ScreenTarget::Home),
];

pub(crate) const KEYS_IMPORT_ITEMS: &[MenuItem] = &[
    prompt("File", "Path to jammed keys file", TextThen::KeysImportFile),
    prompt(
        "Extended key",
        "Extended key (zprv/zpub…)",
        TextThen::KeysImportExtended,
    ),
    prompt("Seed phrase", "Seed phrase", TextThen::KeysImportSeed),
    goto("Back", ScreenTarget::Keys(2)),
];

pub(crate) const NOTES_ITEMS: &[MenuItem] = &[
    run("List all notes", Commands::ListNotes),
    prompt(
        "List notes by address (required)",
        "Public key / filter",
        TextThen::NotesListByAddr,
    ),
    prompt(
        "List notes by address (CSV)",
        "Public key",
        TextThen::NotesListCsv,
    ),
    run("Show balance", Commands::ShowBalance),
    goto("Back", ScreenTarget::Home),
];

pub(crate) const TX_ITEMS: &[MenuItem] = &[
    goto("Create transaction (planner)", ScreenTarget::CreateTx),
    prompt(
        "Send transaction file",
        "Transaction file path",
        TextThen::TxSendPath,
    ),
    prompt(
        "Show transaction file",
        "Transaction file path",
        TextThen::TxShowPath,
    ),
    prompt(
        "Sign multisig transaction",
        "Transaction file path",
        TextThen::TxSignMultisigTxFile,
    ),
    prompt(
        "Migrate v0 notes",
        "Destination v1 address (base58)",
        TextThen::TxMigrateDest,
    ),
    goto("Register .nock name (NNS)", ScreenTarget::NnsBuy),
    goto("Back", ScreenTarget::Home),
];

pub(crate) const WATCH_ITEMS: &[MenuItem] = &[
    prompt(
        "Address or pubkey",
        "Address or pubkey (base58)",
        TextThen::WatchAddr,
    ),
    prompt("Pubkey only", "Pubkey (base58)", TextThen::WatchPubkey),
    prompt("Multisig", "Threshold (m)", TextThen::TxMultisigThreshold),
    goto("Back", ScreenTarget::Home),
];

pub(crate) const SIGN_ITEMS: &[MenuItem] = &[
    prompt(
        "Sign message",
        "Message to sign",
        TextThen::SignMsgStepMessage,
    ),
    prompt("Verify message", "Message (plain text)", TextThen::VerifyMsgM),
    prompt("Sign hash", "Hash (base58)", TextThen::SignHashGetHash),
    prompt("Verify hash", "Hash (base58)", TextThen::VerifyHashFirst),
    goto("Back", ScreenTarget::Home),
];
