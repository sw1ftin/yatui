mod app;
mod config;
mod kodik;
mod library;
mod player;
mod source;
mod strings;
mod tui;
mod yummy;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};

use config::{Config, ENV_APP_TOKEN, ENV_USER_TOKEN, mask};
use library::{Library, WatchStatus};
use player::{Mpv, Player};
use source::{Source, Title, TitleDetails, pick_translation};
use yummy::YummyAnime;

#[derive(Parser)]
#[command(name = strings::APP_NAME, about = strings::ABOUT, version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Args)]
struct Pick {
    #[arg(help = strings::HELP_QUERY)]
    query: String,
    #[arg(short = 'n', long, default_value_t = 1, help = strings::HELP_INDEX)]
    index: usize,
}

#[derive(Subcommand)]
enum Cmd {
    #[command(about = strings::HELP_SEARCH)]
    Search {
        #[arg(help = strings::HELP_QUERY)]
        query: String,
    },
    #[command(about = strings::HELP_INFO)]
    Info {
        #[command(flatten)]
        pick: Pick,
    },
    #[command(about = strings::HELP_PLAY)]
    Play {
        #[command(flatten)]
        pick: Pick,
        #[arg(short, long, help = strings::HELP_EPISODE)]
        episode: Option<String>,
        #[arg(short, long, help = strings::HELP_TRANSLATION)]
        translation: Option<String>,
        #[arg(short, long, help = strings::HELP_QUALITY)]
        quality: Option<u32>,
        #[arg(long, help = strings::HELP_PRINT_CMD)]
        print_cmd: bool,
    },
    #[command(about = strings::HELP_LIST)]
    List {
        #[arg(help = strings::HELP_LIST_STATUS)]
        status: Option<WatchStatus>,
    },
    #[command(about = strings::HELP_STATUS)]
    Status {
        #[arg(help = strings::HELP_SLUG)]
        slug: String,
        #[arg(help = strings::HELP_NEW_STATUS)]
        status: Option<WatchStatus>,
    },
    #[command(about = strings::HELP_TOKEN, subcommand)]
    Token(TokenCmd),
}

#[derive(Subcommand)]
enum TokenCmd {
    #[command(about = strings::HELP_TOKEN_SET)]
    Set {
        #[arg(help = strings::HELP_TOKEN_VALUE)]
        token: String,
    },
    #[command(about = strings::HELP_TOKEN_APP)]
    App {
        #[arg(help = strings::HELP_TOKEN_VALUE)]
        token: String,
    },
    #[command(about = strings::HELP_TOKEN_STATUS)]
    Status,
    #[command(about = strings::HELP_TOKEN_REFRESH)]
    Refresh,
    #[command(about = strings::HELP_TOKEN_CLEAR)]
    Clear,
}

fn config_path() -> Result<std::path::PathBuf> {
    Config::default_path().context("no config directory")
}

fn client(cfg: &Config) -> Result<YummyAnime> {
    YummyAnime::new(cfg.tokens(
        std::env::var(ENV_APP_TOKEN).ok(),
        std::env::var(ENV_USER_TOKEN).ok(),
    ))
}

fn run_token(cmd: TokenCmd, path: &std::path::Path, mut cfg: Config) -> Result<()> {
    match cmd {
        TokenCmd::Set { token } => {
            cfg.user_token = Some(token.trim().to_owned()).filter(|t| !t.is_empty());
            cfg.save(path)?;
            println!("{}", strings::TOKEN_SAVED);
            let nickname = client(&cfg)?.profile()?;
            println!("{}", strings::token_logged_in(&nickname));
        }
        TokenCmd::App { token } => {
            cfg.app_token = Some(token.trim().to_owned()).filter(|t| !t.is_empty());
            cfg.save(path)?;
            println!("{}", strings::TOKEN_SAVED);
        }
        TokenCmd::Status => {
            let tokens = cfg.tokens(
                std::env::var(ENV_APP_TOKEN).ok(),
                std::env::var(ENV_USER_TOKEN).ok(),
            );
            println!("{}", strings::config_path(&path.display().to_string()));
            println!(
                "{}",
                strings::token_app_line(
                    &tokens
                        .app
                        .as_deref()
                        .map_or(strings::TOKEN_PUBLIC.into(), mask)
                )
            );
            println!(
                "{}",
                strings::token_user_line(
                    &tokens
                        .user
                        .as_deref()
                        .map_or(strings::TOKEN_NONE.into(), mask)
                )
            );
            for var in [ENV_APP_TOKEN, ENV_USER_TOKEN] {
                if std::env::var(var).is_ok_and(|v| !v.trim().is_empty()) {
                    println!("{}", strings::token_env_override(var));
                }
            }
            if tokens.user.is_some() {
                println!("{}", strings::token_logged_in(&client(&cfg)?.profile()?));
            }
        }
        TokenCmd::Refresh => {
            if cfg
                .tokens(None, std::env::var(ENV_USER_TOKEN).ok())
                .user
                .is_none()
            {
                return Err(anyhow!(strings::ERR_NO_USER_TOKEN));
            }
            cfg.user_token = Some(client(&cfg)?.refresh_token()?);
            cfg.save(path)?;
            println!("{}", strings::TOKEN_REFRESHED);
        }
        TokenCmd::Clear => {
            cfg = Config::default();
            cfg.save(path)?;
            println!("{}", strings::TOKEN_CLEARED);
        }
    }
    Ok(())
}

fn open_library() -> Result<Library> {
    Library::open(Library::default_path().context("no data directory")?)
}

fn pick_title(source: &dyn Source, pick: &Pick) -> Result<Title> {
    let mut results = source.search(&pick.query)?;
    if results.is_empty() {
        return Err(anyhow!(strings::ERR_NO_RESULTS));
    }
    let total = results.len();
    if pick.index == 0 || pick.index > total {
        return Err(anyhow!(strings::err_result_index(pick.index, total)));
    }
    Ok(results.swap_remove(pick.index - 1))
}

fn print_title(n: usize, t: &Title, library: &Library) {
    let mut line = format!("{n:>2}. {}", t.name);
    if let Some(y) = t.year {
        line.push_str(&format!(" ({y})"));
    }
    if let Some(k) = &t.kind {
        line.push_str(&format!(" [{k}]"));
    }
    line.push_str(&format!("  {}", t.slug));
    if let Some(s) = library.status(&t.slug) {
        line.push_str(&format!("  <{}>", strings::status_label(s)));
    }
    println!("{line}");
}

fn details_for(source: &dyn Source, pick: &Pick) -> Result<TitleDetails> {
    let title = pick_title(source, pick)?;
    let details = source.details(&title.slug)?;
    if details.translations.is_empty() {
        return Err(anyhow!(strings::ERR_NO_TRANSLATIONS));
    }
    Ok(details)
}

fn run(cmd: Cmd, cfg: Config) -> Result<()> {
    match cmd {
        Cmd::Search { query } => {
            let library = open_library()?;
            let results = client(&cfg)?.search(&query)?;
            if results.is_empty() {
                return Err(anyhow!(strings::ERR_NO_RESULTS));
            }
            for (i, t) in results.iter().enumerate() {
                print_title(i + 1, t, &library);
            }
        }
        Cmd::Info { pick } => {
            let library = open_library()?;
            let d = details_for(&client(&cfg)?, &pick)?;
            let entry = library.get(&d.title.slug);
            println!("{}  {}", d.title.name, d.title.slug);
            println!(
                "{}",
                strings::episodes_progress(d.title.episodes_aired, d.title.episodes_total)
            );
            println!("{}", strings::list_status(entry.and_then(|e| e.status)));
            println!("{}:", strings::TRANSLATIONS);
            for t in &d.translations {
                let watched = t
                    .episodes
                    .iter()
                    .filter(|e| entry.is_some_and(|en| en.is_watched(&e.number)))
                    .count();
                println!(
                    "  {}  ({}, {})",
                    t.name,
                    strings::episode_count(t.episodes.len()),
                    strings::watched_progress(watched, d.title.episodes_total)
                );
            }
        }
        Cmd::Play {
            pick,
            episode,
            translation,
            quality,
            print_cmd,
        } => {
            let mut library = open_library()?;
            let source = client(&cfg)?;
            let d = details_for(&source, &pick)?;
            let entry = library.get(&d.title.slug);
            let wanted = translation
                .as_deref()
                .or(entry.and_then(|e| e.translation.as_deref()));
            let ti = pick_translation(&d.translations, wanted)
                .or_else(|| translation.is_none().then_some(0))
                .ok_or_else(|| anyhow!(strings::err_no_translation(wanted.unwrap_or_default())))?;
            let tr = &d.translations[ti];
            let ep = match &episode {
                Some(n) => app::find_episode(tr, n)?,
                None => {
                    app::next_episode(tr, entry).ok_or_else(|| anyhow!(strings::ERR_NO_STREAMS))?
                }
            };
            let req = app::resolve(&source, &d.title, ep, quality)?;
            if print_cmd {
                println!("{}", Mpv.command_line(&req));
            } else {
                eprintln!("{}", strings::now_playing(&d.title.name, &ep.number));
                app::play_and_record(&Mpv, &req, &mut library, &d, tr, ep)?;
                eprintln!("{}", strings::marked_watched(&ep.number));
            }
        }
        Cmd::List { status } => {
            let library = open_library()?;
            let statuses: Vec<_> = status.map_or(WatchStatus::ALL.to_vec(), |s| vec![s]);
            let mut any = false;
            for s in statuses {
                let items = library.by_status(s);
                if items.is_empty() {
                    continue;
                }
                any = true;
                println!("{} ({})", strings::status_label(s), items.len());
                for (slug, e) in items {
                    println!(
                        "  {}  {}  {}",
                        e.name,
                        strings::watched_progress(e.watched.len(), e.total_episodes),
                        slug
                    );
                }
            }
            if !any {
                println!("{}", strings::LIST_EMPTY);
            }
        }
        Cmd::Status { slug, status } => {
            let mut library = open_library()?;
            let name = match library.get(&slug) {
                Some(e) if !e.name.is_empty() => e.name.clone(),
                _ => {
                    let d = client(&cfg)?.details(&slug)?;
                    library.set_status(&slug, &d.title.name, status);
                    library.set_total(&slug, d.title.episodes_total);
                    d.title.name
                }
            };
            library.set_status(&slug, &name, status);
            library.save()?;
            println!("{}", strings::status_set(&name, status));
        }
        Cmd::Token(cmd) => run_token(cmd, &config_path()?, cfg)?,
    }
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    let result = config_path()
        .and_then(|p| Config::load(&p))
        .and_then(|cfg| match cli.command {
            Some(cmd) => run(cmd, cfg),
            None => {
                let lib = open_library()?;
                tui::run(Box::new(client(&cfg)?), Box::new(Mpv), lib)
            }
        });
    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
