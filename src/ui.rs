use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{App, PreviewMediaState, RequestState, Screen},
    media,
    models::{Comment, MediaKind, Post},
};

const ORANGE: Color = Color::Rgb(255, 90, 0);
const GREY: Color = Color::Rgb(150, 150, 150);
const DARK: Color = Color::Rgb(30, 30, 30);

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(DARK)), area);

    match app.screen {
        Screen::SubredditInput => draw_input(f, app, area),
        Screen::PostList => draw_post_list(f, app, area),
        Screen::Comments => draw_comments(f, app, area),
    }

    if app.help_visible {
        draw_help_modal(f, app, area);
    }
}

pub fn post_viewport_rows(area: Rect) -> usize {
    let chunks = post_layout(area);
    let content = post_content_layout(chunks[1]);
    content[0].height.max(1) as usize
}

pub fn comment_viewport_rows(app: &App, area: Rect) -> usize {
    let chunks = comments_layout(app, area);
    comment_list_area(&chunks).height.max(1) as usize
}

pub fn comment_item_heights(app: &App, comments: &[Comment], area: Rect) -> Vec<usize> {
    let chunks = comments_layout(app, area);
    let body_width = comment_body_width(comment_list_area(&chunks));
    comments
        .iter()
        .map(|comment| rendered_comment_lines(comment, body_width))
        .collect()
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Length(3),
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let title = Paragraph::new("reddit-tui")
        .style(Style::default().fg(ORANGE).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center);
    f.render_widget(title, chunks[0]);

    let input_width = area.width.clamp(16, 60);
    let input = Paragraph::new(format!("r/{}_", app.subreddit_input))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(ORANGE))
                .title(" Enter subreddit "),
        )
        .style(Style::default().fg(Color::White));
    f.render_widget(input, centre_rect(input_width, 3, chunks[1]));

    let status =
        Paragraph::new(request_message(app).unwrap_or("Type a subreddit name and press Enter."))
            .style(request_style(app))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
    f.render_widget(status, chunks[2]);

    let recent_text = if app.recent_subreddits.is_empty() {
        String::from("Recent: none yet")
    } else {
        format!(
            "Recent: {}",
            app.recent_subreddits
                .iter()
                .take(4)
                .map(|sub| format!("r/{sub}"))
                .collect::<Vec<_>>()
                .join("  ")
        )
    };
    let recent = Paragraph::new(recent_text)
        .style(Style::default().fg(GREY))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
    f.render_widget(recent, chunks[3]);

    draw_status(
        f,
        app,
        chunks[5],
        "Enter browse  Tab recent  Esc cancel  ? help",
    );
}

fn draw_post_list(f: &mut Frame, app: &App, area: Rect) {
    let chunks = post_layout(area);
    let content = post_content_layout(chunks[1]);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("r/", Style::default().fg(ORANGE)),
        Span::styled(
            truncate_text(
                app.current_subreddit.as_str(),
                chunks[0].width.saturating_sub(22) as usize,
            ),
            Style::default().fg(ORANGE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  [{}]", app.sort.as_str()),
            Style::default().fg(GREY),
        ),
        request_badge(app),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(ORANGE)),
    );
    f.render_widget(header, chunks[0]);

    if app.posts.is_empty() {
        let empty = Paragraph::new(empty_posts_message(app))
            .style(request_style(app))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(Block::default());
        f.render_widget(empty, chunks[1]);
    } else {
        draw_post_list_panel(f, app, content[0]);
        draw_post_preview_panel(f, app, content[1]);
    }

    draw_status(
        f,
        app,
        chunks[2],
        "Up/Down move  PgUp/PgDn page  Enter comments  1-4 sort  / switch  r refresh  ? help",
    );
}

fn draw_post_list_panel(f: &mut Frame, app: &App, area: Rect) {
    let visible = area.height.max(1) as usize;
    let title_width = area.width.saturating_sub(18) as usize;
    let items: Vec<ListItem> = app
        .posts
        .iter()
        .skip(app.post_scroll)
        .take(visible)
        .enumerate()
        .map(|(i, post)| {
            let global_idx = i + app.post_scroll;
            let selected = global_idx == app.post_cursor;
            let accent = if post.is_self { "txt" } else { "url" };

            let line = Line::from(vec![
                Span::styled(
                    format!("{:>4} ", post.score),
                    Style::default().fg(if selected { ORANGE } else { GREY }),
                ),
                Span::styled(
                    format!("{:>4} ", post.num_comments),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    truncate_text(&post.title, title_width),
                    Style::default()
                        .fg(if selected {
                            Color::White
                        } else {
                            Color::Rgb(205, 205, 205)
                        })
                        .add_modifier(if selected {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    format!(" {accent}"),
                    Style::default().fg(if post.is_self {
                        Color::Yellow
                    } else {
                        Color::Cyan
                    }),
                ),
            ]);

            let item = ListItem::new(line);
            if selected {
                item.style(Style::default().bg(Color::Rgb(50, 30, 20)))
            } else {
                item
            }
        })
        .collect();

    let mut list_state = ListState::default();
    list_state.select(Some(app.post_cursor.saturating_sub(app.post_scroll)));

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(Color::Rgb(70, 70, 70)))
            .title(" Posts "),
    );
    f.render_stateful_widget(list, area, &mut list_state);
}

fn draw_post_preview_panel(f: &mut Frame, app: &App, area: Rect) {
    let Some(post) = app.selected_post() else {
        return;
    };

    let chunks = if post.media.is_some() && area.height >= 10 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(area.height.saturating_mul(3) / 4),
                Constraint::Min(4),
            ])
            .split(area)
            .to_vec()
    } else {
        vec![area]
    };
    if chunks.len() == 2 {
        draw_post_media_panel(f, app, post, chunks[0]);
    }

    let text_area = *chunks.last().unwrap_or(&area);
    let metadata = post.metadata_tags().join(" | ");
    let preview = preview_body(post);
    let body_width = text_area.width.saturating_sub(4) as usize;
    let wrapped = wrap_text_lines(&preview, body_width);
    let preview_text = wrapped.join("\n");

    let lines = vec![
        Line::from(Span::styled(
            truncate_text(&post.title, body_width),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!(
                "u/{}  {} points  {} comments",
                post.author, post.score, post.num_comments
            ),
            Style::default().fg(GREY),
        )),
        Line::from(Span::styled(
            truncate_text(&metadata, body_width),
            Style::default().fg(ORANGE),
        )),
        Line::from(Span::styled(
            truncate_text(preview_hint(post), body_width),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(preview_text),
    ];

    let preview_widget = Paragraph::new(lines)
        .block(Block::default().title(" Preview "))
        .wrap(Wrap { trim: false });
    f.render_widget(preview_widget, text_area);
}

fn draw_comments(f: &mut Frame, app: &App, area: Rect) {
    let post = match app.selected_post() {
        Some(post) => post.clone(),
        None => return,
    };

    let chunks = comments_layout(app, area);
    let post_meta = format!(
        "u/{}  {} points  {}",
        post.author,
        post.score,
        post.metadata_tags().join(" | ")
    );

    let header_text = vec![
        Line::from(vec![
            Span::styled(
                truncate_text(&post_meta, chunks[0].width as usize),
                Style::default().fg(GREY),
            ),
            request_badge(app),
        ]),
        Line::from(Span::styled(
            truncate_text(&post.title, chunks[0].width.saturating_sub(2) as usize),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            truncate_text(
                post_preview_line(&post),
                chunks[0].width.saturating_sub(2) as usize,
            ),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            format!("{} comments", post.num_comments),
            Style::default().fg(GREY),
        )),
    ];
    let header = Paragraph::new(header_text)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(ORANGE))
                .title(format!(" r/{} ", truncate_text(&post.subreddit, 20))),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(header, chunks[0]);

    let list_area = comment_list_area(&chunks);
    if chunks.len() == 4 {
        draw_post_media_panel(f, app, &post, chunks[1]);
    }

    if app.comments.is_empty() {
        let empty = Paragraph::new(empty_comments_message(app))
            .style(request_style(app))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .block(Block::default());
        f.render_widget(empty, list_area);
    } else {
        let viewport_rows = list_area.height.max(1) as usize;
        let body_width = comment_body_width(list_area);
        let heights = comment_item_heights(app, &app.comments, area);
        let mut rendered_rows = 0usize;
        let mut items = Vec::new();
        let mut selected_relative = None;

        for (index, comment) in app.comments.iter().enumerate().skip(app.comment_scroll) {
            let item_height = heights.get(index).copied().unwrap_or(1);
            if !items.is_empty() && rendered_rows + item_height > viewport_rows {
                break;
            }

            if rendered_rows >= viewport_rows {
                break;
            }

            if index == app.comment_cursor {
                selected_relative = Some(items.len());
            }

            items.push(build_comment_item(
                comment,
                index == app.comment_cursor,
                body_width,
            ));
            rendered_rows += item_height;
        }

        let mut list_state = ListState::default();
        list_state.select(selected_relative);
        let list = List::new(items).block(Block::default());
        f.render_stateful_widget(list, list_area, &mut list_state);
    }

    draw_status(
        f,
        app,
        *chunks.last().unwrap_or(&area),
        "Up/Down move  PgUp/PgDn page  r reload  Esc back  ? help",
    );
}

fn draw_help_modal(f: &mut Frame, app: &App, area: Rect) {
    let modal = centre_rect(
        area.width.saturating_mul(3) / 4,
        area.height.saturating_mul(3) / 5,
        area,
    );
    f.render_widget(Clear, modal);

    let lines = match app.screen {
        Screen::SubredditInput => vec![
            Line::from("Input"),
            Line::from("Enter: load subreddit"),
            Line::from("Tab / Shift+Tab: cycle recent subreddits"),
            Line::from("Esc: cancel input or quit if nothing is loaded"),
            Line::from("? : close help"),
        ],
        Screen::PostList => vec![
            Line::from("Posts"),
            Line::from("Up/Down or j/k: move selection"),
            Line::from("PgUp/PgDn or u/d: page movement"),
            Line::from("Enter: open comments"),
            Line::from("s: cycle sort"),
            Line::from("1/2/3/4: set hot/new/top/rising"),
            Line::from("/: edit subreddit"),
            Line::from("r: refresh posts"),
            Line::from("q: quit"),
        ],
        Screen::Comments => vec![
            Line::from("Comments"),
            Line::from("Up/Down or j/k: move selection"),
            Line::from("PgUp/PgDn or u/d: page movement"),
            Line::from("r: reload comments"),
            Line::from("Esc or b: back to posts"),
            Line::from("q: quit"),
        ],
    };

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .border_style(Style::default().fg(ORANGE)),
        )
        .style(Style::default().fg(Color::White).bg(Color::Rgb(18, 18, 18)))
        .wrap(Wrap { trim: true });
    f.render_widget(widget, modal);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect, hint: &str) {
    let prefix = match &app.request_state {
        RequestState::Error(message) => message.as_str(),
        _ => &app.status,
    };

    let text = if prefix.is_empty() {
        format!(" {hint}")
    } else {
        format!(
            " {} | {}",
            truncate_text(prefix, area.width.saturating_sub(4) as usize),
            hint
        )
    };

    let status = Paragraph::new(text)
        .style(Style::default().fg(GREY).bg(Color::Rgb(20, 20, 20)))
        .wrap(Wrap { trim: true });
    f.render_widget(status, area);
}

fn post_layout(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area)
        .to_vec()
}

fn post_content_layout(area: Rect) -> Vec<Rect> {
    let left = if area.width < 90 { 45 } else { 40 };
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(left),
            Constraint::Percentage(100 - left),
        ])
        .split(area)
        .to_vec()
}

fn comments_layout(app: &App, area: Rect) -> Vec<Rect> {
    let header_height = if area.height < 12 { 4 } else { 5 };
    if app.selected_preview_media().is_some() && area.height >= 14 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_height),
                Constraint::Length(area.height.saturating_mul(2) / 5),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area)
            .to_vec()
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(header_height),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area)
            .to_vec()
    }
}

fn comment_list_area(chunks: &[Rect]) -> Rect {
    if chunks.len() == 4 {
        chunks[2]
    } else {
        chunks[1]
    }
}

fn build_comment_item(comment: &Comment, selected: bool, body_width: usize) -> ListItem<'static> {
    let branch = comment_branch(comment.depth);
    let author_label = if comment.is_deleted {
        "[deleted]"
    } else {
        comment.author.as_str()
    };
    let author_style = if comment.is_removed {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(if selected { ORANGE } else { Color::Cyan })
    };
    let body_style = if comment.is_removed {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC)
    } else {
        Style::default().fg(if selected {
            Color::White
        } else {
            Color::Rgb(200, 200, 200)
        })
    };
    let meta_style = Style::default().fg(GREY);

    let mut lines = vec![Line::from(vec![
        Span::styled(branch.clone(), Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!(" u/{}", truncate_text(author_label, 16)),
            author_style,
        ),
        Span::styled(format!("  {} points", comment.score), meta_style),
    ])];

    let body_indent = comment_body_indent(comment.depth);
    let available_width = body_width.saturating_sub(body_indent.chars().count());
    for line in wrap_text_lines(&comment.body.replace('\n', " "), available_width) {
        lines.push(Line::from(vec![
            Span::styled(body_indent.clone(), Style::default().fg(Color::DarkGray)),
            Span::styled(line, body_style),
        ]));
    }

    let item = ListItem::new(lines);
    if selected {
        item.style(Style::default().bg(Color::Rgb(30, 40, 60)))
    } else {
        item
    }
}

fn request_badge(app: &App) -> Span<'static> {
    match &app.request_state {
        RequestState::Idle => Span::raw(""),
        RequestState::Loading(message) => Span::styled(
            format!("  [{}]", message),
            Style::default().fg(Color::Yellow),
        ),
        RequestState::Error(_) => Span::styled("  [error]", Style::default().fg(Color::Red)),
    }
}

fn request_message(app: &App) -> Option<&str> {
    match &app.request_state {
        RequestState::Idle => None,
        RequestState::Loading(message) => Some(message.as_str()),
        RequestState::Error(message) => Some(message.as_str()),
    }
}

fn request_style(app: &App) -> Style {
    match app.request_state {
        RequestState::Idle => Style::default().fg(GREY),
        RequestState::Loading(_) => Style::default().fg(Color::Yellow),
        RequestState::Error(_) => Style::default().fg(Color::Red),
    }
}

fn empty_posts_message(app: &App) -> &str {
    match app.request_state {
        RequestState::Loading(_) => "Loading posts...",
        RequestState::Error(_) => request_message(app).unwrap_or("Unable to load posts."),
        RequestState::Idle => "No posts loaded. Press / to choose a subreddit or r to retry.",
    }
}

fn empty_comments_message(app: &App) -> &str {
    match app.request_state {
        RequestState::Loading(_) => "Loading comments...",
        RequestState::Error(_) => request_message(app).unwrap_or("Unable to load comments."),
        RequestState::Idle => "No comments available for this post.",
    }
}

fn preview_hint(post: &Post) -> &'static str {
    if let Some(media) = &post.media {
        match media.kind {
            MediaKind::Image => "Inline image preview",
            MediaKind::Gif => "Animated GIF preview",
        }
    } else if post.is_self {
        "Self post"
    } else if !post.url.is_empty() {
        "External link"
    } else {
        "Preview"
    }
}

fn preview_body(post: &Post) -> String {
    if post.is_self && !post.selftext.is_empty() {
        post.selftext.clone()
    } else if !post.url.is_empty() {
        format!("Open URL:\n{}", post.url)
    } else {
        String::from("No preview available.")
    }
}

fn post_preview_line(post: &Post) -> &str {
    if post.is_self && !post.selftext.is_empty() {
        post.selftext.as_str()
    } else if !post.url.is_empty() {
        post.url.as_str()
    } else {
        "No preview available"
    }
}

fn comment_body_width(area: Rect) -> usize {
    area.width.saturating_sub(4).max(18) as usize
}

fn rendered_comment_lines(comment: &Comment, body_width: usize) -> usize {
    let prefix_width = comment_body_indent(comment.depth).chars().count();
    let available_width = body_width.saturating_sub(prefix_width).max(8);
    1 + wrap_text_lines(&comment.body.replace('\n', " "), available_width).len()
}

fn comment_branch(depth: u32) -> String {
    if depth == 0 {
        String::from("+-")
    } else {
        format!("{}+-", "| ".repeat(depth.min(6) as usize))
    }
}

fn comment_body_indent(depth: u32) -> String {
    if depth == 0 {
        String::from("| ")
    } else {
        format!("{}| ", "| ".repeat(depth.min(6) as usize))
    }
}

fn draw_post_media_panel(f: &mut Frame, app: &App, post: &Post, area: Rect) {
    let title = if app.screen == Screen::Comments && app.selected_comment_media().is_some() {
        " Comment Media "
    } else {
        " Media "
    };
    let block = Block::default().borders(Borders::BOTTOM).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let content = match &app.preview_media {
        PreviewMediaState::Ready { media, started_at } => media::render_lines(
            media,
            inner.width,
            inner.height,
            started_at.elapsed().as_millis(),
        ),
        PreviewMediaState::Loading(kind) => {
            vec![Line::from(format!("Loading {} preview...", kind.as_str()))]
        }
        PreviewMediaState::Error { kind, message } => vec![
            Line::from(format!("Unable to load {} preview.", kind.as_str())),
            Line::from(truncate_text(
                message,
                inner.width.saturating_sub(1) as usize,
            )),
        ],
        PreviewMediaState::Empty => vec![Line::from(
            post.media
                .as_ref()
                .map(|media| format!("No {} preview available.", media.kind.as_str()))
                .unwrap_or_else(|| String::from("No media attached.")),
        )],
    };

    let widget = Paragraph::new(content)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: false });
    f.render_widget(widget, inner);
}

fn wrap_text_lines(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        let current_len = current.chars().count();

        if current.is_empty() {
            if word_len <= width {
                current.push_str(word);
            } else {
                lines.extend(split_long_word(word, width));
            }
            continue;
        }

        if current_len + 1 + word_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = String::new();
            if word_len <= width {
                current.push_str(word);
            } else {
                lines.extend(split_long_word(word, width));
            }
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        vec![String::from("[empty]")]
    } else {
        lines
    }
}

fn split_long_word(word: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for ch in word.chars() {
        current.push(ch);
        if current.chars().count() >= width {
            chunks.push(current);
            current = String::new();
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

fn truncate_text(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if max <= 3 {
        return ".".repeat(max);
    }

    if s.chars().count() <= max {
        s.to_owned()
    } else {
        format!(
            "{}...",
            s.chars().take(max.saturating_sub(3)).collect::<String>()
        )
    }
}

fn centre_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    let horizontal_padding = area.width.saturating_sub(width) / 2;
    let vertical_padding = area.height.saturating_sub(height) / 2;

    Rect {
        x: area.x + horizontal_padding,
        y: area.y + vertical_padding,
        width,
        height,
    }
}
