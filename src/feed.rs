use crate::model::{Article, Feed, hash, valid_url};
use anyhow::{Context, Result, bail};
use reqwest::blocking::Client;
use scraper::{Html, Node};
use std::{collections::HashSet, io::Read, time::Duration};

const MAX_BYTES: u64 = 5_000_000;

pub fn plain(value: &str) -> String {
    let html = Html::parse_fragment(value);
    let mut text = String::new();
    for node in html.tree.nodes() {
        if let Node::Text(part) = node.value() {
            let hidden = node.ancestors().any(
                |a| matches!(a.value(), Node::Element(e) if matches!(e.name(), "script" | "style")),
            );
            if !hidden {
                text.push_str(part);
                text.push(' ');
            }
        }
    }
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

pub fn parse(payload: &[u8], source: &str) -> Result<Vec<Article>> {
    if payload.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{') {
        return parse_morot(payload, source);
    }
    // Preserve explicit feed IDs exactly for compatibility with the Python cache.
    let feed = feed_rs::parser::Builder::new()
        .id_generator(|_, _, _| String::new())
        .build()
        .parse(payload)?;
    let base = url::Url::parse(source)?;
    let mut seen = HashSet::new();
    let mut articles = Vec::new();
    for entry in feed.entries {
        let title = entry
            .title
            .map(|t| plain(&t.content))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Untitled".into());
        let link = entry
            .links
            .iter()
            .find(|l| l.rel.as_deref().is_none_or(|r| r == "alternate"))
            .and_then(|l| base.join(&l.href).ok())
            .map(|u| u.to_string())
            .filter(|u| valid_url(u))
            .unwrap_or_default();
        let identity = if !entry.id.is_empty() {
            &entry.id
        } else if !link.is_empty() {
            &link
        } else {
            &title
        };
        let id = hash(&format!("{source}\n{identity}"));
        if !seen.insert(id.clone()) {
            continue;
        }
        let summary = entry
            .content
            .and_then(|c| c.body)
            .or_else(|| entry.summary.map(|s| s.content))
            .unwrap_or_default();
        articles.push(Article {
            id,
            source: source.into(),
            title,
            url: link,
            summary: plain(&summary),
            published: entry
                .published
                .or(entry.updated)
                .map(|d| d.timestamp() as f64)
                .unwrap_or_default(),
        });
        if articles.len() == 500 {
            break;
        }
    }
    Ok(articles)
}

pub fn client() -> Result<Client> {
    Ok(Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Terminapp/0.1 RSS reader")
        .build()?)
}

pub fn fetch(client: &Client, feed: &Feed) -> Result<Vec<Article>> {
    let key = match &feed.api_key_env {
        Some(name) => Some(
            std::env::var(name)
                .with_context(|| format!("Missing API key environment variable: {name}"))?,
        ),
        None => feed.api_key.clone(),
    };
    // Custom API keys must never be forwarded by an automatic redirect.
    let authenticated;
    let client = if key.is_some() {
        authenticated = Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("Terminapp/0.1 RSS reader")
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        &authenticated
    } else {
        client
    };
    let mut request = client.get(&feed.url).header(
        "Accept",
        "application/json, application/rss+xml, application/atom+xml, application/xml, text/xml",
    );
    if let Some(key) = key {
        if key.is_empty() {
            bail!("API key is empty");
        }
        let mut value =
            reqwest::header::HeaderValue::from_str(&key).context("Invalid API key header value")?;
        value.set_sensitive(true);
        request = request.header("api-key", value);
    }
    let response = request.send()?.error_for_status()?;
    if response.status().is_redirection() {
        bail!("Authenticated feed redirected; configure its final URL directly");
    }
    let mut bytes = Vec::new();
    response.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        bail!("Feed exceeds the 5 MB size limit");
    }
    parse(&bytes, &feed.url)
}

#[derive(serde::Deserialize)]
struct MorotFeed {
    articles: Vec<MorotArticle>,
}

#[derive(serde::Deserialize)]
struct MorotArticle {
    id: serde_json::Value,
    title: String,
    url: Option<String>,
    summary: Option<String>,
    published_at: Option<String>,
    discussion_url: Option<String>,
}

fn parse_morot(payload: &[u8], source: &str) -> Result<Vec<Article>> {
    let feed: MorotFeed = serde_json::from_slice(payload).context("Invalid Morot feed JSON")?;
    let mut seen = HashSet::new();
    let mut articles = Vec::new();
    for entry in feed.articles {
        let identity = match entry.id {
            serde_json::Value::String(id) if !id.is_empty() => id,
            serde_json::Value::Number(id) => id.to_string(),
            _ => bail!("Morot article has an invalid ID"),
        };
        let id = hash(&format!("{source}\n{identity}"));
        if !seen.insert(id.clone()) {
            continue;
        }
        let title = plain(&entry.title);
        let url = entry
            .url
            .filter(|u| valid_url(u))
            .or_else(|| entry.discussion_url.filter(|u| valid_url(u)))
            .unwrap_or_default();
        articles.push(Article {
            id,
            source: source.into(),
            title: if title.is_empty() {
                "Untitled".into()
            } else {
                title
            },
            url,
            summary: plain(entry.summary.as_deref().unwrap_or("")),
            published: entry
                .published_at
                .and_then(|date| chrono::DateTime::parse_from_rfc3339(&date).ok())
                .map(|date| date.timestamp() as f64)
                .unwrap_or_default(),
        });
        if articles.len() == 500 {
            break;
        }
    }
    Ok(articles)
}

#[cfg(test)]
mod tests {
    use super::*;
    const MOROT: &str = r#"{"articles":[{"id":42,"title":"Ränta &amp; ekonomi","url":null,"summary":null,"published_at":"2026-10-13T16:03:16.000Z","discussion_url":"https://news.ycombinator.com/item?id=42"},{"id":43,"title":"Another story","url":"https://example.com/story","summary":"<p>A summary</p>","published_at":null}]}"#;

    #[test]
    fn morot_json_nullable_fields_and_stable_ids() {
        let articles = parse(MOROT.as_bytes(), "https://morot.app/api/hn").unwrap();
        assert_eq!(articles.len(), 2);
        assert_eq!(articles[0].title, "Ränta & ekonomi");
        assert_eq!(articles[0].url, "https://news.ycombinator.com/item?id=42");
        assert!(articles[0].summary.is_empty());
        assert!(articles[0].published > 0.0);
        assert_eq!(articles[1].summary, "A summary");
        let changed = MOROT.replace("Another story", "Updated title");
        assert_eq!(
            articles[1].id,
            parse(changed.as_bytes(), "https://morot.app/api/hn").unwrap()[1].id
        );
        assert!(parse(br#"{"error":"Unauthorized"}"#, "https://morot.app/api/hn").is_err());
        assert!(
            parse(br#"{"articles":[]}"#, "https://morot.app/api/hn")
                .unwrap()
                .is_empty()
        );
        let unsafe_url = MOROT.replace("https://example.com/story", "javascript:alert(1)");
        assert!(
            parse(unsafe_url.as_bytes(), "https://morot.app/api/hn").unwrap()[1]
                .url
                .is_empty()
        );
    }

    #[test]
    fn authenticated_json_fetch_and_redirect_protection() {
        use std::{io::Write, net::TcpListener};
        for status in ["200 OK", "302 Found", "401 Unauthorized"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/hn", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                assert!(String::from_utf8_lossy(&request).contains("api-key: test-secret"));
                write!(stream, "HTTP/1.1 {status}\r\nLocation: http://127.0.0.1:1/elsewhere\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{MOROT}", MOROT.len()).unwrap();
            });
            let feed = Feed {
                url,
                api_key: Some("test-secret".into()),
                ..Feed::default()
            };
            let result = fetch(&client().unwrap(), &feed);
            if status.starts_with("200") {
                assert_eq!(result.unwrap().len(), 2);
            } else {
                let error = format!("{:#}", result.unwrap_err());
                assert!(!error.contains("test-secret"));
                if status.starts_with("302") {
                    assert!(error.contains("redirected"));
                }
            }
            server.join().unwrap();
        }
    }
    const RSS: &str = r#"<rss version="2.0"><channel><title>Feed</title><link>https://example.com</link><description>Feed</description><item><guid isPermaLink="false">one</guid><title>Ränta &amp; ekonomi</title><link>https://example.com/story</link><description>&lt;p&gt;Hello &lt;b&gt;world&lt;/b&gt;&lt;/p&gt;</description><pubDate>Tue, 13 Oct 2026 12:00:00 GMT</pubDate></item></channel></rss>"#;
    #[test]
    fn rss_unicode_dates_and_python_identity() {
        let articles = parse(RSS.as_bytes(), "https://example.com/rss").unwrap();
        let a = &articles[0];
        assert_eq!(a.title, "Ränta & ekonomi");
        assert_eq!(a.summary, "Hello world");
        assert!(a.published > 0.0);
        assert_eq!(a.id, hash("https://example.com/rss\none"));
    }
    #[test]
    fn missing_guid_uses_python_link_identity() {
        let xml = RSS.replace(r#"<guid isPermaLink="false">one</guid>"#, "");
        let articles = parse(xml.as_bytes(), "https://example.com/rss").unwrap();
        assert_eq!(
            articles[0].id,
            hash("https://example.com/rss\nhttps://example.com/story")
        );
    }

    #[test]
    fn http_fetch_and_error_status() {
        use std::{io::Write, net::TcpListener};
        for status in ["200 OK", "503 Service Unavailable"] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/rss", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request);
                write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{RSS}",
                    RSS.len()
                )
                .unwrap();
            });
            let http = Client::builder().no_proxy().build().unwrap();
            let result = fetch(
                &http,
                &Feed {
                    url,
                    ..Feed::default()
                },
            );
            assert_eq!(result.is_ok(), status.starts_with("200"));
            server.join().unwrap();
        }
    }

    #[test]
    #[ignore = "requires access to public feed endpoints"]
    fn live_default_feeds() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::model::Config::load(&dir.path().join("feeds.json")).unwrap();
        let http = client().unwrap();
        for feed in config.feeds {
            let articles = fetch(&http, &feed).unwrap();
            assert!(!articles.is_empty(), "{} was empty", feed.name);
            println!("{}: {} articles", feed.name, articles.len());
        }
    }
    #[test]
    fn atom_relative_links_and_deduplication() {
        let entry = r#"<entry><id>one</id><title>Atom</title><link rel="self" href="/api"/><link href="/story"/><updated>2026-10-13T12:00:00Z</updated></entry>"#;
        let xml = format!(
            r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Feed</title><id>feed</id><updated>2026-10-13T12:00:00Z</updated>{entry}{entry}</feed>"#
        );
        let articles = parse(xml.as_bytes(), "https://example.com/rss").unwrap();
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0].url, "https://example.com/story");
    }
    #[test]
    fn unsafe_content_and_invalid_documents() {
        assert_eq!(plain("<script>bad()</script><p>Fine</p>"), "Fine");
        assert!(parse(b"<html>Denied</html>", "https://example.com").is_err());
        let xml = RSS.replace("https://example.com/story", "javascript:alert(1)");
        assert!(
            parse(xml.as_bytes(), "https://example.com").unwrap()[0]
                .url
                .is_empty()
        );
    }
}
