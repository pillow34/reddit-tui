use std::{io, sync::mpsc::Receiver, time::Duration};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal};
use reddit_tui::{
    app::{self, App, RecentDirection, Sort},
    error,
    events::{FetchCommand, FetchEvent},
    reddit_client::RedditWorker,
    ui,
};

fn main() -> anyhow::Result<()> {
    let mut tui = Tui::enter()?;
    let mut app = App::new();
    let (worker, results) = RedditWorker::spawn()?;
    let result = run(tui.terminal_mut(), &mut app, &worker, &results);

    if let Err(error) = result {
        eprintln!("Error: {error}");
    }

    Ok(())
}

struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl Tui {
    fn enter() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        Ok(Self { terminal })
    }

    fn terminal_mut(&mut self) -> &mut Terminal<CrosstermBackend<io::Stdout>> {
        &mut self.terminal
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        );
        let _ = self.terminal.show_cursor();
    }
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    worker: &RedditWorker,
    results: &Receiver<FetchEvent>,
) -> anyhow::Result<()> {
    let mut needs_redraw = true;

    loop {
        if drain_fetch_events(app, results) {
            needs_redraw = true;
        }
        if sync_preview_media(app, worker)? {
            needs_redraw = true;
        }
        if needs_redraw {
            terminal.draw(|frame| ui::draw(frame, app))?;
            needs_redraw = false;
        }

        if app.should_quit {
            break;
        }

        let poll_timeout = next_poll_timeout(app);
        if !event::poll(poll_timeout)? {
            if app.preview_animation_delay().is_some() {
                needs_redraw = true;
            }
            continue;
        }

        match event::read()? {
            Event::Key(key) => {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                let action = map_key_to_action(app, key.code, key.modifiers);
                update(app, action, terminal, worker)?;
                needs_redraw = true;
            }
            Event::Resize(_, _) => {
                update(app, AppAction::Resize, terminal, worker)?;
                needs_redraw = true;
            }
            _ => {}
        }
    }

    Ok(())
}

enum AppAction {
    None,
    Quit,
    Resize,
    ToggleHelp,
    DismissHelp,
    InputChar(char),
    InputBackspace,
    SubmitSubreddit,
    CancelInput,
    RecentPrevious,
    RecentNext,
    PostUp,
    PostDown,
    PostPageUp,
    PostPageDown,
    CommentUp,
    CommentDown,
    CommentPageUp,
    CommentPageDown,
    OpenComments,
    CycleSort,
    SetSort(Sort),
    ShowSubredditInput,
    RefreshPosts,
    RefreshComments,
    Back,
}

fn map_key_to_action(app: &App, code: KeyCode, modifiers: KeyModifiers) -> AppAction {
    if matches!(code, KeyCode::Char('?')) {
        return AppAction::ToggleHelp;
    }

    if app.help_visible {
        return match code {
            KeyCode::Esc | KeyCode::Char('?') => AppAction::DismissHelp,
            _ => AppAction::None,
        };
    }

    match app.screen {
        app::Screen::SubredditInput => match code {
            KeyCode::Char('q') if modifiers.is_empty() => AppAction::Quit,
            KeyCode::Char(c) if modifiers.is_empty() => AppAction::InputChar(c),
            KeyCode::Backspace => AppAction::InputBackspace,
            KeyCode::Enter => AppAction::SubmitSubreddit,
            KeyCode::Esc => AppAction::CancelInput,
            KeyCode::Tab => AppAction::RecentPrevious,
            KeyCode::BackTab => AppAction::RecentNext,
            _ => AppAction::None,
        },
        app::Screen::PostList => match code {
            KeyCode::Char('q') => AppAction::Quit,
            KeyCode::Esc => AppAction::Back,
            KeyCode::Up | KeyCode::Char('k') => AppAction::PostUp,
            KeyCode::Down | KeyCode::Char('j') => AppAction::PostDown,
            KeyCode::PageUp | KeyCode::Char('u') => AppAction::PostPageUp,
            KeyCode::PageDown | KeyCode::Char('d') => AppAction::PostPageDown,
            KeyCode::Enter => AppAction::OpenComments,
            KeyCode::Char('s') => AppAction::CycleSort,
            KeyCode::Char('/') => AppAction::ShowSubredditInput,
            KeyCode::Char('r') => AppAction::RefreshPosts,
            KeyCode::Char(digit) => Sort::from_shortcut(digit)
                .map(AppAction::SetSort)
                .unwrap_or(AppAction::None),
            _ => AppAction::None,
        },
        app::Screen::Comments => match code {
            KeyCode::Char('q') => AppAction::Quit,
            KeyCode::Esc | KeyCode::Char('b') => AppAction::Back,
            KeyCode::Up | KeyCode::Char('k') => AppAction::CommentUp,
            KeyCode::Down | KeyCode::Char('j') => AppAction::CommentDown,
            KeyCode::PageUp | KeyCode::Char('u') => AppAction::CommentPageUp,
            KeyCode::PageDown | KeyCode::Char('d') => AppAction::CommentPageDown,
            KeyCode::Char('r') => AppAction::RefreshComments,
            _ => AppAction::None,
        },
    }
}

fn update(
    app: &mut App,
    action: AppAction,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    worker: &RedditWorker,
) -> anyhow::Result<()> {
    match action {
        AppAction::None => {}
        AppAction::Quit => app.request_quit(),
        AppAction::Resize => clamp_to_viewport(app, terminal)?,
        AppAction::ToggleHelp => app.toggle_help(),
        AppAction::DismissHelp => app.hide_help(),
        AppAction::InputChar(c) => app.push_input(c),
        AppAction::InputBackspace => app.pop_input(),
        AppAction::SubmitSubreddit => match app.take_subreddit_input() {
            Ok(Some(subreddit)) => queue_post_load(app, worker, subreddit)?,
            Ok(None) => {}
            Err(error) => app.set_input_error(error),
        },
        AppAction::CancelInput => app.close_input(),
        AppAction::RecentPrevious => app.apply_recent_subreddit(RecentDirection::Previous),
        AppAction::RecentNext => app.apply_recent_subreddit(RecentDirection::Next),
        AppAction::PostUp => {
            let area = terminal_area(terminal)?;
            app.post_up(ui::post_viewport_rows(area));
        }
        AppAction::PostDown => {
            let area = terminal_area(terminal)?;
            app.post_down(ui::post_viewport_rows(area));
        }
        AppAction::PostPageUp => {
            let area = terminal_area(terminal)?;
            app.post_page_up(ui::post_viewport_rows(area));
        }
        AppAction::PostPageDown => {
            let area = terminal_area(terminal)?;
            app.post_page_down(ui::post_viewport_rows(area));
        }
        AppAction::CommentUp => {
            let area = terminal_area(terminal)?;
            let visible_rows = ui::comment_viewport_rows(app, area);
            let heights = ui::comment_item_heights(app, &app.comments, area);
            app.comment_up(visible_rows, &heights);
        }
        AppAction::CommentDown => {
            let area = terminal_area(terminal)?;
            let visible_rows = ui::comment_viewport_rows(app, area);
            let heights = ui::comment_item_heights(app, &app.comments, area);
            app.comment_down(visible_rows, &heights);
        }
        AppAction::CommentPageUp => {
            let area = terminal_area(terminal)?;
            let visible_rows = ui::comment_viewport_rows(app, area);
            let heights = ui::comment_item_heights(app, &app.comments, area);
            app.comment_page_up(visible_rows, &heights);
        }
        AppAction::CommentPageDown => {
            let area = terminal_area(terminal)?;
            let visible_rows = ui::comment_viewport_rows(app, area);
            let heights = ui::comment_item_heights(app, &app.comments, area);
            app.comment_page_down(visible_rows, &heights);
        }
        AppAction::OpenComments => {
            if let Some(post) = app.selected_post().cloned() {
                queue_comment_load(app, worker, post.permalink)?;
            }
        }
        AppAction::CycleSort => {
            app.cycle_sort();
            reload_posts(app, worker)?;
        }
        AppAction::SetSort(sort) => {
            if app.set_sort(sort) {
                reload_posts(app, worker)?;
            }
        }
        AppAction::ShowSubredditInput => app.show_subreddit_input(),
        AppAction::RefreshPosts => reload_posts(app, worker)?,
        AppAction::RefreshComments => reload_comments(app, worker)?,
        AppAction::Back => app.go_back_screen(),
    }

    Ok(())
}

fn clamp_to_viewport(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> anyhow::Result<()> {
    let area = terminal_area(terminal)?;
    let post_rows = ui::post_viewport_rows(area);
    let comment_rows = ui::comment_viewport_rows(app, area);
    let comment_heights = ui::comment_item_heights(app, &app.comments, area);
    app.on_resize(post_rows, comment_rows, &comment_heights);
    Ok(())
}

fn terminal_area(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> anyhow::Result<Rect> {
    let size = terminal.size()?;
    Ok(Rect::new(0, 0, size.width, size.height))
}

fn reload_posts(app: &mut App, worker: &RedditWorker) -> anyhow::Result<()> {
    let subreddit = app.current_subreddit.clone();
    if !subreddit.is_empty() {
        queue_post_load(app, worker, subreddit)?;
    }
    Ok(())
}

fn reload_comments(app: &mut App, worker: &RedditWorker) -> anyhow::Result<()> {
    if let Some(post) = app.selected_post().cloned() {
        queue_comment_load(app, worker, post.permalink)?;
    }
    Ok(())
}

fn queue_post_load(app: &mut App, worker: &RedditWorker, subreddit: String) -> anyhow::Result<()> {
    let request_id = app.start_loading_posts(&subreddit);
    worker.submit(FetchCommand::Posts {
        request_id,
        subreddit,
        sort: app.sort.as_str().to_owned(),
    })?;
    Ok(())
}

fn queue_comment_load(
    app: &mut App,
    worker: &RedditWorker,
    permalink: String,
) -> anyhow::Result<()> {
    let request_id = app.start_loading_comments(&permalink);
    worker.submit(FetchCommand::Comments {
        request_id,
        permalink,
    })?;
    Ok(())
}

fn sync_preview_media(app: &mut App, worker: &RedditWorker) -> anyhow::Result<bool> {
    if let Some(request) = app.sync_preview_media() {
        worker.submit(FetchCommand::Media {
            request_id: request.request_id,
            url: request.url,
            kind: request.kind,
        })?;
        return Ok(true);
    }
    Ok(false)
}

fn drain_fetch_events(app: &mut App, results: &Receiver<FetchEvent>) -> bool {
    let mut changed = false;
    while let Ok(event) = results.try_recv() {
        changed = true;
        match event {
            FetchEvent::PostsLoaded {
                request_id,
                subreddit,
                posts,
            } => app.finish_loading_posts(request_id, &subreddit, posts),
            FetchEvent::CommentsLoaded {
                request_id,
                comments,
            } => app.finish_loading_comments(request_id, comments),
            FetchEvent::MediaLoaded {
                request_id,
                url,
                media,
            } => app.finish_loading_media(request_id, &url, media),
            FetchEvent::Failed { request_id, error } => apply_fetch_error(app, request_id, error),
            FetchEvent::MediaFailed {
                request_id,
                url,
                error,
            } => app.fail_loading_media(request_id, &url, error),
        }
    }
    changed
}

fn apply_fetch_error(app: &mut App, request_id: u64, error: error::RedditError) {
    match app.active_request.clone() {
        Some(app::ActiveRequest::Posts {
            request_id: active_id,
            subreddit,
        }) if active_id == request_id => app.fail_loading_posts(request_id, &subreddit, error),
        Some(app::ActiveRequest::Comments {
            request_id: active_id,
            ..
        }) if active_id == request_id => app.fail_loading_comments(request_id, error),
        _ => {}
    }
}

fn next_poll_timeout(app: &App) -> Duration {
    if app.active_request.is_some() {
        Duration::from_millis(50)
    } else if let Some(delay) = app.preview_animation_delay() {
        delay.min(Duration::from_millis(120))
    } else {
        Duration::from_millis(250)
    }
}
