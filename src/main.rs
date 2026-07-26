mod capabilities;
mod codex;
mod events;
mod hooks;
mod paths;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Doctor) => capabilities::print_doctor(),
        Some(Command::Emit) => events::ingest_stdin(),
        Some(Command::Integrate) => hooks::install(),
        Some(Command::Uninstall { purge }) => hooks::uninstall(purge),
        None => run_dashboard(cli.graphics),
    }
}

fn run_dashboard(graphics: GraphicsMode) -> Result<()> {
    let capabilities = capabilities::Capabilities::detect();
    let selected = capabilities.select(graphics);
    let threads = codex::list_threads(100).unwrap_or_else(|error| {
        eprintln!("Codex app-server unavailable: {error:#}");
        Vec::new()
    });
    let live_events = events::recent(100).unwrap_or_default();

    println!("Codex Operations Center");
    println!("rendering profile: {selected:?}");
    println!("local Codex threads: {}", threads.len());
    println!("recent lifecycle events: {}", live_events.len());
    println!("Run `codex-ops integrate` to enable live lifecycle events.");
    Ok(())
}
