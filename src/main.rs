mod app;
mod capabilities;
mod codex;
mod events;
mod hooks;
mod kitty;
mod paths;
mod scene;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "codex-ops", version, about)]
struct Cli {
    #[arg(long, value_enum, default_value_t = GraphicsMode::Auto)]
    graphics: GraphicsMode,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
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
    let selected = capabilities.select(graphics);
    app::run(capabilities, selected)
}
