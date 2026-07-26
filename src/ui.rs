use std::collections::BTreeSet;

use chrono::Local;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap};

use crate::app::Dashboard;
use crate::capabilities::RenderingProfile;
use crate::codex::{ThreadStatus, ThreadSummary};
use crate::scene::{Scene, UnicodeScene, project_name};

const BG: Color = Color::Rgb(3, 8, 22);
const PANEL: Color = Color::Rgb(7, 18, 39);
const CYAN: Color = Color::Rgb(61, 226, 255);
const BLUE: Color = Color::Rgb(63, 122, 255);
const MUTED: Color = Color::Rgb(112, 139, 174);
const AMBER: Color = Color::Rgb(255, 188, 76);
const RED: Color = Color::Rgb(255, 83, 104);

pub fn draw(frame: &mut Frame<'_>, dashboard: &mut Dashboard) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(BG)), area);
    if area.width < 96 || area.height < 28 {
        frame.render_widget(
            Paragraph::new("Agrandissez le terminal à au moins 96 × 28.")
                .alignment(Alignment::Center)
                .style(Style::new().fg(AMBER).bg(BG)),
            area,
        );
        return;
    }

    let vertical = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(12),
        Constraint::Length(3),
    ])
    .split(area);
    draw_header(frame, dashboard, vertical[0]);
    if area.width >= 132 {
        let body = Layout::horizontal([
            Constraint::Percentage(21),
            Constraint::Percentage(57),
            Constraint::Percentage(22),
        ])
        .split(vertical[1]);
        let left = Layout::vertical([Constraint::Length(13), Constraint::Min(10)]).split(body[0]);
        draw_projects(frame, dashboard, left[0]);
        draw_threads(frame, dashboard, left[1]);
        draw_scene(frame, dashboard, body[1]);
        let right = Layout::vertical([Constraint::Length(18), Constraint::Min(8)]).split(body[2]);
        draw_inspector(frame, dashboard, right[0]);
        draw_events(frame, dashboard, right[1]);
    } else {
        let body = Layout::horizontal([Constraint::Percentage(68), Constraint::Percentage(32)])
            .split(vertical[1]);
        draw_scene(frame, dashboard, body[0]);
        let right = Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(body[1]);
        draw_threads(frame, dashboard, right[0]);
        draw_inspector(frame, dashboard, right[1]);
    }
    draw_footer(frame, dashboard, vertical[2]);
}

fn draw_header(frame: &mut Frame<'_>, dashboard: &Dashboard, area: Rect) {
    let threads = dashboard.effective_threads();
    let project_count = dashboard
        .threads
        .iter()
        .map(|thread| &thread.cwd)
        .collect::<BTreeSet<_>>()
        .len();
    let active = threads
        .iter()
        .filter(|thread| {
            matches!(
                thread.status,
                ThreadStatus::Active { .. } | ThreadStatus::RecentlyActive
            )
        })
        .count();
    let attention = threads
        .iter()
        .filter(|thread| {
            matches!(
                thread.status,
                ThreadStatus::SystemError | ThreadStatus::NeedsAttention
            )
        })
        .count();
    let mode = match dashboard.profile {
        RenderingProfile::Ultra => "ULTRA 3D",
        RenderingProfile::Unicode => "UNICODE 3D",
        RenderingProfile::Safe => "SAFE",
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(" CODEX ", Style::new().fg(BG).bg(CYAN).bold()),
            Span::styled(" OPERATIONS CENTER", Style::new().fg(Color::White).bold()),
            Span::raw("  "),
            Span::styled(mode, Style::new().fg(CYAN)),
        ]),
        Line::from(vec![
            Span::styled(format!(" {active} EN COURS "), Style::new().fg(CYAN)),
            Span::styled(
                format!("  {attention} INTERVENTION(S) "),
                Style::new().fg(if attention > 0 { AMBER } else { MUTED }),
            ),
            Span::styled(
                format!("  {project_count} PROJETS  "),
                Style::new().fg(MUTED),
            ),
            Span::styled(
                Local::now().format("%A %d %B · %H:%M:%S").to_string(),
                Style::new().fg(MUTED),
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::new()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::new().fg(BLUE)),
            )
            .style(Style::new().bg(BG)),
        area,
    );
}

fn draw_scene(frame: &mut Frame<'_>, dashboard: &mut Dashboard, area: Rect) {
    let block = panel("COMPLEXE D’OPÉRATIONS");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    dashboard.scene_area = inner;
    let threads = dashboard.effective_threads();
    let scene = Scene::from_threads(
        &threads,
        dashboard.camera_yaw,
        dashboard.camera_pitch,
        dashboard.camera_zoom,
        dashboard.started_at.elapsed().as_secs_f32(),
        dashboard.selected,
    );
    match dashboard.profile {
        RenderingProfile::Ultra => frame.render_widget(Block::new(), inner),
        RenderingProfile::Unicode => frame.render_widget(UnicodeScene { scene: &scene }, inner),
        RenderingProfile::Safe => frame.render_widget(
            Paragraph::new(
                threads
                    .iter()
                    .take(inner.height as usize)
                    .map(|thread| {
                        Line::from(vec![
                            Span::styled(" ◉ ", Style::new().fg(status_color(thread))),
                            Span::styled(project_name(&thread.cwd), Style::new().fg(Color::White)),
                            Span::styled(format!("  {}", status(thread)), Style::new().fg(MUTED)),
                        ])
                    })
                    .collect::<Vec<_>>(),
            ),
            inner,
        ),
    }
    if dashboard.threads.is_empty() {
        frame.render_widget(
            Paragraph::new("Aucune session locale trouvée")
                .alignment(Alignment::Center)
                .style(Style::new().fg(MUTED)),
            inner,
        );
    }
}

fn draw_projects(frame: &mut Frame<'_>, dashboard: &Dashboard, area: Rect) {
    let block = panel("ESPACES");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let threads = dashboard.effective_threads();
    let mut projects = std::collections::BTreeMap::<String, (usize, usize)>::new();
    for thread in &threads {
        let entry = projects.entry(project_name(&thread.cwd)).or_default();
        entry.0 += 1;
        if matches!(
            thread.status,
            ThreadStatus::Active { .. } | ThreadStatus::RecentlyActive
        ) {
            entry.1 += 1;
        }
    }
    let mut lines = vec![Line::from(vec![
        Span::styled(" ● ", Style::new().fg(CYAN)),
        Span::styled("TOUT LE COMPLEXE", Style::new().fg(Color::White).bold()),
        Span::styled(format!("  {}", threads.len()), Style::new().fg(MUTED)),
    ])];
    for (index, (project, (count, active))) in projects.into_iter().take(4).enumerate() {
        let color = [
            CYAN,
            Color::Rgb(153, 104, 255),
            AMBER,
            Color::Rgb(75, 226, 164),
        ][index % 4];
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled(" ◆ ", Style::new().fg(color)),
            Span::styled(
                truncate(&project, inner.width.saturating_sub(9) as usize),
                Style::new().fg(Color::White),
            ),
            Span::styled(format!(" {count}"), Style::new().fg(MUTED)),
        ]));
        lines.push(Line::from(Span::styled(
            format!("     {active} en activité"),
            Style::new().fg(if active > 0 { color } else { MUTED }),
        )));
    }
    frame.render_widget(Paragraph::new(lines).style(Style::new().bg(PANEL)), inner);
}

fn draw_inspector(frame: &mut Frame<'_>, dashboard: &Dashboard, area: Rect) {
    let block = panel("SESSION ACTIVE");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let threads = dashboard.effective_threads();
    let Some(thread) = threads.get(dashboard.selected) else {
        frame.render_widget(
            Paragraph::new("Sélectionnez une session dans le complexe.")
                .style(Style::new().fg(MUTED))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    };
    let name = thread
        .agent_nickname
        .as_deref()
        .or(thread.name.as_deref())
        .filter(|value| !value.is_empty())
        .unwrap_or("Agent Codex");
    let updated = chrono::DateTime::from_timestamp(thread.updated_at, 0)
        .map(|date| {
            date.with_timezone(&Local)
                .format("%d/%m · %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "inconnue".to_owned());
    let content = vec![
        Line::from(vec![
            Span::styled(" ◆ ", Style::new().fg(status_color(thread))),
            Span::styled(name, Style::new().fg(Color::White).bold()),
        ]),
        Line::from(Span::styled(
            status(thread),
            Style::new().fg(status_color(thread)).bold(),
        )),
        Line::default(),
        section("PROJET"),
        Line::from(Span::styled(
            project_name(&thread.cwd),
            Style::new().fg(CYAN),
        )),
        section("CONTEXTE ACTUEL"),
        Line::from(Span::styled(
            truncate(&thread.preview, inner.width as usize * 2),
            Style::new().fg(Color::White),
        )),
        section("MOTEUR"),
        Line::from(Span::styled(&thread.model_provider, Style::new().fg(MUTED))),
        section("DERNIÈRE ACTIVITÉ"),
        Line::from(Span::styled(updated, Style::new().fg(MUTED))),
        section("DOSSIER"),
        Line::from(Span::styled(
            truncate(&thread.cwd, inner.width as usize),
            Style::new().fg(Color::Rgb(104, 144, 194)),
        )),
    ];
    frame.render_widget(
        Paragraph::new(content)
            .style(Style::new().bg(PANEL))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn section(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("\n{label}"),
        Style::new().fg(Color::Rgb(73, 105, 148)).bold(),
    ))
}

fn draw_threads(frame: &mut Frame<'_>, dashboard: &mut Dashboard, area: Rect) {
    let inner = panel("SESSIONS").inner(area);
    dashboard.thread_area = inner;
    frame.render_widget(panel("SESSIONS"), area);
    let threads = dashboard.effective_threads();
    let capacity = inner.height as usize / 3;
    let start = thread_list_start(dashboard, capacity);
    let items = threads
        .iter()
        .enumerate()
        .skip(start)
        .take(capacity)
        .map(|(index, thread)| {
            let selected = index == dashboard.selected;
            let status = status(thread);
            let title = thread
                .name
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| project_name(&thread.cwd));
            let marker = if selected { "▶" } else { " " };
            let color = if selected { CYAN } else { Color::White };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(format!("{marker} {title}"), Style::new().fg(color).bold()),
                    Span::styled(format!("  {status}"), Style::new().fg(status_color(thread))),
                ]),
                Line::from(Span::styled(
                    truncate(&thread.preview, inner.width.saturating_sub(3) as usize),
                    Style::new().fg(MUTED),
                )),
                Line::default(),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items).style(Style::new().bg(PANEL)), inner);
}

fn draw_events(frame: &mut Frame<'_>, dashboard: &Dashboard, area: Rect) {
    let block = panel("JOURNAL D’ACTIVITÉ");
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = dashboard
        .events
        .iter()
        .rev()
        .take(inner.height as usize / 2)
        .map(|event| {
            let time = event.received_at.with_timezone(&Local).format("%H:%M:%S");
            Line::from(vec![
                Span::styled(format!(" {time} "), Style::new().fg(MUTED)),
                Span::styled(
                    truncate(&event.summary, inner.width.saturating_sub(11) as usize),
                    Style::new().fg(Color::White),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let content = if lines.is_empty() {
        vec![Line::from(Span::styled(
            " Aucun événement capturé — lancez `codex-ops integrate`.",
            Style::new().fg(MUTED),
        ))]
    } else {
        lines
    };
    frame.render_widget(Paragraph::new(content).wrap(Wrap { trim: true }), inner);
}

fn draw_footer(frame: &mut Frame<'_>, dashboard: &mut Dashboard, area: Rect) {
    let status = dashboard.status_message.as_deref().unwrap_or("");
    let columns = Layout::horizontal([
        Constraint::Min(40),
        Constraint::Length(16),
        Constraint::Length(12),
    ])
    .split(area);
    dashboard.refresh_button = columns[1];
    dashboard.quit_button = columns[2];
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" CLIC/↑↓ ", Style::new().fg(BG).bg(CYAN).bold()),
            Span::styled(" Sélection  ", Style::new().fg(MUTED)),
            Span::styled("GLISSER/←→", Style::new().fg(Color::White)),
            Span::styled(" Caméra  ", Style::new().fg(MUTED)),
            Span::styled("MOLETTE/+−", Style::new().fg(Color::White)),
            Span::styled(" Zoom  ", Style::new().fg(MUTED)),
            Span::styled(
                format!("  {}", dashboard.capabilities.terminal),
                Style::new().fg(Color::Rgb(54, 82, 116)),
            ),
            Span::styled(format!("  {status}"), Style::new().fg(RED)),
        ]))
        .style(Style::new().bg(BG)),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new("↻ ACTUALISER")
            .alignment(Alignment::Center)
            .style(Style::new().fg(CYAN).bg(Color::Rgb(9, 32, 54)))
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_style(Style::new().fg(CYAN)),
            ),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new("QUITTER")
            .alignment(Alignment::Center)
            .style(Style::new().fg(Color::White).bg(Color::Rgb(42, 15, 29)))
            .block(
                Block::new()
                    .borders(Borders::ALL)
                    .border_style(Style::new().fg(RED)),
            ),
        columns[2],
    );
}

pub fn thread_at(dashboard: &Dashboard, column: u16, row: u16) -> Option<usize> {
    let area = dashboard.scene_area;
    if area.contains((column, row).into()) {
        let scene = Scene::from_threads(
            &dashboard.threads,
            dashboard.camera_yaw,
            dashboard.camera_pitch,
            dashboard.camera_zoom,
            dashboard.started_at.elapsed().as_secs_f32(),
            dashboard.selected,
        );
        let local_x = (column - area.x) as f32;
        let local_y = (row - area.y) as f32 * 2.0;
        return scene
            .project_nodes(area.width as f32, area.height as f32 * 2.0)
            .into_iter()
            .filter_map(|(index, point, radius)| {
                let distance = ((point.x - local_x).powi(2) + (point.y - local_y).powi(2)).sqrt();
                (distance <= radius.max(2.5)).then_some((index, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index);
    }
    if dashboard.thread_area.contains((column, row).into()) {
        let capacity = dashboard.thread_area.height as usize / 3;
        let index =
            thread_list_start(dashboard, capacity) + ((row - dashboard.thread_area.y) / 3) as usize;
        return (index < dashboard.threads.len()).then_some(index);
    }
    None
}

fn thread_list_start(dashboard: &Dashboard, capacity: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    dashboard.selected.saturating_sub(capacity - 1)
}

fn panel(title: &'static str) -> Block<'static> {
    Block::new()
        .title(Line::from(vec![
            Span::styled(" ◆ ", Style::new().fg(CYAN)),
            Span::styled(title, Style::new().fg(Color::White).bold()),
            Span::raw(" "),
        ]))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Rgb(26, 66, 109)))
        .style(Style::new().bg(PANEL))
}

fn status(thread: &ThreadSummary) -> &'static str {
    match thread.status {
        ThreadStatus::Active { .. } => "EN COURS",
        ThreadStatus::RecentlyActive => "ACTIVITÉ RÉCENTE",
        ThreadStatus::NeedsAttention => "INTERVENTION",
        ThreadStatus::SystemError => "ERREUR",
        ThreadStatus::Idle => "DISPONIBLE",
        ThreadStatus::NotLoaded => "ENREGISTRÉE",
    }
}

fn status_color(thread: &ThreadSummary) -> Color {
    match thread.status {
        ThreadStatus::Active { .. } => CYAN,
        ThreadStatus::RecentlyActive => CYAN,
        ThreadStatus::NeedsAttention => AMBER,
        ThreadStatus::SystemError => RED,
        ThreadStatus::Idle => AMBER,
        ThreadStatus::NotLoaded => MUTED,
    }
}

fn truncate(value: &str, width: usize) -> String {
    let value = value.replace(['\n', '\r'], " ");
    if value.chars().count() <= width {
        value
    } else {
        format!(
            "{}…",
            value
                .chars()
                .take(width.saturating_sub(1))
                .collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::capabilities::Capabilities;
    use crate::codex::ThreadSummary;

    #[test]
    fn dashboard_renders_at_supported_size() {
        let mut dashboard = Dashboard {
            capabilities: Capabilities {
                terminal: "test-terminal".to_owned(),
                true_color: true,
                kitty_graphics: false,
                sixel_graphics: false,
                mouse: true,
                tmux: false,
                ssh: false,
            },
            profile: RenderingProfile::Unicode,
            threads: vec![ThreadSummary {
                id: "thread-1".to_owned(),
                session_id: "session-1".to_owned(),
                cwd: "/work/example".to_owned(),
                preview: "Inspect the project".to_owned(),
                name: Some("Example operation".to_owned()),
                model_provider: "openai".to_owned(),
                created_at: 1,
                updated_at: 2,
                parent_thread_id: None,
                agent_nickname: None,
                agent_role: None,
                status: ThreadStatus::Active {
                    active_flags: Vec::new(),
                },
            }],
            events: Vec::new(),
            selected: 0,
            camera_yaw: 0.3,
            camera_pitch: 0.2,
            camera_zoom: 1.0,
            started_at: Instant::now(),
            last_refresh: Instant::now(),
            scene_area: Rect::default(),
            thread_area: Rect::default(),
            refresh_button: Rect::default(),
            quit_button: Rect::default(),
            should_quit: false,
            dragging: false,
            last_mouse: None,
            status_message: None,
            last_ultra_frame: Instant::now(),
        };
        let backend = TestBackend::new(180, 52);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| draw(frame, &mut dashboard)).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("CODEX"));
        assert!(rendered.contains("ESPACES"));
        assert!(rendered.contains("SESSION ACTIVE"));
        assert!(rendered.contains("COMPLEXE D’OPÉRATIONS"));
        assert!(rendered.contains("Example operation"));
        assert!(!dashboard.scene_area.is_empty());
    }
}
