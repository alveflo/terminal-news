use crate::{
    feed,
    model::{Article, Feed, Store, save_json},
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
};

enum Update {
    Feed(String, Result<Vec<Article>, String>),
    Done,
}

pub struct App {
    pub feeds: Vec<Feed>,
    pub store: Store,
    pub path: PathBuf,
    pub feed: usize,
    pub selected: usize,
    pub scroll: u16,
    pub query: String,
    pub searching: bool,
    pub unread: bool,
    pub feed_focus: bool,
    pub help: bool,
    pub loading: bool,
    pub status: String,
    pub errors: HashMap<String, String>,
    updates: Option<Receiver<Update>>,
}

impl App {
    pub fn new(feeds: Vec<Feed>, store: Store, path: PathBuf, status: String) -> Self {
        Self {
            feeds,
            store,
            path,
            feed: 0,
            selected: 0,
            scroll: 0,
            query: String::new(),
            searching: false,
            unread: false,
            feed_focus: false,
            help: false,
            loading: false,
            status,
            errors: HashMap::new(),
            updates: None,
        }
    }

    pub fn articles(&self) -> Vec<&Article> {
        let query = self.query.to_lowercase();
        let candidates: Vec<_> = if self.feed == 1 {
            self.store.starred.values().collect()
        } else {
            self.feeds
                .iter()
                .enumerate()
                .filter(|(i, _)| self.feed == 0 || self.feed == i + 2)
                .flat_map(|(_, f)| self.store.articles.get(&f.url).into_iter().flatten())
                .collect()
        };
        let mut items: Vec<_> = candidates
            .into_iter()
            .filter(|a| {
                (!self.unread || !self.store.read.contains(&a.id))
                    && (a.title.to_lowercase().contains(&query)
                        || a.summary.to_lowercase().contains(&query))
            })
            .collect();
        items.sort_by(|a, b| b.published.total_cmp(&a.published).then(a.id.cmp(&b.id)));
        items
    }

    pub fn save(&mut self) {
        if let Err(e) = save_json(&self.path, &self.store) {
            self.status = format!("Cannot save state: {e}");
        }
    }

    pub fn startup(&mut self) {
        let missing = self
            .feeds
            .iter()
            .filter(|f| !self.store.articles.contains_key(&f.url))
            .cloned()
            .collect();
        self.refresh_feeds(missing);
    }

    pub fn refresh(&mut self) {
        self.refresh_feeds(self.feeds.clone());
    }

    fn refresh_feeds(&mut self, feeds: Vec<Feed>) {
        if self.loading || feeds.is_empty() {
            return;
        }
        let client = match feed::client() {
            Ok(client) => client,
            Err(e) => {
                self.status = format!("Cannot create HTTP client: {e}");
                return;
            }
        };
        self.loading = true;
        self.errors.clear();
        self.status = "Refreshing feeds…".into();
        let (tx, rx) = mpsc::channel();
        self.updates = Some(rx);
        thread::spawn(move || {
            thread::scope(|scope| {
                for offset in 0..4 {
                    let (feeds, client, tx) = (&feeds, &client, &tx);
                    scope.spawn(move || {
                        for f in feeds.iter().skip(offset).step_by(4) {
                            let result =
                                feed::fetch(client, f).map_err(|e| format!("{}: {e:#}", f.name));
                            if tx.send(Update::Feed(f.url.clone(), result)).is_err() {
                                break;
                            }
                        }
                    });
                }
            });
            let _ = tx.send(Update::Done);
        });
    }

    pub fn drain(&mut self) {
        let updates: Vec<_> = self
            .updates
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default();
        for update in updates {
            match update {
                Update::Feed(url, Ok(items)) => {
                    self.store.articles.insert(url, items);
                }
                Update::Feed(url, Err(error)) => {
                    self.errors.insert(url, error);
                }
                Update::Done => {
                    self.loading = false;
                    self.updates = None;
                    self.status = if self.errors.is_empty() {
                        "Refresh complete".into()
                    } else {
                        format!(
                            "Cached articles retained · {}",
                            self.errors.values().cloned().collect::<Vec<_>>().join("; ")
                        )
                    };
                    self.save();
                }
            }
        }
        self.selected = self.selected.min(self.articles().len().saturating_sub(1));
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return false;
        }
        if self.searching {
            match key.code {
                KeyCode::Enter => self.searching = false,
                KeyCode::Esc => {
                    self.searching = false;
                    self.query.clear();
                }
                KeyCode::Backspace => {
                    self.query.pop();
                }
                KeyCode::Char(c) => self.query.push(c),
                _ => {}
            }
            self.selected = 0;
            self.scroll = 0;
            return true;
        }
        match key.code {
            KeyCode::Char('q') => return false,
            KeyCode::Tab => self.feed_focus = !self.feed_focus,
            KeyCode::Left | KeyCode::Char('h') => self.feed_focus = true,
            KeyCode::Right | KeyCode::Char('l') => self.feed_focus = false,
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
                let up = matches!(key.code, KeyCode::Up | KeyCode::Char('k'));
                if self.feed_focus {
                    self.feed = if up {
                        self.feed.saturating_sub(1)
                    } else {
                        (self.feed + 1).min(self.feeds.len() + 1)
                    };
                    self.selected = 0;
                } else {
                    self.selected = if up {
                        self.selected.saturating_sub(1)
                    } else {
                        (self.selected + 1).min(self.articles().len().saturating_sub(1))
                    };
                }
                self.scroll = 0;
            }
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(5),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(5),
            KeyCode::Char('/') => self.searching = true,
            KeyCode::Esc => {
                self.query.clear();
                self.selected = 0;
                self.scroll = 0;
            }
            KeyCode::Char('u') => {
                self.unread = !self.unread;
                self.selected = 0;
                self.scroll = 0;
            }
            KeyCode::Char('r' | 'R') => self.refresh(),
            KeyCode::Char('g' | 'G') => {
                self.feed = 1;
                self.selected = 0;
                self.scroll = 0;
                self.query.clear();
                self.unread = false;
                self.feed_focus = false;
            }
            KeyCode::Char('s' | '*') => {
                if let Some(article) = self.articles().get(self.selected).map(|a| (*a).clone()) {
                    if self.store.starred.remove(&article.id).is_some() {
                        self.status = "Removed from Starred".into();
                    } else {
                        self.store.starred.insert(article.id.clone(), article);
                        self.status = "Saved to Starred · g to view".into();
                    }
                    self.selected = self.selected.min(self.articles().len().saturating_sub(1));
                    self.scroll = 0;
                    self.save();
                }
            }
            KeyCode::Char('?') => self.help = !self.help,
            KeyCode::Char('m') | KeyCode::Char('o') | KeyCode::Enter => {
                if let Some(article) = self.articles().get(self.selected).map(|a| (*a).clone()) {
                    if key.code == KeyCode::Char('m') {
                        if !self.store.read.remove(&article.id) {
                            self.store.read.insert(article.id);
                        }
                    } else if article.url.is_empty() {
                        self.status = "This article has no HTTP(S) link".into();
                    } else {
                        match webbrowser::open(&article.url) {
                            Ok(()) => {
                                self.store.read.insert(article.id);
                                self.status = "Opened in browser".into();
                            }
                            Err(e) => {
                                self.status = format!("Cannot open browser: {e} · {}", article.url)
                            }
                        }
                    }
                    self.save();
                }
            }
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stars_survive_refresh_restart_and_subscription_removal() {
        let dir = tempfile::tempdir().unwrap();
        let feed = Feed {
            name: "Example".into(),
            url: "https://example.com/rss".into(),
            ..Feed::default()
        };
        let article = Article {
            id: "saved".into(),
            source: feed.url.clone(),
            title: "Saved headline".into(),
            url: "https://example.com/story".into(),
            summary: "Saved summary".into(),
            published: 1.0,
        };
        let mut store = Store::default();
        store.articles.insert(feed.url.clone(), vec![article]);
        let path = dir.path().join("state.json");
        let mut app = App::new(vec![feed.clone()], store, path.clone(), String::new());
        press(&mut app, KeyCode::Char('s'));
        let (tx, rx) = mpsc::channel();
        app.updates = Some(rx);
        tx.send(Update::Feed(feed.url, Ok(vec![]))).unwrap();
        tx.send(Update::Done).unwrap();
        app.drain();
        assert!(app.articles().is_empty());
        let (store, status) = Store::load(&path);
        let mut app = App::new(vec![], store, path.clone(), status);
        press(&mut app, KeyCode::Char('g'));
        assert_eq!(app.articles()[0].summary, "Saved summary");
        press(&mut app, KeyCode::Char('m'));
        assert!(app.store.read.contains("saved"));
        press(&mut app, KeyCode::Char('*'));
        assert!(app.articles().is_empty());
        assert!(Store::load(&path).0.starred.is_empty());
    }

    #[test]
    fn cached_startup_skips_http_and_uppercase_r_refreshes() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            time::{Duration, Instant},
        };
        let dir = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let feed = Feed {
            name: "Local".into(),
            url: format!("http://{}/rss", listener.local_addr().unwrap()),
            ..Feed::default()
        };
        let mut store = Store::default();
        // An empty but successfully fetched feed is still cached.
        store.articles.insert(feed.url.clone(), vec![]);
        let mut app = App::new(
            vec![feed],
            store,
            dir.path().join("state.json"),
            String::new(),
        );
        app.startup();
        assert!(!app.loading);
        assert!(app.updates.is_none());
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let _ = stream.read(&mut request);
            let body = r#"{"articles":[{"id":1,"title":"Fresh","url":"https://example.com"}]}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        press(&mut app, KeyCode::Char('R'));
        assert!(app.loading);
        // Repeated refresh keys do not start additional workers.
        press(&mut app, KeyCode::Char('r'));
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.loading && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            app.drain();
        }
        assert!(!app.loading);
        assert_eq!(app.articles()[0].title, "Fresh");
        assert_eq!(
            Store::load(&app.path)
                .0
                .articles
                .values()
                .next()
                .unwrap()
                .len(),
            1
        );
        server.join().unwrap();
    }
    fn press(app: &mut App, key: KeyCode) {
        app.key(KeyEvent::new(key, KeyModifiers::NONE));
    }
    #[test]
    fn search_read_filter_and_failed_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let f = Feed {
            name: "Example".into(),
            url: "https://example.com".into(),
            ..Feed::default()
        };
        let a = Article {
            id: "one".into(),
            source: f.url.clone(),
            title: "Ränta".into(),
            url: f.url.clone(),
            summary: "Economy".into(),
            published: 1.0,
        };
        let mut store = Store::default();
        store.articles.insert(f.url.clone(), vec![a]);
        let mut app = App::new(
            vec![f.clone()],
            store,
            dir.path().join("state.json"),
            String::new(),
        );
        press(&mut app, KeyCode::Char('/'));
        for c in "RÄNTA".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.articles().len(), 1);
        press(&mut app, KeyCode::Char('m'));
        assert!(Store::load(&app.path).0.read.contains("one"));
        press(&mut app, KeyCode::Char('u'));
        assert!(app.articles().is_empty());
        press(&mut app, KeyCode::Char('m'));
        press(&mut app, KeyCode::Char('u'));
        let (tx, rx) = mpsc::channel();
        app.updates = Some(rx);
        tx.send(Update::Feed(f.url, Err("offline".into()))).unwrap();
        tx.send(Update::Done).unwrap();
        app.drain();
        assert_eq!(app.articles().len(), 1);
        assert!(app.status.contains("offline"));
    }
}
