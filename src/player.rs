use std::process::{Command, Stdio};

use anyhow::{Result, anyhow, bail};

use crate::source::{Subtitle, Variant};
use crate::strings;

#[derive(Debug, Clone, PartialEq)]
pub struct PlayRequest {
    pub title: String,
    pub variant: Variant,
    pub subtitles: Vec<Subtitle>,
    pub referer: String,
    pub user_agent: String,
}

pub trait Player {
    fn name(&self) -> &str;
    fn args(&self, req: &PlayRequest) -> Vec<String>;

    fn command_line(&self, req: &PlayRequest) -> String {
        std::iter::once(self.name().to_owned())
            .chain(self.args(req))
            .map(|a| shell_quote(&a))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn play(&self, req: &PlayRequest) -> Result<()> {
        let status = Command::new(self.name())
            .args(self.args(req))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| anyhow!(strings::err_spawn_player(self.name(), &e.to_string())))?;
        if !status.success() {
            bail!("{}: {status}", strings::ERR_PLAYER_FAILED);
        }
        Ok(())
    }
}

pub struct Mpv;

impl Player for Mpv {
    fn name(&self) -> &str {
        "mpv"
    }

    fn args(&self, req: &PlayRequest) -> Vec<String> {
        let mut args = vec![
            format!("--force-media-title={}", req.title),
            format!("--referrer={}", req.referer),
            format!("--user-agent={}", req.user_agent),
        ];
        args.extend(
            req.subtitles
                .iter()
                .map(|s| format!("--sub-file={}", s.url)),
        );
        args.push(req.variant.url.clone());
        args
    }
}

pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./=:,@%+".contains(&b))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(subs: &[&str]) -> PlayRequest {
        PlayRequest {
            title: "Frieren's Journey — Episode 1".into(),
            variant: Variant {
                quality: 720,
                url: "https://cdn/720.mp4:hls:manifest.m3u8".into(),
            },
            subtitles: subs
                .iter()
                .map(|u| Subtitle {
                    url: (*u).into(),
                    label: None,
                })
                .collect(),
            referer: "https://kodikplayer.com/seria/1?a=1&b=2".into(),
            user_agent: "UA 1.0".into(),
        }
    }

    #[test]
    fn mpv_args_put_stream_last_and_add_each_subtitle() {
        let args = Mpv.args(&req(&["https://s/1.ass", "https://s/2.vtt"]));
        assert_eq!(
            args.last().unwrap(),
            "https://cdn/720.mp4:hls:manifest.m3u8"
        );
        assert!(args.contains(&"--sub-file=https://s/1.ass".to_owned()));
        assert!(args.contains(&"--sub-file=https://s/2.vtt".to_owned()));
        assert!(args.contains(&"--referrer=https://kodikplayer.com/seria/1?a=1&b=2".to_owned()));
        assert!(
            !Mpv.args(&req(&[]))
                .iter()
                .any(|a| a.starts_with("--sub-file"))
        );
    }

    #[test]
    fn command_line_is_shell_safe() {
        let line = Mpv.command_line(&req(&[]));
        assert!(
            line.starts_with("mpv '--force-media-title=Frieren'\\''s Journey — Episode 1' "),
            "{line}"
        );
        assert!(
            line.contains("'--referrer=https://kodikplayer.com/seria/1?a=1&b=2'"),
            "{line}"
        );
        assert!(
            line.ends_with(" https://cdn/720.mp4:hls:manifest.m3u8"),
            "{line}"
        );
        assert_eq!(shell_quote(""), "''");
    }
}
