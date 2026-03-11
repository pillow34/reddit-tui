use reqwest::StatusCode;

#[derive(Debug)]
pub enum RedditError {
    InvalidSubreddit(String),
    NotFound,
    RateLimited,
    Http(StatusCode),
    Parse(String),
    Transport(String),
}

impl RedditError {
    pub fn user_message(&self) -> String {
        match self {
            RedditError::InvalidSubreddit(name) => format!("r/{name} is not a valid subreddit"),
            RedditError::NotFound => String::from("Subreddit or post not found"),
            RedditError::RateLimited => String::from("Reddit rate limited the request"),
            RedditError::Http(status) => format!("Reddit returned HTTP {status}"),
            RedditError::Parse(_) => String::from("Reddit returned data the app could not read"),
            RedditError::Transport(message) => message.clone(),
        }
    }

    pub fn detail_message(&self) -> String {
        match self {
            RedditError::InvalidSubreddit(name) => format!("invalid subreddit input: {name}"),
            RedditError::NotFound => String::from("resource not found"),
            RedditError::RateLimited => String::from("received HTTP 429 from Reddit"),
            RedditError::Http(status) => format!("received HTTP {status} from Reddit"),
            RedditError::Parse(message) => message.clone(),
            RedditError::Transport(message) => message.clone(),
        }
    }
}

impl std::fmt::Display for RedditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.user_message())
    }
}

impl std::error::Error for RedditError {}
