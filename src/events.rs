use crate::{
    error::RedditError,
    media::LoadedMedia,
    models::{Comment, MediaKind, Post},
};

#[derive(Debug, Clone)]
pub enum FetchCommand {
    Posts {
        request_id: u64,
        subreddit: String,
        sort: String,
    },
    Comments {
        request_id: u64,
        permalink: String,
    },
    Media {
        request_id: u64,
        url: String,
        kind: MediaKind,
    },
}

#[derive(Debug)]
pub enum FetchEvent {
    PostsLoaded {
        request_id: u64,
        subreddit: String,
        posts: Vec<Post>,
    },
    CommentsLoaded {
        request_id: u64,
        comments: Vec<Comment>,
    },
    MediaLoaded {
        request_id: u64,
        url: String,
        media: LoadedMedia,
    },
    Failed {
        request_id: u64,
        error: RedditError,
    },
    MediaFailed {
        request_id: u64,
        url: String,
        error: RedditError,
    },
}
