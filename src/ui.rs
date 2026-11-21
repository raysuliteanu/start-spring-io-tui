use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{App, FIELDS, Field, Mode, Pane};
use crate::cli::version_info;
use crate::model::Metadata;

const HELP: &str = "Tab pane · ↑↓ move · ←→ choose · Enter edit · a add · d remove · X clear · g generate · ? about · q quit";
const SELECTED: Color = Color::LightBlue;
const WELL: Color = Color::DarkGray;
const LABEL_WIDTH: usize = 13;

pub fn draw(frame: &mut Frame, app: &App) {
    let [main, status] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    frame.render_widget(status_bar(app), status);

    let Some(metadata) = &app.metadata else {
        let message = match app.mode {
            Mode::Connecting => "Connecting to start.spring.io…",
            _ => "No cached options — run once while online (q to quit)",
        };
        let [_, middle, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(main);
        frame.render_widget(Paragraph::new(message).alignment(Alignment::Center), middle);
        return;
    };

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)]).areas(main);
    draw_project(frame, app, metadata, left);
    draw_selected(frame, app, right);
    if app.picker.is_some() {
        draw_picker(frame, app, main);
    }
    if app.show_about {
        draw_about(frame, main);
    }
}

fn focused_block(title: String, focused: bool) -> Block<'static> {
    let style = if focused {
        Style::new().fg(Color::Green)
    } else {
        Style::new()
    };
    Block::bordered().title(title).border_style(style)
}

fn choice_name(metadata: &Metadata, field: Field, id: &str) -> String {
    let name = match field {
        Field::Project => metadata
            .project_type
            .values
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.name.clone()),
        Field::Language => find_name(&metadata.language.values, id),
        Field::Boot => find_name(&metadata.boot_version.values, id),
        Field::Java => find_name(&metadata.java_version.values, id),
        Field::Packaging => find_name(&metadata.packaging.values, id),
        _ => None,
    };
    name.unwrap_or_else(|| id.to_string())
}

fn find_name(values: &[crate::model::Choice], id: &str) -> Option<String> {
    values.iter().find(|v| v.id == id).map(|v| v.name.clone())
}

fn draw_project(frame: &mut Frame, app: &App, metadata: &Metadata, area: Rect) {
    let offline = matches!(app.mode, Mode::Offline { .. });
    // Inside the borders, after the label column, minus the two well edges.
    let well_width = (area.width as usize).saturating_sub(2 + LABEL_WIDTH + 2);
    let lines: Vec<Line> = FIELDS
        .iter()
        .map(|&field| {
            let selected = app.pane == Pane::Project && app.field == field;
            let value = if field.is_text() {
                let mut v = match field {
                    Field::Group => app.req.group_id.clone(),
                    Field::Artifact => app.req.artifact_id.clone(),
                    Field::Name => app.req.name.clone(),
                    Field::Description => app.req.description.clone(),
                    _ => app.req.package_name.clone(),
                };
                if selected && app.editing {
                    v.push('▏');
                }
                v
            } else {
                let id = match field {
                    Field::Project => &app.req.project_type,
                    Field::Language => &app.req.language,
                    Field::Boot => &app.req.boot_version,
                    Field::Java => &app.req.java_version,
                    _ => &app.req.packaging,
                };
                let mut name = choice_name(metadata, field, id);
                if field == Field::Boot && offline && app.cached_boots.contains(id) {
                    name.push_str(" (cached)");
                }
                format!("◂ {name} ▸")
            };
            let label = Span::styled(
                format!("{:<LABEL_WIDTH$}", field.label()),
                Style::new().fg(Color::Cyan),
            );
            let mut style = Style::new();
            if selected {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if !field.is_text() {
                return Line::from(vec![label, Span::styled(value, style)]);
            }
            // Text fields sit in a shaded "well" so empty ones are still visible.
            let editing = selected && app.editing;
            let len = value.chars().count();
            let value: String = if len <= well_width {
                value
            } else if editing {
                // Keep the end (and the cursor) in view while typing.
                value.chars().skip(len - well_width).collect()
            } else {
                truncate(&value, well_width)
            };
            let padded = format!("{value:<well_width$}");
            let edge = Style::new().fg(WELL);
            Line::from(vec![
                label,
                Span::styled("▕", edge),
                Span::styled(padded, style.bg(WELL)),
                Span::styled("▏", edge),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(focused_block(" Project ".into(), app.pane == Pane::Project)),
        area,
    );
}

/// Cuts `text` to `width` characters, ending with `…` when shortened.
fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let mut out: String = text.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn draw_selected(frame: &mut Frame, app: &App, area: Rect) {
    let selected = app.selected_dependencies();
    let title = format!(" Dependencies ({}) · a: add ", selected.len());
    let focused = app.pane == Pane::Dependencies;
    let block = focused_block(title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if selected.is_empty() {
        frame.render_widget(
            Paragraph::new("No dependencies — press a to add").style(Style::new().fg(WELL)),
            inner,
        );
        return;
    }
    let width = inner.width as usize;
    let items: Vec<ListItem> = selected
        .iter()
        .map(|(group, dep)| {
            let tail = format!("{group} ✕");
            let name = truncate(&dep.name, width.saturating_sub(tail.chars().count() + 1));
            let pad = width.saturating_sub(name.chars().count() + tail.chars().count());
            let first = Line::from(vec![
                Span::styled(name, Style::new().fg(SELECTED)),
                Span::raw(" ".repeat(pad)),
                Span::raw(tail),
            ]);
            let description = dep.description.as_deref().unwrap_or("");
            let second = Line::styled(
                format!("  {}", truncate(description, width.saturating_sub(2))),
                Style::new().fg(WELL),
            );
            ListItem::new(vec![first, second])
        })
        .collect();
    let highlight = if focused {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new()
    };
    let mut state = ListState::default().with_selected(Some(app.sel_cursor));
    frame.render_stateful_widget(
        List::new(items).highlight_style(highlight),
        inner,
        &mut state,
    );
}

fn draw_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Some(picker) = &app.picker else {
        return;
    };
    let width = (area.width * 4 / 5).max(20).min(area.width);
    let height = (area.height * 4 / 5).max(6).min(area.height);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .title(" Add dependencies ")
        .border_style(Style::new().fg(Color::Green));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [search_area, list_area, help_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(format!("Search: {}▏", picker.filter)),
        search_area,
    );
    frame.render_widget(
        Paragraph::new("Space toggle · Enter OK · Esc cancel").style(Style::new().fg(WELL)),
        help_area,
    );

    let width = list_area.width as usize;
    let filtering = !picker.filter.is_empty();
    let mut items = Vec::new();
    let mut selected_row = None;
    let mut last_group = None;
    for (i, (group, dep, compatible)) in app.visible_dependencies().into_iter().enumerate() {
        if !filtering && last_group != Some(group) {
            items.push(ListItem::new(Line::styled(
                group.to_string(),
                Style::new().add_modifier(Modifier::BOLD).fg(Color::Yellow),
            )));
            last_group = Some(group);
        }
        let selected = app.is_selected(&dep.id);
        let mark = if selected { "[x] " } else { "[ ] " };
        let name_style = match (compatible, selected) {
            (false, _) => Style::new().fg(WELL),
            (true, true) => Style::new().fg(SELECTED),
            (true, false) => Style::new(),
        };
        let mut spans = vec![
            Span::styled(mark, name_style),
            Span::styled(dep.name.clone(), name_style),
        ];
        if filtering {
            let used = mark.chars().count() + dep.name.chars().count();
            let pad = width.saturating_sub(used + group.chars().count());
            spans.push(Span::raw(" ".repeat(pad.max(1))));
            spans.push(Span::styled(group.to_string(), Style::new().fg(WELL)));
        }
        if i == picker.cursor {
            selected_row = Some(items.len());
        }
        items.push(ListItem::new(Line::from(spans)));
    }
    let mut state = ListState::default().with_selected(selected_row);
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
        list_area,
        &mut state,
    );
}

fn cache_age(cached_at: SystemTime) -> String {
    let secs = SystemTime::now()
        .duration_since(cached_at)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    match secs {
        s if s >= 86_400 => format!("{}d", s / 86_400),
        s if s >= 3_600 => format!("{}h", s / 3_600),
        s => format!("{}m", s / 60),
    }
}

fn status_bar(app: &App) -> Paragraph<'static> {
    let mode_style = match app.mode {
        Mode::Online => Style::new().fg(Color::Black).bg(Color::Green),
        Mode::Connecting => Style::new().fg(Color::Black).bg(Color::Yellow),
        Mode::Offline { .. } => Style::new().fg(Color::Black).bg(Color::Red),
    };
    let mut spans = vec![Span::styled(format!(" {} ", app.mode_label()), mode_style)];
    if let Mode::Offline {
        cached_at: Some(t), ..
    } = app.mode
    {
        spans.push(Span::raw(format!(" · cache {} old", cache_age(t))));
    }
    let message = if app.status.is_empty() {
        HELP.to_string()
    } else {
        app.status.clone()
    };
    spans.push(Span::raw(format!(" · {message}")));
    Paragraph::new(Line::from(spans))
}

fn draw_about(frame: &mut Frame, area: Rect) {
    let info = version_info();
    let width = 50.min(area.width);
    let height = 8.min(area.height);
    let popup = Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    };
    let dirty = if info.git_dirty { " (dirty)" } else { "" };
    let lines = vec![
        Line::from(format!("start-spring-io-tui {}", info.version)),
        Line::from(format!("commit:  {}{dirty}", info.git_sha)),
        Line::from(format!("built:   {}", info.build_date)),
        Line::from(format!("rustc:   {}", info.rustc)),
        Line::from(""),
        Line::from("any key to close"),
    ];
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(Block::bordered().title(" About ")),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Mode;
    use crate::model::tests::metadata;
    use crate::worker::Event;
    use ratatui::{Terminal, backend::TestBackend};

    fn render(app: &App, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(|f| draw(f, app)).expect("draws");
        terminal
    }

    fn text(terminal: &Terminal<TestBackend>) -> String {
        terminal.backend().to_string()
    }

    fn offline_app(requested: bool) -> App {
        let mut app = App::new(Mode::Offline {
            requested,
            cached_at: None,
        });
        app.on_event(Event::Offline {
            metadata: Some(metadata()),
            cached_at: None,
            cached_boots: vec!["4.2.0".into()],
        });
        app
    }

    #[test]
    fn status_bar_shows_mode() {
        let screen = text(&render(&offline_app(true), 80, 24));
        assert!(screen.contains("OFFLINE"), "{screen}");
        assert!(!screen.contains("no connection"), "{screen}");
    }

    #[test]
    fn offline_marks_cached_boot() {
        let screen = text(&render(&offline_app(false), 100, 24));
        assert!(screen.contains("4.2.0 (cached)"), "{screen}");
    }

    #[test]
    fn about_popup_shows_sha() {
        let mut app = offline_app(true);
        app.show_about = true;
        let screen = text(&render(&app, 80, 24));
        assert!(
            screen.contains(crate::cli::version_info().git_sha),
            "{screen}"
        );
    }

    #[test]
    fn tiny_terminal_does_not_panic() {
        let mut app = offline_app(true);
        app.on_key(ratatui::crossterm::event::KeyEvent::from(
            ratatui::crossterm::event::KeyCode::Char('a'),
        ));
        let last = app.visible_dependencies().len() - 1;
        app.picker.as_mut().expect("open").cursor = last;
        app.pane = crate::app::Pane::Dependencies;
        render(&app, 40, 10);
    }

    #[test]
    fn filtered_list_shows_category() {
        let mut app = offline_app(true);
        app.picker = Some(crate::app::Picker {
            filter: "sweb".into(),
            ..Default::default()
        });
        let screen = text(&render(&app, 100, 24));
        assert!(screen.contains("Spring Web"), "{screen}");
        assert!(screen.contains("Web"), "{screen}");
    }

    #[test]
    fn no_cache_message() {
        let mut app = App::new(Mode::Offline {
            requested: true,
            cached_at: None,
        });
        app.on_event(Event::Offline {
            metadata: None,
            cached_at: None,
            cached_boots: vec![],
        });
        let screen = text(&render(&app, 80, 24));
        assert!(screen.contains("No cached options"), "{screen}");
    }

    #[test]
    fn default_screen_snapshot() {
        let mut app = App::new(Mode::Connecting);
        app.on_event(Event::Online(metadata()));
        app.req.dependencies = vec!["web".into(), "lombok".into()];
        insta::assert_snapshot!(render(&app, 100, 24).backend());
    }

    fn online_app(deps: &[&str]) -> App {
        let mut app = App::new(Mode::Connecting);
        app.on_event(Event::Online(metadata()));
        app.req.dependencies = deps.iter().map(|d| d.to_string()).collect();
        app
    }

    fn open_picker(app: &mut App, filter: &str, draft: &[&str]) {
        app.picker = Some(crate::app::Picker {
            draft: draft.iter().map(|d| d.to_string()).collect(),
            filter: filter.into(),
            cursor: 0,
        });
    }

    /// Position of the first cell of `needle` in the rendered buffer.
    fn find(terminal: &Terminal<TestBackend>, needle: &str) -> (u16, u16) {
        let buf = terminal.backend().buffer();
        for y in 0..buf.area.height {
            let row: String = (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            if let Some(byte) = row.find(needle) {
                let x = row[..byte].chars().count() as u16;
                return (x, y);
            }
        }
        panic!("{needle} not on screen");
    }

    #[test]
    fn main_pane_lists_selected_with_remove_marker() {
        let screen = text(&render(&online_app(&["web", "lombok"]), 100, 24));
        for expected in ["Spring Web", "Lombok", "✕", "Dependencies (2)"] {
            assert!(screen.contains(expected), "{expected} missing:\n{screen}");
        }
        assert!(!screen.contains("Spring Reactive Web"), "{screen}");
    }

    #[test]
    fn main_pane_empty_message() {
        let screen = text(&render(&online_app(&[]), 100, 24));
        assert!(
            screen.contains("No dependencies — press a to add"),
            "{screen}"
        );
    }

    #[test]
    fn selected_name_is_light_blue() {
        let terminal = render(&online_app(&["web"]), 100, 24);
        let (x, y) = find(&terminal, "Spring Web");
        assert_eq!(terminal.backend().buffer()[(x, y)].fg, Color::LightBlue);
    }

    #[test]
    fn long_description_is_truncated() {
        let terminal = render(&online_app(&["web"]), 100, 24);
        let (_, y) = find(&terminal, "Spring Web");
        let buf = terminal.backend().buffer();
        let row: String = (0..buf.area.width)
            .map(|x| buf[(x, y + 1)].symbol().to_string())
            .collect();
        assert!(row.trim_end().ends_with("…│"), "{row}");
    }

    #[test]
    fn picker_modal_shows_search_and_list() {
        let mut app = online_app(&[]);
        open_picker(&mut app, "web", &[]);
        let screen = text(&render(&app, 100, 24));
        for expected in ["Add dependencies", "Search: web", "Esc cancel"] {
            assert!(screen.contains(expected), "{expected} missing:\n{screen}");
        }
    }

    #[test]
    fn picker_selected_is_marked() {
        let mut app = online_app(&[]);
        open_picker(&mut app, "", &["web"]);
        let screen = text(&render(&app, 100, 24));
        assert!(screen.contains("[x] Spring Web"), "{screen}");
    }

    #[test]
    fn empty_text_field_has_well() {
        let mut app = online_app(&[]);
        app.req.description.clear();
        let terminal = render(&app, 100, 24);
        let (x, y) = find(&terminal, "Description");
        assert_eq!(terminal.backend().buffer()[(x + 14, y)].bg, Color::DarkGray);
    }

    #[test]
    fn tiny_terminal_with_picker_does_not_panic() {
        let mut app = online_app(&[]);
        open_picker(&mut app, "", &[]);
        let last = app.visible_dependencies().len() - 1;
        app.picker.as_mut().expect("open").cursor = last;
        render(&app, 40, 10);
    }

    #[test]
    fn picker_snapshot() {
        let mut app = online_app(&[]);
        open_picker(&mut app, "sweb", &["web"]);
        insta::assert_snapshot!(render(&app, 100, 24).backend());
    }

    fn package_row(app: &App, width: u16) -> String {
        let terminal = render(app, width, 24);
        let (_, y) = find(&terminal, "Package name");
        let buf = terminal.backend().buffer();
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    #[test]
    fn long_text_value_is_marked_truncated() {
        let mut app = online_app(&[]);
        app.req.package_name = "com.example.averyveryverylongname".into();
        let row = package_row(&app, 80);
        assert!(row.contains("com.example.a"), "{row}");
        assert!(row.contains("…▏"), "{row}");
    }

    #[test]
    fn editing_long_value_shows_its_end_and_cursor() {
        let mut app = online_app(&[]);
        app.req.package_name = "com.example.averyveryverylongname".into();
        app.pane = crate::app::Pane::Project;
        app.field = crate::app::Field::Package;
        app.editing = true;
        let row = package_row(&app, 80);
        assert!(row.contains("longname▏"), "{row}");
    }
}
