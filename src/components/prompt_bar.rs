//! Bottom-bar prompts (text entry and yes/no) — activity panel keeps the underlay screen.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::menus::BOOL;
use super::theme::{THEME_ACCENT_GREEN, THEME_BG_DEEP, THEME_MUTED};
use crate::app_state::AppState;
use crate::screens::Overlay;

// Top border (1) + title (1) + input/options (1). With only 2, the `Borders::TOP` ate the input row.
const PROMPT_BAR_LINES: u16 = 3;

pub(crate) fn prompt_bar_height(overlay: &Option<Overlay>) -> u16 {
    if overlay.is_some() {
        PROMPT_BAR_LINES
    } else {
        0
    }
}

pub(crate) fn draw_prompt_bar(f: &mut Frame<'_>, app: &AppState, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(THEME_ACCENT_GREEN))
        .style(Style::default().bg(THEME_BG_DEEP));
    let inner = block.inner(area);
    f.render_widget(block, area);

    match &app.overlay {
        Some(Overlay::Prompt { title, value, .. }) => draw_text_prompt(f, inner, title, value),
        Some(Overlay::Confirm {
            title, sel, labels, ..
        }) => draw_confirm_prompt(f, inner, title, labels, *sel),
        Some(Overlay::ExitConfirm { sel }) => {
            draw_confirm_prompt(f, inner, "Exit TUI?", BOOL, *sel)
        }
        None => {}
    }
}

fn draw_text_prompt(f: &mut Frame<'_>, area: Rect, title: &str, value: &str) {
    let input = format!("> {value}_");
    let lines = vec![
        Line::from(vec![Span::styled(
            title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            input,
            Style::default().fg(THEME_ACCENT_GREEN),
        )]),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .alignment(Alignment::Left),
        area,
    );
}

fn draw_confirm_prompt(f: &mut Frame<'_>, area: Rect, title: &str, labels: &[&str], sel: usize) {
    let mut option_spans: Vec<Span> = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            option_spans.push(Span::styled("  ·  ", Style::default().fg(THEME_MUTED)));
        }
        let style = if i == sel {
            Style::default()
                .fg(Color::Black)
                .bg(THEME_ACCENT_GREEN)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(THEME_MUTED)
        };
        let prefix = if i == sel { "▸ " } else { "  " };
        option_spans.push(Span::styled(format!("{prefix}{label}"), style));
    }

    let lines = if area.height >= 2 {
        vec![
            Line::from(vec![Span::styled(
                title,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(option_spans),
        ]
    } else {
        let mut merged = vec![Span::styled(
            format!("{title}  "),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )];
        merged.extend(option_spans);
        vec![Line::from(merged)]
    };

    f.render_widget(Paragraph::new(lines), area);
}
