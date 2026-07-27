use std::sync::{Mutex, OnceLock};

use fontdue::layout::{CoordinateSystem, Layout, LayoutSettings, TextStyle};
use glam::{Vec2, Vec3};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Widget;

use crate::codex::{ThreadStatus, ThreadSummary};

const PALETTE: [Vec3; 6] = [
    Vec3::new(0.08, 0.72, 1.00),
    Vec3::new(0.48, 0.35, 1.00),
    Vec3::new(0.93, 0.31, 0.78),
    Vec3::new(1.00, 0.54, 0.18),
    Vec3::new(0.20, 0.90, 0.58),
    Vec3::new(0.08, 0.86, 0.84),
];

#[derive(Clone, Debug)]
pub struct SceneNode {
    pub thread_id: String,
    pub parent_thread_id: Option<String>,
    /// Normalized screen position. The z component is used for draw ordering.
    pub position: Vec3,
    pub radius: f32,
    pub color: Vec3,
    pub label: String,
    pub state_label: String,
    pub thread_index: usize,
    pub active: bool,
    pub attention: bool,
}

#[derive(Clone, Debug)]
pub struct SceneRoom {
    pub label: String,
    pub center: Vec2,
    pub half_width: f32,
    pub half_height: f32,
    pub color: Vec3,
    pub options: bool,
    pub thread_indices: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub nodes: Vec<SceneNode>,
    pub rooms: Vec<SceneRoom>,
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub time: f32,
    pub selected: usize,
    pub focused_room: usize,
    pub camera_focus: Vec2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoomTarget {
    Thread(usize),
    Options,
}

impl Scene {
    pub fn from_threads(
        threads: &[ThreadSummary],
        yaw: f32,
        pitch: f32,
        zoom: f32,
        time: f32,
        selected: usize,
    ) -> Self {
        Self::from_threads_with_options(threads, yaw, pitch, zoom, time, selected, 0, true)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_threads_with_options(
        threads: &[ThreadSummary],
        yaw: f32,
        pitch: f32,
        zoom: f32,
        time: f32,
        selected: usize,
        focused_room: usize,
        show_resting_agents: bool,
    ) -> Self {
        let mut projects = Vec::<(String, Vec<(usize, &ThreadSummary)>)>::new();
        for (index, thread) in threads.iter().enumerate() {
            let project = project_key(&thread.cwd);
            if let Some((_, project_threads)) = projects.iter_mut().find(|(key, _)| key == &project)
            {
                project_threads.push((index, thread));
            } else {
                projects.push((project, vec![(index, thread)]));
            }
        }
        projects.sort_by_key(|(_, project_threads)| {
            std::cmp::Reverse(
                project_threads
                    .iter()
                    .map(|(_, thread)| thread.updated_at)
                    .max()
                    .unwrap_or_default(),
            )
        });
        for (_, project_threads) in &mut projects {
            project_threads.sort_by_key(|(_, thread)| std::cmp::Reverse(thread.updated_at));
        }

        let visible = projects.into_iter().take(6).collect::<Vec<_>>();
        let room_layout = room_layout(visible.len());
        let mut rooms = Vec::new();
        let mut nodes = Vec::new();
        for (project_index, ((cwd, project_threads), layout)) in
            visible.into_iter().zip(room_layout).enumerate()
        {
            let color = PALETTE[stable_byte(&cwd, 1) as usize % PALETTE.len()];
            let thread_indices = project_threads
                .iter()
                .map(|(index, _)| *index)
                .collect::<Vec<_>>();
            rooms.push(SceneRoom {
                label: project_name(&cwd),
                center: layout.0,
                half_width: layout.1,
                half_height: layout.2,
                color,
                options: false,
                thread_indices,
            });

            let slots = agent_slots(project_threads.len().min(3));
            for ((thread_index, thread), slot) in project_threads
                .into_iter()
                .filter(|(_, thread)| show_resting_agents || is_operational(thread))
                .take(3)
                .zip(slots)
            {
                let active = matches!(
                    thread.status,
                    ThreadStatus::Active { .. } | ThreadStatus::ObservedRunning
                );
                let attention = matches!(
                    thread.status,
                    ThreadStatus::NeedsAttention | ThreadStatus::SystemError
                );
                let status_color = if matches!(thread.status, ThreadStatus::SystemError) {
                    Vec3::new(1.0, 0.18, 0.28)
                } else if attention {
                    Vec3::new(1.0, 0.62, 0.16)
                } else if active {
                    Vec3::new(0.20, 0.96, 0.70)
                } else {
                    color
                };
                let label_source = thread
                    .agent_nickname
                    .as_deref()
                    .or(thread.name.as_deref())
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| project_name(&thread.cwd));
                let label = label_source.chars().take(18).collect();
                let state_label = match thread.status {
                    ThreadStatus::Active { .. } | ThreadStatus::ObservedRunning => "ACTIF",
                    ThreadStatus::ObservedOpen | ThreadStatus::Idle => "REPOS",
                    ThreadStatus::RecentlyActive => "RÉCENT",
                    ThreadStatus::NeedsAttention => "À VOIR",
                    ThreadStatus::SystemError => "ERREUR",
                    ThreadStatus::NotLoaded => "REPOS",
                }
                .to_owned();
                nodes.push(SceneNode {
                    thread_id: thread.id.clone(),
                    parent_thread_id: thread.parent_thread_id.clone(),
                    position: Vec3::new(
                        layout.0.x + slot.x * layout.1,
                        layout.0.y + slot.y * layout.2,
                        project_index as f32,
                    ),
                    radius: 0.035,
                    color: status_color,
                    label,
                    state_label,
                    thread_index,
                    active,
                    attention,
                });
            }
        }
        rooms.push(SceneRoom {
            label: "OPTIONS".to_owned(),
            center: Vec2::new(0.50, 0.84),
            half_width: 0.145,
            half_height: 0.095,
            color: Vec3::new(1.00, 0.62, 0.20),
            options: true,
            thread_indices: Vec::new(),
        });

        let focused_room = focused_room.min(rooms.len().saturating_sub(1));
        Self {
            nodes,
            rooms,
            yaw,
            pitch,
            zoom,
            time,
            selected,
            focused_room,
            camera_focus: Vec2::splat(0.5),
        }
    }

    pub fn render_rgba(&self, width: usize, height: usize) -> Vec<u8> {
        self.render_rgba_for_display(width, height, width, height)
    }

    pub fn render_rgba_for_display(
        &self,
        width: usize,
        height: usize,
        display_width: usize,
        display_height: usize,
    ) -> Vec<u8> {
        let display_scale = (display_width as f32 / width.max(1) as f32)
            .min(display_height as f32 / height.max(1) as f32)
            .max(1.0);
        let mut canvas = Canvas::with_background(width, height, display_scale);
        if width == 0 || height == 0 {
            return canvas.pixels;
        }

        self.draw_corridors(&mut canvas);
        for room in &self.rooms {
            self.draw_room(&mut canvas, room);
        }

        let mut nodes = self.nodes.iter().collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.position.y.total_cmp(&right.position.y));
        for node in nodes {
            self.draw_workstation(&mut canvas, node);
        }
        self.draw_connections(&mut canvas);
        if width >= 320 {
            self.draw_raster_labels(&mut canvas);
        }
        canvas.pixels
    }

    pub fn project_nodes(&self, width: f32, height: f32) -> Vec<(usize, Vec2, f32)> {
        self.nodes
            .iter()
            .map(|node| {
                (
                    node.thread_index,
                    {
                        let point = self.view_point(node.position.truncate());
                        Vec2::new(point.x * width, point.y * height)
                    },
                    node.radius * width,
                )
            })
            .collect()
    }

    pub fn room_at(&self, width: f32, height: f32, x: f32, y: f32) -> Option<usize> {
        self.rooms
            .iter()
            .enumerate()
            .filter_map(|(index, room)| {
                let center = self.view_point(room.center) * Vec2::new(width, height);
                let distance = center.distance(Vec2::new(x, y));
                let radius = room.half_width * width * self.zoom * 0.9;
                (distance <= radius.max(4.0)).then_some((index, distance))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index)
    }

    pub fn room_for_thread(&self, thread_index: usize) -> Option<usize> {
        self.rooms
            .iter()
            .position(|room| room.thread_indices.contains(&thread_index))
    }

    pub fn room_target(&self, room_index: usize) -> Option<RoomTarget> {
        let room = self.rooms.get(room_index)?;
        if room.options {
            Some(RoomTarget::Options)
        } else {
            room.thread_indices.first().copied().map(RoomTarget::Thread)
        }
    }

    pub fn next_room(&self, current: usize, direction: Vec2) -> usize {
        let Some(origin) = self.rooms.get(current).map(|room| room.center) else {
            return 0;
        };
        let direction = direction.normalize_or_zero();
        self.rooms
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != current)
            .filter_map(|(index, room)| {
                let delta = room.center - origin;
                let forward = delta.dot(direction);
                if forward <= 0.025 {
                    return None;
                }
                let side = (delta - direction * forward).length();
                Some((index, forward + side * 2.2))
            })
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(index, _)| index)
            .unwrap_or(current)
    }

    fn draw_room(&self, canvas: &mut Canvas, room: &SceneRoom) {
        let center = canvas.point(self.view_point(room.center));
        let half_w = room.half_width * canvas.width as f32 * self.zoom.min(1.35);
        let half_h = room.half_height * canvas.height as f32 * self.zoom.min(1.35);
        let left = center + Vec2::new(-half_w, 0.0);
        let back = center + Vec2::new(0.0, -half_h);
        let right = center + Vec2::new(half_w, 0.0);
        let front = center + Vec2::new(0.0, half_h);
        let wall = (canvas.height as f32 * 0.075).max(canvas.logical(6.0));
        let floor = Vec3::new(0.025, 0.055, 0.105) + room.color * 0.075;

        if canvas.width >= 320 {
            canvas.sprite(
                if room.options {
                    options_room_sprite()
                } else {
                    project_room_sprite()
                },
                center + Vec2::Y * half_h * 1.08,
                half_w * 1.72,
            );
            canvas.glow_line(left, front, room.color, 2);
            canvas.glow_line(front, right, room.color, 2);
            if self
                .rooms
                .get(self.focused_room)
                .is_some_and(|focused| std::ptr::eq(focused, room))
            {
                canvas.glow_line(left, back, Vec3::new(1.0, 0.72, 0.22), 3);
                canvas.glow_line(back, right, Vec3::new(1.0, 0.72, 0.22), 3);
                canvas.glow_line(right, front, Vec3::new(1.0, 0.72, 0.22), 3);
                canvas.glow_line(front, left, Vec3::new(1.0, 0.72, 0.22), 3);
            }
            return;
        }

        canvas.polygon(&[left, back, right, front], floor);
        canvas.polygon(
            &[left, back, back - Vec2::Y * wall, left - Vec2::Y * wall],
            floor * 0.74 + room.color * 0.035,
        );
        canvas.polygon(
            &[back, right, right - Vec2::Y * wall, back - Vec2::Y * wall],
            floor * 0.62 + room.color * 0.025,
        );

        for step in 1..8 {
            let t = step as f32 / 8.0;
            canvas.line(left.lerp(back, t), front.lerp(right, t), room.color, 0.12);
            canvas.line(back.lerp(right, t), left.lerp(front, t), room.color, 0.12);
        }
        canvas.glow_line(left, front, room.color, 2);
        canvas.glow_line(front, right, room.color, 2);
        canvas.glow_line(left - Vec2::Y * wall, back - Vec2::Y * wall, room.color, 1);
        canvas.glow_line(back - Vec2::Y * wall, right - Vec2::Y * wall, room.color, 1);

        // Wall display and a small plant make each workspace read as an office.
        let display = back.lerp(right, 0.42) - Vec2::Y * wall * 0.52;
        canvas.rect(
            display - Vec2::new(half_w * 0.13, wall * 0.25),
            display + Vec2::new(half_w * 0.13, wall * 0.25),
            Vec3::new(0.01, 0.03, 0.055),
        );
        canvas.glow_rect(display, half_w * 0.11, wall * 0.18, room.color);
        for line in 0..3 {
            let y = display.y - wall * 0.10 + line as f32 * wall * 0.10;
            canvas.line(
                Vec2::new(display.x - half_w * 0.075, y),
                Vec2::new(display.x + half_w * (0.02 + line as f32 * 0.02), y),
                room.color,
                0.75,
            );
        }
        self.draw_plant(canvas, left.lerp(back, 0.22) - Vec2::Y * 2.0, room.color);
    }

    fn draw_workstation(&self, canvas: &mut Canvas, node: &SceneNode) {
        let center = canvas.point(self.view_point(node.position.truncate()));
        let scale = (canvas.width.min(canvas.height * 2) as f32 / 185.0)
            .clamp(canvas.logical(0.45), canvas.logical(4.5));
        let selected = node.thread_index == self.selected;
        let pulse = (self.time * 3.2).sin() * 0.5 + 0.5;

        if selected {
            canvas.glow_circle(
                center + Vec2::new(0.0, -18.0 * scale),
                5.5 * scale,
                Vec3::new(1.0, 0.72, 0.20),
            );
            let diamond = center + Vec2::new(0.0, -28.0 * scale - pulse * 2.0);
            canvas.polygon(
                &[
                    diamond + Vec2::new(0.0, -4.0 * scale),
                    diamond + Vec2::new(4.0 * scale, 0.0),
                    diamond + Vec2::new(0.0, 4.0 * scale),
                    diamond + Vec2::new(-4.0 * scale, 0.0),
                ],
                Vec3::new(1.0, 0.73, 0.24),
            );
        }

        if canvas.width >= 320 {
            let target_width = (canvas.width as f32
                * if self.rooms.len() > 4 { 0.095 } else { 0.17 })
            .clamp(canvas.logical(62.0), canvas.logical(230.0));
            if node.active {
                canvas.glow_circle(
                    center + Vec2::Y * target_width * 0.17,
                    target_width * (0.19 + pulse * 0.025),
                    node.color * 0.55,
                );
            }
            canvas.sprite_modulated(
                workstation_sprite(),
                center + Vec2::new(0.0, 13.0 * scale),
                target_width,
                if node.active || node.attention {
                    1.0
                } else {
                    0.52
                },
                if node.active || node.attention {
                    1.0
                } else {
                    0.22
                },
            );
            let beacon = center + Vec2::new(-target_width * 0.22, -target_width * 0.34);
            canvas.glow_circle(beacon, 3.0 * scale + pulse, node.color);
            canvas.circle(beacon, 1.5 * scale, node.color);
            if node.attention {
                let warning = center + Vec2::new(target_width * 0.34, -target_width * 0.42);
                canvas.glow_circle(warning, 5.0 * scale, node.color);
            } else if !node.active {
                canvas.label(
                    center + Vec2::new(target_width * 0.22, -target_width * 0.28),
                    "Z·Z",
                    Vec3::new(0.38, 0.46, 0.58),
                    (target_width * 0.10).max(canvas.logical(8.0)),
                    false,
                );
            }
            return;
        }

        // Desk as a small isometric cuboid.
        let desk = center + Vec2::new(5.0 * scale, 4.0 * scale);
        canvas.iso_box(
            desk,
            13.0 * scale,
            5.0 * scale,
            5.0 * scale,
            Vec3::new(0.34, 0.42, 0.51),
        );
        // Monitor and emissive code lines.
        let monitor = desk + Vec2::new(1.5 * scale, -10.0 * scale);
        canvas.rect(
            monitor - Vec2::new(6.0 * scale, 5.0 * scale),
            monitor + Vec2::new(6.0 * scale, 5.0 * scale),
            Vec3::new(0.01, 0.018, 0.028),
        );
        canvas.glow_rect(monitor, 5.2 * scale, 4.2 * scale, node.color);
        for line in 0..3 {
            let y = monitor.y - 2.6 * scale + line as f32 * 2.2 * scale;
            canvas.line(
                Vec2::new(monitor.x - 3.8 * scale, y),
                Vec2::new(monitor.x + (0.8 + line as f32) * scale, y),
                node.color,
                if node.active { 0.95 } else { 0.45 },
            );
        }

        // Operator: chair, body, head and arms facing the monitor.
        canvas.iso_box(
            center + Vec2::new(-5.0 * scale, 8.0 * scale),
            6.0 * scale,
            3.0 * scale,
            4.0 * scale,
            Vec3::new(0.025, 0.04, 0.065),
        );
        canvas.circle(
            center + Vec2::new(-4.0 * scale, -8.0 * scale),
            4.0 * scale,
            Vec3::new(0.92, 0.57, 0.36),
        );
        canvas.polygon(
            &[
                center + Vec2::new(-8.0, -4.0) * scale,
                center + Vec2::new(-1.0, -5.0) * scale,
                center + Vec2::new(1.0, 6.0) * scale,
                center + Vec2::new(-7.0, 6.0) * scale,
            ],
            node.color * 0.82 + Vec3::splat(0.12),
        );
        canvas.line(
            center + Vec2::new(-1.0, -1.0) * scale,
            center + Vec2::new(5.0, 2.0) * scale,
            Vec3::new(0.92, 0.57, 0.36),
            0.92,
        );
        if node.active {
            canvas.glow_circle(monitor, (8.0 + pulse * 2.0) * scale, node.color * 0.7);
        }
        if node.attention {
            canvas.glow_circle(
                center + Vec2::new(-4.0 * scale, -18.0 * scale),
                3.2 * scale,
                node.color,
            );
        }
    }

    fn draw_plant(&self, canvas: &mut Canvas, center: Vec2, color: Vec3) {
        let scale = (canvas.width.min(canvas.height * 2) as f32 / 220.0)
            .clamp(canvas.logical(0.4), canvas.logical(3.0));
        canvas.iso_box(
            center,
            5.0 * scale,
            2.5 * scale,
            5.0 * scale,
            Vec3::new(0.34, 0.16, 0.07),
        );
        canvas.line(
            center - Vec2::Y * 4.0 * scale,
            center - Vec2::Y * 13.0 * scale,
            Vec3::new(0.18, 0.55, 0.29),
            0.9,
        );
        for direction in [-1.0_f32, 1.0] {
            canvas.glow_circle(
                center + Vec2::new(direction * 4.0, -12.0) * scale,
                3.2 * scale,
                color * 0.55 + Vec3::new(0.05, 0.28, 0.08),
            );
        }
    }

    fn draw_corridors(&self, canvas: &mut Canvas) {
        if self.rooms.len() < 2 {
            return;
        }
        let mut connected = vec![0usize];
        while connected.len() < self.rooms.len() {
            let mut edge: Option<(usize, usize, f32)> = None;
            for &from in &connected {
                for to in 0..self.rooms.len() {
                    if connected.contains(&to) {
                        continue;
                    }
                    let distance = self.rooms[from].center.distance(self.rooms[to].center);
                    if edge.is_none_or(|best| distance < best.2) {
                        edge = Some((from, to, distance));
                    }
                }
            }
            let Some((from, to, _)) = edge else { break };
            let a = canvas.point(self.view_point(self.rooms[from].center));
            let b = canvas.point(self.view_point(self.rooms[to].center));
            let delta = (b - a).normalize_or_zero();
            let normal = Vec2::new(-delta.y, delta.x);
            let width =
                (canvas.width as f32 * 0.014).clamp(canvas.logical(5.0), canvas.logical(18.0));
            let color = Vec3::new(0.035, 0.11, 0.17);
            canvas.polygon(
                &[
                    a + normal * width,
                    b + normal * width,
                    b - normal * width,
                    a - normal * width,
                ],
                color,
            );
            canvas.glow_line(
                a + normal * width,
                b + normal * width,
                Vec3::new(0.08, 0.75, 0.82),
                1,
            );
            canvas.glow_line(
                a - normal * width,
                b - normal * width,
                Vec3::new(0.08, 0.75, 0.82),
                1,
            );
            connected.push(to);
        }
    }

    fn draw_connections(&self, canvas: &mut Canvas) {
        for node in &self.nodes {
            if !node.active && !node.attention && node.thread_index != self.selected {
                continue;
            }
            let Some(parent_id) = node.parent_thread_id.as_deref() else {
                continue;
            };
            let Some(parent) = self
                .nodes
                .iter()
                .find(|candidate| candidate.thread_id == parent_id)
            else {
                continue;
            };
            canvas.glow_line(
                canvas.point(self.view_point(parent.position.truncate())),
                canvas.point(self.view_point(node.position.truncate())),
                node.color,
                1,
            );
        }
    }

    fn draw_raster_labels(&self, canvas: &mut Canvas) {
        let room_size =
            (canvas.width as f32 / 62.0).clamp(canvas.logical(13.0), canvas.logical(22.0));
        let agent_size =
            (canvas.width as f32 / 78.0).clamp(canvas.logical(11.0), canvas.logical(17.0));
        for room in &self.rooms {
            let center = canvas.point(self.view_point(room.center));
            let y =
                center.y - room.half_height * self.zoom * canvas.height as f32 - room_size * 3.2;
            canvas.label(
                Vec2::new(center.x, y.max(canvas.logical(8.0))),
                &room.label,
                room.color,
                room_size,
                false,
            );
        }
        for node in &self.nodes {
            if !node.active && !node.attention && node.thread_index != self.selected {
                continue;
            }
            let center = canvas.point(self.view_point(node.position.truncate()));
            let text = format!("{}  ·  {}", node.label, node.state_label);
            canvas.label(
                Vec2::new(center.x, center.y - canvas.width as f32 * 0.075),
                &text,
                node.color,
                agent_size,
                node.thread_index == self.selected,
            );
        }
    }

    fn view_point(&self, point: Vec2) -> Vec2 {
        let center = Vec2::splat(0.5);
        center
            + (point - self.camera_focus) * self.zoom
            + Vec2::new((self.yaw - 0.35) * 0.055, (self.pitch - 0.22) * 0.08)
    }
}

pub struct UnicodeScene<'a> {
    pub scene: &'a Scene,
}

impl Widget for UnicodeScene<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let width = area.width as usize;
        let height = area.height as usize * 2;
        let pixels = self.scene.render_rgba(width, height);
        for cell_y in 0..area.height as usize {
            for cell_x in 0..width {
                let top = pixel(&pixels, width, cell_x, cell_y * 2);
                let bottom = pixel(&pixels, width, cell_x, cell_y * 2 + 1);
                let cell = &mut buffer[(area.x + cell_x as u16, area.y + cell_y as u16)];
                cell.set_char('▀');
                cell.set_fg(Color::Rgb(top[0], top[1], top[2]));
                cell.set_bg(Color::Rgb(bottom[0], bottom[1], bottom[2]));
            }
        }

        for room in &self.scene.rooms {
            let center = self.scene.view_point(room.center);
            let x = area.x + (center.x * area.width as f32).max(0.0) as u16;
            let y = area.y
                + ((center.y - room.half_height * self.scene.zoom - 0.07) * area.height as f32)
                    .max(0.0) as u16;
            let label = format!(" {} ", room.label);
            if x < area.right() && y < area.bottom() {
                buffer.set_string(
                    x.saturating_sub((label.len() / 2) as u16),
                    y,
                    label,
                    Style::new()
                        .fg(Color::White)
                        .bg(Color::Rgb(4, 12, 27))
                        .bold(),
                );
            }
        }
        for node in &self.scene.nodes {
            if !node.active && !node.attention && node.thread_index != self.scene.selected {
                continue;
            }
            let normalized = self.scene.view_point(node.position.truncate());
            let point = Vec2::new(normalized.x * width as f32, normalized.y * height as f32);
            let x = area.x.saturating_add(point.x.max(0.0) as u16);
            let y = area.y.saturating_add((point.y.max(0.0) / 2.0) as u16);
            if x < area.right() && y < area.bottom() {
                let marker = if node.thread_index == self.scene.selected {
                    "◆"
                } else {
                    "●"
                };
                let state = if node.active {
                    "ACTIF"
                } else {
                    &node.state_label
                };
                let label = format!(" {marker} {} · {state} ", node.label);
                buffer.set_string(
                    x.saturating_sub(2),
                    y.saturating_sub(3),
                    label,
                    Style::new()
                        .fg(to_color(node.color))
                        .bg(Color::Rgb(4, 12, 27))
                        .bold(),
                );
            }
        }
    }
}

struct Canvas {
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    display_scale: f32,
}

type BackgroundFrame = (usize, usize, Vec<u8>);

impl Canvas {
    fn new(width: usize, height: usize, display_scale: f32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height * 4],
            display_scale,
        }
    }

    fn with_background(width: usize, height: usize, display_scale: f32) -> Self {
        static BACKGROUNDS: OnceLock<Mutex<Vec<BackgroundFrame>>> = OnceLock::new();
        if width == 0 || height == 0 {
            return Self::new(width, height, display_scale);
        }
        let cache = BACKGROUNDS.get_or_init(|| Mutex::new(Vec::new()));
        if let Ok(cached) = cache.lock()
            && let Some((_, _, pixels)) = cached.iter().find(|(cached_width, cached_height, _)| {
                (*cached_width, *cached_height) == (width, height)
            })
        {
            return Self {
                width,
                height,
                pixels: pixels.clone(),
                display_scale,
            };
        }
        let mut canvas = Self::new(width, height, display_scale);
        canvas.background();
        canvas.technical_grid();
        if let Ok(mut cached) = cache.lock() {
            if cached.len() >= 4 {
                cached.remove(0);
            }
            cached.push((width, height, canvas.pixels.clone()));
        }
        canvas
    }

    fn logical(&self, value: f32) -> f32 {
        value / self.display_scale
    }

    fn point(&self, point: Vec2) -> Vec2 {
        Vec2::new(point.x * self.width as f32, point.y * self.height as f32)
    }

    fn background(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                let nx = x as f32 / self.width.max(1) as f32;
                let ny = y as f32 / self.height.max(1) as f32;
                let vignette =
                    (1.0 - ((nx - 0.5).powi(2) + (ny - 0.45).powi(2)) * 1.2).clamp(0.25, 1.0);
                self.set(
                    x as isize,
                    y as isize,
                    Vec3::new(0.008, 0.018, 0.042) * vignette,
                    1.0,
                );
            }
        }
    }

    fn technical_grid(&mut self) {
        let spacing = (self.width as f32 / 24.0).max(7.0);
        let color = Vec3::new(0.08, 0.18, 0.30);
        let diagonal = self.height as f32 * 0.58;
        let count = (self.width as f32 / spacing) as isize + 16;
        for index in -8..count {
            let x = index as f32 * spacing;
            self.line(
                Vec2::new(x, 0.0),
                Vec2::new(x + diagonal, self.height as f32),
                color,
                0.18,
            );
            self.line(
                Vec2::new(x, 0.0),
                Vec2::new(x - diagonal, self.height as f32),
                color,
                0.18,
            );
        }
    }

    fn set(&mut self, x: isize, y: isize, color: Vec3, alpha: f32) {
        if x < 0 || y < 0 || x >= self.width as isize || y >= self.height as isize {
            return;
        }
        let offset = (y as usize * self.width + x as usize) * 4;
        let color = color.clamp(Vec3::ZERO, Vec3::ONE);
        if alpha >= 0.999 {
            self.pixels[offset] = (color.x * 255.0) as u8;
            self.pixels[offset + 1] = (color.y * 255.0) as u8;
            self.pixels[offset + 2] = (color.z * 255.0) as u8;
            self.pixels[offset + 3] = 255;
            return;
        }
        let existing = Vec3::new(
            self.pixels[offset] as f32 / 255.0,
            self.pixels[offset + 1] as f32 / 255.0,
            self.pixels[offset + 2] as f32 / 255.0,
        );
        let blended = existing.lerp(color, alpha.clamp(0.0, 1.0));
        self.pixels[offset] = (blended.x * 255.0) as u8;
        self.pixels[offset + 1] = (blended.y * 255.0) as u8;
        self.pixels[offset + 2] = (blended.z * 255.0) as u8;
        self.pixels[offset + 3] = 255;
    }

    fn line(&mut self, from: Vec2, to: Vec2, color: Vec3, alpha: f32) {
        let delta = to - from;
        let steps = delta.x.abs().max(delta.y.abs()).ceil().max(1.0) as usize;
        for step in 0..=steps {
            let point = from + delta * (step as f32 / steps as f32);
            self.set(
                point.x.round() as isize,
                point.y.round() as isize,
                color,
                alpha,
            );
        }
    }

    fn glow_line(&mut self, from: Vec2, to: Vec2, color: Vec3, radius: isize) {
        for offset in -radius..=radius {
            let alpha = if offset == 0 { 0.9 } else { 0.18 };
            self.line(
                from + Vec2::Y * offset as f32,
                to + Vec2::Y * offset as f32,
                color,
                alpha,
            );
        }
    }

    fn rect(&mut self, min: Vec2, max: Vec2, color: Vec3) {
        let x0 = min.x.floor() as isize;
        let x1 = max.x.ceil() as isize;
        let y0 = min.y.floor() as isize;
        let y1 = max.y.ceil() as isize;
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, color, 1.0);
            }
        }
    }

    fn polygon_outline(&mut self, points: &[Vec2], color: Vec3) {
        for index in 0..points.len() {
            self.glow_line(points[index], points[(index + 1) % points.len()], color, 1);
        }
    }

    fn label(&mut self, top_center: Vec2, text: &str, color: Vec3, size: f32, selected: bool) {
        let font = ui_font();
        let mut layout = Layout::new(CoordinateSystem::PositiveYDown);
        layout.reset(&LayoutSettings::default());
        layout.append(&[font], &TextStyle::new(text, size, 0));
        let width = layout
            .glyphs()
            .iter()
            .map(|glyph| glyph.x + glyph.width as f32)
            .fold(0.0_f32, f32::max)
            .ceil();
        let height = (size * 1.45).ceil();
        let padding_x = (size * 0.72).ceil();
        let cut = (size * 0.42).ceil();
        let left = top_center.x - width * 0.5 - padding_x;
        let right = top_center.x + width * 0.5 + padding_x;
        let top = top_center.y;
        let bottom = top + height;
        let panel = [
            Vec2::new(left + cut, top),
            Vec2::new(right - cut, top),
            Vec2::new(right, top + cut),
            Vec2::new(right, bottom - cut),
            Vec2::new(right - cut, bottom),
            Vec2::new(left + cut, bottom),
            Vec2::new(left, bottom - cut),
            Vec2::new(left, top + cut),
        ];
        self.polygon(&panel, Vec3::new(0.006, 0.018, 0.042));
        self.polygon_outline(
            &panel,
            if selected {
                Vec3::new(1.0, 0.72, 0.22)
            } else {
                color * 0.75
            },
        );

        let text_x = top_center.x - width * 0.5;
        let text_y = top + (height - size) * 0.28;
        layout.reset(&LayoutSettings {
            x: text_x,
            y: text_y,
            ..LayoutSettings::default()
        });
        layout.append(&[font], &TextStyle::new(text, size, 0));
        for glyph in layout.glyphs() {
            let (_, bitmap) = font.rasterize_config(glyph.key);
            for y in 0..glyph.height {
                for x in 0..glyph.width {
                    let alpha = bitmap[y * glyph.width + x] as f32 / 255.0;
                    if alpha > 0.01 {
                        self.set(
                            glyph.x.round() as isize + x as isize,
                            glyph.y.round() as isize + y as isize,
                            Vec3::new(0.90, 0.96, 1.0),
                            alpha,
                        );
                    }
                }
            }
        }
    }

    fn glow_rect(&mut self, center: Vec2, half_w: f32, half_h: f32, color: Vec3) {
        let min = center - Vec2::new(half_w, half_h);
        let max = center + Vec2::new(half_w, half_h);
        for offset in 0..=2 {
            let o = offset as f32;
            let alpha = if offset == 0 { 0.95 } else { 0.18 };
            self.line(
                Vec2::new(min.x - o, min.y - o),
                Vec2::new(max.x + o, min.y - o),
                color,
                alpha,
            );
            self.line(
                Vec2::new(max.x + o, min.y - o),
                Vec2::new(max.x + o, max.y + o),
                color,
                alpha,
            );
            self.line(
                Vec2::new(max.x + o, max.y + o),
                Vec2::new(min.x - o, max.y + o),
                color,
                alpha,
            );
            self.line(
                Vec2::new(min.x - o, max.y + o),
                Vec2::new(min.x - o, min.y - o),
                color,
                alpha,
            );
        }
    }

    fn circle(&mut self, center: Vec2, radius: f32, color: Vec3) {
        let radius_sq = radius * radius;
        for y in (center.y - radius).floor() as isize..=(center.y + radius).ceil() as isize {
            for x in (center.x - radius).floor() as isize..=(center.x + radius).ceil() as isize {
                let delta = Vec2::new(x as f32, y as f32) - center;
                if delta.length_squared() <= radius_sq {
                    self.set(x, y, color, 1.0);
                }
            }
        }
    }

    fn glow_circle(&mut self, center: Vec2, radius: f32, color: Vec3) {
        let outer = radius.max(1.0) * 1.8;
        for y in (center.y - outer).floor() as isize..=(center.y + outer).ceil() as isize {
            for x in (center.x - outer).floor() as isize..=(center.x + outer).ceil() as isize {
                let distance = (Vec2::new(x as f32, y as f32) - center).length();
                if distance <= outer {
                    let alpha = ((1.0 - distance / outer) * 0.36).max(0.0);
                    self.set(x, y, color, alpha);
                }
            }
        }
    }

    fn polygon(&mut self, points: &[Vec2], color: Vec3) {
        if points.len() < 3 {
            return;
        }
        for index in 1..points.len() - 1 {
            self.triangle(points[0], points[index], points[index + 1], color);
        }
    }

    fn triangle(&mut self, a: Vec2, b: Vec2, c: Vec2, color: Vec3) {
        let min_x = a.x.min(b.x).min(c.x).floor() as isize;
        let max_x = a.x.max(b.x).max(c.x).ceil() as isize;
        let min_y = a.y.min(b.y).min(c.y).floor() as isize;
        let max_y = a.y.max(b.y).max(c.y).ceil() as isize;
        let area = edge(a, b, c);
        if area.abs() < f32::EPSILON {
            return;
        }
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let point = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w0 = edge(b, c, point);
                let w1 = edge(c, a, point);
                let w2 = edge(a, b, point);
                if (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0) {
                    self.set(x, y, color, 1.0);
                }
            }
        }
    }

    fn iso_box(&mut self, center: Vec2, half_w: f32, half_h: f32, depth: f32, color: Vec3) {
        let left = center + Vec2::new(-half_w, 0.0);
        let back = center + Vec2::new(0.0, -half_h);
        let right = center + Vec2::new(half_w, 0.0);
        let front = center + Vec2::new(0.0, half_h);
        self.polygon(&[left, back, right, front], color + Vec3::splat(0.08));
        self.polygon(
            &[left, front, front + Vec2::Y * depth, left + Vec2::Y * depth],
            color * 0.68,
        );
        self.polygon(
            &[
                front,
                right,
                right + Vec2::Y * depth,
                front + Vec2::Y * depth,
            ],
            color * 0.48,
        );
    }

    fn sprite(&mut self, sprite: &Sprite, ground: Vec2, target_width: f32) {
        self.sprite_modulated(sprite, ground, target_width, 1.0, 1.0);
    }

    fn sprite_modulated(
        &mut self,
        sprite: &Sprite,
        ground: Vec2,
        target_width: f32,
        brightness: f32,
        saturation: f32,
    ) {
        let target_width = target_width.max(1.0) as usize;
        let target_height =
            ((target_width as f32 * sprite.height as f32 / sprite.width as f32).max(1.0)) as usize;
        let left = ground.x.round() as isize - target_width as isize / 2;
        let top = ground.y.round() as isize - (target_height as f32 * 0.88) as isize;
        for y in 0..target_height {
            let source_y = y * sprite.height / target_height;
            for x in 0..target_width {
                let source_x = x * sprite.width / target_width;
                let offset = (source_y * sprite.width + source_x) * 4;
                let alpha = sprite.rgba[offset + 3] as f32 / 255.0;
                if alpha <= 0.01 {
                    continue;
                }
                let color = Vec3::new(
                    sprite.rgba[offset] as f32 / 255.0,
                    sprite.rgba[offset + 1] as f32 / 255.0,
                    sprite.rgba[offset + 2] as f32 / 255.0,
                );
                let luma = color.dot(Vec3::new(0.2126, 0.7152, 0.0722));
                self.set(
                    left + x as isize,
                    top + y as isize,
                    Vec3::splat(luma).lerp(color, saturation.clamp(0.0, 1.0)) * brightness,
                    alpha,
                );
            }
        }
    }
}

struct Sprite {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

fn workstation_sprite() -> &'static Sprite {
    static SPRITE: OnceLock<Sprite> = OnceLock::new();
    SPRITE.get_or_init(|| {
        decode_sprite(
            include_bytes!("../assets/generated/workstation-operator.png"),
            "workstation",
        )
    })
}

fn project_room_sprite() -> &'static Sprite {
    static SPRITE: OnceLock<Sprite> = OnceLock::new();
    SPRITE.get_or_init(|| {
        decode_sprite(
            include_bytes!("../assets/generated/project-room.png"),
            "project room",
        )
    })
}

fn options_room_sprite() -> &'static Sprite {
    static SPRITE: OnceLock<Sprite> = OnceLock::new();
    SPRITE.get_or_init(|| {
        decode_sprite(
            include_bytes!("../assets/generated/options-room-v2.png"),
            "options room",
        )
    })
}

fn decode_sprite(bytes: &[u8], name: &str) -> Sprite {
    let image = image::load_from_memory(bytes)
        .unwrap_or_else(|_| panic!("embedded {name} sprite must be a valid PNG"))
        .to_rgba8();
    let (width, height) = image.dimensions();
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0;
    let mut max_y = 0;
    for (x, y, pixel) in image.enumerate_pixels() {
        if pixel[3] > 8 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    let cropped = image::imageops::crop_imm(
        &image,
        min_x,
        min_y,
        max_x.saturating_sub(min_x) + 1,
        max_y.saturating_sub(min_y) + 1,
    )
    .to_image();
    Sprite {
        width: cropped.width() as usize,
        height: cropped.height() as usize,
        rgba: cropped.into_raw(),
    }
}

fn ui_font() -> &'static fontdue::Font {
    static FONT: OnceLock<fontdue::Font> = OnceLock::new();
    FONT.get_or_init(|| {
        fontdue::Font::from_bytes(
            include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .expect("embedded UI font must be valid")
    })
}

fn edge(a: Vec2, b: Vec2, point: Vec2) -> f32 {
    (point.x - a.x) * (b.y - a.y) - (point.y - a.y) * (b.x - a.x)
}

fn room_layout(count: usize) -> Vec<(Vec2, f32, f32)> {
    match count {
        0 => Vec::new(),
        1 => vec![(Vec2::new(0.50, 0.30), 0.20, 0.14)],
        2 => vec![
            (Vec2::new(0.32, 0.30), 0.18, 0.12),
            (Vec2::new(0.68, 0.30), 0.18, 0.12),
        ],
        3 => vec![
            (Vec2::new(0.20, 0.28), 0.15, 0.10),
            (Vec2::new(0.50, 0.28), 0.15, 0.10),
            (Vec2::new(0.80, 0.28), 0.15, 0.10),
        ],
        4 => [
            Vec2::new(0.31, 0.24),
            Vec2::new(0.69, 0.24),
            Vec2::new(0.31, 0.53),
            Vec2::new(0.69, 0.53),
        ]
        .into_iter()
        .map(|center| (center, 0.17, 0.105))
        .collect(),
        5 => [
            Vec2::new(0.20, 0.22),
            Vec2::new(0.50, 0.22),
            Vec2::new(0.80, 0.22),
            Vec2::new(0.34, 0.52),
            Vec2::new(0.66, 0.52),
        ]
        .into_iter()
        .map(|center| (center, 0.145, 0.095))
        .collect(),
        _ => [
            Vec2::new(0.19, 0.20),
            Vec2::new(0.50, 0.20),
            Vec2::new(0.81, 0.20),
            Vec2::new(0.19, 0.50),
            Vec2::new(0.50, 0.50),
            Vec2::new(0.81, 0.50),
        ]
        .into_iter()
        .take(count.min(6))
        .map(|center| (center, 0.14, 0.09))
        .collect(),
    }
}

fn is_operational(thread: &ThreadSummary) -> bool {
    matches!(
        thread.status,
        ThreadStatus::Active { .. }
            | ThreadStatus::ObservedRunning
            | ThreadStatus::NeedsAttention
            | ThreadStatus::SystemError
    )
}

fn agent_slots(count: usize) -> Vec<Vec2> {
    match count {
        0 => Vec::new(),
        1 => vec![Vec2::new(0.0, 0.10)],
        2 => vec![Vec2::new(-0.38, -0.05), Vec2::new(0.38, 0.05)],
        3 => vec![
            Vec2::new(-0.42, -0.18),
            Vec2::new(0.38, -0.02),
            Vec2::new(0.0, 0.45),
        ],
        _ => vec![
            Vec2::new(-0.42, -0.20),
            Vec2::new(0.40, -0.12),
            Vec2::new(-0.35, 0.44),
            Vec2::new(0.40, 0.40),
        ],
    }
}

fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 3] {
    let offset = (y * width + x) * 4;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

fn stable_byte(value: &str, index: usize) -> u8 {
    blake3::hash(value.as_bytes()).as_bytes()[index % 32]
}

fn to_color(value: Vec3) -> Color {
    Color::Rgb(
        (value.x.clamp(0.0, 1.0) * 255.0) as u8,
        (value.y.clamp(0.0, 1.0) * 255.0) as u8,
        (value.z.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

pub fn project_key(cwd: &str) -> String {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    if let Some(cached) = cache.lock().expect("project cache").get(cwd).cloned() {
        return cached;
    }
    let path = std::path::Path::new(cwd);
    let key = path
        .ancestors()
        .find(|candidate| candidate.join(".git").exists())
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned();
    cache
        .lock()
        .expect("project cache")
        .insert(cwd.to_owned(), key.clone());
    key
}

pub fn project_name(cwd: &str) -> String {
    let key = project_key(cwd);
    std::path::Path::new(&key)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(&key)
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scene_renders_opaque_frame() {
        let scene = Scene {
            nodes: Vec::new(),
            rooms: Vec::new(),
            yaw: 0.35,
            pitch: 0.22,
            zoom: 1.0,
            time: 0.0,
            selected: 0,
            focused_room: 0,
            camera_focus: Vec2::splat(0.5),
        };
        let pixels = scene.render_rgba(32, 20);
        assert_eq!(pixels.len(), 32 * 20 * 4);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn spatial_navigation_reaches_the_options_room() {
        let scene = Scene::from_threads(&[], 0.35, 0.22, 1.0, 0.0, 0);
        assert_eq!(scene.room_target(0), Some(RoomTarget::Options));

        let mut scene = scene;
        scene.rooms.insert(
            0,
            SceneRoom {
                label: "PROJECT".to_owned(),
                center: Vec2::new(0.5, 0.25),
                half_width: 0.15,
                half_height: 0.10,
                color: PALETTE[0],
                options: false,
                thread_indices: vec![3],
            },
        );
        assert_eq!(scene.next_room(0, Vec2::Y), 1);
        assert_eq!(scene.room_target(1), Some(RoomTarget::Options));

        scene.camera_focus = scene.rooms[1].center;
        assert!(
            scene
                .view_point(scene.rooms[1].center)
                .distance(Vec2::splat(0.5))
                < 0.001
        );
    }
}
