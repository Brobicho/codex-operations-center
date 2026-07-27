use std::io::{self, Stdout, Write};
use std::sync::mpsc;
use std::thread;
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
use crate::config::{GraphicsChoice, OptionAction, UserSettings};
use crate::events::{self, EventRecord};
use crate::scene::{RoomTarget, Scene};
use crate::{kitty, ui};

const MIN_CAMERA_ZOOM: f32 = 0.65;
const MAX_CAMERA_ZOOM: f32 = 1.30;
const CAMERA_SETTLE: Duration = Duration::from_millis(100);
const PREVIEW_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const FULL_FRAME_INTERVAL: Duration = Duration::from_millis(80);

pub struct Dashboard {
    pub capabilities: Capabilities,
    pub profile: RenderingProfile,
    pub threads: Vec<ThreadSummary>,
    pub events: Vec<EventRecord>,
    pub selected: usize,
    pub camera_yaw: f32,
    pub camera_pitch: f32,
    pub camera_zoom: f32,
    pub camera_focus: glam::Vec2,
    pub camera_focus_target: glam::Vec2,
    pub started_at: Instant,
    pub last_refresh: Instant,
    pub scene_area: ratatui::layout::Rect,
    pub thread_area: ratatui::layout::Rect,
    pub thread_hitboxes: Vec<(usize, ratatui::layout::Rect)>,
    pub refresh_button: ratatui::layout::Rect,
    pub quit_button: ratatui::layout::Rect,
    pub event_area: ratatui::layout::Rect,
    pub should_quit: bool,
    pub dragging: bool,
    pub last_mouse: Option<(u16, u16)>,
    pub status_message: Option<String>,
    pub last_ultra_frame: Instant,
    pub scene_dirty: bool,
    pub last_camera_input: Option<Instant>,
    pub zoom_gesture: Option<(i8, Instant)>,
    pub scene_refresh_pending: bool,
    pub refresh_requested: bool,
    pub final_frame_pending: bool,
    pub hovered_event: Option<usize>,
    pub selected_event: Option<usize>,
    pub agent_detail_open: bool,
    pub mouse_position: Option<(u16, u16)>,
    pub pointer_shape: &'static str,
    pub settings: UserSettings,
    pub focused_room: usize,
    pub options_open: bool,
    pub option_hitboxes: Vec<(OptionAction, ratatui::layout::Rect)>,
}

impl Dashboard {
    fn new(capabilities: Capabilities, profile: RenderingProfile, settings: UserSettings) -> Self {
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
            camera_focus: glam::Vec2::splat(0.5),
            camera_focus_target: glam::Vec2::splat(0.5),
            started_at: Instant::now(),
            last_refresh: Instant::now(),
            scene_area: ratatui::layout::Rect::default(),
            thread_area: ratatui::layout::Rect::default(),
            thread_hitboxes: Vec::new(),
            refresh_button: ratatui::layout::Rect::default(),
            quit_button: ratatui::layout::Rect::default(),
            event_area: ratatui::layout::Rect::default(),
            should_quit: false,
            dragging: false,
            last_mouse: None,
            status_message: None,
            last_ultra_frame: Instant::now() - Duration::from_secs(1),
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
            settings,
            focused_room: 0,
            options_open: false,
            option_hitboxes: Vec::new(),
        }
    }

    pub fn scene(&self) -> Scene {
        let mut scene = Scene::from_threads_with_options(
            &self.effective_threads(),
            self.camera_yaw,
            self.camera_pitch,
            self.camera_zoom,
            self.started_at.elapsed().as_secs_f32(),
            self.selected,
            self.focused_room,
            self.settings.show_resting_agents,
        );
        scene.camera_focus = self.camera_focus;
        scene
    }

    fn activate_room(&mut self, room_index: usize) {
        let scene = self.scene();
        self.focused_room = room_index.min(scene.rooms.len().saturating_sub(1));
        if let Some(center) = scene.rooms.get(self.focused_room).map(|room| room.center) {
            self.camera_focus_target = self.camera_target_for(center);
            self.last_camera_input = Some(Instant::now());
            self.final_frame_pending = true;
        }
        match scene.room_target(self.focused_room) {
            Some(RoomTarget::Thread(index)) => {
                self.selected = index;
                self.options_open = false;
            }
            Some(RoomTarget::Options) => {
                self.options_open = true;
                self.agent_detail_open = false;
            }
            None => {}
        }
        self.selected_event = None;
        self.scene_dirty = true;
    }

    fn move_room_focus(&mut self, direction: glam::Vec2) {
        let scene = self.scene();
        let next = scene.next_room(self.focused_room, direction);
        self.activate_room(next);
    }

    fn sync_room_to_thread(&mut self) {
        let scene = self.scene();
        if let Some(room) = scene.room_for_thread(self.selected) {
            self.focused_room = room;
            self.options_open = false;
            if let Some(center) = scene.rooms.get(room).map(|room| room.center) {
                self.camera_focus_target = self.camera_target_for(center);
                self.last_camera_input = Some(Instant::now());
                self.final_frame_pending = true;
            }
        }
    }

    fn camera_target_for(&self, room_center: glam::Vec2) -> glam::Vec2 {
        let manual_offset = glam::Vec2::new(
            (self.camera_yaw - 0.35) * 0.055,
            (self.camera_pitch - 0.22) * 0.08,
        );
        room_center + manual_offset / self.camera_zoom.max(0.01)
    }

    fn animate_camera_focus(&mut self) -> bool {
        let delta = self.camera_focus_target - self.camera_focus;
        if delta.length_squared() < 0.000_001 {
            if self.camera_focus != self.camera_focus_target {
                self.camera_focus = self.camera_focus_target;
                self.scene_dirty = true;
                return true;
            }
            return false;
        }
        self.camera_focus += delta * 0.22;
        self.scene_dirty = true;
        self.last_camera_input = Some(Instant::now());
        true
    }

    fn cycle_option(&mut self, action: OptionAction) {
        let previous_profile = self.profile;
        self.settings.cycle(action);
        self.profile = self.capabilities.select(match self.settings.graphics {
            GraphicsChoice::Auto => crate::GraphicsMode::Auto,
            GraphicsChoice::Ultra => crate::GraphicsMode::Ultra,
            GraphicsChoice::Unicode => crate::GraphicsMode::Unicode,
            GraphicsChoice::Safe => crate::GraphicsMode::Safe,
        });
        self.status_message = self
            .settings
            .save()
            .err()
            .map(|error| format!("Options non enregistrées : {error}"));
        self.scene_dirty = true;
        self.final_frame_pending = true;
        if previous_profile == RenderingProfile::Ultra && self.profile != RenderingProfile::Ultra {
            let _ = kitty::delete_scene();
        }
    }

    fn apply_refresh(&mut self, result: Result<(Vec<ThreadSummary>, Vec<EventRecord>), String>) {
        let previous_scene = self.scene_signature();
        match result {
            Ok((threads, events)) => {
                self.threads = threads;
                self.events = events;
                self.selected = self.selected.min(self.threads.len().saturating_sub(1));
                self.status_message = None;
            }
            Err(error) => self.status_message = Some(format!("Codex indisponible : {error}")),
        }
        self.scene_refresh_pending |= previous_scene != self.scene_signature();
    }

    fn scene_signature(&self) -> Vec<(String, String, String, bool, &'static str)> {
        let mut projects = Vec::<(String, usize)>::new();
        let mut signature = Vec::new();
        for (index, thread) in self.effective_threads().into_iter().enumerate() {
            let thread_project = crate::scene::project_key(&thread.cwd);
            let project_index = projects
                .iter()
                .position(|(key, _)| key == &thread_project)
                .or_else(|| {
                    if projects.len() >= 6 {
                        None
                    } else {
                        projects.push((thread_project, 0));
                        Some(projects.len() - 1)
                    }
                });
            let Some(project_index) = project_index else {
                continue;
            };
            if projects[project_index].1 >= 3 {
                continue;
            }
            projects[project_index].1 += 1;
            let label = thread
                .name
                .clone()
                .unwrap_or_else(|| thread.preview.clone());
            let state = match thread.status {
                crate::codex::ThreadStatus::Active { .. }
                | crate::codex::ThreadStatus::ObservedRunning => "active",
                crate::codex::ThreadStatus::SystemError
                | crate::codex::ThreadStatus::NeedsAttention => "attention",
                _ => "rest",
            };
            signature.push((thread.id, thread.cwd, label, index == self.selected, state));
        }
        signature
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

    fn wheel_zoom(&mut self, direction: i8) {
        let now = Instant::now();
        if self.zoom_gesture.is_some_and(|(previous, received_at)| {
            previous != direction && received_at.elapsed() < Duration::from_millis(260)
        }) {
            self.last_camera_input = Some(now);
            return;
        }
        self.camera_zoom =
            (self.camera_zoom + direction as f32 * 0.04).clamp(MIN_CAMERA_ZOOM, MAX_CAMERA_ZOOM);
        self.zoom_gesture = Some((direction, now));
        self.last_camera_input = Some(now);
        self.scene_dirty = true;
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Key(key)
                if key.kind == KeyEventKind::Press
                    && key.code == KeyCode::Char('c')
                    && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.should_quit = true
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') => self.should_quit = true,
                KeyCode::Esc => {
                    if self.selected_event.is_some() {
                        self.selected_event = None;
                    } else if self.agent_detail_open {
                        self.agent_detail_open = false;
                    } else if self.options_open {
                        self.options_open = false;
                    } else {
                        self.should_quit = true;
                    }
                }
                KeyCode::Char('j') => {
                    if !self.threads.is_empty() {
                        self.selected = (self.selected + 1).min(self.threads.len() - 1);
                        self.sync_room_to_thread();
                        self.scene_dirty = true;
                    }
                }
                KeyCode::Char('k') => {
                    self.selected = self.selected.saturating_sub(1);
                    self.sync_room_to_thread();
                    self.scene_dirty = true;
                }
                KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.camera_yaw -= 0.12;
                    self.scene_dirty = true;
                    self.last_camera_input = Some(Instant::now());
                }
                KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.camera_yaw += 0.12;
                    self.scene_dirty = true;
                    self.last_camera_input = Some(Instant::now());
                }
                KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.camera_pitch = (self.camera_pitch - 0.08).max(-0.15);
                    self.scene_dirty = true;
                    self.last_camera_input = Some(Instant::now());
                }
                KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    self.camera_pitch = (self.camera_pitch + 0.08).min(0.65);
                    self.scene_dirty = true;
                    self.last_camera_input = Some(Instant::now());
                }
                KeyCode::Left => self.move_room_focus(glam::Vec2::NEG_X),
                KeyCode::Right => self.move_room_focus(glam::Vec2::X),
                KeyCode::Up => self.move_room_focus(glam::Vec2::NEG_Y),
                KeyCode::Down => self.move_room_focus(glam::Vec2::Y),
                KeyCode::Char('h') => {
                    self.camera_yaw -= 0.12;
                    self.scene_dirty = true;
                    self.last_camera_input = Some(Instant::now());
                }
                KeyCode::Char('l') => {
                    self.camera_yaw += 0.12;
                    self.scene_dirty = true;
                    self.last_camera_input = Some(Instant::now());
                }
                KeyCode::Char('+') | KeyCode::Char('=') => {
                    self.camera_zoom = (self.camera_zoom + 0.06).min(MAX_CAMERA_ZOOM);
                    self.scene_dirty = true;
                    self.last_camera_input = Some(Instant::now());
                }
                KeyCode::Char('-') => {
                    self.camera_zoom = (self.camera_zoom - 0.06).max(MIN_CAMERA_ZOOM);
                    self.scene_dirty = true;
                    self.last_camera_input = Some(Instant::now());
                }
                KeyCode::Char('0') => {
                    self.camera_yaw = 0.35;
                    self.camera_pitch = 0.22;
                    self.camera_zoom = 1.0;
                    self.camera_focus = glam::Vec2::splat(0.5);
                    self.camera_focus_target = glam::Vec2::splat(0.5);
                    self.scene_dirty = true;
                    self.last_camera_input = Some(Instant::now());
                }
                KeyCode::Char('r') => {
                    self.refresh_requested = true;
                    self.scene_dirty = true;
                }
                _ => {}
            },
            Event::Mouse(mouse) => {
                self.mouse_position = Some((mouse.column, mouse.row));
                self.hovered_event = ui::event_at(self, mouse.column, mouse.row);
                match mouse.kind {
                    MouseEventKind::ScrollUp
                        if mouse.modifiers.contains(KeyModifiers::CONTROL)
                            && self.scene_area.contains((mouse.column, mouse.row).into()) =>
                    {
                        self.wheel_zoom(1);
                    }
                    MouseEventKind::ScrollDown
                        if mouse.modifiers.contains(KeyModifiers::CONTROL)
                            && self.scene_area.contains((mouse.column, mouse.row).into()) =>
                    {
                        self.wheel_zoom(-1);
                    }
                    MouseEventKind::Down(MouseButton::Left) => {
                        let point = (mouse.column, mouse.row).into();
                        if let Some(action) = ui::option_at(self, mouse.column, mouse.row) {
                            self.cycle_option(action);
                            return;
                        }
                        if let Some(index) = ui::event_at(self, mouse.column, mouse.row) {
                            self.selected_event = Some(index);
                            self.agent_detail_open = false;
                            return;
                        }
                        if self.refresh_button.contains(point) {
                            self.refresh_requested = true;
                            self.scene_dirty = true;
                            return;
                        }
                        if self.quit_button.contains(point) {
                            self.should_quit = true;
                            return;
                        }
                        if let Some(index) = ui::thread_at(self, mouse.column, mouse.row) {
                            if self.selected != index {
                                self.selected = index;
                                self.scene_dirty = true;
                            }
                            self.selected_event = None;
                            self.agent_detail_open = true;
                            self.sync_room_to_thread();
                            return;
                        }
                        if let Some(room) = ui::room_at(self, mouse.column, mouse.row) {
                            self.activate_room(room);
                            if self.options_open {
                                return;
                            }
                        }
                        if !self.scene_area.contains(point) {
                            return;
                        }
                        self.dragging = true;
                        self.last_mouse = Some((mouse.column, mouse.row));
                    }
                    MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                        if let Some((column, row)) = self.last_mouse {
                            self.camera_yaw += (mouse.column as f32 - column as f32) * 0.025;
                            self.camera_pitch = (self.camera_pitch
                                + (mouse.row as f32 - row as f32) * 0.015)
                                .clamp(-0.15, 0.65);
                        }
                        self.last_mouse = Some((mouse.column, mouse.row));
                        self.scene_dirty = true;
                        self.last_camera_input = Some(Instant::now());
                    }
                    MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                        self.dragging = false;
                        self.last_mouse = None;
                        self.scene_dirty = true;
                        self.last_camera_input = Some(Instant::now() - Duration::from_millis(200));
                    }
                    _ => {}
                }
            }
            Event::Resize(_, _) => self.scene_dirty = true,
            _ => {}
        }
    }

    fn update_pointer_shape(&mut self) -> Result<()> {
        let Some((column, row)) = self.mouse_position else {
            return Ok(());
        };
        let point = (column, row).into();
        let desired = if self.dragging {
            "grabbing"
        } else if ui::event_at(self, column, row).is_some()
            || (self.thread_area.contains(point) && ui::thread_at(self, column, row).is_some())
            || ui::room_at(self, column, row).is_some()
            || ui::option_at(self, column, row).is_some()
            || self.refresh_button.contains(point)
            || self.quit_button.contains(point)
        {
            "pointer"
        } else if self.scene_area.contains(point) {
            "grab"
        } else {
            "default"
        };
        if desired != self.pointer_shape {
            let mut stdout = io::stdout().lock();
            write!(stdout, "\x1b]22;{desired}\x1b\\")?;
            stdout.flush()?;
            self.pointer_shape = desired;
        }
        Ok(())
    }
}

pub fn run(
    capabilities: Capabilities,
    profile: RenderingProfile,
    settings: UserSettings,
) -> Result<()> {
    // This is an interactive visual application: an inherited NO_COLOR from a
    // parent agent shell must not silently erase the dashboard palette.
    crossterm::style::force_color_output(true);
    if !capabilities.mouse {
        println!("Codex Operations Center requires an interactive terminal.");
        println!("Run `codex-ops doctor` for capability details.");
        return Ok(());
    }

    let mut guard = TerminalGuard::enter()?;
    let mut dashboard = Dashboard::new(capabilities, profile, settings);
    let tick_rate = Duration::from_millis(16);
    let mut ui_dirty = true;
    let mut last_ui_frame = Instant::now() - Duration::from_secs(1);
    let (refresh_sender, refresh_receiver) = mpsc::channel();
    let mut refresh_in_flight = false;

    while !dashboard.should_quit {
        let frame_started = Instant::now();
        let wait = tick_rate.saturating_sub(frame_started.elapsed());
        if event::poll(wait)? {
            for _ in 0..64 {
                let event = event::read()?;
                let previous_hover = dashboard.hovered_event;
                let affects_ui = !matches!(
                    event,
                    Event::Mouse(crossterm::event::MouseEvent {
                        kind: MouseEventKind::ScrollUp
                            | MouseEventKind::ScrollDown
                            | MouseEventKind::Drag(_)
                            | MouseEventKind::Up(_),
                        ..
                    }) | Event::Key(crossterm::event::KeyEvent {
                        code: KeyCode::Char('h' | 'l' | '+' | '=' | '-' | '0'),
                        ..
                    })
                );
                dashboard.on_event(event);
                ui_dirty |= affects_ui || previous_hover != dashboard.hovered_event;
                dashboard.update_pointer_shape()?;
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
        if let Ok(result) = refresh_receiver.try_recv() {
            refresh_in_flight = false;
            dashboard.apply_refresh(result);
            if dashboard.scene_refresh_pending
                && dashboard.last_ultra_frame.elapsed() >= Duration::from_secs(30)
            {
                dashboard.scene_dirty = true;
                dashboard.scene_refresh_pending = false;
            }
            ui_dirty = true;
        }
        if dashboard.animate_camera_focus() {
            ui_dirty = true;
        }
        if (dashboard.refresh_requested
            || dashboard.last_refresh.elapsed()
                >= Duration::from_secs(dashboard.settings.refresh.seconds()))
            && !refresh_in_flight
        {
            dashboard.refresh_requested = false;
            dashboard.last_refresh = Instant::now();
            refresh_in_flight = true;
            let sender = refresh_sender.clone();
            thread::spawn(move || {
                let _ = sender.send(load_dashboard_data());
            });
        }
        if ui_dirty || last_ui_frame.elapsed() >= Duration::from_secs(1) {
            guard
                .terminal
                .draw(|frame| ui::draw(frame, &mut dashboard))?;
            ui_dirty = false;
            last_ui_frame = Instant::now();
        }
        if dashboard.profile == RenderingProfile::Ultra && !dashboard.scene_area.is_empty() {
            let camera_active = dashboard.dragging
                || dashboard
                    .last_camera_input
                    .is_some_and(|last_input| last_input.elapsed() < CAMERA_SETTLE);
            let preview_requested = dashboard.scene_dirty && camera_active;
            let full_requested =
                !camera_active && (dashboard.scene_dirty || dashboard.final_frame_pending);
            let frame_interval = if preview_requested {
                PREVIEW_FRAME_INTERVAL
            } else {
                FULL_FRAME_INTERVAL
            };
            if (preview_requested || full_requested)
                && dashboard.last_ultra_frame.elapsed() >= frame_interval
            {
                kitty::draw_scene(&dashboard, preview_requested)?;
                dashboard.last_ultra_frame = Instant::now();
                dashboard.scene_dirty = false;
                dashboard.final_frame_pending = preview_requested;
                if !preview_requested {
                    dashboard.last_camera_input = None;
                    dashboard.zoom_gesture = None;
                }
            }
        }
    }
    if dashboard.profile == RenderingProfile::Ultra {
        kitty::delete_scene()?;
    }
    Ok(())
}

fn load_dashboard_data() -> Result<(Vec<ThreadSummary>, Vec<EventRecord>), String> {
    let threads = codex::list_threads(250).map_err(|error| format!("{error:#}"))?;
    let events = events::recent_for_threads(&threads, 500).map_err(|error| format!("{error:#}"))?;
    Ok((threads, events))
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
        let _ = write!(self.terminal.backend_mut(), "\x1b]22;\x1b\\");
    }
}
