use chrono::Local;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::app::Dashboard;
use crate::capabilities::RenderingProfile;
use crate::codex::{ThreadStatus, ThreadSummary};
use crate::config::{GraphicsChoice, JournalDensity, OptionAction, RefreshPace};
use crate::scene::{UnicodeScene, project_key, project_name};

const BG: Color = Color::Rgb(3, 8, 22);
const PANEL: Color = Color::Rgb(7, 18, 39);
const CYAN: Color = Color::Rgb(61, 226, 255);
const BLUE: Color = Color::Rgb(63, 122, 255);
const MUTED: Color = Color::Rgb(112, 139, 174);
const AMBER: Color = Color::Rgb(255, 188, 76);
const RED: Color = Color::Rgb(255, 83, 104);
const GREEN: Color = Color::Rgb(75, 226, 164);
const MAGENTA: Color = Color::Rgb(238, 91, 201);
const VIOLET: Color = Color::Rgb(153, 104, 255);

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
        Constraint::Length(3),
        Constraint::Min(12),
        Constraint::Length(3),
    ])
    .split(area);
    draw_header(frame, dashboard, vertical[0]);
    if area.width >= 132 {
        let body = Layout::horizontal([
            Constraint::Percentage(23),
            Constraint::Percentage(59),
            Constraint::Percentage(18),
        ])
        .split(vertical[1]);
        draw_threads(frame, dashboard, body[0]);
        draw_scene(frame, dashboard, body[1]);
        let right = if dashboard.selected_event.is_some()
            || dashboard.agent_detail_open
            || dashboard.options_open
        {
            Layout::vertical([Constraint::Percentage(64), Constraint::Percentage(36)])
                .split(body[2])
        } else {
            Layout::vertical([Constraint::Percentage(44), Constraint::Percentage(56)])
                .split(body[2])
        };
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
    let project_count = project_groups(&threads).len();
    let active = threads
        .iter()
        .filter(|thread| {
            matches!(
                thread.status,
                ThreadStatus::Active { .. } | ThreadStatus::ObservedRunning
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
    let lines = vec![Line::from(vec![
        Span::styled(" CODEX ", Style::new().fg(BG).bg(CYAN).bold()),
        Span::styled(" OPERATIONS HUB", Style::new().fg(Color::White).bold()),
        Span::styled(
            format!("   ● {active} actifs"),
            Style::new().fg(GREEN).bold(),
        ),
        Span::styled(
            format!("   ◆ {project_count} projets"),
            Style::new().fg(BLUE),
        ),
        Span::styled(
            format!("   ! {attention}"),
            Style::new().fg(if attention > 0 { AMBER } else { MUTED }),
        ),
        Span::styled(
            format!("   {}", Local::now().format("%H:%M:%S")),
            Style::new().fg(MUTED),
        ),
    ])];
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
    let scene = dashboard.scene();
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

fn draw_inspector(frame: &mut Frame<'_>, dashboard: &mut Dashboard, area: Rect) {
    dashboard.option_hitboxes.clear();
    if let Some(index) = dashboard.selected_event
        && let Some(event) = dashboard.events.get(index)
    {
        draw_activity_detail(frame, event, area);
        return;
    }
    if dashboard.options_open {
        draw_options(frame, dashboard, area);
        return;
    }
    let block = panel(if dashboard.agent_detail_open {
        "FICHE AGENT"
    } else {
        "DÉTAIL DE LA ROOM"
    });
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
    let belongs_to_thread = |event: &&crate::events::EventRecord| {
        event.session_id == thread.session_id || event.session_id == thread.id
    };
    let latest_event = dashboard.events.iter().rev().find(belongs_to_thread);
    let latest_command_event = dashboard
        .events
        .iter()
        .rev()
        .filter(belongs_to_thread)
        .find(|event| event.command_detail().is_some());
    let current_action = thread
        .runtime
        .last_action
        .clone()
        .or_else(|| latest_event.map(|event| event.summary.clone()))
        .unwrap_or_else(|| "En attente d’une nouvelle tâche".to_owned());
    let runtime_command = thread.runtime.last_command.as_deref();
    let exact_command =
        runtime_command.or_else(|| latest_command_event.and_then(|event| event.command_detail()));
    let command_workdir = thread
        .runtime
        .last_command_workdir
        .as_deref()
        .or_else(|| latest_command_event.and_then(|event| event.working_directory()))
        .unwrap_or(&thread.cwd);
    let command_tool = if runtime_command.is_some() {
        thread.runtime.last_tool.as_deref()
    } else {
        latest_command_event.and_then(|event| event.tool_name.as_deref())
    };
    let activity_time = thread
        .runtime
        .last_action_at
        .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
        .map(|date| date.with_timezone(&Local).format("%H:%M:%S").to_string())
        .or_else(|| {
            latest_event.map(|event| {
                event
                    .received_at
                    .with_timezone(&Local)
                    .format("%H:%M:%S")
                    .to_string()
            })
        });
    let command_time = if runtime_command.is_some() {
        thread
            .runtime
            .last_action_at
            .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0))
            .map(|date| date.with_timezone(&Local).format("%H:%M:%S").to_string())
    } else {
        latest_command_event.map(|event| {
            event
                .received_at
                .with_timezone(&Local)
                .format("%H:%M:%S")
                .to_string()
        })
    };
    if !dashboard.agent_detail_open {
        let mut content = vec![
            Line::from(vec![
                Span::styled(" ◆ ", Style::new().fg(status_color(thread))),
                Span::styled(
                    truncate(name, inner.width.saturating_sub(5) as usize),
                    Style::new().fg(Color::White).bold(),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    status_short(thread),
                    Style::new().fg(status_color(thread)).bold(),
                ),
                Span::styled("  ·  ", Style::new().fg(MUTED)),
                Span::styled(project_name(&thread.cwd), Style::new().fg(CYAN)),
            ]),
            section_colored("ACTIVITÉ COURANTE", GREEN),
            Line::from(Span::styled(&current_action, Style::new().fg(Color::White))),
            Line::from(vec![
                Span::styled(
                    activity_time.as_deref().unwrap_or("heure inconnue"),
                    Style::new().fg(CYAN),
                ),
                Span::styled(
                    thread
                        .runtime
                        .last_tool
                        .as_deref()
                        .or_else(|| latest_event.and_then(|event| event.tool_name.as_deref()))
                        .map(|tool| format!("  ·  {tool}"))
                        .unwrap_or_default(),
                    Style::new().fg(MAGENTA),
                ),
            ]),
        ];
        if let Some(command) = exact_command {
            content.push(Line::from(vec![
                Span::styled("\nDERNIÈRE COMMANDE", Style::new().fg(AMBER).bold()),
                Span::styled(
                    command_time
                        .as_deref()
                        .map(|time| format!("  ·  {time}"))
                        .unwrap_or_default(),
                    Style::new().fg(MUTED),
                ),
            ]));
            content.push(Line::from(Span::styled(
                command,
                Style::new().fg(Color::Rgb(255, 218, 132)),
            )));
            content.push(Line::from(vec![
                Span::styled("dans  ", Style::new().fg(MUTED)),
                Span::styled(command_workdir, Style::new().fg(CYAN)),
            ]));
        }
        content.extend([
            Line::default(),
            Line::from(Span::styled(
                "Cliquez sur l’agent pour toutes les métriques.",
                Style::new().fg(MUTED),
            )),
        ]);
        frame.render_widget(
            Paragraph::new(content)
                .style(Style::new().bg(PANEL))
                .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }
    let updated = chrono::DateTime::from_timestamp(thread.updated_at, 0)
        .map(|date| {
            date.with_timezone(&Local)
                .format("%d/%m · %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "inconnue".to_owned());
    let started = thread.runtime.activity_started_at.and_then(|timestamp| {
        chrono::DateTime::from_timestamp(timestamp, 0).map(|date| {
            let local = date.with_timezone(&Local);
            let elapsed = Local::now().signed_duration_since(local);
            format!(
                "{} · depuis {}",
                local.format("%H:%M:%S"),
                format_duration(elapsed.num_milliseconds().max(0) as u64)
            )
        })
    });
    let model = thread
        .runtime
        .model
        .as_deref()
        .unwrap_or(&thread.model_provider);
    let reasoning = thread
        .runtime
        .reasoning_effort
        .as_deref()
        .unwrap_or("non exposé");
    let context = match (thread.runtime.context_tokens, thread.runtime.context_window) {
        (Some(used), Some(window)) if window > 0 => {
            format!(
                "{} / {} · {:.0}%",
                compact_number(used),
                compact_number(window),
                used as f64 / window as f64 * 100.0
            )
        }
        _ => "non exposé".to_owned(),
    };
    let mut content = vec![
        Line::from(vec![
            Span::styled(" ◆ ", Style::new().fg(status_color(thread))),
            Span::styled(name, Style::new().fg(Color::White).bold()),
        ]),
        Line::from(Span::styled(
            status_short(thread),
            Style::new().fg(status_color(thread)).bold(),
        )),
        Line::default(),
        section_colored("ACTION EN COURS", GREEN),
        Line::from(Span::styled(&current_action, Style::new().fg(Color::White))),
        section_colored("DÉBUT D’ACTIVITÉ", AMBER),
        Line::from(Span::styled(
            started.unwrap_or_else(|| "Au repos".to_owned()),
            Style::new().fg(AMBER),
        )),
    ];
    if let Some(command) = exact_command {
        content.push(section_colored("DERNIÈRE COMMANDE EXACTE", AMBER));
        content.push(Line::from(vec![
            Span::styled(
                command_time.as_deref().unwrap_or("heure inconnue"),
                Style::new().fg(CYAN).bold(),
            ),
            Span::styled(
                command_tool
                    .map(|tool| format!("  ·  {tool}"))
                    .unwrap_or_default(),
                Style::new().fg(MAGENTA),
            ),
        ]));
        content.push(Line::from(Span::styled(
            command,
            Style::new().fg(Color::Rgb(255, 218, 132)),
        )));
        content.push(Line::from(vec![
            Span::styled("Dossier  ", Style::new().fg(MUTED)),
            Span::styled(command_workdir, Style::new().fg(CYAN)),
        ]));
    }
    content.extend([
        section_colored("RAISONNEMENT CODEX", VIOLET),
        Line::from(vec![
            Span::styled(reasoning.to_uppercase(), Style::new().fg(VIOLET).bold()),
            Span::styled("  (valeur déclarée)", Style::new().fg(MUTED)),
        ]),
        section_colored("MÉTRIQUES", CYAN),
        Line::from(vec![
            Span::styled("Contexte  ", Style::new().fg(MUTED)),
            Span::styled(context, Style::new().fg(CYAN)),
        ]),
        Line::from(vec![
            Span::styled("Actions   ", Style::new().fg(MUTED)),
            Span::styled(
                thread.runtime.actions_this_turn.to_string(),
                Style::new().fg(GREEN),
            ),
            Span::styled("  ·  raisonnement ", Style::new().fg(MUTED)),
            Span::styled(
                thread
                    .runtime
                    .reasoning_tokens
                    .map(compact_number)
                    .unwrap_or_else(|| "—".to_owned()),
                Style::new().fg(VIOLET),
            ),
            Span::styled(" tok.", Style::new().fg(MUTED)),
        ]),
        section("PROJET"),
        Line::from(Span::styled(
            project_name(&thread.cwd),
            Style::new().fg(CYAN),
        )),
        section("MOTEUR"),
        Line::from(Span::styled(model, Style::new().fg(BLUE))),
        section("DERNIÈRE ACTIVITÉ"),
        Line::from(Span::styled(updated, Style::new().fg(MUTED))),
        section("DOSSIER"),
        Line::from(Span::styled(
            truncate(&thread.cwd, inner.width as usize),
            Style::new().fg(Color::Rgb(104, 144, 194)),
        )),
    ]);
    if let Some(duration) = thread.runtime.last_turn_duration_ms {
        content.push(section("DERNIER TOUR"));
        content.push(Line::from(vec![
            Span::styled(format_duration(duration), Style::new().fg(GREEN)),
            Span::styled(
                thread
                    .runtime
                    .time_to_first_token_ms
                    .map(|value| format!("  ·  1er token {}", format_duration(value)))
                    .unwrap_or_default(),
                Style::new().fg(MUTED),
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(content)
            .style(Style::new().bg(PANEL))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_options(frame: &mut Frame<'_>, dashboard: &mut Dashboard, area: Rect) {
    let block = panel("OPTIONS DU CENTRE").border_style(Style::new().fg(AMBER));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    dashboard.option_hitboxes.clear();
    let cards = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(1),
    ])
    .split(inner);
    let values = [
        (
            OptionAction::Graphics,
            "GRAPHISMES",
            match dashboard.settings.graphics {
                GraphicsChoice::Auto => "AUTO",
                GraphicsChoice::Ultra => "ULTRA 3D",
                GraphicsChoice::Unicode => "UNICODE",
                GraphicsChoice::Safe => "COMPATIBLE",
            },
            CYAN,
        ),
        (
            OptionAction::Refresh,
            "ACTUALISATION",
            match dashboard.settings.refresh {
                RefreshPace::Fast => "RAPIDE · 2 s",
                RefreshPace::Balanced => "ÉQUILIBRÉE · 5 s",
                RefreshPace::Quiet => "CALME · 15 s",
            },
            GREEN,
        ),
        (
            OptionAction::RestingAgents,
            "AGENTS AU REPOS",
            if dashboard.settings.show_resting_agents {
                "AFFICHÉS"
            } else {
                "MASQUÉS"
            },
            VIOLET,
        ),
        (
            OptionAction::JournalDensity,
            "DENSITÉ DU JOURNAL",
            match dashboard.settings.journal_density {
                JournalDensity::Compact => "COMPACT · 8",
                JournalDensity::Balanced => "ÉQUILIBRÉ · 20",
                JournalDensity::Full => "COMPLET",
            },
            MAGENTA,
        ),
    ];
    for (card, (action, label, value, color)) in cards.iter().zip(values) {
        dashboard.option_hitboxes.push((action, *card));
        let hovered = dashboard
            .mouse_position
            .is_some_and(|point| card.contains(point.into()));
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(label, Style::new().fg(MUTED).bold())),
                Line::from(vec![
                    Span::styled(format!("  {value}"), Style::new().fg(color).bold()),
                    Span::styled(
                        "   ↻ changer",
                        Style::new().fg(if hovered { AMBER } else { MUTED }),
                    ),
                ]),
            ])
            .style(Style::new().bg(if hovered {
                Color::Rgb(24, 38, 58)
            } else {
                PANEL
            })),
            *card,
        );
    }
    frame.render_widget(
        Paragraph::new("Cliquez sur un réglage pour le faire défiler. Les choix sont enregistrés automatiquement.")
            .style(Style::new().fg(MUTED))
            .wrap(Wrap { trim: true }),
        cards[4],
    );
}

fn draw_activity_detail(frame: &mut Frame<'_>, event: &crate::events::EventRecord, area: Rect) {
    let (marker, color) = activity_appearance(event);
    let block = panel("DÉTAIL ACTIVITÉ").border_style(Style::new().fg(color));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let timestamp = event
        .received_at
        .with_timezone(&Local)
        .format("%d/%m/%Y · %H:%M:%S%.3f")
        .to_string();
    let mut content = vec![
        Line::from(vec![
            Span::styled(format!(" {marker} "), Style::new().fg(color).bold()),
            Span::styled(&event.summary, Style::new().fg(color).bold()),
        ]),
        section_colored("HEURE EXACTE", VIOLET),
        Line::from(Span::styled(timestamp, Style::new().fg(CYAN))),
        section_colored("PROJET", BLUE),
        Line::from(Span::styled(
            project_name(&event.cwd),
            Style::new().fg(GREEN),
        )),
        section_colored("OUTIL", MAGENTA),
        Line::from(Span::styled(
            event.tool_name.as_deref().unwrap_or("Codex"),
            Style::new().fg(MAGENTA),
        )),
    ];
    if let Some(command) = event.command_detail() {
        content.push(section_colored("COMMANDE", AMBER));
        content.push(Line::from(Span::styled(
            command,
            Style::new().fg(Color::Rgb(255, 218, 132)),
        )));
    }
    let files = event.changed_files();
    if !files.is_empty() {
        content.push(section_colored("FICHIERS", MAGENTA));
        content.extend(files.into_iter().take(5).map(|path| {
            Line::from(Span::styled(
                format!("• {path}"),
                Style::new().fg(Color::Rgb(244, 142, 219)),
            ))
        }));
    }
    content.push(Line::default());
    content.push(Line::from(Span::styled(
        "Échap : revenir à la session",
        Style::new().fg(MUTED),
    )));
    frame.render_widget(
        Paragraph::new(content)
            .style(Style::new().bg(PANEL))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn section(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        format!("\n{label}"),
        Style::new().fg(Color::Rgb(73, 105, 148)).bold(),
    ))
}

fn section_colored(label: &'static str, color: Color) -> Line<'static> {
    Line::from(Span::styled(
        format!("\n{label}"),
        Style::new().fg(color).bold(),
    ))
}

fn draw_threads(frame: &mut Frame<'_>, dashboard: &mut Dashboard, area: Rect) {
    let block = panel("PROJETS · CONVERSATIONS");
    let inner = block.inner(area);
    dashboard.thread_area = inner;
    dashboard.thread_hitboxes.clear();
    frame.render_widget(block, area);
    let threads = dashboard.effective_threads();
    let groups = project_groups(&threads);
    let mut lines = Vec::new();
    for (project_index, group) in groups.iter().enumerate() {
        if lines.len() >= inner.height as usize {
            break;
        }
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        let color = project_color(project_index);
        let active = group
            .threads
            .iter()
            .filter(|&&index| is_active(&threads[index]))
            .count();
        lines.push(Line::from(vec![
            Span::styled(" ◆ ", Style::new().fg(color)),
            Span::styled(
                truncate(&group.name, inner.width.saturating_sub(12) as usize),
                Style::new().fg(Color::White).bold(),
            ),
            Span::styled(format!("  {}", group.threads.len()), Style::new().fg(MUTED)),
            Span::styled(
                if active > 0 {
                    format!("  ●{active}")
                } else {
                    String::new()
                },
                Style::new().fg(GREEN).bold(),
            ),
        ]));
        for &index in &group.threads {
            if lines.len() >= inner.height as usize {
                break;
            }
            let thread = &threads[index];
            let selected = index == dashboard.selected;
            let row = inner.y + lines.len() as u16;
            let hitbox = Rect::new(inner.x, row, inner.width, 1);
            dashboard.thread_hitboxes.push((index, hitbox));
            let hovered = dashboard
                .mouse_position
                .is_some_and(|point| hitbox.contains(point.into()));
            let title = thread
                .name
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| truncate(&thread.preview, 40));
            let ago = relative_time(thread.updated_at);
            let available = inner.width.saturating_sub(12) as usize;
            let bg = if hovered {
                Color::Rgb(13, 42, 66)
            } else if selected {
                Color::Rgb(18, 34, 62)
            } else {
                PANEL
            };
            lines.push(Line::from(vec![
                Span::styled(
                    if selected { " ▶ " } else { "   " },
                    Style::new().fg(CYAN).bg(bg).bold(),
                ),
                Span::styled(
                    if is_active(thread) { "● " } else { "○ " },
                    Style::new().fg(status_color(thread)).bg(bg).bold(),
                ),
                Span::styled(
                    truncate(&title, available),
                    Style::new()
                        .fg(if selected { CYAN } else { Color::White })
                        .bg(bg),
                ),
                Span::styled(format!("  {ago}"), Style::new().fg(MUTED).bg(bg)),
            ]));
        }
    }
    frame.render_widget(Paragraph::new(lines).style(Style::new().bg(PANEL)), inner);
}

fn draw_events(frame: &mut Frame<'_>, dashboard: &mut Dashboard, area: Rect) {
    let block = panel("JOURNAL D’ACTIVITÉ");
    let inner = block.inner(area);
    dashboard.event_area = inner;
    frame.render_widget(block, area);
    let lines = dashboard
        .events
        .iter()
        .rev()
        .take(
            dashboard
                .settings
                .journal_density
                .rows(inner.height as usize),
        )
        .enumerate()
        .map(|(offset, event)| {
            let index = dashboard.events.len() - 1 - offset;
            let time = event.received_at.with_timezone(&Local).format("%H:%M:%S");
            let (marker, color) = activity_appearance(event);
            let hovered = dashboard.hovered_event == Some(index);
            let selected = dashboard.selected_event == Some(index);
            let row_style = Style::new().fg(color).bg(if selected {
                Color::Rgb(39, 28, 67)
            } else if hovered {
                Color::Rgb(13, 42, 66)
            } else {
                PANEL
            });
            Line::from(vec![
                Span::styled(
                    format!("{} {time} ", if hovered { "▶" } else { " " }),
                    row_style.fg(if hovered { CYAN } else { MUTED }),
                ),
                Span::styled(format!("{marker} "), row_style.bold()),
                Span::styled(
                    truncate(&event.summary, inner.width.saturating_sub(13) as usize),
                    row_style,
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
    frame.render_widget(
        Paragraph::new(content)
            .style(Style::new().bg(PANEL))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn activity_appearance(event: &crate::events::EventRecord) -> (&'static str, Color) {
    if event.summary.contains("échou") || event.summary.contains("Interrompt") {
        return ("!", RED);
    }
    match event.event.as_str() {
        "TaskComplete" | "SessionEnd" | "Stop" => ("✓", GREEN),
        "PatchApplied" => ("◆", MAGENTA),
        "TaskStarted" | "SessionStart" => ("▶", CYAN),
        "PermissionRequest" => ("!", AMBER),
        "ContextCompacted" => ("◇", VIOLET),
        _ => match event.tool_name.as_deref() {
            Some("exec" | "exec_command" | "Bash") => ("›", CYAN),
            Some("apply_patch" | "Edit" | "Write") => ("◆", MAGENTA),
            Some("web" | "web__run") => ("◉", BLUE),
            Some(tool) if tool.starts_with("mcp__") => ("●", VIOLET),
            _ => ("·", Color::White),
        },
    }
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
    let hover_hint = if dashboard.hovered_event.is_some() {
        "  CLIQUER : ouvrir le détail"
    } else if dashboard
        .mouse_position
        .is_some_and(|(column, row)| thread_at(dashboard, column, row).is_some())
    {
        "  CLIQUER : fiche agent"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" FLÈCHES ", Style::new().fg(BG).bg(CYAN).bold()),
            Span::styled(" Pièces  ", Style::new().fg(MUTED)),
            Span::styled("GLISSER/MAJ+FLÈCHES", Style::new().fg(Color::White)),
            Span::styled(" Caméra  ", Style::new().fg(MUTED)),
            Span::styled("CTRL+MOLETTE/+−", Style::new().fg(Color::White)),
            Span::styled(" Zoom  ", Style::new().fg(MUTED)),
            Span::styled("0", Style::new().fg(Color::White)),
            Span::styled(" Recentrer  ", Style::new().fg(MUTED)),
            Span::styled(
                format!("  {}", dashboard.capabilities.terminal),
                Style::new().fg(Color::Rgb(54, 82, 116)),
            ),
            Span::styled(format!("  {status}"), Style::new().fg(RED)),
            Span::styled(hover_hint, Style::new().fg(AMBER).bold()),
        ]))
        .style(Style::new().bg(BG)),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new("↻ ACTUALISER")
            .alignment(Alignment::Center)
            .style(
                Style::new().fg(CYAN).bg(
                    if dashboard
                        .mouse_position
                        .is_some_and(|point| columns[1].contains(point.into()))
                    {
                        Color::Rgb(15, 61, 84)
                    } else {
                        Color::Rgb(9, 32, 54)
                    },
                ),
            )
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
            .style(
                Style::new().fg(Color::White).bg(
                    if dashboard
                        .mouse_position
                        .is_some_and(|point| columns[2].contains(point.into()))
                    {
                        Color::Rgb(82, 24, 45)
                    } else {
                        Color::Rgb(42, 15, 29)
                    },
                ),
            )
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
        let scene = dashboard.scene();
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
    dashboard
        .thread_hitboxes
        .iter()
        .find_map(|(index, area)| area.contains((column, row).into()).then_some(*index))
}

pub fn room_at(dashboard: &Dashboard, column: u16, row: u16) -> Option<usize> {
    let area = dashboard.scene_area;
    if !area.contains((column, row).into()) {
        return None;
    }
    dashboard.scene().room_at(
        area.width as f32,
        area.height as f32 * 2.0,
        (column - area.x) as f32,
        (row - area.y) as f32 * 2.0,
    )
}

pub fn option_at(dashboard: &Dashboard, column: u16, row: u16) -> Option<OptionAction> {
    dashboard
        .option_hitboxes
        .iter()
        .find_map(|(action, area)| area.contains((column, row).into()).then_some(*action))
}

pub fn event_at(dashboard: &Dashboard, column: u16, row: u16) -> Option<usize> {
    if !dashboard.event_area.contains((column, row).into()) {
        return None;
    }
    let offset = (row - dashboard.event_area.y) as usize;
    if offset
        >= dashboard
            .settings
            .journal_density
            .rows(dashboard.event_area.height as usize)
    {
        return None;
    }
    dashboard.events.len().checked_sub(offset + 1)
}

struct ProjectGroup {
    key: String,
    name: String,
    threads: Vec<usize>,
    updated_at: i64,
}

fn project_groups(threads: &[ThreadSummary]) -> Vec<ProjectGroup> {
    let mut groups = Vec::<ProjectGroup>::new();
    for (index, thread) in threads.iter().enumerate() {
        let key = project_key(&thread.cwd);
        if let Some(group) = groups.iter_mut().find(|group| group.key == key) {
            group.threads.push(index);
            group.updated_at = group.updated_at.max(thread.updated_at);
        } else {
            groups.push(ProjectGroup {
                name: project_name(&thread.cwd),
                key,
                threads: vec![index],
                updated_at: thread.updated_at,
            });
        }
    }
    for group in &mut groups {
        group
            .threads
            .sort_by_key(|&index| std::cmp::Reverse(threads[index].updated_at));
    }
    groups.sort_by_key(|group| std::cmp::Reverse(group.updated_at));
    groups
}

fn project_color(index: usize) -> Color {
    [CYAN, VIOLET, MAGENTA, AMBER, GREEN, BLUE][index % 6]
}

fn is_active(thread: &ThreadSummary) -> bool {
    matches!(
        thread.status,
        ThreadStatus::Active { .. } | ThreadStatus::ObservedRunning
    )
}

fn relative_time(timestamp: i64) -> String {
    let seconds = Local::now().timestamp().saturating_sub(timestamp).max(0) as u64;
    match seconds {
        0..=59 => "maint.".to_owned(),
        60..=3599 => format!("{}m", seconds / 60),
        3600..=86_399 => format!("{}h", seconds / 3600),
        _ => format!("{}j", seconds / 86_400),
    }
}

fn compact_number(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.0}k", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn format_duration(milliseconds: u64) -> String {
    let seconds = milliseconds / 1_000;
    if seconds >= 3600 {
        format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60)
    } else if seconds >= 60 {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    } else if seconds > 0 {
        format!("{}.{:01}s", seconds, milliseconds % 1_000 / 100)
    } else {
        format!("{milliseconds}ms")
    }
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
        ThreadStatus::ObservedRunning => "EN COURS · OBSERVÉE",
        ThreadStatus::ObservedOpen => "SESSION OUVERTE",
        ThreadStatus::RecentlyActive => "ACTIVITÉ RÉCENTE",
        ThreadStatus::NeedsAttention => "INTERVENTION",
        ThreadStatus::SystemError => "ERREUR",
        ThreadStatus::Idle => "DISPONIBLE",
        ThreadStatus::NotLoaded => "ENREGISTRÉE",
    }
}

fn status_short(thread: &ThreadSummary) -> &'static str {
    match thread.status {
        ThreadStatus::Active { .. } | ThreadStatus::ObservedRunning => "● ACTIF",
        ThreadStatus::NeedsAttention => "! INTERVENTION",
        ThreadStatus::SystemError => "! ERREUR",
        ThreadStatus::RecentlyActive => "◐ RÉCENT",
        ThreadStatus::ObservedOpen | ThreadStatus::Idle | ThreadStatus::NotLoaded => "○ REPOS",
    }
}

fn status_color(thread: &ThreadSummary) -> Color {
    match thread.status {
        ThreadStatus::Active { .. } => CYAN,
        ThreadStatus::ObservedRunning => Color::Rgb(61, 244, 178),
        ThreadStatus::ObservedOpen => BLUE,
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
                vte: false,
                vte_sixel_build: false,
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
                runtime: Default::default(),
            }],
            events: Vec::new(),
            selected: 0,
            camera_yaw: 0.3,
            camera_pitch: 0.2,
            camera_zoom: 1.0,
            camera_focus: glam::Vec2::splat(0.5),
            camera_focus_target: glam::Vec2::splat(0.5),
            started_at: Instant::now(),
            last_refresh: Instant::now(),
            scene_area: Rect::default(),
            thread_area: Rect::default(),
            thread_hitboxes: Vec::new(),
            refresh_button: Rect::default(),
            quit_button: Rect::default(),
            event_area: Rect::default(),
            should_quit: false,
            dragging: false,
            last_mouse: None,
            status_message: None,
            last_ultra_frame: Instant::now(),
            scene_dirty: true,
            last_camera_input: None,
            zoom_gesture: None,
            scene_refresh_pending: false,
            refresh_requested: false,
            final_frame_pending: false,
            hovered_event: None,
            selected_event: None,
            agent_detail_open: false,
            mouse_position: None,
            pointer_shape: "default",
            settings: crate::config::UserSettings::default(),
            focused_room: 0,
            options_open: false,
            option_hitboxes: Vec::new(),
        };
        let backend = TestBackend::new(180, 52);
        let mut terminal = Terminal::new(backend).unwrap();
        dashboard.threads[0].runtime.last_action = Some("Exécute les tests".to_owned());
        dashboard.threads[0].runtime.last_command = Some("cargo test --all-features".to_owned());
        dashboard.threads[0].runtime.last_command_workdir = Some("/work/example".to_owned());
        dashboard.threads[0].runtime.last_tool = Some("exec_command".to_owned());
        dashboard.threads[0].runtime.last_action_at = Some(1_785_096_002);
        terminal.draw(|frame| draw(frame, &mut dashboard)).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("CODEX"));
        assert!(rendered.contains("PROJETS · CONVERSATIONS"));
        assert!(rendered.contains("DÉTAIL DE LA ROOM"));
        assert!(rendered.contains("cargo test --all-features"));
        assert!(rendered.contains("COMPLEXE D’OPÉRATIONS"));
        assert!(rendered.contains("Example operation"));
        assert!(!dashboard.scene_area.is_empty());
        assert!(!dashboard.thread_hitboxes.is_empty());
        let (_, hitbox) = dashboard.thread_hitboxes[0];
        assert_eq!(thread_at(&dashboard, hitbox.x, hitbox.y), Some(0));

        dashboard.agent_detail_open = true;
        terminal.draw(|frame| draw(frame, &mut dashboard)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("FICHE AGENT"));
        assert!(rendered.contains("RAISONNEMENT CODEX"));

        dashboard.agent_detail_open = false;
        dashboard.options_open = true;
        terminal.draw(|frame| draw(frame, &mut dashboard)).unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("OPTIONS DU CENTRE"));
        assert!(rendered.contains("ACTUALISATION"));
        assert_eq!(dashboard.option_hitboxes.len(), 4);
    }
}
