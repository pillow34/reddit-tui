#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    Image,
    Gif,
}

impl MediaKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MediaKind::Image => "image",
            MediaKind::Gif => "gif",
        }
    }

    pub fn detect_url(url: &str) -> Option<Self> {
        let lower = url.to_ascii_lowercase();
        if lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".webp")
            || lower.contains(".png?")
            || lower.contains(".jpg?")
            || lower.contains(".jpeg?")
            || lower.contains(".webp?")
        {
            Some(MediaKind::Image)
        } else if lower.ends_with(".gif") || lower.contains(".gif?") {
            Some(MediaKind::Gif)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostMedia {
    pub url: String,
    pub kind: MediaKind,
}

#[derive(Debug, Clone)]
pub struct Post {
    pub title: String,
    pub author: String,
    pub subreddit: String,
    pub score: i64,
    pub num_comments: u64,
    pub url: String,
    pub selftext: String,
    pub permalink: String,
    pub is_self: bool,
    pub is_nsfw: bool,
    pub is_spoiler: bool,
    pub is_stickied: bool,
    pub media: Option<PostMedia>,
}

impl Post {
    pub fn metadata_tags(&self) -> Vec<&'static str> {
        let mut tags = Vec::new();
        if self.is_self {
            tags.push("self");
        } else {
            tags.push("link");
        }
        if self.is_stickied {
            tags.push("stickied");
        }
        if self.is_nsfw {
            tags.push("nsfw");
        }
        if self.is_spoiler {
            tags.push("spoiler");
        }
        if let Some(media) = &self.media {
            tags.push(media.kind.as_str());
        }
        tags
    }
}

#[derive(Debug, Clone)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub score: i64,
    pub depth: u32,
    pub is_deleted: bool,
    pub is_removed: bool,
}
