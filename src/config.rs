use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const ENV_APP_TOKEN: &str = "YATUI_APP_TOKEN";
pub const ENV_USER_TOKEN: &str = "YATUI_USER_TOKEN";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_token: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tokens {
    pub app: Option<String>,
    pub user: Option<String>,
}

fn clean(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

impl Config {
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("yatui").join("config.toml"))
    }

    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).with_context(|| path.display().to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| path.display().to_string()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(path, toml::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn tokens(&self, env_app: Option<String>, env_user: Option<String>) -> Tokens {
        Tokens {
            app: clean(env_app).or_else(|| clean(self.app_token.clone())),
            user: clean(env_user).or_else(|| clean(self.user_token.clone())),
        }
    }
}

pub fn mask(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 8 {
        "*".repeat(chars.len())
    } else {
        format!(
            "{}…{}",
            chars[..4].iter().collect::<String>(),
            chars[chars.len() - 4..].iter().collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_config_and_blank_values_are_ignored() {
        let cfg = Config {
            app_token: Some("cfg_app".into()),
            user_token: Some("  ".into()),
        };
        assert_eq!(
            cfg.tokens(None, None),
            Tokens {
                app: Some("cfg_app".into()),
                user: None
            }
        );
        assert_eq!(
            cfg.tokens(Some("env_app".into()), Some(" env_user ".into())),
            Tokens {
                app: Some("env_app".into()),
                user: Some("env_user".into())
            }
        );
        assert_eq!(
            cfg.tokens(Some("".into()), None).app.as_deref(),
            Some("cfg_app")
        );
    }

    #[test]
    fn save_load_roundtrip_is_private_and_missing_file_is_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/config.toml");
        assert_eq!(Config::load(&path).unwrap(), Config::default());
        let cfg = Config {
            app_token: None,
            user_token: Some("secret.jwt.token".into()),
        };
        cfg.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), cfg);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn mask_hides_middle_and_short_tokens() {
        assert_eq!(mask("abcdefghijkl"), "abcd…ijkl");
        assert_eq!(mask("short"), "*****");
    }
}
