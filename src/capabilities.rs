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
    pub vte_sixel_build: bool,
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
        let vte_sixel_build = vte && vte_has_sixel_build();
        let sixel_graphics = std::env::var_os("WT_SESSION").is_some()
            || ["contour", "foot", "mlterm", "yaft"]
                .iter()
                .any(|name| lower.contains(name))
            || (vte_sixel_build && gnome_sixel_enabled().unwrap_or(false));
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
            vte_sixel_build,
            mouse: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
            tmux: std::env::var_os("TMUX").is_some(),
            ssh: std::env::var_os("SSH_CONNECTION").is_some(),
        }
    }

    pub fn select(&self, requested: GraphicsMode) -> RenderingProfile {
        match requested {
            GraphicsMode::Ultra if (self.kitty_graphics || self.sixel_graphics) && !self.tmux => {
                RenderingProfile::Ultra
            }
            GraphicsMode::Ultra if self.true_color => RenderingProfile::Unicode,
            GraphicsMode::Ultra => RenderingProfile::Safe,
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
    if capabilities.vte {
        println!("vte sixel build   {}", yes_no(capabilities.vte_sixel_build));
    }
    if capabilities.vte && !capabilities.vte_sixel_build {
        println!("graphics hint     bundled HD terminal required");
    } else if capabilities.vte && !capabilities.sixel_graphics {
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
    if !disable {
        anyhow::ensure!(
            vte_has_sixel_build(),
            "this GNOME Terminal/VTE build does not include +SIXEL; use the bundled HD terminal"
        );
    }
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

fn vte_has_sixel_build() -> bool {
    clean_desktop_command("gnome-terminal")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            let version = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            version.contains("+SIXEL")
        })
        .unwrap_or(false)
}

fn clean_desktop_command(program: &str) -> Command {
    let mut command = Command::new(program);
    for variable in [
        "GIO_MODULE_DIR",
        "GTK_EXE_PREFIX",
        "GTK_IM_MODULE_FILE",
        "GTK_PATH",
        "SNAP_LIBRARY_PATH",
    ] {
        command.env_remove(variable);
    }
    command
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ultra_never_selects_a_blank_pixel_backend() {
        let capabilities = Capabilities {
            terminal: "vte-without-sixel".to_owned(),
            true_color: true,
            kitty_graphics: false,
            sixel_graphics: false,
            vte: true,
            vte_sixel_build: false,
            mouse: true,
            tmux: false,
            ssh: false,
        };

        assert_eq!(
            capabilities.select(GraphicsMode::Ultra),
            RenderingProfile::Unicode
        );
    }
}
