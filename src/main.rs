mod app;
mod feed;
mod model;
mod ui;

use anyhow::{Result, bail};
use clap::Parser;
use model::{Config, Feed, Store, save_json};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
};

#[derive(Parser)]
#[command(version, about = "Terminapp — terminal RSS and Atom reader")]
#[command(group(clap::ArgGroup::new("action").args(["list", "add", "remove", "check"])))]
struct Args {
    /// Feed configuration JSON path
    #[arg(long, default_value_os_t = model::config_path())]
    config: PathBuf,
    /// List configured feeds
    #[arg(long)]
    list: bool,
    /// Add a feed
    #[arg(long, num_args = 2, value_names = ["NAME", "URL"])]
    add: Option<Vec<String>>,
    /// API key stored with the feed (sent in the api-key header)
    #[arg(long, requires = "add", conflicts_with_all = ["api_key_env", "list", "remove", "check"])]
    api_key: Option<String>,
    /// Read the api-key header from this environment variable at fetch time
    #[arg(long, requires = "add", conflicts_with_all = ["list", "remove", "check"])]
    api_key_env: Option<String>,
    /// Remove a feed by exact URL
    #[arg(long, value_name = "URL")]
    remove: Option<String>,
    /// Fetch each feed and report health without opening the TUI
    #[arg(long)]
    check: bool,
}

fn run(args: Args) -> Result<()> {
    let mut config = Config::load(&args.config)?;
    if let Some(add) = args.add {
        let (name, url) = (&add[0], &add[1]);
        if name.trim().is_empty() || !model::valid_url(url) {
            bail!("Use a nonempty name and an HTTP(S) URL");
        }
        if config.feeds.iter().any(|f| &f.url == url) {
            bail!("That feed URL is already configured");
        }
        let feed = Feed {
            name: name.clone(),
            url: url.clone(),
            api_key: args.api_key,
            api_key_env: args.api_key_env,
        };
        feed.validate()?;
        config.feeds.push(feed);
        save_json(&args.config, &config)?;
        println!("Added {name}");
    } else if let Some(url) = args.remove {
        let count = config.feeds.len();
        config.feeds.retain(|f| f.url != url);
        if count == config.feeds.len() {
            bail!("No feed has that URL");
        }
        save_json(&args.config, &config)?;
        println!("Feed removed");
    } else if args.list {
        println!("Configuration: {}", args.config.display());
        for f in config.feeds {
            println!("{}: {}", f.name, f.url);
        }
    } else if args.check {
        let client = feed::client()?;
        let mut failed = false;
        for f in config.feeds {
            match feed::fetch(&client, &f) {
                Ok(items) => println!("OK  {}: {} articles", f.name, items.len()),
                Err(e) => {
                    failed = true;
                    eprintln!("FAIL  {}: {e:#}", f.name);
                }
            }
        }
        if failed {
            bail!("One or more feeds could not be fetched");
        }
    } else {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            bail!("Run in an interactive terminal, or use --check / --list");
        }
        let path = model::state_path(&args.config)?;
        let (store, status) = Store::load(&path);
        ui::run(&mut app::App::new(config.feeds, store, path, status))?;
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run(Args::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("terminapp: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
