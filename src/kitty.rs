use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::app::Dashboard;
use crate::scene::Scene;

const IMAGE_ID: u32 = 7_140_289;
const MAX_LIVE_WIDTH: usize = 960;
const MAX_LIVE_HEIGHT: usize = 640;
static FRAME_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn draw_scene(dashboard: &Dashboard) -> Result<()> {
    let area = dashboard.scene_area;
    let (cell_width, cell_height) = terminal_cell_size();
    let width = area.width as usize * cell_width;
    let height = area.height as usize * cell_height;
    if width == 0 || height == 0 {
        return Ok(());
    }
    let threads = dashboard.effective_threads();
    let scene = Scene::from_threads(
        &threads,
        dashboard.camera_yaw,
        dashboard.camera_pitch,
        dashboard.camera_zoom,
        dashboard.started_at.elapsed().as_secs_f32(),
        dashboard.selected,
    );
    let scale = (MAX_LIVE_WIDTH as f32 / width as f32)
        .min(MAX_LIVE_HEIGHT as f32 / height as f32)
        .min(1.0);
    let render_width = (width as f32 * scale).round().max(1.0) as usize;
    let render_height = (height as f32 * scale).round().max(1.0) as usize;
    let rgba = scene.render_rgba(render_width, render_height);
    if dashboard.capabilities.kitty_graphics {
        draw_kitty(area, render_width, render_height, &rgba)
    } else if dashboard.capabilities.sixel_graphics {
        draw_sixel(area, render_width, render_height, rgba)
    } else {
        Ok(())
    }
}

fn draw_kitty(area: ratatui::layout::Rect, width: usize, height: usize, rgba: &[u8]) -> Result<()> {
    let sequence = FRAME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "tty-graphics-protocol-codex-ops-{}-{sequence}.rgba",
        std::process::id()
    ));
    std::fs::write(&path, rgba)?;
    let encoded_path = STANDARD.encode(path.as_os_str().as_encoded_bytes());
    let mut stdout = io::stdout().lock();
    write!(stdout, "\x1b7\x1b[{};{}H", area.y + 1, area.x + 1)?;
    write!(
        stdout,
        "\x1b_Ga=T,f=32,t=t,s={width},v={height},i={IMAGE_ID},p=1,c={},r={},z=-1,C=1,q=2;{encoded_path}\x1b\\",
        area.width, area.height
    )?;
    write!(stdout, "\x1b8")?;
    stdout.flush()?;
    Ok(())
}

fn draw_sixel(
    area: ratatui::layout::Rect,
    width: usize,
    height: usize,
    rgba: Vec<u8>,
) -> Result<()> {
    let sixel = icy_sixel::SixelImage::try_from_rgba(rgba, width, height)?.encode()?;
    let mut stdout = io::stdout().lock();
    write!(
        stdout,
        "\x1b7\x1b[{};{}H{sixel}\x1b8",
        area.y + 1,
        area.x + 1
    )?;
    stdout.flush()?;
    Ok(())
}

fn terminal_cell_size() -> (usize, usize) {
    crossterm::terminal::window_size()
        .ok()
        .filter(|size| size.columns > 0 && size.rows > 0)
        .map(|size| {
            (
                (size.width / size.columns).max(1) as usize,
                (size.height / size.rows).max(1) as usize,
            )
        })
        .unwrap_or((8, 16))
}

pub fn delete_scene() -> Result<()> {
    let mut stdout = io::stdout().lock();
    write!(stdout, "\x1b_Ga=d,d=i,i={IMAGE_ID};\x1b\\")?;
    stdout.flush()?;
    Ok(())
}

pub fn save_snapshot(path: &Path, width: u32, height: u32) -> Result<()> {
    let threads = crate::codex::list_threads(36).unwrap_or_default();
    let scene = Scene::from_threads(&threads, 0.42, 0.22, 1.0, 12.0, 0);
    let rgba = scene.render_rgba(width as usize, height as usize);
    std::fs::write(path, encode_png(width, height, &rgba)?)?;
    println!("3D operations scene written to {}", path.display());
    Ok(())
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut output, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fastest);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_valid_png() {
        let rgba = vec![10, 20, 30, 255, 50, 60, 70, 255];
        let png = encode_png(2, 1, &rgba).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn encodes_valid_sixel() {
        let rgba = vec![10, 20, 30, 255, 50, 60, 70, 255];
        let sixel = icy_sixel::SixelImage::try_from_rgba(rgba, 2, 1)
            .unwrap()
            .encode()
            .unwrap();
        assert!(sixel.starts_with("\x1bP"));
        assert!(sixel.ends_with("\x1b\\"));
    }
}
