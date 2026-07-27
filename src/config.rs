use std::fs;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphicsChoice {
    #[default]
    Auto,
    Ultra,
    Unicode,
    Safe,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshPace {
    Fast,
    #[default]
    Balanced,
    Quiet,
}

impl RefreshPace {
    pub fn seconds(self) -> u64 {
        match self {
            Self::Fast => 2,
            Self::Balanced => 5,
            Self::Quiet => 15,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JournalDensity {
    Compact,
    #[default]
    Balanced,
    Full,
}

impl JournalDensity {
    pub fn rows(self, available: usize) -> usize {
        match self {
            Self::Compact => available.min(8),
            Self::Balanced => available.min(20),
            Self::Full => available,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptionAction {
    Graphics,
    Refresh,
    RestingAgents,
    JournalDensity,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UserSettings {
    pub graphics: GraphicsChoice,
    pub refresh: RefreshPace,
    pub show_resting_agents: bool,
    pub journal_density: JournalDensity,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            graphics: GraphicsChoice::Auto,
            refresh: RefreshPace::Balanced,
            show_resting_agents: true,
            journal_density: JournalDensity::Balanced,
        }
    }
}

impl UserSettings {
    pub fn load() -> Result<Self> {
        let path = crate::paths::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        serde_json::from_slice(&fs::read(&path)?)
            .with_context(|| format!("unable to decode {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = crate::paths::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&temporary, &path)?;
        Ok(())
    }

    pub fn cycle(&mut self, action: OptionAction) {
        match action {
            OptionAction::Graphics => {
                self.graphics = match self.graphics {
                    GraphicsChoice::Auto => GraphicsChoice::Ultra,
                    GraphicsChoice::Ultra => GraphicsChoice::Unicode,
                    GraphicsChoice::Unicode => GraphicsChoice::Safe,
                    GraphicsChoice::Safe => GraphicsChoice::Auto,
                }
            }
            OptionAction::Refresh => {
                self.refresh = match self.refresh {
                    RefreshPace::Fast => RefreshPace::Balanced,
                    RefreshPace::Balanced => RefreshPace::Quiet,
                    RefreshPace::Quiet => RefreshPace::Fast,
                }
            }
            OptionAction::RestingAgents => {
                self.show_resting_agents = !self.show_resting_agents;
            }
            OptionAction::JournalDensity => {
                self.journal_density = match self.journal_density {
                    JournalDensity::Compact => JournalDensity::Balanced,
                    JournalDensity::Balanced => JournalDensity::Full,
                    JournalDensity::Full => JournalDensity::Compact,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_each_user_facing_option() {
        let mut settings = UserSettings::default();
        settings.cycle(OptionAction::Graphics);
        settings.cycle(OptionAction::Refresh);
        settings.cycle(OptionAction::RestingAgents);
        settings.cycle(OptionAction::JournalDensity);
        assert_eq!(settings.graphics, GraphicsChoice::Ultra);
        assert_eq!(settings.refresh, RefreshPace::Quiet);
        assert!(!settings.show_resting_agents);
        assert_eq!(settings.journal_density, JournalDensity::Full);
    }
}
