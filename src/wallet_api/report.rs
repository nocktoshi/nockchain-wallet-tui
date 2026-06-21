//! TUI-owned command response contract: typed `events` + normalized `reports`.
//!
//! This envelope is what the TUI (over loopback HTTP) and any future web UI consume. Kernel
//! `%markdown` is normalized into [`Report`]s **server-side** ([`super::normalize`]) and never
//! crosses this boundary. As upstream grafts structured `[%wallet …]` effects, `events` grow and
//! `reports` shrink — the contract is unchanged.

use serde::{Deserialize, Serialize};

use nockchain_wallet::wallet_outcome::WalletEvent;

/// Top-level schema id for [`TuiCommandResponse`].
pub const TUI_OUTCOME_SCHEMA: &str = "tui-wallet-outcome-v1";

/// A structured-but-textual fragment of a command [`Report`].
///
/// A web UI renders these as real elements; the TUI flattens them to terminal text via
/// [`Report::to_text`]. `Raw` is the explicit fallback for kernel markdown not yet grafted to
/// structured events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Section {
    Heading { text: String },
    Text { text: String },
    KeyValue { key: String, value: String },
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
    /// Unstructured passthrough (kernel markdown without a structured event yet).
    Raw { text: String },
}

impl Section {
    pub fn heading(text: impl Into<String>) -> Self {
        Section::Heading { text: text.into() }
    }
    pub fn text(text: impl Into<String>) -> Self {
        Section::Text { text: text.into() }
    }
    pub fn kv(key: impl Into<String>, value: impl Into<String>) -> Self {
        Section::KeyValue {
            key: key.into(),
            value: value.into(),
        }
    }
    pub fn raw(text: impl Into<String>) -> Self {
        Section::Raw { text: text.into() }
    }

    /// Flatten one section to terminal text lines.
    fn to_text(&self) -> String {
        match self {
            Section::Heading { text } => format!("## {text}"),
            Section::Text { text } => text.clone(),
            Section::KeyValue { key, value } => format!("- {key}: {value}"),
            Section::Raw { text } => text.clone(),
            Section::Table { headers, rows } => {
                let mut lines = Vec::with_capacity(rows.len() + 2);
                lines.push(format!("| {} |", headers.join(" | ")));
                lines.push(format!(
                    "| {} |",
                    headers.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")
                ));
                for row in rows {
                    lines.push(format!("| {} |", row.join(" | ")));
                }
                lines.join("\n")
            }
        }
    }
}

/// One normalized command report — produced per structured event, or per markdown-only command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    /// Kebab command id (e.g. `show-balance`) for grouping / web routing.
    pub command: String,
    pub title: String,
    pub sections: Vec<Section>,
}

impl Report {
    pub fn new(command: impl Into<String>, title: impl Into<String>, sections: Vec<Section>) -> Self {
        Self {
            command: command.into(),
            title: title.into(),
            sections,
        }
    }

    /// Flatten this report to terminal text for the TUI output panel.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        if !self.title.is_empty() {
            out.push_str(&format!("## {}\n", self.title));
        }
        for section in &self.sections {
            // Headings already start with `## `; avoid double-heading the title line above.
            out.push_str(&section.to_text());
            out.push('\n');
        }
        out.trim_end().to_string()
    }
}

/// Flatten a list of reports into one terminal-text blob (TUI output panel).
pub fn reports_to_text(reports: &[Report]) -> String {
    reports
        .iter()
        .map(Report::to_text)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// TUI-owned response envelope: typed events + normalized reports + optional error.
///
/// Round-trippable (the loopback HTTP client deserializes it), so `schema_version` is owned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiCommandResponse {
    pub schema_version: String,
    pub events: Vec<WalletEvent>,
    pub reports: Vec<Report>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TuiCommandResponse {
    pub fn ok(events: Vec<WalletEvent>, reports: Vec<Report>) -> Self {
        Self {
            schema_version: TUI_OUTCOME_SCHEMA.to_string(),
            events,
            reports,
            error: None,
        }
    }

    pub fn failure(msg: impl Into<String>) -> Self {
        Self {
            schema_version: TUI_OUTCOME_SCHEMA.to_string(),
            events: Vec::new(),
            reports: Vec::new(),
            error: Some(msg.into()),
        }
    }

    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_json_shape_for_web_client() {
        let resp = TuiCommandResponse::ok(
            vec![WalletEvent::BalanceSnapshotV1 {
                wallet_version: 1,
                block_id_b58: "blk".into(),
                height: 7,
                note_count: 2,
                total_assets: 131072,
            }],
            vec![Report::new(
                "show-balance",
                "Balance",
                vec![
                    Section::kv("height", "7"),
                    Section::Table {
                        headers: vec!["a".into()],
                        rows: vec![vec!["1".into()]],
                    },
                ],
            )],
        );
        let v: serde_json::Value = serde_json::to_value(&resp).unwrap();
        assert_eq!(v["schema_version"], TUI_OUTCOME_SCHEMA);
        // Events keep their upstream `kind` tag; reports are typed sections.
        assert_eq!(v["events"][0]["kind"], "balance_snapshot_v1");
        assert_eq!(v["reports"][0]["command"], "show-balance");
        assert_eq!(v["reports"][0]["sections"][0]["type"], "key_value");
        assert_eq!(v["reports"][0]["sections"][1]["type"], "table");
        assert!(v.get("error").is_none());
    }

    #[test]
    fn report_flattens_to_terminal_text() {
        let r = Report::new("show-balance", "Balance", vec![Section::kv("height", "7")]);
        assert_eq!(r.to_text(), "## Balance\n- height: 7");
    }
}
