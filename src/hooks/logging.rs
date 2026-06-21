//! Help / logging text shown in the output panel (stderr logs are hidden behind the TUI).

pub(crate) fn help_text() -> String {
    "Quick commands\n\n\
     Type a wallet subcommand and press Enter to run it, e.g.:\n\
     \x20 show-balance\n\
     \x20 list-notes\n\
     \x20 list-active-addresses\n\n\
     Built-in:\n\
     \x20 help     show this help\n\
     \x20 verbose  logging info\n\
     \x20 menu     back to the wallet menu\n\
     \x20 exit     quit the TUI\n\
     \x20 Esc      back to Home"
        .to_string()
}

pub(crate) fn verbose_text() -> String {
    "Logging\n\n\
     Logs are written to stderr, which is hidden while the TUI holds the screen. \
     Restart with `-v` (or set `RUST_LOG` before launch) for more detail on the terminal \
     after you exit."
        .to_string()
}
