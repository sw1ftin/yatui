use anyhow::Result;
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph, Tabs, Wrap};

use crate::app;
use crate::library::{Library, WatchStatus};
use crate::player::Player;
use crate::source::{Source, StreamSet, Title, TitleDetails};
use crate::strings;

enum Screen {
    Search,
    Library,
    Title,
    Episodes,
    Quality,
}

struct Ui {
    source: Box<dyn Source>,
    player: Box<dyn Player>,
    library: Library,
    screen: Screen,
    editing: bool,
    query: String,
    results: Vec<Title>,
    results_state: ListState,
    tab: usize,
    library_state: ListState,
    details: Option<TitleDetails>,
    translation_state: ListState,
    episode_state: ListState,
    streams: Option<(usize, StreamSet)>,
    quality_state: ListState,
    back_to_library: bool,
    message: Option<String>,
    quit: bool,
}

fn move_sel(state: &mut ListState, len: usize, delta: isize) {
    if len == 0 {
        state.select(None);
        return;
    }
    let cur = state.selected().unwrap_or(0) as isize;
    state.select(Some((cur + delta).clamp(0, len as isize - 1) as usize));
}

fn reset(state: &mut ListState, len: usize) {
    state.select((len > 0).then_some(0));
}

fn nav_delta(code: KeyCode) -> Option<isize> {
    match code {
        KeyCode::Down | KeyCode::Char('j') => Some(1),
        KeyCode::Up | KeyCode::Char('k') => Some(-1),
        KeyCode::PageDown => Some(10),
        KeyCode::PageUp => Some(-10),
        KeyCode::Home | KeyCode::Char('g') => Some(isize::MIN / 2),
        KeyCode::End | KeyCode::Char('G') => Some(isize::MAX / 2),
        _ => None,
    }
}

fn highlight() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

impl Ui {
    fn selected_translation(&self) -> Option<usize> {
        self.translation_state.selected()
    }

    fn library_items(&self) -> Vec<(String, String)> {
        self.library
            .by_status(WatchStatus::ALL[self.tab])
            .into_iter()
            .map(|(slug, e)| (slug.to_owned(), e.name.clone()))
            .collect()
    }

    fn reset_library(&mut self) {
        let n = self.library_items().len();
        reset(&mut self.library_state, n);
    }

    fn with_loading<T>(
        &mut self,
        term: &mut DefaultTerminal,
        f: impl FnOnce(&Self) -> Result<T>,
    ) -> Option<T> {
        self.message = Some(strings::LOADING.into());
        let _ = term.draw(|fr| self.draw(fr));
        match f(self) {
            Ok(v) => {
                self.message = None;
                Some(v)
            }
            Err(e) => {
                self.message = Some(format!("{e:#}"));
                None
            }
        }
    }

    fn open_title(&mut self, term: &mut DefaultTerminal, slug: String, from_library: bool) {
        let Some(details) = self.with_loading(term, |ui| ui.source.details(&slug)) else {
            return;
        };
        self.library
            .set_total(&details.title.slug, details.title.episodes_total);
        let saved = self.library.get(&slug).and_then(|e| e.translation.clone());
        let idx =
            crate::source::pick_translation(&details.translations, saved.as_deref()).unwrap_or(0);
        self.translation_state
            .select((!details.translations.is_empty()).then_some(idx));
        if details.translations.is_empty() {
            self.message = Some(strings::ERR_NO_TRANSLATIONS.into());
        }
        self.details = Some(details);
        self.back_to_library = from_library;
        self.screen = Screen::Title;
    }

    fn open_episodes(&mut self) {
        let (Some(d), Some(ti)) = (&self.details, self.selected_translation()) else {
            return;
        };
        let tr = &d.translations[ti];
        let entry = self.library.get(&d.title.slug);
        let next = app::next_episode(tr, entry)
            .and_then(|n| tr.episodes.iter().position(|e| e.number == n.number));
        self.episode_state
            .select(next.or((!tr.episodes.is_empty()).then_some(0)));
        self.screen = Screen::Episodes;
    }

    fn open_quality(&mut self, term: &mut DefaultTerminal) {
        let (Some(ti), Some(ei)) = (self.selected_translation(), self.episode_state.selected())
        else {
            return;
        };
        let streams = self.with_loading(term, |ui| {
            let ep = &ui.details.as_ref().expect("details").translations[ti].episodes[ei];
            ui.source.streams(ep)
        });
        if let Some(s) = streams {
            reset(&mut self.quality_state, s.variants.len());
            self.streams = Some((ei, s));
            self.screen = Screen::Quality;
        }
    }

    fn play(&mut self, term: &mut DefaultTerminal) {
        let (Some(d), Some(ti), Some((ei, streams)), Some(vi)) = (
            &self.details,
            self.selected_translation(),
            &self.streams,
            self.quality_state.selected(),
        ) else {
            return;
        };
        let details = d.clone();
        let tr = details.translations[ti].clone();
        let ep = tr.episodes[*ei].clone();
        let req = match app::build_request(&details.title, &ep, streams, vi) {
            Ok(r) => r,
            Err(e) => {
                self.message = Some(format!("{e:#}"));
                return;
            }
        };
        self.message = Some(strings::now_playing(&details.title.name, &ep.number));
        let _ = term.draw(|fr| self.draw(fr));
        self.message = Some(
            match app::play_and_record(
                self.player.as_ref(),
                &req,
                &mut self.library,
                &details,
                &tr,
                &ep,
            ) {
                Ok(()) => strings::marked_watched(&ep.number),
                Err(e) => format!("{e:#}"),
            },
        );
        let _ = term.clear();
        self.screen = Screen::Episodes;
        move_sel(&mut self.episode_state, tr.episodes.len(), 1);
    }

    fn toggle_watched(&mut self) {
        let (Some(d), Some(ti), Some(ei)) = (
            &self.details,
            self.selected_translation(),
            self.episode_state.selected(),
        ) else {
            return;
        };
        let tr = &d.translations[ti];
        let ep = &tr.episodes[ei].number;
        let slug = &d.title.slug;
        if self.library.get(slug).is_some_and(|e| e.is_watched(ep)) {
            self.library.unmark_watched(slug, ep);
            self.message = Some(strings::marked_unwatched(ep));
        } else {
            self.library
                .mark_watched(slug, &d.title.name, ep, d.title.episodes_total, &tr.name);
            self.message = Some(strings::marked_watched(ep));
        }
        self.save();
    }

    fn set_status(&mut self, status: Option<WatchStatus>) {
        let Some(d) = &self.details else { return };
        self.library
            .set_status(&d.title.slug, &d.title.name, status);
        self.library
            .set_total(&d.title.slug, d.title.episodes_total);
        self.message = Some(strings::status_set(&d.title.name, status));
        self.save();
    }

    fn save(&mut self) {
        if let Err(e) = self.library.save() {
            self.message = Some(format!("{e:#}"));
        }
    }

    fn back(&mut self) {
        self.screen = match self.screen {
            Screen::Quality => Screen::Episodes,
            Screen::Episodes => Screen::Title,
            Screen::Title if self.back_to_library => Screen::Library,
            Screen::Title => Screen::Search,
            Screen::Search | Screen::Library => return,
        };
    }

    fn handle(&mut self, term: &mut DefaultTerminal, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if matches!(self.screen, Screen::Search) && self.editing {
            match key.code {
                KeyCode::Enter if !self.query.trim().is_empty() => {
                    let q = self.query.clone();
                    if let Some(r) = self.with_loading(term, |ui| ui.source.search(&q)) {
                        if r.is_empty() {
                            self.message = Some(strings::ERR_NO_RESULTS.into());
                        }
                        reset(&mut self.results_state, r.len());
                        self.results = r;
                        self.editing = false;
                    }
                }
                KeyCode::Esc => self.editing = false,
                KeyCode::Tab => {
                    self.editing = false;
                    self.screen = Screen::Library;
                    self.reset_library();
                }
                KeyCode::Backspace => {
                    self.query.pop();
                }
                KeyCode::Char(c) => self.query.push(c),
                _ => {}
            }
            return;
        }
        if key.code == KeyCode::Char('q') {
            self.quit = true;
            return;
        }
        if key.code == KeyCode::Esc || key.code == KeyCode::Backspace {
            self.back();
            return;
        }
        match self.screen {
            Screen::Search => match key.code {
                KeyCode::Char('/') | KeyCode::Char('i') => self.editing = true,
                KeyCode::Tab => {
                    self.screen = Screen::Library;
                    self.reset_library();
                }
                KeyCode::Enter => {
                    if let Some(t) = self
                        .results_state
                        .selected()
                        .and_then(|i| self.results.get(i))
                    {
                        let slug = t.slug.clone();
                        self.open_title(term, slug, false);
                    }
                }
                c => {
                    if let Some(d) = nav_delta(c) {
                        move_sel(&mut self.results_state, self.results.len(), d);
                    }
                }
            },
            Screen::Library => match key.code {
                KeyCode::Tab | KeyCode::Char('/') => {
                    self.screen = Screen::Search;
                    self.editing = key.code == KeyCode::Char('/');
                }
                KeyCode::Left | KeyCode::Char('h') | KeyCode::Right | KeyCode::Char('l') => {
                    let n = WatchStatus::ALL.len();
                    self.tab = if matches!(key.code, KeyCode::Left | KeyCode::Char('h')) {
                        (self.tab + n - 1) % n
                    } else {
                        (self.tab + 1) % n
                    };
                    self.reset_library();
                }
                KeyCode::Enter => {
                    let items = self.library_items();
                    if let Some((slug, _)) =
                        self.library_state.selected().and_then(|i| items.get(i))
                    {
                        self.open_title(term, slug.clone(), true);
                    }
                }
                c => {
                    if let Some(d) = nav_delta(c) {
                        {
                            let n = self.library_items().len();
                            move_sel(&mut self.library_state, n, d);
                        }
                    }
                }
            },
            Screen::Title => match key.code {
                KeyCode::Enter => self.open_episodes(),
                KeyCode::Char(c @ '1'..='5') => {
                    self.set_status(Some(WatchStatus::ALL[(c as u8 - b'1') as usize]));
                }
                KeyCode::Char('x') => self.set_status(None),
                c => {
                    let len = self.details.as_ref().map_or(0, |d| d.translations.len());
                    if let Some(d) = nav_delta(c) {
                        move_sel(&mut self.translation_state, len, d);
                    }
                }
            },
            Screen::Episodes => match key.code {
                KeyCode::Enter => self.open_quality(term),
                KeyCode::Char('w') => self.toggle_watched(),
                c => {
                    let len = match (&self.details, self.selected_translation()) {
                        (Some(d), Some(ti)) => d.translations[ti].episodes.len(),
                        _ => 0,
                    };
                    if let Some(d) = nav_delta(c) {
                        move_sel(&mut self.episode_state, len, d);
                    }
                }
            },
            Screen::Quality => match key.code {
                KeyCode::Enter => self.play(term),
                c => {
                    let len = self.streams.as_ref().map_or(0, |(_, s)| s.variants.len());
                    if let Some(d) = nav_delta(c) {
                        move_sel(&mut self.quality_state, len, d);
                    }
                }
            },
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let [header, body, status, hint] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(f.area());
        let crumbs = match (&self.screen, &self.details) {
            (Screen::Search, _) => strings::SEARCH.to_owned(),
            (Screen::Library, _) => strings::LIBRARY.to_owned(),
            (_, Some(d)) => d.title.name.clone(),
            _ => String::new(),
        };
        f.render_widget(
            Line::from(vec![
                Span::styled(format!(" {} ", strings::APP_NAME), highlight()),
                Span::raw(format!(" {crumbs}")),
            ]),
            header,
        );
        match self.screen {
            Screen::Search => self.draw_search(f, body),
            Screen::Library => self.draw_library(f, body),
            Screen::Title => self.draw_title(f, body),
            Screen::Episodes => self.draw_episodes(f, body),
            Screen::Quality => self.draw_quality(f, body),
        }
        if let Some(m) = &self.message {
            f.render_widget(
                Paragraph::new(m.as_str()).style(Style::default().fg(Color::Yellow)),
                status,
            );
        }
        let hint_text = match self.screen {
            Screen::Search if self.editing => strings::HINT_SEARCH_EDIT,
            Screen::Search => strings::HINT_SEARCH,
            Screen::Library => strings::HINT_LIBRARY,
            Screen::Title => strings::HINT_TITLE,
            Screen::Episodes => strings::HINT_EPISODES,
            Screen::Quality => strings::HINT_QUALITY,
        };
        f.render_widget(
            Paragraph::new(hint_text).style(Style::default().fg(Color::DarkGray)),
            hint,
        );
    }

    fn draw_search(&mut self, f: &mut Frame, area: Rect) {
        let [input, list] =
            Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).areas(area);
        let border = if self.editing {
            Color::Cyan
        } else {
            Color::DarkGray
        };
        let text = if self.query.is_empty() && !self.editing {
            strings::SEARCH_PROMPT
        } else {
            &self.query
        };
        f.render_widget(
            Paragraph::new(text).block(
                Block::bordered()
                    .title(strings::SEARCH)
                    .border_style(Style::default().fg(border)),
            ),
            input,
        );
        if self.editing {
            f.set_cursor_position((input.x + 1 + self.query.chars().count() as u16, input.y + 1));
        }
        let items: Vec<ListItem> = self
            .results
            .iter()
            .map(|t| {
                let mut spans = vec![Span::raw(t.name.clone())];
                if let Some(y) = t.year {
                    spans.push(Span::styled(
                        format!(" ({y})"),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                if let Some(s) = self.library.status(&t.slug) {
                    spans.push(Span::styled(
                        format!("  [{}]", strings::status_label(s)),
                        Style::default().fg(Color::Green),
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        f.render_stateful_widget(
            List::new(items)
                .block(Block::bordered().title(strings::RESULTS))
                .highlight_style(highlight()),
            list,
            &mut self.results_state,
        );
    }

    fn draw_library(&mut self, f: &mut Frame, area: Rect) {
        let [tabs, list] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        let titles: Vec<String> = WatchStatus::ALL
            .iter()
            .map(|s| {
                format!(
                    "{} ({})",
                    strings::status_label(*s),
                    self.library.by_status(*s).len()
                )
            })
            .collect();
        f.render_widget(
            Tabs::new(titles)
                .select(self.tab)
                .highlight_style(highlight()),
            tabs,
        );
        let items: Vec<ListItem> = self
            .library
            .by_status(WatchStatus::ALL[self.tab])
            .into_iter()
            .map(|(_, e)| {
                ListItem::new(Line::from(vec![
                    Span::raw(e.name.clone()),
                    Span::styled(
                        format!(
                            "  {}",
                            strings::watched_progress(e.watched.len(), e.total_episodes)
                        ),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();
        let block = Block::bordered().title(strings::LIBRARY);
        if items.is_empty() {
            f.render_widget(Paragraph::new(strings::LIST_EMPTY).block(block), list);
        } else {
            f.render_stateful_widget(
                List::new(items).block(block).highlight_style(highlight()),
                list,
                &mut self.library_state,
            );
        }
    }

    fn draw_title(&mut self, f: &mut Frame, area: Rect) {
        let Some(d) = &self.details else { return };
        let [info, list] =
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .areas(area);
        let t = &d.title;
        let mut lines = vec![Line::styled(
            t.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )];
        if let Some(y) = t.year {
            lines.push(Line::raw(strings::year_label(y)));
        }
        if let Some(k) = &t.kind {
            lines.push(Line::raw(strings::type_label(k)));
        }
        lines.push(Line::raw(strings::episodes_progress(
            t.episodes_aired,
            t.episodes_total,
        )));
        lines.push(Line::styled(
            strings::list_status(self.library.status(&t.slug)),
            Style::default().fg(Color::Green),
        ));
        if !t.other_names.is_empty() {
            lines.push(Line::raw(strings::also_known_as(&t.other_names.join(", "))));
        }
        if let Some(desc) = &t.description {
            lines.push(Line::raw(""));
            lines.push(Line::raw(desc.clone()));
        }
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .block(Block::bordered().title(strings::DETAILS)),
            info,
        );
        let entry = self.library.get(&t.slug);
        let items: Vec<ListItem> = d
            .translations
            .iter()
            .map(|tr| {
                let watched = tr
                    .episodes
                    .iter()
                    .filter(|e| entry.is_some_and(|en| en.is_watched(&e.number)))
                    .count();
                ListItem::new(Line::from(vec![
                    Span::raw(tr.name.clone()),
                    Span::styled(
                        format!(
                            "  {} · {}",
                            strings::episode_count(tr.episodes.len()),
                            strings::watched_progress(watched, t.episodes_total)
                        ),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();
        f.render_stateful_widget(
            List::new(items)
                .block(Block::bordered().title(strings::TRANSLATIONS))
                .highlight_style(highlight()),
            list,
            &mut self.translation_state,
        );
    }

    fn draw_episodes(&mut self, f: &mut Frame, area: Rect) {
        let (Some(d), Some(ti)) = (&self.details, self.translation_state.selected()) else {
            return;
        };
        let tr = &d.translations[ti];
        let entry = self.library.get(&d.title.slug);
        let items: Vec<ListItem> = tr
            .episodes
            .iter()
            .map(|e| {
                let watched = entry.is_some_and(|en| en.is_watched(&e.number));
                let mark = if watched { "✓ " } else { "  " };
                let style = if watched {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default()
                };
                ListItem::new(Line::styled(
                    format!("{mark}{}", strings::episode_label(&e.number)),
                    style,
                ))
            })
            .collect();
        f.render_stateful_widget(
            List::new(items)
                .block(Block::bordered().title(format!("{} — {}", strings::EPISODES, tr.name)))
                .highlight_style(highlight()),
            area,
            &mut self.episode_state,
        );
    }

    fn draw_quality(&mut self, f: &mut Frame, area: Rect) {
        let Some((_, s)) = &self.streams else { return };
        let subs = if s.subtitles.is_empty() {
            strings::NO_SUBTITLES.to_owned()
        } else {
            strings::subtitles_count(s.subtitles.len())
        };
        let items: Vec<ListItem> = s
            .variants
            .iter()
            .map(|v| ListItem::new(strings::quality_label(v.quality)))
            .collect();
        f.render_stateful_widget(
            List::new(items)
                .block(Block::bordered().title(format!("{} · {subs}", strings::QUALITY)))
                .highlight_style(highlight()),
            area,
            &mut self.quality_state,
        );
    }
}

pub fn run(source: Box<dyn Source>, player: Box<dyn Player>, library: Library) -> Result<()> {
    let mut ui = Ui {
        source,
        player,
        library,
        screen: Screen::Search,
        editing: true,
        query: String::new(),
        results: Vec::new(),
        results_state: ListState::default(),
        tab: 0,
        library_state: ListState::default(),
        details: None,
        translation_state: ListState::default(),
        episode_state: ListState::default(),
        streams: None,
        quality_state: ListState::default(),
        back_to_library: false,
        message: None,
        quit: false,
    };
    let mut term = ratatui::init();
    let result = (|| -> Result<()> {
        while !ui.quit {
            term.draw(|f| ui.draw(f))?;
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                ui.handle(&mut term, key);
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}
