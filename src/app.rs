use std::{sync::Arc, time::Instant};

use crate::{
    error::RedditError,
    media::LoadedMedia,
    models::{Comment, MediaKind, Post, PostMedia},
};

const MAX_RECENT_SUBREDDITS: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    SubredditInput,
    PostList,
    Comments,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sort {
    Hot,
    New,
    Top,
    Rising,
}

impl Sort {
    pub fn as_str(&self) -> &'static str {
        match self {
            Sort::Hot => "hot",
            Sort::New => "new",
            Sort::Top => "top",
            Sort::Rising => "rising",
        }
    }

    pub fn next(&self) -> Sort {
        match self {
            Sort::Hot => Sort::New,
            Sort::New => Sort::Top,
            Sort::Top => Sort::Rising,
            Sort::Rising => Sort::Hot,
        }
    }

    pub fn from_shortcut(digit: char) -> Option<Self> {
        match digit {
            '1' => Some(Sort::Hot),
            '2' => Some(Sort::New),
            '3' => Some(Sort::Top),
            '4' => Some(Sort::Rising),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RequestState {
    Idle,
    Loading(String),
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActiveRequest {
    Posts { request_id: u64, subreddit: String },
    Comments { request_id: u64, permalink: String },
}

#[derive(Clone)]
pub struct PendingMediaRequest {
    pub request_id: u64,
    pub url: String,
    pub kind: MediaKind,
}

pub enum PreviewMediaState {
    Empty,
    Loading(MediaKind),
    Ready {
        media: Arc<LoadedMedia>,
        started_at: Instant,
    },
    Error {
        kind: MediaKind,
        message: String,
    },
}

pub struct App {
    pub screen: Screen,
    pub subreddit_input: String,
    pub current_subreddit: String,
    pub sort: Sort,
    pub should_quit: bool,
    pub help_visible: bool,

    pub posts: Vec<Post>,
    pub post_cursor: usize,
    pub post_scroll: usize,
    saved_post_view: Option<(usize, usize)>,

    pub comments: Vec<Comment>,
    pub comment_cursor: usize,
    pub comment_scroll: usize,

    pub recent_subreddits: Vec<String>,
    recent_index: Option<usize>,

    pub status: String,
    pub request_state: RequestState,
    pub error_detail: Option<String>,
    pub active_request: Option<ActiveRequest>,
    pub preview_media: PreviewMediaState,
    preview_media_target: Option<PostMedia>,
    active_media_request: Option<(u64, String)>,
    next_request_id: u64,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::SubredditInput,
            subreddit_input: String::new(),
            current_subreddit: String::new(),
            sort: Sort::Hot,
            should_quit: false,
            help_visible: false,

            posts: vec![],
            post_cursor: 0,
            post_scroll: 0,
            saved_post_view: None,

            comments: vec![],
            comment_cursor: 0,
            comment_scroll: 0,

            recent_subreddits: vec![],
            recent_index: None,

            status: String::from("Enter a subreddit to browse"),
            request_state: RequestState::Idle,
            error_detail: None,
            active_request: None,
            preview_media: PreviewMediaState::Empty,
            preview_media_target: None,
            active_media_request: None,
            next_request_id: 1,
        }
    }

    pub fn request_quit(&mut self) {
        self.should_quit = true;
    }

    pub fn toggle_help(&mut self) {
        self.help_visible = !self.help_visible;
    }

    pub fn hide_help(&mut self) {
        self.help_visible = false;
    }

    pub fn push_input(&mut self, c: char) {
        self.subreddit_input.push(c);
        self.recent_index = None;
        self.clear_error();
    }

    pub fn pop_input(&mut self) {
        self.subreddit_input.pop();
        self.recent_index = None;
        self.clear_error();
    }

    pub fn take_subreddit_input(&mut self) -> Result<Option<String>, RedditError> {
        if self.subreddit_input.trim().is_empty() {
            return Ok(None);
        }

        let normalized = normalize_subreddit(&self.subreddit_input)?;
        self.subreddit_input.clear();
        self.recent_index = None;
        Ok(Some(normalized))
    }

    pub fn show_subreddit_input(&mut self) {
        self.screen = Screen::SubredditInput;
        self.help_visible = false;
        self.subreddit_input = self.current_subreddit.clone();
        self.recent_index = None;
        self.active_request = None;
        self.error_detail = None;
        self.request_state = RequestState::Idle;
        self.status = if self.current_subreddit.is_empty() {
            String::from("Enter a subreddit to browse")
        } else {
            String::from("Edit subreddit and press Enter")
        };
    }

    pub fn close_input(&mut self) {
        if self.current_subreddit.is_empty() {
            self.request_quit();
        } else {
            self.screen = Screen::PostList;
            self.request_state = RequestState::Idle;
            self.status = format!(
                "r/{} - {} posts ({})",
                self.current_subreddit,
                self.posts.len(),
                self.sort.as_str()
            );
        }
    }

    pub fn apply_recent_subreddit(&mut self, direction: RecentDirection) {
        if self.recent_subreddits.is_empty() {
            return;
        }

        let len = self.recent_subreddits.len();
        let next = match (self.recent_index, direction) {
            (None, RecentDirection::Previous) => 0,
            (None, RecentDirection::Next) => len - 1,
            (Some(index), RecentDirection::Previous) => (index + 1) % len,
            (Some(index), RecentDirection::Next) => (index + len - 1) % len,
        };

        self.recent_index = Some(next);
        self.subreddit_input = self.recent_subreddits[next].clone();
        self.clear_error();
    }

    pub fn start_loading_posts(&mut self, subreddit: &str) -> u64 {
        let request_id = self.next_request_id();
        self.active_request = Some(ActiveRequest::Posts {
            request_id,
            subreddit: subreddit.to_owned(),
        });
        self.clear_preview_media();
        self.help_visible = false;
        self.error_detail = None;
        self.request_state = RequestState::Loading(format!("Loading r/{subreddit}"));
        self.status = format!("Fetching posts for r/{subreddit}");
        request_id
    }

    pub fn finish_loading_posts(&mut self, request_id: u64, subreddit: &str, posts: Vec<Post>) {
        if !self.is_active_posts_request(request_id, subreddit) {
            return;
        }

        self.active_request = None;
        self.error_detail = None;
        self.current_subreddit = subreddit.to_owned();
        self.add_recent_subreddit(subreddit);
        self.posts = posts;
        self.post_cursor = 0;
        self.post_scroll = 0;
        self.saved_post_view = None;
        self.comments.clear();
        self.comment_cursor = 0;
        self.comment_scroll = 0;
        self.screen = Screen::PostList;
        self.request_state = RequestState::Idle;
        self.status = format!(
            "r/{} - {} posts ({})",
            self.current_subreddit,
            self.posts.len(),
            self.sort.as_str()
        );
    }

    pub fn fail_loading_posts(&mut self, request_id: u64, subreddit: &str, error: RedditError) {
        if !self.is_active_posts_request(request_id, subreddit) {
            return;
        }

        self.active_request = None;
        self.error_detail = Some(error.detail_message());
        self.request_state = RequestState::Error(error.user_message());
        self.status = format!("Unable to load r/{subreddit}");
        self.clear_preview_media();
    }

    pub fn start_loading_comments(&mut self, permalink: &str) -> u64 {
        let request_id = self.next_request_id();
        self.active_request = Some(ActiveRequest::Comments {
            request_id,
            permalink: permalink.to_owned(),
        });
        self.saved_post_view = Some((self.post_cursor, self.post_scroll));
        self.help_visible = false;
        self.error_detail = None;
        self.screen = Screen::Comments;
        self.comments.clear();
        self.comment_cursor = 0;
        self.comment_scroll = 0;
        self.request_state = RequestState::Loading(String::from("Loading comments"));
        self.status = String::from("Fetching comments");
        request_id
    }

    pub fn finish_loading_comments(&mut self, request_id: u64, comments: Vec<Comment>) {
        if !self.is_active_comments_request(request_id) {
            return;
        }

        self.active_request = None;
        self.error_detail = None;
        self.comments = comments;
        self.open_comments();
    }

    pub fn fail_loading_comments(&mut self, request_id: u64, error: RedditError) {
        if !self.is_active_comments_request(request_id) {
            return;
        }

        self.active_request = None;
        self.error_detail = Some(error.detail_message());
        self.request_state = RequestState::Error(error.user_message());
        self.status = String::from("Comments unavailable");
    }

    pub fn open_comments(&mut self) {
        self.screen = Screen::Comments;
        self.request_state = RequestState::Idle;
        self.status = format!("{} comments", self.comments.len());
    }

    pub fn go_back(&mut self) {
        if matches!(self.active_request, Some(ActiveRequest::Comments { .. })) {
            self.active_request = None;
        }
        if let Some((cursor, scroll)) = self.saved_post_view {
            self.post_cursor = cursor;
            self.post_scroll = scroll;
        }
        self.screen = Screen::PostList;
        self.request_state = RequestState::Idle;
        self.status = format!(
            "r/{} - {} posts ({})",
            self.current_subreddit,
            self.posts.len(),
            self.sort.as_str()
        );
    }

    pub fn go_back_screen(&mut self) {
        match self.screen {
            Screen::SubredditInput => self.request_quit(),
            Screen::PostList => self.show_subreddit_input(),
            Screen::Comments => self.go_back(),
        }
    }

    pub fn sync_preview_media(&mut self) -> Option<PendingMediaRequest> {
        let Some(media) = self.selected_preview_media() else {
            self.clear_preview_media();
            return None;
        };

        if self.preview_media_target.as_ref() == Some(&media) {
            return None;
        }

        self.preview_media_target = Some(media.clone());

        let request_id = self.next_request_id();
        self.active_media_request = Some((request_id, media.url.clone()));
        self.preview_media = PreviewMediaState::Loading(media.kind);
        Some(PendingMediaRequest {
            request_id,
            url: media.url,
            kind: media.kind,
        })
    }

    pub fn finish_loading_media(&mut self, request_id: u64, url: &str, media: LoadedMedia) {
        if !self.is_active_media_request(request_id, url) {
            return;
        }

        let media = Arc::new(media);
        self.active_media_request = None;
        self.preview_media = PreviewMediaState::Ready {
            media,
            started_at: Instant::now(),
        };
    }

    pub fn fail_loading_media(&mut self, request_id: u64, url: &str, error: RedditError) {
        if !self.is_active_media_request(request_id, url) {
            return;
        }

        let kind = self
            .preview_media_target
            .as_ref()
            .map(|media| media.kind)
            .unwrap_or(MediaKind::Image);
        self.active_media_request = None;
        self.preview_media = PreviewMediaState::Error {
            kind,
            message: error.user_message(),
        };
    }

    pub fn selected_post_media(&self) -> Option<&PostMedia> {
        self.selected_post().and_then(|post| post.media.as_ref())
    }

    pub fn selected_preview_media(&self) -> Option<PostMedia> {
        match self.screen {
            Screen::Comments => self
                .selected_comment_media()
                .or_else(|| self.selected_post_media().cloned()),
            _ => self.selected_post_media().cloned(),
        }
    }

    pub fn selected_comment_media(&self) -> Option<PostMedia> {
        self.comments
            .get(self.comment_cursor)
            .and_then(|comment| extract_media_from_text(&comment.body))
    }

    pub fn preview_animation_delay(&self) -> Option<std::time::Duration> {
        match &self.preview_media {
            PreviewMediaState::Ready { media, started_at } if media.kind == MediaKind::Gif => {
                Some(std::time::Duration::from_millis(
                    crate::media::current_frame_delay_ms(media, started_at.elapsed().as_millis())
                        as u64,
                ))
            }
            _ => None,
        }
    }

    pub fn cycle_sort(&mut self) {
        self.sort = self.sort.next();
        self.request_state = RequestState::Idle;
    }

    pub fn set_sort(&mut self, sort: Sort) -> bool {
        if self.sort == sort {
            false
        } else {
            self.sort = sort;
            self.request_state = RequestState::Idle;
            true
        }
    }

    pub fn on_resize(&mut self, post_rows: usize, comment_rows: usize, comment_heights: &[usize]) {
        self.clamp_post_view(post_rows);
        self.clamp_comment_view(comment_rows, comment_heights);
    }

    pub fn post_up(&mut self, visible_rows: usize) {
        if self.post_cursor > 0 {
            self.post_cursor -= 1;
        }
        self.clamp_post_view(visible_rows);
    }

    pub fn post_down(&mut self, visible_rows: usize) {
        if !self.posts.is_empty() && self.post_cursor + 1 < self.posts.len() {
            self.post_cursor += 1;
        }
        self.clamp_post_view(visible_rows);
    }

    pub fn post_page_up(&mut self, visible_rows: usize) {
        self.post_cursor = self.post_cursor.saturating_sub(visible_rows.max(1));
        self.clamp_post_view(visible_rows);
    }

    pub fn post_page_down(&mut self, visible_rows: usize) {
        if !self.posts.is_empty() {
            self.post_cursor =
                (self.post_cursor + visible_rows.max(1)).min(self.posts.len().saturating_sub(1));
        }
        self.clamp_post_view(visible_rows);
    }

    pub fn comment_up(&mut self, visible_rows: usize, heights: &[usize]) {
        if self.comment_cursor > 0 {
            self.comment_cursor -= 1;
        }
        self.clamp_comment_view(visible_rows, heights);
    }

    pub fn comment_down(&mut self, visible_rows: usize, heights: &[usize]) {
        if !self.comments.is_empty() && self.comment_cursor + 1 < self.comments.len() {
            self.comment_cursor += 1;
        }
        self.clamp_comment_view(visible_rows, heights);
    }

    pub fn comment_page_up(&mut self, visible_rows: usize, heights: &[usize]) {
        let jump = heights_per_page(visible_rows, heights, self.comment_scroll).max(1);
        self.comment_cursor = self.comment_cursor.saturating_sub(jump);
        self.clamp_comment_view(visible_rows, heights);
    }

    pub fn comment_page_down(&mut self, visible_rows: usize, heights: &[usize]) {
        let jump = heights_per_page(visible_rows, heights, self.comment_scroll).max(1);
        if !self.comments.is_empty() {
            self.comment_cursor =
                (self.comment_cursor + jump).min(self.comments.len().saturating_sub(1));
        }
        self.clamp_comment_view(visible_rows, heights);
    }

    pub fn clamp_post_view(&mut self, visible_rows: usize) {
        clamp_fixed_list(
            self.posts.len(),
            visible_rows,
            &mut self.post_cursor,
            &mut self.post_scroll,
        );
    }

    pub fn clamp_comment_view(&mut self, visible_rows: usize, heights: &[usize]) {
        if self.comments.is_empty() || heights.is_empty() {
            self.comment_cursor = 0;
            self.comment_scroll = 0;
            return;
        }

        self.comment_cursor = self
            .comment_cursor
            .min(self.comments.len().saturating_sub(1));
        self.comment_scroll = self.comment_scroll.min(self.comment_cursor);
        self.ensure_variable_list_visible(visible_rows.max(1), heights);
    }

    pub fn selected_post(&self) -> Option<&Post> {
        self.posts.get(self.post_cursor)
    }

    pub fn set_input_error(&mut self, error: RedditError) {
        self.error_detail = Some(error.detail_message());
        self.request_state = RequestState::Error(error.user_message());
        self.status = String::from("Input rejected");
    }

    fn add_recent_subreddit(&mut self, subreddit: &str) {
        self.recent_subreddits.retain(|entry| entry != subreddit);
        self.recent_subreddits.insert(0, subreddit.to_owned());
        self.recent_subreddits.truncate(MAX_RECENT_SUBREDDITS);
        self.recent_index = None;
    }

    fn clear_error(&mut self) {
        if matches!(self.request_state, RequestState::Error(_)) {
            self.request_state = RequestState::Idle;
            self.error_detail = None;
        }
    }

    fn clear_preview_media(&mut self) {
        self.preview_media = PreviewMediaState::Empty;
        self.preview_media_target = None;
        self.active_media_request = None;
    }

    fn next_request_id(&mut self) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        request_id
    }

    fn is_active_posts_request(&self, request_id: u64, subreddit: &str) -> bool {
        matches!(
            self.active_request,
            Some(ActiveRequest::Posts {
                request_id: active_id,
                subreddit: ref active_subreddit,
            }) if active_id == request_id && active_subreddit == subreddit
        )
    }

    fn is_active_comments_request(&self, request_id: u64) -> bool {
        matches!(
            self.active_request,
            Some(ActiveRequest::Comments { request_id: active_id, .. }) if active_id == request_id
        )
    }

    fn is_active_media_request(&self, request_id: u64, url: &str) -> bool {
        matches!(
            self.active_media_request,
            Some((active_id, ref active_url)) if active_id == request_id && active_url == url
        )
    }

    fn ensure_variable_list_visible(&mut self, visible_rows: usize, heights: &[usize]) {
        if self.comment_scroll > self.comment_cursor {
            self.comment_scroll = self.comment_cursor;
        }

        while self.comment_scroll < self.comment_cursor
            && self.variable_rows_used(self.comment_scroll, self.comment_cursor, heights)
                >= visible_rows
        {
            self.comment_scroll += 1;
        }

        while self.comment_scroll > 0
            && self.variable_rows_used(self.comment_scroll - 1, self.comment_cursor, heights)
                < visible_rows
        {
            self.comment_scroll -= 1;
        }

        self.comment_scroll = self.comment_scroll.min(self.comment_cursor);
    }

    fn variable_rows_used(&self, start: usize, end: usize, heights: &[usize]) -> usize {
        heights
            .iter()
            .skip(start)
            .take(end.saturating_sub(start) + 1)
            .copied()
            .sum()
    }
}

fn extract_media_from_text(text: &str) -> Option<PostMedia> {
    for token in text.split_whitespace() {
        if let Some(url) = extract_url_candidate(token) {
            let normalized_url = normalize_media_url(&url);
            if let Some(kind) = MediaKind::detect_url(&normalized_url) {
                return Some(PostMedia {
                    url: normalized_url,
                    kind,
                });
            }
        }
    }
    None
}

fn extract_url_candidate(token: &str) -> Option<String> {
    let trimmed = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '"' | '\'' | '<' | '>' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });

    if let Some(start) = trimmed.find("http://").or_else(|| trimmed.find("https://")) {
        let candidate = &trimmed[start..];
        if let Some(open_paren) = candidate.rfind('(') {
            let suffix = &candidate[open_paren + 1..];
            if let Some(close_paren) = suffix.find(')') {
                return Some(suffix[..close_paren].trim_end_matches('.').to_owned());
            }
        }

        return Some(
            candidate
                .trim_end_matches([')', ']', '}', '.', ',', ';', ':'])
                .to_owned(),
        );
    }

    None
}

fn normalize_media_url(url: &str) -> String {
    url.replace("&amp;", "&")
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum RecentDirection {
    Previous,
    Next,
}

fn normalize_subreddit(input: &str) -> Result<String, RedditError> {
    let trimmed = input
        .trim()
        .trim_start_matches("r/")
        .trim_start_matches('/');
    let is_valid = !trimmed.is_empty()
        && trimmed.len() <= 21
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');

    if is_valid {
        Ok(trimmed.to_ascii_lowercase())
    } else {
        Err(RedditError::InvalidSubreddit(trimmed.to_owned()))
    }
}

fn clamp_fixed_list(len: usize, visible_rows: usize, cursor: &mut usize, scroll: &mut usize) {
    if len == 0 {
        *cursor = 0;
        *scroll = 0;
        return;
    }

    *cursor = (*cursor).min(len.saturating_sub(1));
    let visible_rows = visible_rows.max(1);
    let max_scroll = len.saturating_sub(visible_rows);

    if *cursor < *scroll {
        *scroll = *cursor;
    } else if *cursor >= *scroll + visible_rows {
        *scroll = cursor.saturating_sub(visible_rows - 1);
    }

    *scroll = (*scroll).min(max_scroll);
}

fn heights_per_page(visible_rows: usize, heights: &[usize], start: usize) -> usize {
    let mut used_rows = 0;
    let mut item_count = 0;

    for height in heights.iter().skip(start).copied() {
        if item_count > 0 && used_rows + height > visible_rows {
            break;
        }
        used_rows += height;
        item_count += 1;
        if used_rows >= visible_rows {
            break;
        }
    }

    item_count
}

#[cfg(test)]
mod tests {
    use super::{extract_media_from_text, App, RecentDirection, RequestState, Screen, Sort};
    use crate::{
        error::RedditError,
        models::{Comment, MediaKind, Post, PostMedia},
    };

    fn post(title: &str) -> Post {
        Post {
            title: title.to_owned(),
            author: String::from("author"),
            subreddit: String::from("rust"),
            score: 10,
            num_comments: 3,
            url: String::from("https://example.com"),
            selftext: String::new(),
            permalink: String::from("/r/rust/comments/test"),
            is_self: false,
            is_nsfw: false,
            is_spoiler: false,
            is_stickied: false,
            media: None,
        }
    }

    fn comment(depth: u32, body: &str) -> Comment {
        Comment {
            author: String::from("author"),
            body: body.to_owned(),
            score: 1,
            depth,
            is_deleted: false,
            is_removed: false,
        }
    }

    #[test]
    fn normalizes_valid_subreddit_input() {
        let mut app = App::new();
        app.subreddit_input = String::from(" r/Rust ");

        let subreddit = app.take_subreddit_input().expect("valid input");

        assert_eq!(subreddit.as_deref(), Some("rust"));
        assert!(app.subreddit_input.is_empty());
    }

    #[test]
    fn rejects_invalid_subreddit_input() {
        let mut app = App::new();
        app.subreddit_input = String::from("bad-name!");

        let error = app.take_subreddit_input().expect_err("invalid input");

        assert!(matches!(error, RedditError::InvalidSubreddit(_)));
    }

    #[test]
    fn cycles_recent_subreddits_both_directions() {
        let mut app = App::new();
        app.recent_subreddits = vec![
            String::from("rust"),
            String::from("programming"),
            String::from("opensource"),
        ];

        app.apply_recent_subreddit(RecentDirection::Previous);
        assert_eq!(app.subreddit_input, "rust");

        app.apply_recent_subreddit(RecentDirection::Previous);
        assert_eq!(app.subreddit_input, "programming");

        app.apply_recent_subreddit(RecentDirection::Next);
        assert_eq!(app.subreddit_input, "rust");
    }

    #[test]
    fn post_navigation_clamps_scroll_and_cursor() {
        let mut app = App::new();
        app.posts = vec![post("a"), post("b"), post("c"), post("d"), post("e")];
        app.screen = Screen::PostList;

        app.post_page_down(2);
        assert_eq!(app.post_cursor, 2);
        assert_eq!(app.post_scroll, 1);

        app.post_down(2);
        assert_eq!(app.post_cursor, 3);
        assert_eq!(app.post_scroll, 2);

        app.post_page_up(2);
        assert_eq!(app.post_cursor, 1);
        assert_eq!(app.post_scroll, 1);
    }

    #[test]
    fn comment_page_movement_respects_variable_heights() {
        let mut app = App::new();
        app.comments = vec![
            comment(0, "one"),
            comment(1, "two"),
            comment(2, "three"),
            comment(0, "four"),
        ];
        let heights = vec![1, 2, 2, 1];

        app.comment_page_down(3, &heights);
        assert_eq!(app.comment_cursor, 2);
        assert_eq!(app.comment_scroll, 2);

        app.comment_page_up(3, &heights);
        assert_eq!(app.comment_cursor, 0);
        assert_eq!(app.comment_scroll, 0);
    }

    #[test]
    fn post_loading_success_updates_screen_and_history() {
        let mut app = App::new();
        let request_id = app.start_loading_posts("rust");
        let posts = vec![post("hello"), post("world")];

        app.finish_loading_posts(request_id, "rust", posts);

        assert_eq!(app.screen, Screen::PostList);
        assert!(matches!(app.request_state, RequestState::Idle));
        assert_eq!(app.current_subreddit, "rust");
        assert_eq!(app.posts.len(), 2);
        assert_eq!(app.recent_subreddits, vec![String::from("rust")]);
    }

    #[test]
    fn loading_comments_and_back_restores_post_view() {
        let mut app = App::new();
        app.posts = vec![post("a"), post("b"), post("c")];
        app.current_subreddit = String::from("rust");
        app.screen = Screen::PostList;
        app.post_cursor = 2;
        app.post_scroll = 1;

        let request_id = app.start_loading_comments("/r/rust/comments/test");
        app.finish_loading_comments(request_id, vec![comment(0, "body")]);
        app.go_back();

        assert_eq!(app.screen, Screen::PostList);
        assert_eq!(app.post_cursor, 2);
        assert_eq!(app.post_scroll, 1);
    }

    #[test]
    fn stale_results_are_ignored() {
        let mut app = App::new();
        let first = app.start_loading_posts("rust");
        let second = app.start_loading_posts("golang");
        let posts = vec![post("stale")];

        app.finish_loading_posts(first, "rust", posts.clone());
        assert!(app.posts.is_empty());

        app.finish_loading_posts(second, "golang", posts);
        assert_eq!(app.current_subreddit, "golang");
        assert_eq!(app.posts.len(), 1);
    }

    #[test]
    fn set_input_error_updates_state() {
        let mut app = App::new();
        app.set_input_error(RedditError::InvalidSubreddit(String::from("bad!")));

        assert!(matches!(app.request_state, RequestState::Error(_)));
        assert_eq!(app.status, "Input rejected");
        assert!(app.error_detail.is_some());
    }

    #[test]
    fn direct_sort_selection_reports_change() {
        let mut app = App::new();

        assert!(app.set_sort(Sort::Top));
        assert_eq!(app.sort, Sort::Top);
        assert!(!app.set_sort(Sort::Top));
    }

    #[test]
    fn back_navigation_matches_screen_context() {
        let mut app = App::new();
        app.go_back_screen();
        assert!(app.should_quit);

        let mut app = App::new();
        app.current_subreddit = String::from("rust");
        app.posts = vec![post("a")];
        app.screen = Screen::PostList;
        app.go_back_screen();
        assert_eq!(app.screen, Screen::SubredditInput);
        assert_eq!(app.subreddit_input, "rust");

        let mut app = App::new();
        app.current_subreddit = String::from("rust");
        app.posts = vec![post("a"), post("b")];
        app.screen = Screen::PostList;
        app.post_cursor = 1;
        app.post_scroll = 1;
        let request_id = app.start_loading_comments("/r/rust/comments/test");
        app.finish_loading_comments(request_id, vec![comment(0, "body")]);
        app.go_back_screen();
        assert_eq!(app.screen, Screen::PostList);
        assert_eq!(app.post_cursor, 1);
    }

    #[test]
    fn selected_preview_media_prefers_comment_media_then_post_media() {
        let mut app = App::new();
        app.screen = Screen::Comments;
        app.posts = vec![Post {
            title: String::from("post"),
            author: String::from("a"),
            subreddit: String::from("rust"),
            score: 1,
            num_comments: 1,
            url: String::from("https://example.com"),
            selftext: String::new(),
            permalink: String::from("/r/rust/comments/test"),
            is_self: false,
            is_nsfw: false,
            is_spoiler: false,
            is_stickied: false,
            media: Some(PostMedia {
                url: String::from("https://i.redd.it/post.png"),
                kind: MediaKind::Image,
            }),
        }];
        app.comments = vec![Comment {
            author: String::from("u"),
            body: String::from("see https://i.redd.it/comment.gif"),
            score: 1,
            depth: 0,
            is_deleted: false,
            is_removed: false,
        }];

        let media = app.selected_preview_media().expect("media from comment");
        assert_eq!(media.kind, MediaKind::Gif);
        assert!(media.url.contains("comment.gif"));

        app.comments[0].body = String::from("no media here");
        let media = app
            .selected_preview_media()
            .expect("fallback to post media");
        assert_eq!(media.kind, MediaKind::Image);
        assert!(media.url.contains("post.png"));
    }

    #[test]
    fn extract_media_from_text_supports_markdown_and_decodes_entities() {
        let text = "look [img](https://preview.redd.it/x.png?width=200&amp;format=png)";
        let media = extract_media_from_text(text).expect("markdown media link");
        assert_eq!(media.kind, MediaKind::Image);
        assert!(media.url.contains("&format=png"));
        assert!(!media.url.contains("&amp;"));
    }
}
