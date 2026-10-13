use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Feed {
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
}

impl Feed {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() || !valid_url(&self.url) {
            bail!("Each feed needs a name and an HTTP(S) URL");
        }
        if self.api_key.is_some() && self.api_key_env.is_some() {
            bail!("Use either api_key or api_key_env, not both");
        }
        if let Some(key) = &self.api_key
            && (key.is_empty() || reqwest::header::HeaderValue::from_str(key).is_err())
        {
            bail!("Invalid API key header value");
        }
        if self
            .api_key_env
            .as_ref()
            .is_some_and(|name| name.is_empty() || name.contains(['=', '\0']))
        {
            bail!("Invalid API key environment variable name");
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub feeds: Vec<Feed>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Article {
    pub id: String,
    pub source: String,
    pub title: String,
    pub url: String,
    pub summary: String,
    pub published: f64,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Store {
    pub articles: HashMap<String, Vec<Article>>,
    pub read: HashSet<String>,
    #[serde(default)]
    pub starred: HashMap<String, Article>,
}

pub fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn directory(variable: &str, fallback: &str) -> PathBuf {
    env::var_os(variable).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env::var_os("HOME").unwrap_or_else(|| ".".into())).join(fallback)
    })
}

pub fn config_path() -> PathBuf {
    directory("XDG_CONFIG_HOME", ".config").join("terminapp/feeds.json")
}

pub fn state_path(config: &Path) -> Result<PathBuf> {
    let absolute = fs::canonicalize(config)?;
    let key = hash(&absolute.to_string_lossy());
    Ok(directory("XDG_DATA_HOME", ".local/share").join(format!("terminapp/{}.json", &key[..16])))
}

pub fn valid_url(value: &str) -> bool {
    url::Url::parse(value)
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

pub fn save_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(file)?;
    file.as_file().sync_all()?;
    file.persist(path).context("Could not save file")?;
    Ok(())
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            let config = Self {
                feeds: vec![
                    Feed {
                        name: "Hacker News".into(),
                        url: "https://news.ycombinator.com/rss".into(),
                        ..Feed::default()
                    },
                    Feed {
                        name: "SVT Nyheter".into(),
                        url: "https://www.svt.se/nyheter/rss.xml".into(),
                        ..Feed::default()
                    },
                    Feed {
                        name: "Dagens industri".into(),
                        url: "https://www.di.se/rss".into(),
                        ..Feed::default()
                    },
                ],
            };
            save_json(path, &config)?;
        }
        let config: Self =
            serde_json::from_slice(&fs::read(path)?).context("Invalid feed configuration")?;
        let mut seen = HashSet::new();
        for feed in &config.feeds {
            feed.validate()?;
            if !seen.insert(&feed.url) {
                bail!("Duplicate feed URL: {}", feed.url);
            }
        }
        Ok(config)
    }
}

impl Store {
    pub fn load(path: &Path) -> (Self, String) {
        if !path.exists() {
            return (Self::default(), "Ready".into());
        }
        match fs::read(path)
            .context("Cannot read state")
            .and_then(|bytes| Ok(serde_json::from_slice::<Self>(&bytes)?))
        {
            Ok(mut store) => {
                for article in store
                    .articles
                    .values_mut()
                    .flatten()
                    .chain(store.starred.values_mut())
                {
                    if !valid_url(&article.url) {
                        article.url.clear();
                    }
                }
                (store, "Loaded local cache · R to refresh".into())
            }
            Err(error) => (
                Self::default(),
                format!("Could not load saved state: {error}"),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_validation_and_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("feeds.json");
        assert_eq!(Config::load(&path).unwrap().feeds.len(), 3);
        fs::write(
            &path,
            r#"{"feeds":[{"name":"bad","url":"file:///etc/passwd"}]}"#,
        )
        .unwrap();
        assert!(Config::load(&path).is_err());
        fs::write(&path, r#"{"feeds":[]}"#).unwrap();
        assert!(Config::load(&path).unwrap().feeds.is_empty());
    }
    #[test]
    fn python_state_compatibility_and_corrupt_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        fs::write(&path, r#"{"articles":{"https://example.com":[{"id":"one","source":"https://example.com","title":"Ränta","url":"https://example.com/1","summary":"Text","published":1791892800.0}]},"read":["one"]}"#).unwrap();
        let (store, _) = Store::load(&path);
        assert!(store.read.contains("one"));
        assert_eq!(store.articles["https://example.com"][0].title, "Ränta");
        save_json(&path, &store).unwrap();
        assert!(Store::load(&path).0.read.contains("one"));
        fs::write(&path, "invalid").unwrap();
        assert!(Store::load(&path).1.contains("Could not load"));
    }
}
