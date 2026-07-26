use std::io::IsTerminal;
use std::process::Command;

use crate::GraphicsMode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RenderingProfile {
    Ultra,
    Unicode,
    Safe,
}

#[derive(Clone, Debug)]
pub struct Capabilities {
    pub terminal: String,
    pub true_color: bool,
    pub kitty_graphics: bool,
    pub sixel_graphics: bool,
    pub vte: bool,
    pub mouse: bool,
    pub tmux: bool,
    pub ssh: bool,
}

impl Capabilities {
    pub fn detect() -> Self {
        let env = |key: &str| std::env::var(key).unwrap_or_default();
        let term = env("TERM");
        let program = env("TERM_PROGRAM");
        let terminal = if !program.is_empty() {
            program
        } else if !term.is_empty() {
            term.clone()
        } else {
            "unknown".to_owned()
        };
        let lower = format!(
            "{} {} {} {}",
            terminal,
            term,
            env("KITTY_WINDOW_ID"),
            env("GHOSTTY_RESOURCES_DIR")
        )
        .to_lowercase();
        let kitty_graphics = ["kitty", "ghostty", "wezterm", "konsole"]
            .iter()
            .any(|name| lower.contains(name));
        let vte = !env("VTE_VERSION").is_empty();
        let sixel_graphics = std::env::var_os("WT_SESSION").is_some()
            || ["contour", "foot", "mlterm", "yaft"]
                .iter()
                .any(|name| lower.contains(name))
            || (vte && gnome_sixel_enabled().unwrap_or(false));
        let color = env("COLORTERM").to_lowercase();
        let true_color = color.contains("truecolor")
            || color.contains("24bit")
            || kitty_graphics
            || term.contains("256color");

        Self {
            terminal,
            true_color,
            kitty_graphics,
            sixel_graphics,
            vte,
            mouse: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            tmux: std::env::var_os("TMUX").is_some(),
            ssh: std::env::var_os("SSH_CONNECTION").is_some(),
        }
    }

    pub fn select(&self, requested: GraphicsMode) -> RenderingProfile {
        match requested {
            GraphicsMode::Ultra => RenderingProfile::Ultra,
            GraphicsMode::Unicode => RenderingProfile::Unicode,
            GraphicsMode::Safe => RenderingProfile::Safe,
            GraphicsMode::Auto if (self.kitty_graphics || self.sixel_graphics) && !self.tmux => {
                RenderingProfile::Ultra
            }
            GraphicsMode::Auto if self.true_color => RenderingProfile::Unicode,
            GraphicsMode::Auto => RenderingProfile::Safe,
        }
    }
}

pub fn print_doctor() -> anyhow::Result<()> {
    let capabilities = Capabilities::detect();
    println!("Codex Operations Center diagnostics");
    println!("terminal          {}", capabilities.terminal);
    println!("true color        {}", yes_no(capabilities.true_color));
    println!("kitty graphics    {}", yes_no(capabilities.kitty_graphics));
    println!("sixel graphics    {}", yes_no(capabilities.sixel_graphics));
    if capabilities.vte && !capabilities.sixel_graphics {
        println!("graphics hint     run `codex-ops terminal-graphics`");
    }
    println!("mouse             {}", yes_no(capabilities.mouse));
    println!("tmux              {}", yes_no(capabilities.tmux));
    println!("ssh               {}", yes_no(capabilities.ssh));
    println!(
        "selected profile  {:?}",
        capabilities.select(GraphicsMode::Auto)
    );
    println!("codex             {}", command_available("codex"));
    match crate::codex::list_threads(250) {
        Ok(threads) => {
            let running = threads
                .iter()
                .filter(|thread| {
                    matches!(
                        thread.status,
                        crate::codex::ThreadStatus::Active { .. }
                            | crate::codex::ThreadStatus::ObservedRunning
                    )
                })
                .count();
            let open = threads
                .iter()
                .filter(|thread| matches!(thread.status, crate::codex::ThreadStatus::ObservedOpen))
                .count();
            println!("running tasks     {running}");
            println!("open sessions     {open}");
            match crate::events::recent_for_threads(&threads, 500) {
                Ok(events) => println!("activity events   {}", events.len()),
                Err(error) => println!("activity probe    unavailable: {error:#}"),
            }
        }
        Err(error) => println!("session probe     unavailable: {error:#}"),
    }
    println!(
        "hooks             {}",
        crate::paths::hooks_path()?.display()
    );
    println!("data              {}", crate::paths::data_dir()?.display());
    Ok(())
}

pub fn configure_terminal_graphics(disable: bool) -> anyhow::Result<()> {
    let (profile_id, schema) = gnome_profile()?;
    let value = if disable { "false" } else { "true" };
    let status = Command::new("gsettings")
        .args(["set", &schema, "enable-sixel", value])
        .status()?;
    anyhow::ensure!(
        status.success(),
        "gsettings could not update the GNOME Terminal profile"
    );
    let action = if disable { "disabled" } else { "enabled" };
    println!("Sixel graphics {action} for GNOME Terminal profile {profile_id}.");
    println!("Close and reopen the terminal if the current window does not refresh the setting.");
    Ok(())
}

fn gnome_sixel_enabled() -> anyhow::Result<bool> {
    let (_, schema) = gnome_profile()?;
    let output = Command::new("gsettings")
        .args(["get", &schema, "enable-sixel"])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "gsettings could not read enable-sixel"
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim() == "true")
}

fn gnome_profile() -> anyhow::Result<(String, String)> {
    let output = Command::new("gsettings")
        .args(["get", "org.gnome.Terminal.ProfilesList", "default"])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "GNOME Terminal profile is unavailable"
    );
    let profile_id = String::from_utf8(output.stdout)?
        .trim()
        .trim_matches('\'')
        .to_owned();
    anyhow::ensure!(
        !profile_id.is_empty(),
        "GNOME Terminal default profile is empty"
    );
    let schema = format!(
        "org.gnome.Terminal.Legacy.Profile:/org/gnome/terminal/legacy/profiles:/:{profile_id}/"
    );
    Ok((profile_id, schema))
}

fn command_available(command: &str) -> &'static str {
    if std::process::Command::new(command)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        "available"
    } else {
        "not found"
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
