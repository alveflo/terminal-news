use crate::app::App;
use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use std::time::Duration;

fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .collect()
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 65 || area.height < 15 {
        frame.render_widget(
            Paragraph::new("Resize terminal to at least 65 × 15. Press q to quit."),
            area,
        );
        return;
    }
    let accent = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(6),
        Constraint::Length(if app.help { 5 } else { 3 }),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(format!(
            " TERMINAPP / your daily signal\n RSS READER · {}",
            if app.loading {
                "refreshing…"
            } else {
                "local cache · R to refresh"
            }
        ))
        .style(accent),
        rows[0],
    );
    let columns = Layout::horizontal([
        Constraint::Length(27.min(area.width / 3)),
        Constraint::Min(20),
    ])
    .split(rows[1]);
    let mut feeds = vec![ListItem::new(format!(
        "All feeds ({})",
        app.feeds
            .iter()
            .flat_map(|f| app.store.articles.get(&f.url).into_iter().flatten())
            .filter(|a| !app.store.read.contains(&a.id))
            .count()
    ))];
    feeds.push(ListItem::new(format!(
        "★ Starred ({})",
        app.store.starred.len()
    )));
    for feed in &app.feeds {
        let count = app
            .store
            .articles
            .get(&feed.url)
            .into_iter()
            .flatten()
            .filter(|a| !app.store.read.contains(&a.id))
            .count();
        feeds.push(ListItem::new(format!(
            "{}{} ({count})",
            if app.errors.contains_key(&feed.url) {
                "! "
            } else {
                ""
            },
            clean(&feed.name)
        )));
    }
    let selected_style = Style::default().bg(Color::DarkGray).fg(Color::White);
    let sidebar = List::new(feeds)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(if app.feed_focus {
                    " FEEDS ◀ "
                } else {
                    " FEEDS "
                }),
        )
        .highlight_style(selected_style);
    frame.render_stateful_widget(
        sidebar,
        columns[0],
        &mut ListState::default().with_selected(Some(app.feed)),
    );
    let right = Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(columns[1]);
    let articles = app.articles();
    let selected = app.selected.min(articles.len().saturating_sub(1));
    let title = format!(
        " {} / {} {} ",
        if app.feed == 1 {
            "Starred"
        } else if app.unread {
            "Unread"
        } else {
            "Headlines"
        },
        articles.len(),
        clean(&app.query)
    );
    let items: Vec<_> = articles
        .iter()
        .map(|a| {
            ListItem::new(format!(
                "{}{} {}",
                if app.store.starred.contains_key(&a.id) {
                    "★ "
                } else {
                    ""
                },
                if app.store.read.contains(&a.id) {
                    "·"
                } else {
                    "●"
                },
                clean(&a.title)
            ))
        })
        .collect();
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new(if app.feed == 1 {
                "No saved items match. Press s on an article to star it."
            } else if app.feeds.is_empty() {
                "No feeds configured. Use --add NAME URL."
            } else {
                "No articles match. Try r to refresh or Esc to clear search."
            })
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(title)),
            right[0],
        );
    } else {
        frame.render_stateful_widget(
            List::new(items)
                .block(Block::bordered().title(title))
                .highlight_style(selected_style),
            right[0],
            &mut ListState::default().with_selected(Some(selected)),
        );
    }
    if let Some(article) = articles.get(selected) {
        let name = app
            .feeds
            .iter()
            .find(|f| f.url == article.source)
            .map(|f| f.name.as_str())
            .unwrap_or(&article.source);
        let date = if article.published == 0.0 {
            None
        } else {
            chrono::DateTime::from_timestamp(article.published as i64, 0)
        }
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%d %b %Y %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "Date unavailable".into());
        let text = format!(
            "{} · {date}\n\n{}\n\n{}\n\n{}",
            clean(name),
            clean(&article.title),
            if article.summary.is_empty() {
                "No summary provided. Press o to open the original article.".into()
            } else {
                clean(&article.summary)
            },
            clean(&article.url)
        );
        let preview = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::bordered().title(" ARTICLE "));
        let max_scroll = preview
            .line_count(right[1].width)
            .saturating_sub(usize::from(right[1].height.saturating_sub(2)))
            .min(u16::MAX as usize) as u16;
        app.scroll = app.scroll.min(max_scroll);
        frame.render_widget(preview.scroll((app.scroll, 0)), right[1]);
    } else {
        frame.render_widget(Block::bordered().title(" ARTICLE "), right[1]);
    }
    let footer = if app.searching {
        format!("Search: {}▏", clean(&app.query))
    } else {
        "q quit  ? help  Tab pane  / search  s star  g starred  R refresh".into()
    };
    let help = if app.help {
        "\nm read/unread · u unread only · PgUp/PgDn preview · s/* star\nEnter/o open · Esc clear search · h/l panes · r/R refresh all"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(format!("{}\n{footer}{help}", clean(&app.status))).style(accent),
        rows[2],
    );
}

pub fn run(app: &mut App) -> Result<()> {
    let mut terminal = ratatui::init();
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            ratatui::restore();
        }
    }
    let _restore = Restore;
    app.startup();
    let result = (|| -> Result<()> {
        loop {
            app.drain();
            terminal.draw(|frame| draw(frame, app))?;
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
                && key.kind != KeyEventKind::Release
                && !app.key(key)
            {
                break;
            }
        }
        Ok(())
    })();
    crate::model::save_json(&app.path, &app.store)?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Store;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn renders_normal_and_tiny_terminals() {
        let mut app = App::new(
            vec![],
            Store::default(),
            "unused.json".into(),
            "Ready".into(),
        );
        for (width, height) in [(80, 24), (65, 15), (20, 5)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(text.contains(if width < 65 { "Resize" } else { "TERMINAPP" }));
        }
    }
}
