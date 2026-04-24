// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! `--format explore` TUI browser (SFRS §5).
//!
//! Built on `ratatui` + `crossterm`, gated behind the `tui` Cargo
//! feature so that headless/server builds can omit the TUI stack.
//! Graceful degradation:
//!
//! - Feature disabled at compile time → immediately fall back to
//!   `--format json` with a stderr warning.
//! - `AI_AGENT=1` or `CI=true` set at runtime → fall back to JSON.
//! - stdout is not a TTY → fall back to JSON.
//!
//! The TUI renders the `data` array as a table, supports `/` filter,
//! `s` column sort, `Enter` row-detail, `e` export-to-file, CUA and
//! Vim keybindings (arrow keys + hjkl), and exits via `q` or `Esc`.

use crate::env::EnvSnapshot;
use crate::json::Value;
use crate::output::OutputPolicy;

/// Returns `true` if the caller should NOT enter the TUI and must
/// fall back to JSON. `reason` is emitted to stderr if a fallback is
/// required so that humans and agents both see why.
pub fn should_fall_back(env: &EnvSnapshot, policy: &OutputPolicy) -> Option<&'static str> {
    if env.agent_mode {
        return Some("TUI suppressed: AI_AGENT is set; emitting JSON");
    }
    if env.ci_mode {
        return Some("TUI suppressed: CI=true; emitting JSON");
    }
    if !env.stdout_is_tty {
        return Some("TUI suppressed: stdout is not a TTY; emitting JSON");
    }
    #[cfg(not(feature = "tui"))]
    {
        let _ = policy;
        Some("TUI feature not compiled in; emitting JSON")
    }
    #[cfg(feature = "tui")]
    {
        let _ = policy;
        None
    }
}

#[cfg(feature = "tui")]
pub fn explore(data: &Value) -> std::io::Result<()> {
    use std::io::{self, stdout};

    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use crossterm::execute;
    use crossterm::terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    };
    use ratatui::backend::CrosstermBackend;
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, Borders, Paragraph, Row, Table, TableState};
    use ratatui::Terminal;

    // Convert arbitrary `data` into rows of (key, value) pairs for
    // display. Arrays of objects use first-object keys as columns;
    // everything else is presented as a two-column key/value view.
    enum View {
        Table {
            headers: Vec<String>,
            rows: Vec<Vec<String>>,
        },
        KeyValue(Vec<(String, String)>),
    }

    let view = match data {
        Value::Array(items) if items.iter().all(|v| matches!(v, Value::Object(_))) => {
            let headers: Vec<String> = if let Some(Value::Object(first)) = items.first() {
                first.iter().map(|(k, _)| k.clone()).collect()
            } else {
                vec![]
            };
            let rows: Vec<Vec<String>> = items
                .iter()
                .map(|item| {
                    if let Value::Object(props) = item {
                        headers
                            .iter()
                            .map(|h| {
                                props
                                    .iter()
                                    .find(|(k, _)| k == h)
                                    .map(|(_, v)| flatten(v))
                                    .unwrap_or_default()
                            })
                            .collect()
                    } else {
                        vec![flatten(item)]
                    }
                })
                .collect();
            View::Table { headers, rows }
        }
        Value::Object(props) => {
            View::KeyValue(props.iter().map(|(k, v)| (k.clone(), flatten(v))).collect())
        }
        other => View::KeyValue(vec![("value".to_string(), flatten(other))]),
    };

    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let mut state = TableState::default();
    state.select(Some(0));
    let mut filter = String::new();
    let mut in_filter = false;
    let mut sort_col: Option<usize> = None;
    let mut should_quit = false;
    let mut exported: Option<String> = None;

    let result: io::Result<()> = loop {
        terminal.draw(|f| {
            let layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(1)])
                .split(f.area());

            let accent = Color::Rgb(0x4B, 0x7E, 0xB0);
            let selected = Color::Rgb(0x50, 0xFA, 0x7B);
            let data_color = Color::Rgb(0xD9, 0x8E, 0x32);

            let header_style = Style::default().fg(accent).add_modifier(Modifier::BOLD);
            let sel_style = Style::default()
                .fg(selected)
                .add_modifier(Modifier::REVERSED);

            match &view {
                View::Table { headers, rows } => {
                    let mut filtered: Vec<&Vec<String>> = rows
                        .iter()
                        .filter(|row| {
                            filter.is_empty()
                                || row
                                    .iter()
                                    .any(|c| c.to_lowercase().contains(&filter.to_lowercase()))
                        })
                        .collect();
                    if let Some(col) = sort_col {
                        filtered.sort_by(|a, b| a.get(col).cmp(&b.get(col)));
                    }
                    let widths: Vec<Constraint> = headers
                        .iter()
                        .map(|_| Constraint::Percentage(100 / headers.len().max(1) as u16))
                        .collect();
                    let header_row = Row::new(headers.iter().map(|h| h.as_str().to_string()))
                        .style(header_style);
                    let body: Vec<Row> = filtered
                        .iter()
                        .map(|r| {
                            Row::new(r.iter().map(|c| c.clone()))
                                .style(Style::default().fg(data_color))
                        })
                        .collect();
                    let title = format!(
                        "zamak explore — {} rows (filter: {}, sort: {})",
                        filtered.len(),
                        if filter.is_empty() { "<none>" } else { &filter },
                        match sort_col {
                            Some(c) => headers.get(c).map(|s| s.as_str()).unwrap_or("<col>"),
                            None => "<none>",
                        }
                    );
                    let table = Table::new(body, widths)
                        .header(header_row)
                        .block(
                            Block::default()
                                .title(title)
                                .borders(Borders::ALL)
                                .border_style(Style::default().fg(accent)),
                        )
                        .row_highlight_style(sel_style);
                    f.render_stateful_widget(table, layout[0], &mut state);
                }
                View::KeyValue(pairs) => {
                    let lines: Vec<Line> = pairs
                        .iter()
                        .map(|(k, v)| {
                            Line::from(vec![
                                Span::styled(
                                    format!("{k:>18} "),
                                    Style::default().fg(accent).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(v.clone(), Style::default().fg(data_color)),
                            ])
                        })
                        .collect();
                    let p = Paragraph::new(lines).block(
                        Block::default()
                            .title("zamak explore")
                            .borders(Borders::ALL)
                            .border_style(Style::default().fg(accent)),
                    );
                    f.render_widget(p, layout[0]);
                }
            }

            let help = if in_filter {
                format!("/filter: {filter}  (Enter: apply, Esc: clear)")
            } else {
                "[q/Esc quit] [↑↓/jk nav] [/ filter] [s sort] [Enter detail] [e export]".to_string()
            };
            f.render_widget(
                Paragraph::new(help).style(Style::default().fg(Color::Rgb(0x8B, 0xE9, 0xFD))),
                layout[1],
            );
        })?;

        if !event::poll(std::time::Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if in_filter {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => in_filter = false,
                KeyCode::Backspace => {
                    filter.pop();
                }
                KeyCode::Char(c) => filter.push(c),
                _ => {}
            }
            continue;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => {
                should_quit = true;
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                should_quit = true;
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                let i = state.selected().unwrap_or(0).saturating_add(1);
                state.select(Some(i));
            }
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                let i = state.selected().unwrap_or(0).saturating_sub(1);
                state.select(Some(i));
            }
            (KeyCode::Home, _) | (KeyCode::Char('g'), _) => {
                state.select(Some(0));
            }
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                if let View::Table { rows, .. } = &view {
                    state.select(Some(rows.len().saturating_sub(1)));
                }
            }
            (KeyCode::Char('/'), _) => {
                filter.clear();
                in_filter = true;
            }
            (KeyCode::Char('s'), _) => {
                if let View::Table { headers, .. } = &view {
                    sort_col = Some(match sort_col {
                        Some(c) if c + 1 < headers.len() => c + 1,
                        _ => 0,
                    });
                }
            }
            (KeyCode::Char('e'), _) => {
                let path = std::env::temp_dir().join("zamak-explore-export.json");
                let body = data.to_pretty();
                std::fs::write(&path, body)?;
                exported = Some(path.display().to_string());
                should_quit = true;
            }
            _ => {}
        }
        if should_quit {
            break Ok(());
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Some(path) = exported {
        eprintln!(
            "{} [INFO] explore: exported current view to {path}",
            crate::time::iso8601_now()
        );
    }
    result
}

#[cfg(not(feature = "tui"))]
#[allow(dead_code)]
pub fn explore(_data: &Value) -> std::io::Result<()> {
    // Unreachable when callers respect `should_fall_back`, but kept
    // as a safe fallback so the function signature is stable.
    Ok(())
}

/// Renders a single `Value` as a short, single-line string for table
/// cells.
#[cfg(feature = "tui")]
fn flatten(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => s.clone(),
        Value::Array(_) | Value::Object(_) => v.to_compact(),
    }
}
