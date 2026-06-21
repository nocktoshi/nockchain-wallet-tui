//! Static menu labels and error-action lists. List menus with behavior live in [`crate::actions`];
//! the labels below are for menus whose handlers are bespoke (home, settings) or option lists.

pub(crate) const MAIN_MENU: &[&str] = &[
    "Keys & addresses",
    "Notes & balance",
    "Transactions",
    "Watch-only",
    "Sign / verify",
    "Settings & help",
    "Quick commands (command line)",
    "Exit",
];

pub(crate) const SETTINGS_MENU: &[&str] = &[
    "Public gRPC server",
    "JSON API listen",
    "API token & curl examples",
    "Show help again",
    "Verbose / logging info",
    "Back",
];

pub(crate) const BOOL: &[&str] = &["Yes", "No"];

pub(crate) const NOTE_ORDER: &[&str] = &["Ascending", "Descending"];

pub(crate) const CT_ERR_ACTIONS: &[&str] = &[
    "Retry",
    "Edit planning options",
    "Start over (new recipients)",
    "Back to Transactions menu",
];

pub(crate) const GENERIC_ERR: &[&str] = &["Retry", "Back"];
