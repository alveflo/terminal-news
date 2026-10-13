# Terminapp

A terminal RSS/Atom reader with Hacker News, SVT Nyheter, and Dagens industri configured by default. Written in Rust with Ratatui. The repository is `alveflo/terminal-news`; the installed command is `terminapp`.

## Install

Install the latest release on Linux or macOS (x86_64 or ARM64), including Linux under WSL:

```sh
curl -fsSL https://raw.githubusercontent.com/alveflo/terminal-news/main/install.sh | sh
```

The installer verifies SHA-256 checksums and installs to `~/.local/bin` without sudo. Add that directory to your PATH if necessary:

```sh
export PATH="$HOME/.local/bin:$PATH"
terminapp
```

Run the installer again to upgrade. To select a version or destination:

```sh
curl -fsSL https://raw.githubusercontent.com/alveflo/terminal-news/main/install.sh \
  | TERMINAPP_VERSION=v0.1.0 TERMINAPP_INSTALL_DIR="$HOME/.local/bin" sh
```

You can also download `install.sh`, inspect it, and run `sh install.sh`. Release archives and `SHA256SUMS` are available on the [releases page](https://github.com/alveflo/terminal-news/releases). Linux binaries use musl so no matching system glibc version is needed.

To build from source, install Rust 1.96+ and run:

```sh
cargo run --release
```

The interface has a feed sidebar, searchable headlines, an article preview, and a Starred view. Startup uses the local cache and only fetches feeds that have never been cached. Press `r` or `R` to refresh all feeds in the background; there is no periodic polling or cache expiry. Cached articles remain available offline; read/unread and starred state persist between sessions. Feed summaries are shown as plain text. Full articles open in your browser and may require a publisher subscription.

| Key | Action |
| --- | --- |
| Tab / left / right | Switch between feeds and headlines |
| ↑ / ↓ or k / j | Select feed or article |
| / | Search titles and summaries in the selected feed |
| Enter / Esc while searching | Finish search / clear search |
| Esc | Clear active search |
| Enter / o | Open article in browser and mark read |
| m | Toggle read/unread |
| u | Toggle unread-only view |
| s / * | Save or unsave the selected article |
| g / G | Open Starred (also available in the sidebar) |
| Page Up / Page Down | Scroll article preview |
| r / R | Refresh all feeds, bypassing the local cache |
| ? | Show extra keyboard help |
| q | Quit |

Use a terminal at least 65 columns by 15 rows. On Windows, run inside WSL.

## Configure feeds

The first run creates `$XDG_CONFIG_HOME/terminapp/feeds.json` (default `~/.config/terminapp/feeds.json`). Manage it from the command line:

```sh
cargo run -- --list
cargo run -- --add "Another feed" "https://example.com/rss.xml"
cargo run -- --remove "https://example.com/rss.xml"
cargo run -- --check
python3 scripts/test_installer.py
```

Or edit the JSON file, then restart:

```json
{
  "feeds": [
    {"name": "Hacker News", "url": "https://news.ycombinator.com/rss"},
    {"name": "SVT Nyheter", "url": "https://www.svt.se/nyheter/rss.xml"},
    {"name": "Dagens industri", "url": "https://www.di.se/rss"}
  ]
}
```

Use `--config /path/to/feeds.json` with any command for another configuration. An empty feed list is supported. Cached articles, read IDs, and starred articles are saved per configuration under `$XDG_DATA_HOME/terminapp` (default `~/.local/share/terminapp`). Each successful refresh replaces that feed's cached headlines with its latest entries. Starred articles retain a separate copy of their title, summary, date, and link, even when they leave the feed or you remove its subscription. Full article pages are not downloaded. Failed refreshes retain cached data. Requests time out after 15 seconds and responses are limited to 5 MB. `--check` always makes live requests; it does not update the cache.

## Morot and API keys

The reader detects Morot's JSON response automatically. Add the endpoint with your key:

```sh
cargo run -- --add "Morot HN" "https://morot.app/api/hn" --api-key 'YOUR_API_KEY'
```

This saves the key in the local feed configuration and sends it only in the `api-key` header for that feed. `--list` does not display keys. For a key supplied through your environment instead:

```sh
export MOROT_API_KEY='YOUR_API_KEY'
cargo run -- --add "Morot HN" "https://morot.app/api/hn" --api-key-env MOROT_API_KEY
cargo run --release
```

Choose one of these commands when adding the feed. For an existing subscription, edit its JSON entry:

```json
{"name": "Morot HN", "url": "https://morot.app/api/hn", "api_key_env": "MOROT_API_KEY"}
```

Use either `api_key` or `api_key_env`. Restart after changing subscriptions. Authenticated feeds do not follow redirects; use the final endpoint URL. Morot items without an article URL open their Hacker News discussion instead. Null summaries are supported.

Install a standalone `terminapp` binary:

```sh
cargo install --path . --locked
```

## Development

```sh
cargo test --locked
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run -- --check
```

Existing Python-version JSON configurations and cached read state are reused automatically; no migration command is needed. The Rust application has no Python runtime dependency.

## Publishing a release

Update the package version in `Cargo.toml` and regenerate `Cargo.lock` with `cargo check`. Commit and push the change, then push its matching version tag:

```sh
git tag v0.1.0
git push origin v0.1.0
```

The Release workflow checks that the tag matches the package version, runs tests and lint checks, and builds native Linux and macOS binaries for x86_64 and ARM64. It publishes a GitHub release only after all four archives and their checksums are ready. The release also includes a copy of the installer. A failed run can be retried from the Actions page, or dispatched manually with an existing tag. Already-published releases are not overwritten; publish a new version instead. Publishing requires GitHub Actions to have permission to write repository contents.
