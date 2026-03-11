use reddit_tui::{
    app::{App, RequestState, Screen},
    error::RedditError,
    models::{Comment, Post},
};

fn post(title: &str) -> Post {
    Post {
        title: title.to_owned(),
        author: String::from("author"),
        subreddit: String::from("rust"),
        score: 5,
        num_comments: 2,
        url: String::from("https://example.com"),
        selftext: String::from("preview"),
        permalink: String::from("/r/rust/comments/test/post"),
        is_self: true,
        is_nsfw: false,
        is_spoiler: false,
        is_stickied: false,
        media: None,
    }
}

fn comment(body: &str) -> Comment {
    Comment {
        author: String::from("author"),
        body: body.to_owned(),
        score: 1,
        depth: 0,
        is_deleted: false,
        is_removed: false,
    }
}

#[test]
fn post_then_comment_flow_keeps_post_selection_on_return() {
    let mut app = App::new();
    let posts_request = app.start_loading_posts("rust");
    app.finish_loading_posts(posts_request, "rust", vec![post("a"), post("b"), post("c")]);
    app.post_down(2);

    let comment_request = app.start_loading_comments("/r/rust/comments/test/post");
    app.finish_loading_comments(comment_request, vec![comment("hello"), comment("world")]);
    assert_eq!(app.screen, Screen::Comments);

    app.go_back();

    assert_eq!(app.screen, Screen::PostList);
    assert_eq!(app.post_cursor, 1);
    assert_eq!(app.current_subreddit, "rust");
}

#[test]
fn failed_post_load_does_not_leave_half_loaded_state() {
    let mut app = App::new();
    let request_id = app.start_loading_posts("rust");
    app.posts = vec![post("existing")];

    app.fail_loading_posts(request_id, "rust", RedditError::NotFound);

    assert_eq!(app.posts.len(), 1);
    assert!(matches!(app.request_state, RequestState::Error(_)));
    assert_eq!(app.status, "Unable to load r/rust");
}
