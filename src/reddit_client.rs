use std::{
    collections::HashMap,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use reqwest::{Client, StatusCode};
use tokio::runtime::Builder;

use crate::{
    error::RedditError,
    events::{FetchCommand, FetchEvent},
    media::decode_media,
    models::{Comment, MediaKind, Post},
    reddit::{comments_from_listings, posts_from_listing, Listing},
};

const USER_AGENT: &str = "reddit-tui/0.1.0";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const POSTS_CACHE_TTL: Duration = Duration::from_secs(30);
const COMMENTS_CACHE_TTL: Duration = Duration::from_secs(30);
const MEDIA_CACHE_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Clone)]
pub struct RedditClient {
    client: Client,
}

impl RedditClient {
    pub fn new() -> Result<Self, RedditError> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| RedditError::Transport(error.to_string()))?;
        Ok(Self { client })
    }

    pub async fn fetch_posts(&self, subreddit: &str, sort: &str) -> Result<Vec<Post>, RedditError> {
        validate_subreddit(subreddit)?;

        let url = format!("https://www.reddit.com/r/{subreddit}/{sort}.json?limit=50");
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(map_transport_error)?;
        ensure_success(response.status())?;
        let listing: Listing = response
            .json()
            .await
            .map_err(|error| RedditError::Parse(error.to_string()))?;

        Ok(posts_from_listing(listing))
    }

    pub async fn fetch_comments(&self, permalink: &str) -> Result<Vec<Comment>, RedditError> {
        let url = format!("https://www.reddit.com{permalink}.json?limit=100&depth=3");
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(map_transport_error)?;
        ensure_success(response.status())?;
        let raw: Vec<Listing> = response
            .json()
            .await
            .map_err(|error| RedditError::Parse(error.to_string()))?;

        Ok(comments_from_listings(raw))
    }

    pub async fn fetch_media(
        &self,
        url: &str,
        kind: MediaKind,
    ) -> Result<crate::media::LoadedMedia, RedditError> {
        let candidates = media_url_candidates(url, kind);
        let mut last_error = None;

        for candidate in candidates {
            let response = self
                .client
                .get(&candidate)
                .header(
                    reqwest::header::ACCEPT,
                    "image/avif,image/webp,image/*,*/*;q=0.8",
                )
                .header(reqwest::header::REFERER, "https://www.reddit.com/")
                .send()
                .await
                .map_err(map_transport_error)?;

            match ensure_success(response.status()) {
                Ok(()) => {
                    let bytes = response
                        .bytes()
                        .await
                        .map_err(|error| RedditError::Transport(error.to_string()))?;
                    return decode_media(bytes.as_ref(), kind).map_err(RedditError::Parse);
                }
                Err(RedditError::Http(StatusCode::FORBIDDEN)) => {
                    last_error = Some(RedditError::Http(StatusCode::FORBIDDEN));
                    continue;
                }
                Err(error) => return Err(error),
            }
        }

        Err(last_error.unwrap_or(RedditError::NotFound))
    }
}

fn validate_subreddit(subreddit: &str) -> Result<(), RedditError> {
    let trimmed = subreddit.trim();
    let is_valid = !trimmed.is_empty()
        && trimmed.len() <= 21
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');

    if is_valid {
        Ok(())
    } else {
        Err(RedditError::InvalidSubreddit(trimmed.to_owned()))
    }
}

fn ensure_success(status: StatusCode) -> Result<(), RedditError> {
    match status {
        StatusCode::NOT_FOUND => Err(RedditError::NotFound),
        StatusCode::TOO_MANY_REQUESTS => Err(RedditError::RateLimited),
        code if code.is_success() => Ok(()),
        code => Err(RedditError::Http(code)),
    }
}

fn map_transport_error(error: reqwest::Error) -> RedditError {
    if error.is_timeout() {
        RedditError::Transport(String::from("request timed out"))
    } else if error.is_decode() {
        RedditError::Parse(error.to_string())
    } else {
        RedditError::Transport(error.to_string())
    }
}

fn media_url_candidates(url: &str, kind: MediaKind) -> Vec<String> {
    let mut out = Vec::new();
    let normalized = url.replace("&amp;", "&");
    out.push(normalized.clone());

    if normalized.contains("external-preview.redd.it/") {
        out.push(normalized.replace("external-preview.redd.it/", "preview.redd.it/"));
    }

    if kind == MediaKind::Image && normalized.contains("preview.redd.it/") {
        if let Some(direct) = preview_to_direct_reddit_image(&normalized) {
            out.push(direct);
        }
    }

    out.sort();
    out.dedup();
    out
}

fn preview_to_direct_reddit_image(url: &str) -> Option<String> {
    let query_start = url.find('?')?;
    let base = &url[..query_start];
    let query = &url[query_start + 1..];
    let id = base.rsplit('/').next()?;
    let format = query
        .split('&')
        .find_map(|part| part.strip_prefix("format="))
        .filter(|value| !value.is_empty())?;
    Some(format!("https://i.redd.it/{id}.{format}"))
}

pub struct RedditWorker {
    sender: Sender<FetchCommand>,
}

struct CacheEntry<T> {
    value: T,
    fetched_at: Instant,
}

struct WorkerCache {
    posts: HashMap<(String, String), CacheEntry<Vec<Post>>>,
    comments: HashMap<String, CacheEntry<Vec<Comment>>>,
    media: HashMap<(String, MediaKind), CacheEntry<crate::media::LoadedMedia>>,
}

impl WorkerCache {
    fn new() -> Self {
        Self {
            posts: HashMap::new(),
            comments: HashMap::new(),
            media: HashMap::new(),
        }
    }

    fn get_posts(&self, subreddit: &str, sort: &str) -> Option<Vec<Post>> {
        self.posts
            .get(&(subreddit.to_owned(), sort.to_owned()))
            .filter(|entry| is_fresh(entry.fetched_at, POSTS_CACHE_TTL))
            .map(|entry| entry.value.clone())
    }

    fn put_posts(&mut self, subreddit: String, sort: String, posts: Vec<Post>) -> Vec<Post> {
        self.posts.insert(
            (subreddit, sort),
            CacheEntry {
                value: posts.clone(),
                fetched_at: Instant::now(),
            },
        );
        posts
    }

    fn get_comments(&self, permalink: &str) -> Option<Vec<Comment>> {
        self.comments
            .get(permalink)
            .filter(|entry| is_fresh(entry.fetched_at, COMMENTS_CACHE_TTL))
            .map(|entry| entry.value.clone())
    }

    fn put_comments(&mut self, permalink: String, comments: Vec<Comment>) -> Vec<Comment> {
        self.comments.insert(
            permalink,
            CacheEntry {
                value: comments.clone(),
                fetched_at: Instant::now(),
            },
        );
        comments
    }

    fn get_media(&self, url: &str, kind: MediaKind) -> Option<crate::media::LoadedMedia> {
        self.media
            .get(&(url.to_owned(), kind))
            .filter(|entry| is_fresh(entry.fetched_at, MEDIA_CACHE_TTL))
            .map(|entry| entry.value.clone())
    }

    fn put_media(
        &mut self,
        url: String,
        kind: MediaKind,
        media: crate::media::LoadedMedia,
    ) -> crate::media::LoadedMedia {
        self.media.insert(
            (url, kind),
            CacheEntry {
                value: media.clone(),
                fetched_at: Instant::now(),
            },
        );
        media
    }
}

impl RedditWorker {
    pub fn spawn() -> Result<(Self, Receiver<FetchEvent>), RedditError> {
        let client = RedditClient::new()?;
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        thread::spawn(move || {
            let mut cache = WorkerCache::new();
            let runtime = match Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = event_tx.send(FetchEvent::Failed {
                        request_id: 0,
                        error: RedditError::Transport(format!(
                            "failed to start async runtime: {error}"
                        )),
                    });
                    return;
                }
            };

            for command in command_rx {
                let event = runtime.block_on(handle_command(&client, &mut cache, command));
                if event_tx.send(event).is_err() {
                    break;
                }
            }
        });

        Ok((Self { sender: command_tx }, event_rx))
    }

    pub fn submit(&self, command: FetchCommand) -> Result<(), mpsc::SendError<FetchCommand>> {
        self.sender.send(command)
    }
}

async fn handle_command(
    client: &RedditClient,
    cache: &mut WorkerCache,
    command: FetchCommand,
) -> FetchEvent {
    match command {
        FetchCommand::Posts {
            request_id,
            subreddit,
            sort,
        } => {
            if let Some(posts) = cache.get_posts(&subreddit, &sort) {
                return FetchEvent::PostsLoaded {
                    request_id,
                    subreddit,
                    posts,
                };
            }
            match client.fetch_posts(&subreddit, &sort).await {
                Ok(posts) => FetchEvent::PostsLoaded {
                    request_id,
                    subreddit: subreddit.clone(),
                    posts: cache.put_posts(subreddit.clone(), sort, posts),
                },
                Err(error) => FetchEvent::Failed { request_id, error },
            }
        }
        FetchCommand::Comments {
            request_id,
            permalink,
        } => {
            if let Some(comments) = cache.get_comments(&permalink) {
                return FetchEvent::CommentsLoaded {
                    request_id,
                    comments,
                };
            }
            match client.fetch_comments(&permalink).await {
                Ok(comments) => FetchEvent::CommentsLoaded {
                    request_id,
                    comments: cache.put_comments(permalink, comments),
                },
                Err(error) => FetchEvent::Failed { request_id, error },
            }
        }
        FetchCommand::Media {
            request_id,
            url,
            kind,
        } => {
            if let Some(media) = cache.get_media(&url, kind) {
                return FetchEvent::MediaLoaded {
                    request_id,
                    url,
                    media,
                };
            }
            match client.fetch_media(&url, kind).await {
                Ok(media) => FetchEvent::MediaLoaded {
                    request_id,
                    url: url.clone(),
                    media: cache.put_media(url, kind, media),
                },
                Err(error) => FetchEvent::MediaFailed {
                    request_id,
                    url,
                    error,
                },
            }
        }
    }
}

fn is_fresh(fetched_at: Instant, ttl: Duration) -> bool {
    fetched_at.elapsed() <= ttl
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{is_fresh, media_url_candidates, preview_to_direct_reddit_image};
    use crate::models::MediaKind;

    #[test]
    fn cache_entries_expire_after_ttl() {
        assert!(is_fresh(
            Instant::now() - Duration::from_secs(5),
            Duration::from_secs(10)
        ));
        assert!(!is_fresh(
            Instant::now() - Duration::from_secs(11),
            Duration::from_secs(10)
        ));
    }

    #[test]
    fn media_candidates_include_reddit_fallbacks() {
        let url = "https://external-preview.redd.it/abc123?width=640&amp;format=png&amp;auto=webp";
        let candidates = media_url_candidates(url, MediaKind::Image);
        assert!(candidates.iter().any(|value| value
            .contains("external-preview.redd.it/abc123?width=640&format=png&auto=webp")));
        assert!(candidates
            .iter()
            .any(|value| value.contains("preview.redd.it/abc123?width=640&format=png&auto=webp")));
        assert!(candidates
            .iter()
            .any(|value| value == "https://i.redd.it/abc123.png"));
    }

    #[test]
    fn direct_image_from_preview_query_is_derived() {
        let preview = "https://preview.redd.it/file_id?width=320&format=jpg&auto=webp&s=hash";
        let direct = preview_to_direct_reddit_image(preview).expect("direct url");
        assert_eq!(direct, "https://i.redd.it/file_id.jpg");
    }
}
