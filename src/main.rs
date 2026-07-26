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
    Emit,
    /// Install the global Codex lifecycle integration.
    Integrate,
    /// Remove files and integration owned by Codex Operations Center.
    Uninstall,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Doctor) => println!("Codex Operations Center: bootstrap ready"),
        Some(Command::Emit) => println!("event ingestion is not enabled in this bootstrap"),
        Some(Command::Integrate) => println!("integration is not enabled in this bootstrap"),
        Some(Command::Uninstall) => println!("nothing has been installed yet"),
        None => println!(
            "Codex Operations Center bootstrap ({:?} graphics)",
            cli.graphics
        ),
    }
    Ok(())
}
