use chrono::DateTime;
use human_repr::{HumanCount, HumanThroughput};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, Gauge, Paragraph, Row, Table, Wrap,
};

use super::app::{App, Category, InputMode, View, category_label};

// Dark-canvas palette: teal primary, cyan secondary, purple accent, green
// highlight, orange link, sky blue, red error, light-gray muted. Borders keep
// the light ship-hull brown.
const PRIMARY: Color = Color::Rgb(0, 232, 198);
const SECONDARY: Color = Color::Rgb(0, 198, 232);
const ACCENT: Color = Color::Rgb(130, 100, 255);
const HIGHLIGHT: Color = Color::Rgb(100, 232, 130);
const LINK: Color = Color::Rgb(255, 160, 100);
const SKY: Color = Color::Rgb(0, 210, 255);
const ERROR: Color = Color::Rgb(255, 100, 100);
const MUTED: Color = Color::Rgb(200, 210, 220);
const TEXT: Color = Color::Rgb(0xEC, 0xEC, 0xEC);
const BROWN: Color = Color::Rgb(0xCB, 0xA1, 0x72);
const SEL_BG: Color = Color::Rgb(0x24, 0x2C, 0x36);

pub fn render(frame: &mut Frame, app: &mut App) {
    let has_downloads = !app.downloads.is_empty();
    let dl_height = if has_downloads {
        (app.downloads.len().min(6) as u16) + 2
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(dl_height),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    render_view_tabs(frame, app, chunks[1]);
    render_tabs(frame, app, chunks[2]);
    render_results(frame, app, chunks[3]);
    if has_downloads {
        render_downloads(frame, app, chunks[4]);
    }
    render_footer(frame, app, chunks[5]);

    if app.show_detail {
        render_detail(frame, app);
    }
    if app.show_help {
        render_help(frame);
    }
}

fn render_header(frame: &mut Frame, app: &App, area: Rect) {
    let editing = app.input_mode == InputMode::Editing;
    let border = if editing { ACCENT } else { MUTED };
    let title = if app.category == Category::All {
        " Search "
    } else {
        " Filter (switch to All to search) "
    };
    let query_line = if app.query.is_empty() && !editing {
        Span::styled(
            "type to search torrents…",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )
    } else {
        Span::styled(app.query.clone(), Style::default().fg(SKY).bold())
    };
    let search = Paragraph::new(Line::from(vec![
        Span::styled("🔍 ", Style::default().fg(PRIMARY)),
        query_line,
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border))
            .title(Span::styled(title, Style::default().fg(border).bold())),
    );
    frame.render_widget(search, area);

    if editing {
        let x = area.x + 4 + app.cursor as u16;
        let y = area.y + 1;
        frame.set_cursor_position((x.min(area.x + area.width.saturating_sub(2)), y));
    }
}

fn render_view_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BROWN))
        .title(Span::styled(" View ", Style::default().fg(SKY).bold()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans: Vec<Span> = Vec::new();
    for (index, view) in View::ORDER.iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled("  ", Style::default().fg(MUTED)));
        }
        let selected = *view == app.view;
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(SKY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT)
        };
        spans.push(Span::styled(format!(" {} ", view.title()), style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let categories_apply = app.view == View::Search;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(MUTED))
        .title(Span::styled(
            " Categories ",
            Style::default().fg(PRIMARY).bold(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans: Vec<Span> = Vec::new();
    for (i, cat) in Category::ORDER.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", Style::default().fg(MUTED)));
        }
        let selected = categories_apply && *cat == app.category;
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else if categories_apply {
            Style::default().fg(TEXT)
        } else {
            Style::default().fg(MUTED)
        };
        spans.push(Span::styled(format!(" {} ", cat.title()), style));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), inner);
}

fn render_results(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = if app.loading {
        Span::styled(" ⟳ loading… ", Style::default().fg(LINK).bold())
    } else {
        let label = match app.view {
            View::Trending => "Trending",
            View::Search => "Results",
        };
        Span::styled(
            format!(" {label} ({}) ", app.results.len()),
            Style::default().fg(SKY).bold(),
        )
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BROWN))
        .title(title);

    if app.results.is_empty() {
        let msg = if app.loading {
            "Fetching torrents…"
        } else if app.view == View::Trending {
            "No popular torrents in the last 48 hours."
        } else {
            "No torrents to show.\n\nPress  /  to search, or  ← →  to browse a category."
        };
        let paragraph = Paragraph::new(msg)
            .style(Style::default().fg(MUTED))
            .alignment(Alignment::Center)
            .block(block)
            .wrap(Wrap { trim: true });
        frame.render_widget(paragraph, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("Category"),
        Cell::from("Name"),
        Cell::from("Size"),
        Cell::from("Uploaded"),
        Cell::from("SE"),
        Cell::from("LE"),
        Cell::from("Uploader"),
    ])
    .style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
    .bottom_margin(1);

    let rows = app.results.iter().map(|torrent| {
        let (icon, label) = category_label(torrent.category.as_deref().unwrap_or(""));
        let download = app.downloads.get(&torrent.id);
        let marker = match download {
            Some(item) if item.error.is_some() => Span::styled("✗", Style::default().fg(ERROR)),
            Some(item) if item.done => Span::styled("✓", Style::default().fg(HIGHLIGHT)),
            Some(_) => Span::styled(
                "▼",
                Style::default().fg(PRIMARY).add_modifier(Modifier::BOLD),
            ),
            None => Span::raw(" "),
        };
        Row::new(vec![
            Cell::from(Line::from(marker)),
            Cell::from(format!("{icon} {label}")).style(Style::default().fg(MUTED)),
            Cell::from(torrent.name.clone()).style(Style::default().fg(SKY)),
            Cell::from(human_bytes(torrent.size_bytes)).style(Style::default().fg(SECONDARY)),
            Cell::from(human_date(torrent.added)).style(Style::default().fg(MUTED)),
            Cell::from(torrent.seeders.to_string()).style(Style::default().fg(HIGHLIGHT)),
            Cell::from(torrent.leechers.to_string()).style(Style::default().fg(ERROR)),
            Cell::from(torrent.uploaded_by.clone().unwrap_or_else(|| "—".into()))
                .style(Style::default().fg(ACCENT)),
        ])
    });

    let widths = [
        Constraint::Length(1),
        Constraint::Length(10),
        Constraint::Min(20),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(14),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(
            Style::default()
                .bg(SEL_BG)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(Span::styled("▶ ", Style::default().fg(PRIMARY)))
        .column_spacing(1);

    frame.render_stateful_widget(table, area, &mut app.table_state);
}

fn render_downloads(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BROWN))
        .title(Span::styled(
            format!(
                " Downloads · {} active → {} ",
                app.active_downloads(),
                app.output_dir.display()
            ),
            Style::default().fg(PRIMARY).bold(),
        ));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Length(1); inner.height as usize])
        .split(inner);

    for (download, row) in app
        .downloads
        .values()
        .take(inner.height as usize)
        .zip(rows.iter())
    {
        let ratio = if download.total > 0 {
            (download.downloaded as f64 / download.total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let name = truncate(&download.name, 34);
        let label = if let Some(error) = &download.error {
            format!("✗ {name} — {error}")
        } else if download.done {
            format!("✓ {name} — complete")
        } else {
            format!(
                "▼ {name} — {} / {} @ {}",
                human_bytes(download.downloaded),
                human_bytes(download.total),
                download.speed.human_throughput_bytes()
            )
        };
        let color = if download.error.is_some() {
            ERROR
        } else if download.done {
            HIGHLIGHT
        } else {
            PRIMARY
        };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(color).bg(SEL_BG))
            .ratio(if download.error.is_some() { 1.0 } else { ratio })
            .label(Span::styled(
                label,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        frame.render_widget(gauge, *row);
    }
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) {
    let hint = |key: &'static str, description: &'static str| -> Vec<Span<'static>> {
        vec![
            Span::styled(
                format!(" {key} "),
                Style::default().fg(Color::Black).bg(PRIMARY).bold(),
            ),
            Span::styled(format!(" {description}  "), Style::default().fg(MUTED)),
        ]
    };

    let mut spans: Vec<Span> = Vec::new();
    if app.input_mode == InputMode::Editing {
        spans.extend(hint("Enter", "search"));
        spans.extend(hint("Esc", "cancel"));
        spans.extend(hint("⌫", "delete"));
    } else {
        spans.extend(hint("↑↓/jk", "move"));
        spans.extend(hint("←→", "category"));
        spans.extend(hint(
            "t",
            if app.view == View::Trending {
                "search"
            } else {
                "trending"
            },
        ));
        spans.extend(hint("/", "search"));
        spans.extend(hint("d", "download"));
        spans.extend(hint("c", "copy magnet"));
        spans.extend(hint("Enter", "details"));
        spans.extend(hint("?", "help"));
        spans.extend(hint("q", "quit"));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);

    let status_line = Line::from(vec![Span::styled(
        format!("{}  ", app.status),
        Style::default().fg(ACCENT),
    )])
    .alignment(Alignment::Right);
    frame.render_widget(Paragraph::new(status_line), area);
}

fn render_detail(frame: &mut Frame, app: &App) {
    let Some(torrent) = app.selected() else {
        return;
    };
    let area = centered_rect(72, 70, frame.area());
    frame.render_widget(Clear, area);

    let (icon, label) = category_label(torrent.category.as_deref().unwrap_or(""));
    let magnet = torrent.resolved_magnet();

    let field = |key: &str, value: String, color: Color| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("  {key:<11}"), Style::default().fg(ACCENT).bold()),
            Span::styled(value, Style::default().fg(color)),
        ])
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!("  {}", torrent.name),
            Style::default().fg(SKY).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        field(
            "Category",
            format!(
                "{icon} {label} ({})",
                torrent.category.as_deref().unwrap_or("—")
            ),
            TEXT,
        ),
        field("Size", human_bytes(torrent.size_bytes), SECONDARY),
        field("Seeders", torrent.seeders.to_string(), HIGHLIGHT),
        field("Leechers", torrent.leechers.to_string(), ERROR),
        field("Uploaded", human_date(torrent.added), MUTED),
        field(
            "Uploader",
            torrent.uploaded_by.clone().unwrap_or_else(|| "—".into()),
            ACCENT,
        ),
        field("Info hash", torrent.info_hash.clone(), MUTED),
        Line::from(""),
        Line::from(Span::styled("  Magnet", Style::default().fg(ACCENT).bold())),
        Line::from(Span::styled(
            format!(
                "  {}",
                truncate(&magnet, area.width.saturating_sub(4) as usize)
            ),
            Style::default().fg(LINK),
        )),
    ];

    if let Some(description) = torrent
        .description
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Description",
            Style::default().fg(ACCENT).bold(),
        )));
        for line in description.lines().take(6) {
            lines.push(Line::from(Span::styled(
                format!("  {line}"),
                Style::default().fg(Color::Gray),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  d ", Style::default().fg(Color::Black).bg(PRIMARY).bold()),
        Span::styled(" download   ", Style::default().fg(MUTED)),
        Span::styled(" c ", Style::default().fg(Color::Black).bg(LINK).bold()),
        Span::styled(" copy magnet   ", Style::default().fg(MUTED)),
        Span::styled(" Esc ", Style::default().fg(Color::Black).bg(SKY).bold()),
        Span::styled(" close", Style::default().fg(MUTED)),
    ]));

    let popup = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(BROWN))
                .title(Span::styled(
                    " Torrent details ",
                    Style::default().fg(SKY).bold(),
                )),
        );
    frame.render_widget(popup, area);
}

fn render_help(frame: &mut Frame) {
    let area = centered_rect(56, 62, frame.area());
    frame.render_widget(Clear, area);

    let key = |binding: &'static str, description: &'static str| -> Line<'static> {
        Line::from(vec![
            Span::styled(format!("   {binding:<12}"), Style::default().fg(SKY).bold()),
            Span::styled(description, Style::default().fg(TEXT)),
        ])
    };

    let lines = vec![
        Line::from(""),
        key("/  or  i", "focus the search box"),
        key("Enter", "run search / open details"),
        key("Esc", "leave search / close popup"),
        key("↑ ↓  j k", "move selection"),
        key("PgUp PgDn", "jump by a page"),
        key("g  G", "jump to top / bottom"),
        key("t", "switch Search / Trending"),
        key("← →", "previous / next category"),
        key("d", "download the selected torrent"),
        key("c  or  y", "copy magnet link to clipboard"),
        key("r", "refresh current view"),
        key("?", "toggle this help"),
        key("q", "quit"),
        Line::from(""),
        Line::from(Span::styled(
            "   Downloads land in the shown output folder.",
            Style::default().fg(MUTED).add_modifier(Modifier::ITALIC),
        )),
    ];

    let popup = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(BROWN))
            .title(Span::styled(
                " ☠ Keybindings ",
                Style::default().fg(SKY).bold(),
            )),
    );
    frame.render_widget(popup, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn human_bytes(bytes: u64) -> String {
    bytes.human_count_bytes().to_string()
}

fn human_date(added: Option<u64>) -> String {
    added
        .and_then(|timestamp| DateTime::from_timestamp(timestamp as i64, 0))
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "—".into())
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        let cut: String = value.chars().take(max.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats_si_units() {
        assert_eq!(human_bytes(1024), "1kB");
        assert_eq!(human_bytes(0), "0B");
        assert_eq!(human_bytes(2_500_000), "2.5MB");
    }

    #[test]
    fn human_date_formats_unix_timestamp() {
        assert_eq!(human_date(Some(1_613_347_200)), "2021-02-15");
        assert_eq!(human_date(None), "—");
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hell…");
        assert_eq!(truncate("héllö", 10), "héllö");
    }
}
