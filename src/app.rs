use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::Utc;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::capabilities::{Capabilities, RenderingProfile};
use crate::codex::{self, ThreadSummary};
use crate::events::{self, EventRecord};
use crate::{kitty, ui};

const MIN_CAMERA_ZOOM: f32 = 0.65;
const MAX_CAMERA_ZOOM: f32 = 1.30;

pub struct Dashboard {
    pub capabilities: Capabilities,
    pub profile: RenderingProfile,
    pub threads: Vec<ThreadSummary>,
    pub events: Vec<EventRecord>,
    pub selected: usize,
    pub camera_yaw: f32,
    pub camera_pitch: f32,
    pub camera_zoom: f32,
    pub started_at: Instant,
    pub last_refresh: Instant,
    pub scene_area: ratatui::layout::Rect,
    pub thread_area: ratatui::layout::Rect,
    pub refresh_button: ratatui::layout::Rect,
    pub quit_button: ratatui::layout::Rect,
    pub should_quit: bool,
    pub dragging: bool,
    pub last_mouse: Option<(u16, u16)>,
    pub status_message: Option<String>,
    pub last_ultra_frame: Instant,
    pub scene_dirty: bool,
}

impl Dashboard {
    fn new(capabilities: Capabilities, profile: RenderingProfile) -> Self {
        let threads = codex::list_threads(250).unwrap_or_default();
        let events = events::recent_for_threads(&threads, 500).unwrap_or_default();
        Self {
            capabilities,
            profile,
            threads,
            events,
            selected: 0,
            camera_yaw: 0.35,
            camera_pitch: 0.22,
            camera_zoom: 1.0,
            started_at: Instant::now(),
            last_refresh: Instant::now(),
            scene_area: ratatui::layout::Rect::default(),
            thread_area: ratatui::layout::Rect::default(),
            refresh_button: ratatui::layout::Rect::default(),
            quit_button: ratatui::layout::Rect::default(),
            should_quit: false,
            dragging: false,
            last_mouse: None,
            status_message: None,
            last_ultra_frame: Instant::now() - Duration::from_secs(1),
            scene_dirty: true,
        }
    }

    fn refresh(&mut self) {
        match codex::list_threads(250) {
            Ok(threads) => {
                self.threads = threads;
                self.selected = self.selected.min(self.threads.len().saturating_sub(1));
                self.status_message = None;
            }
            Err(error) => self.status_message = Some(format!("Codex indisponible : {error:#}")),
        }
        if let Ok(events) = events::recent_for_threads(&self.threads, 500) {
            self.events = events;
        }
        self.last_refresh = Instant::now();
        self.scene_dirty = true;
    }

    pub fn effective_threads(&self) -> Vec<ThreadSummary> {
        self.threads
            .iter()
            .cloned()
            .map(|mut thread| {
                if matches!(
                    thread.status,
                    crate::codex::ThreadStatus::Active { .. }
                        | crate::codex::ThreadStatus::SystemError
                        | crate::codex::ThreadStatus::ObservedRunning
                        | crate::codex::ThreadStatus::ObservedOpen
                ) {
                    return thread;
                }
                let latest = self.events.iter().rev().find(|event| {
                    event.session_id == thread.session_id || event.session_id == thread.id
                });
                if let Some(event) = latest {
                    let is_recent = Utc::now()
                        .signed_duration_since(event.received_at)
                        .num_minutes()
                        < 10;
                    thread.status = match event.event.as_str() {
                        "PermissionRequest" => crate::codex::ThreadStatus::NeedsAttention,
                        "SessionEnd" | "Stop" => crate::codex::ThreadStatus::Idle,
                        _ if is_recent => crate::codex::ThreadStatus::RecentlyActive,
                        _ => thread.status,
                    };
                }
                thread
            })
            .collect()
    }

    fn on_event(&mut self, event: Event) {
        self.scene_dirty = true;
        match event {
            Event::Key(key)
                if key.kind == KeyEventKind::Press
                    && key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.should_quit = true
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
                KeyCode::Down | KeyCode::Char('j') => {
                    if !self.threads.is_empty() {
                        self.selected = (self.selected + 1).min(self.threads.len() - 1);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
                KeyCode::Left | KeyCode::Char('h') => self.camera_yaw -= 0.12,
                KeyCode::Right | KeyCode::Char('l') => self.camera_yaw += 0.12,
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    self.camera_zoom = (self.camera_zoom + 0.06).min(MAX_CAMERA_ZOOM)
                }
                KeyCode::Char('-') => {
                    self.camera_zoom = (self.camera_zoom - 0.06).max(MIN_CAMERA_ZOOM)
                }
                KeyCode::Char('0') => {
                    self.camera_yaw = 0.35;
                    self.camera_pitch = 0.22;
                    self.camera_zoom = 1.0;
                }
                KeyCode::Char('r') => self.refresh(),
                _ => {}
            },
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp
                    if mouse.modifiers.contains(KeyModifiers::CONTROL)
                        && self.scene_area.contains((mouse.column, mouse.row).into()) =>
                {
                    self.camera_zoom = (self.camera_zoom + 0.06).min(MAX_CAMERA_ZOOM)
                }
                MouseEventKind::ScrollDown
                    if mouse.modifiers.contains(KeyModifiers::CONTROL)
                        && self.scene_area.contains((mouse.column, mouse.row).into()) =>
                {
                    self.camera_zoom = (self.camera_zoom - 0.06).max(MIN_CAMERA_ZOOM)
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    let point = (mouse.column, mouse.row).into();
                    if self.refresh_button.contains(point) {
                        self.refresh();
                        return;
                    }
                    if self.quit_button.contains(point) {
                        self.should_quit = true;
                        return;
                    }
                    self.dragging = true;
                    self.last_mouse = Some((mouse.column, mouse.row));
                    if let Some(index) = ui::thread_at(self, mouse.column, mouse.row) {
                        self.selected = index;
                    }
                }
                MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                    if let Some((column, row)) = self.last_mouse {
                        self.camera_yaw += (mouse.column as f32 - column as f32) * 0.025;
                        self.camera_pitch = (self.camera_pitch
                            + (mouse.row as f32 - row as f32) * 0.015)
                            .clamp(-0.15, 0.65);
                    }
                    self.last_mouse = Some((mouse.column, mouse.row));
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.dragging = false;
                    self.last_mouse = None;
                }
                _ => {}
            },
            _ => {}
        }
    }
}

pub fn run(capabilities: Capabilities, profile: RenderingProfile) -> Result<()> {
    if !capabilities.mouse {
        println!("Codex Operations Center requires an interactive terminal.");
        println!("Run `codex-ops doctor` for capability details.");
        return Ok(());
    }

    let mut guard = TerminalGuard::enter()?;
    let mut dashboard = Dashboard::new(capabilities, profile);
    let tick_rate = Duration::from_millis(80);

    while !dashboard.should_quit {
        let frame_started = Instant::now();
        guard
            .terminal
            .draw(|frame| ui::draw(frame, &mut dashboard))?;
        if dashboard.profile == RenderingProfile::Ultra
            && !dashboard.scene_area.is_empty()
            && (!dashboard.capabilities.sixel_graphics || dashboard.scene_dirty)
            && dashboard.last_ultra_frame.elapsed()
                >= if dashboard.capabilities.sixel_graphics {
                    Duration::from_millis(450)
                } else {
                    Duration::from_millis(160)
                }
        {
            kitty::draw_scene(&dashboard)?;
            dashboard.last_ultra_frame = Instant::now();
            dashboard.scene_dirty = false;
        }

        let wait = tick_rate.saturating_sub(frame_started.elapsed());
        if event::poll(wait)? {
            dashboard.on_event(event::read()?);
        }
        if dashboard.last_refresh.elapsed() >= Duration::from_secs(3) {
            dashboard.refresh();
        }
    }
    if dashboard.profile == RenderingProfile::Ultra {
        kitty::delete_scene()?;
    }
    Ok(())
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;
        terminal.clear()?;
        Ok(Self { terminal })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();
    }
}
