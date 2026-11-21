use std::time::SystemTime;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{App, FIELDS, Field, Mode, Pane};
use crate::cli::version_info;
use crate::model::Metadata;

const HELP: &str = "Tab pane · ↑↓ move · ←→ choose · Enter edit · / search · Space toggle · X clear · g generate · ? about · q quit";

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
    draw_dependencies(frame, app, right);
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
            let style = if selected {
                Style::new().add_modifier(Modifier::REVERSED)
            } else {
                Style::new()
            };
            Line::from(vec![
                Span::styled(
                    format!("{:<13}", field.label()),
                    Style::new().fg(Color::Cyan),
                ),
                Span::styled(value, style),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).block(focused_block(" Project ".into(), app.pane == Pane::Project)),
        area,
    );
}

fn draw_dependencies(frame: &mut Frame, app: &App, area: Rect) {
    let title = if app.filtering || !app.filter.is_empty() {
        let cursor = if app.filtering { "▏" } else { "" };
        format!(" Search: {}{cursor} ", app.filter)
    } else {
        " Dependencies (/ to search) ".into()
    };
    let block = focused_block(title, app.pane == Pane::Dependencies);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [list_area, selected_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);

    let width = list_area.width as usize;
    let mut items = Vec::new();
    let mut selected_row = None;
    let mut last_group = None;
    for (i, (group, dep, compatible)) in app.visible_dependencies().into_iter().enumerate() {
        if app.filter.is_empty() && last_group != Some(group) {
            items.push(ListItem::new(Line::styled(
                group.to_string(),
                Style::new().add_modifier(Modifier::BOLD).fg(Color::Yellow),
            )));
            last_group = Some(group);
        }
        let mark = if app.req.dependencies.contains(&dep.id) {
            "[x]"
        } else {
            "[ ]"
        };
        let mut text = format!("{mark} {}", dep.name);
        if !app.filter.is_empty() {
            let pad = width.saturating_sub(text.chars().count() + group.chars().count());
            text = format!("{text}{}{group}", " ".repeat(pad.max(1)));
        }
        let style = if compatible {
            Style::new()
        } else {
            Style::new().fg(Color::DarkGray)
        };
        if i == app.dep_cursor {
            selected_row = Some(items.len());
        }
        items.push(ListItem::new(Line::styled(text, style)));
    }
    let highlight = if app.pane == Pane::Dependencies {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new()
    };
    let mut state = ListState::default().with_selected(selected_row);
    frame.render_stateful_widget(
        List::new(items).highlight_style(highlight),
        list_area,
        &mut state,
    );

    let selected = if app.req.dependencies.is_empty() {
        "Selected: none".to_string()
    } else {
        format!("Selected: {}", app.req.dependencies.join(", "))
    };
    frame.render_widget(
        Paragraph::new(selected).style(Style::new().fg(Color::Green)),
        selected_area,
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
        app.dep_cursor = app.visible_dependencies().len() - 1;
        app.pane = crate::app::Pane::Dependencies;
        render(&app, 40, 10);
    }

    #[test]
    fn filtered_list_shows_category() {
        let mut app = offline_app(true);
        app.filter = "sweb".into();
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
}
