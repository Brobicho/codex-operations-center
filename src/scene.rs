use std::f32::consts::TAU;

use glam::{Mat3, Vec2, Vec3};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

use crate::codex::{ThreadStatus, ThreadSummary};

#[derive(Clone, Debug)]
pub struct SceneNode {
    pub thread_id: String,
    pub parent_thread_id: Option<String>,
    pub position: Vec3,
    pub radius: f32,
    pub color: Vec3,
    pub label: String,
    pub thread_index: usize,
}

#[derive(Clone, Debug)]
pub struct Scene {
    pub nodes: Vec<SceneNode>,
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub time: f32,
}

impl Scene {
    pub fn from_threads(
        threads: &[ThreadSummary],
        yaw: f32,
        pitch: f32,
        zoom: f32,
        time: f32,
    ) -> Self {
        let visible = threads.iter().take(18).collect::<Vec<_>>();
        let count = visible.len().max(1) as f32;
        let nodes = visible
            .into_iter()
            .enumerate()
            .map(|(index, thread)| {
                let angle = index as f32 / count * TAU + time * 0.025;
                let ring = 2.6 + (index % 3) as f32 * 0.75;
                let height = 0.55 + ((stable_byte(&thread.id, 0) as f32 / 255.0) * 1.8);
                let active = matches!(
                    thread.status,
                    ThreadStatus::Active { .. } | ThreadStatus::RecentlyActive
                );
                let error = matches!(thread.status, ThreadStatus::SystemError);
                let attention = matches!(thread.status, ThreadStatus::NeedsAttention);
                let project_color = project_color(&thread.cwd);
                let color = if error {
                    Vec3::new(1.0, 0.18, 0.24)
                } else if attention {
                    Vec3::new(1.0, 0.58, 0.14)
                } else if active {
                    Vec3::new(0.12, 0.95, 0.85)
                } else {
                    project_color
                };
                SceneNode {
                    thread_id: thread.id.clone(),
                    parent_thread_id: thread.parent_thread_id.clone(),
                    position: Vec3::new(angle.cos() * ring, height, angle.sin() * ring),
                    radius: if thread.parent_thread_id.is_some() {
                        0.20
                    } else {
                        0.28
                    } + if active {
                        (time * 2.8).sin().abs() * 0.035
                    } else {
                        0.0
                    },
                    color,
                    label: project_name(&thread.cwd),
                    thread_index: index,
                }
            })
            .collect();
        Self {
            nodes,
            yaw,
            pitch,
            zoom,
            time,
        }
    }

    pub fn render_rgba(&self, width: usize, height: usize) -> Vec<u8> {
        let mut output = vec![0_u8; width * height * 4];
        if width == 0 || height == 0 {
            return output;
        }
        let camera = Camera::new(self.yaw, self.pitch, self.zoom, width, height);
        for y in 0..height {
            for x in 0..width {
                let ray = camera.ray(x, y);
                let color = self.trace(camera.position, ray, x, y, width, height);
                let offset = (y * width + x) * 4;
                output[offset] = linear_to_u8(color.x);
                output[offset + 1] = linear_to_u8(color.y);
                output[offset + 2] = linear_to_u8(color.z);
                output[offset + 3] = 255;
            }
        }
        self.draw_connections(&mut output, width, height, &camera);
        output
    }

    pub fn project_nodes(&self, width: f32, height: f32) -> Vec<(usize, Vec2, f32)> {
        let camera = Camera::new(
            self.yaw,
            self.pitch,
            self.zoom,
            width as usize,
            height as usize,
        );
        self.nodes
            .iter()
            .filter_map(|node| {
                camera
                    .project(node.position)
                    .map(|(point, depth)| (node.thread_index, point, node.radius / depth * width))
            })
            .collect()
    }

    fn trace(
        &self,
        origin: Vec3,
        ray: Vec3,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> Vec3 {
        let mut color = background(x, y, width, height, self.time);
        let mut closest = f32::INFINITY;

        if ray.y < -0.001 {
            let distance = -origin.y / ray.y;
            if distance > 0.0 {
                let point = origin + ray * distance;
                let grid_x = grid_line(point.x);
                let grid_z = grid_line(point.z);
                let fade = (1.0 - distance / 18.0).clamp(0.0, 1.0);
                let grid = grid_x.max(grid_z) * fade;
                let horizon = Vec3::new(0.015, 0.045, 0.09) * fade;
                color = color.max(horizon + Vec3::new(0.02, 0.25, 0.34) * grid);
                closest = distance;
            }
        }

        for node in &self.nodes {
            if let Some(distance) = sphere_hit(origin, ray, node.position, node.radius)
                && distance < closest
            {
                closest = distance;
                let point = origin + ray * distance;
                let normal = (point - node.position).normalize();
                let light = Vec3::new(-0.45, 0.85, -0.3).normalize();
                let diffuse = normal.dot(light).max(0.0);
                let rim = (1.0 - normal.dot(-ray).max(0.0)).powf(2.2);
                color =
                    node.color * (0.22 + diffuse * 0.78) + Vec3::new(0.4, 0.9, 1.0) * rim * 0.42;
            }

            let distance_to_ray = distance_to_ray(origin, ray, node.position);
            if distance_to_ray < node.radius * 2.8 {
                let glow = (1.0 - distance_to_ray / (node.radius * 2.8)).powf(3.0);
                color += node.color * glow * 0.12;
            }
        }
        color.clamp(Vec3::ZERO, Vec3::ONE)
    }

    fn draw_connections(&self, output: &mut [u8], width: usize, height: usize, camera: &Camera) {
        for node in &self.nodes {
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
            let (Some((from, _)), Some((to, _))) = (
                camera.project(parent.position),
                camera.project(node.position),
            ) else {
                continue;
            };
            draw_glow_line(output, width, height, from, to, node.color);
        }
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

        for (index, point, _) in self.scene.project_nodes(width as f32, height as f32) {
            let Some(node) = self
                .scene
                .nodes
                .iter()
                .find(|node| node.thread_index == index)
            else {
                continue;
            };
            let x = area.x.saturating_add(point.x.max(0.0) as u16);
            let y = area.y.saturating_add((point.y.max(0.0) / 2.0) as u16);
            if x < area.right() && y < area.bottom() {
                let label = format!(" {} ", node.label);
                buffer.set_string(x, y, label, ratatui::style::Style::new().fg(Color::White));
            }
        }
    }
}

fn background(x: usize, y: usize, width: usize, height: usize, time: f32) -> Vec3 {
    let nx = x as f32 / width.max(1) as f32;
    let ny = y as f32 / height.max(1) as f32;
    let vignette = (1.0 - ((nx - 0.5).powi(2) + (ny - 0.48).powi(2)) * 1.5).max(0.0);
    let star_seed = ((x as u64 * 73856093) ^ (y as u64 * 19349663)) % 997;
    let star = if star_seed < 2 {
        0.35 + (time * 1.7 + star_seed as f32).sin().abs() * 0.45
    } else {
        0.0
    };
    Vec3::new(0.005, 0.012, 0.035) * vignette
        + Vec3::new(0.04, 0.18, 0.28) * (1.0 - ny).powf(5.0) * 0.18
        + Vec3::splat(star)
}

fn sphere_hit(origin: Vec3, ray: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin - center;
    let half_b = oc.dot(ray);
    let c = oc.length_squared() - radius * radius;
    let discriminant = half_b * half_b - c;
    if discriminant < 0.0 {
        return None;
    }
    let near = -half_b - discriminant.sqrt();
    (near > 0.0).then_some(near)
}

fn distance_to_ray(origin: Vec3, ray: Vec3, point: Vec3) -> f32 {
    let along = (point - origin).dot(ray).max(0.0);
    (point - (origin + ray * along)).length()
}

fn grid_line(value: f32) -> f32 {
    let distance = (value - value.round()).abs();
    (1.0 - distance / 0.035).clamp(0.0, 1.0)
}

fn linear_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8
}

fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 3] {
    let offset = (y * width + x) * 4;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

fn stable_byte(value: &str, index: usize) -> u8 {
    blake3::hash(value.as_bytes()).as_bytes()[index % 32]
}

fn project_color(cwd: &str) -> Vec3 {
    const PALETTE: [Vec3; 6] = [
        Vec3::new(0.10, 0.72, 1.00),
        Vec3::new(0.38, 0.32, 1.00),
        Vec3::new(0.85, 0.24, 0.92),
        Vec3::new(1.00, 0.55, 0.18),
        Vec3::new(0.18, 0.90, 0.55),
        Vec3::new(0.08, 0.88, 0.88),
    ];
    PALETTE[stable_byte(cwd, 1) as usize % PALETTE.len()]
}

fn draw_glow_line(
    output: &mut [u8],
    width: usize,
    height: usize,
    from: Vec2,
    to: Vec2,
    color: Vec3,
) {
    let delta = to - from;
    let steps = delta.x.abs().max(delta.y.abs()).ceil().max(1.0) as usize;
    for step in 0..=steps {
        let point = from + delta * (step as f32 / steps as f32);
        for oy in -1..=1 {
            for ox in -1..=1 {
                let x = point.x.round() as isize + ox;
                let y = point.y.round() as isize + oy;
                if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
                    continue;
                }
                let alpha = if ox == 0 && oy == 0 { 0.78 } else { 0.22 };
                let offset = (y as usize * width + x as usize) * 4;
                let existing = Vec3::new(
                    output[offset] as f32 / 255.0,
                    output[offset + 1] as f32 / 255.0,
                    output[offset + 2] as f32 / 255.0,
                );
                let blended = existing.lerp(color, alpha).clamp(Vec3::ZERO, Vec3::ONE);
                output[offset] = (blended.x * 255.0) as u8;
                output[offset + 1] = (blended.y * 255.0) as u8;
                output[offset + 2] = (blended.z * 255.0) as u8;
            }
        }
    }
}

pub fn project_name(cwd: &str) -> String {
    std::path::Path::new(cwd)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(cwd)
        .to_uppercase()
}

struct Camera {
    position: Vec3,
    basis: Mat3,
    aspect: f32,
    focal: f32,
    width: f32,
    height: f32,
}

impl Camera {
    fn new(yaw: f32, pitch: f32, zoom: f32, width: usize, height: usize) -> Self {
        let distance = 9.0 / zoom;
        let position = Vec3::new(
            yaw.sin() * distance,
            3.4 + pitch * 4.0,
            yaw.cos() * distance,
        );
        let target = Vec3::new(0.0, 0.8, 0.0);
        let forward = (target - position).normalize();
        let right = forward.cross(Vec3::Y).normalize();
        let up = right.cross(forward).normalize();
        let width = width.max(1) as f32;
        let height = height.max(1) as f32;
        Self {
            position,
            basis: Mat3::from_cols(right, up, forward),
            aspect: width / height,
            focal: 1.35,
            width,
            height,
        }
    }

    fn ray(&self, x: usize, y: usize) -> Vec3 {
        let nx = ((x as f32 + 0.5) / self.width * 2.0 - 1.0) * self.aspect;
        let ny = 1.0 - (y as f32 + 0.5) / self.height * 2.0;
        (self.basis * Vec3::new(nx, ny, self.focal)).normalize()
    }

    fn project(&self, world: Vec3) -> Option<(Vec2, f32)> {
        let relative = world - self.position;
        let view = self.basis.transpose() * relative;
        if view.z <= 0.05 {
            return None;
        }
        let x = (view.x / view.z * self.focal / self.aspect + 1.0) * 0.5 * self.width;
        let y = (1.0 - view.y / view.z * self.focal) * 0.5 * self.height;
        Some((Vec2::new(x, y), view.z))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_scene_renders_opaque_frame() {
        let scene = Scene {
            nodes: Vec::new(),
            yaw: 0.0,
            pitch: 0.0,
            zoom: 1.0,
            time: 0.0,
        };
        let pixels = scene.render_rgba(32, 20);
        assert_eq!(pixels.len(), 32 * 20 * 4);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
    }
}
