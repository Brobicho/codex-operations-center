mod app;
mod capabilities;
mod codex;
mod events;
mod hooks;
mod kitty;
mod paths;
mod runtime;
mod scene;
mod ui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};

#[derive(Debug, Parser)]
#[command(name = "codex-ops", version, about)]
struct Cli {
    #[arg(long, value_enum, default_value_t = GraphicsMode::Auto)]
    graphics: GraphicsMode,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum GraphicsMode {
    #[default]
    Auto,
    Ultra,
    Unicode,
    Safe,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect terminal and Codex integration capabilities.
    Doctor,
    /// Receive one lifecycle event from a Codex hook on stdin.
    #[command(hide = true)]
    Emit,
    /// Install the global Codex lifecycle integration.
    Integrate,
    /// Enable high-resolution Sixel graphics in the active GNOME Terminal profile.
    TerminalGraphics {
        /// Revert the GNOME Terminal Sixel preference instead of enabling it.
        #[arg(long)]
        disable: bool,
    },
    /// Remove files and integration owned by Codex Operations Center.
    Uninstall {
        /// Also remove locally collected events and settings.
        #[arg(long)]
        purge: bool,
    },
    /// Render a standalone PNG preview of the current operations scene.
    Snapshot {
        #[arg(short, long, default_value = "codex-ops-preview.png")]
        output: PathBuf,
        #[arg(long, default_value_t = 960)]
        width: u32,
        #[arg(long, default_value_t = 540)]
        height: u32,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Doctor) => capabilities::print_doctor(),
        Some(Command::Emit) => events::ingest_stdin(),
        Some(Command::Integrate) => hooks::install(),
        Some(Command::TerminalGraphics { disable }) => {
            capabilities::configure_terminal_graphics(disable)
        }
        Some(Command::Uninstall { purge }) => hooks::uninstall(purge),
        Some(Command::Snapshot {
            output,
            width,
            height,
        }) => kitty::save_snapshot(&output, width, height),
        None => run_dashboard(cli.graphics),
    }
}

fn run_dashboard(graphics: GraphicsMode) -> Result<()> {
    let capabilities = capabilities::Capabilities::detect();
    if should_launch_hd_terminal(graphics, &capabilities) && launch_hd_terminal()? {
        return Ok(());
    }
    let selected = capabilities.select(graphics);
    app::run(capabilities, selected)
}

fn should_launch_hd_terminal(
    graphics: GraphicsMode,
    capabilities: &capabilities::Capabilities,
) -> bool {
    matches!(graphics, GraphicsMode::Auto | GraphicsMode::Ultra)
        && !capabilities.kitty_graphics
        && !capabilities.sixel_graphics
        && !capabilities.tmux
        && !capabilities.ssh
        && std::env::var_os("CODEX_OPS_HD_CHILD").is_none()
        && (std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some())
}

fn launch_hd_terminal() -> Result<bool> {
    let terminal = paths::hd_terminal_path()?;
    if !terminal.is_file() {
        return Ok(false);
    }
    let executable = std::env::current_exe().context("unable to locate codex-ops")?;
    let cwd = std::env::current_dir().context("unable to locate the working directory")?;
    ProcessCommand::new(&terminal)
        .args([
            "--detach",
            "--start-as=maximized",
            "--title",
            "Codex Operations Center",
            "--directory",
        ])
        .arg(cwd)
        .arg(executable)
        .args(["--graphics", "ultra"])
        .env("CODEX_OPS_HD_CHILD", "1")
        .env_remove("NO_COLOR")
        .env("COLORTERM", "truecolor")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("unable to start bundled terminal {}", terminal.display()))?;
    println!("Opening the HD operations center in its compatible terminal...");
    Ok(true)
}
