use serde::Deserialize;

use crate::models::{Comment, MediaKind, Post, PostMedia};

#[derive(Debug, Deserialize)]
pub struct Listing {
    pub data: ListingData,
}

#[derive(Debug, Deserialize)]
pub struct ListingData {
    pub children: Vec<Child>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
pub enum Child {
    #[serde(rename = "t3")]
    Post { data: RawPost },
    #[serde(rename = "t1")]
    Comment { data: RawComment },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct RawPost {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subreddit: Option<String>,
    pub score: Option<i64>,
    pub num_comments: Option<u64>,
    pub url: Option<String>,
    pub url_overridden_by_dest: Option<String>,
    #[serde(default)]
    pub selftext: String,
    pub permalink: Option<String>,
    #[serde(default)]
    pub is_self: bool,
    #[serde(default)]
    pub over_18: bool,
    #[serde(default)]
    pub spoiler: bool,
    #[serde(default)]
    pub stickied: bool,
    pub preview: Option<RawPreview>,
}

#[derive(Debug, Deserialize)]
pub struct RawPreview {
    #[serde(default)]
    pub images: Vec<RawPreviewImage>,
}

#[derive(Debug, Deserialize)]
pub struct RawPreviewImage {
    pub source: RawPreviewSource,
}

#[derive(Debug, Deserialize)]
pub struct RawPreviewSource {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct RawComment {
    pub author: Option<String>,
    pub body: Option<String>,
    pub score: Option<i64>,
    #[serde(default)]
    pub depth: u32,
    #[serde(default)]
    pub replies: RawReplies,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum RawReplies {
    Empty(String),
    Listing(Listing),
}

impl Default for RawReplies {
    fn default() -> Self {
        Self::Empty(String::new())
    }
}

impl RawPost {
    pub fn into_post(self) -> Option<Post> {
        let media = self.resolve_media();
        let url = self
            .url_overridden_by_dest
            .clone()
            .or_else(|| self.url.clone())
            .map(|value| decode_reddit_url(&value))
            .unwrap_or_default();
        let title = self.title.filter(|value| !value.trim().is_empty())?;
        let subreddit = self.subreddit.filter(|value| !value.trim().is_empty())?;
        let permalink = self.permalink.filter(|value| !value.trim().is_empty())?;

        Some(Post {
            title,
            author: normalize_author(self.author),
            subreddit,
            score: self.score.unwrap_or_default(),
            num_comments: self.num_comments.unwrap_or_default(),
            url,
            selftext: self.selftext.trim().to_owned(),
            permalink,
            is_self: self.is_self,
            is_nsfw: self.over_18,
            is_spoiler: self.spoiler,
            is_stickied: self.stickied,
            media,
        })
    }

    fn resolve_media(&self) -> Option<PostMedia> {
        if self.is_self {
            return None;
        }

        for candidate in [&self.url_overridden_by_dest, &self.url] {
            if let Some(url) = candidate.as_ref() {
                if MediaKind::detect_url(url) == Some(MediaKind::Gif) {
                    return Some(PostMedia {
                        url: decode_reddit_url(url),
                        kind: MediaKind::Gif,
                    });
                }
            }
        }

        if let Some(image) = self
            .preview
            .as_ref()
            .and_then(|preview| preview.images.first())
        {
            return Some(PostMedia {
                url: decode_reddit_url(&image.source.url),
                kind: MediaKind::Image,
            });
        }

        for candidate in [&self.url_overridden_by_dest, &self.url] {
            if let Some(url) = candidate.as_ref() {
                if MediaKind::detect_url(url) == Some(MediaKind::Image) {
                    return Some(PostMedia {
                        url: decode_reddit_url(url),
                        kind: MediaKind::Image,
                    });
                }
            }
        }

        None
    }
}

impl RawComment {
    pub fn into_comment(self) -> Comment {
        self.into_parts().0
    }

    fn into_parts(self) -> (Comment, Option<Listing>) {
        let author = normalize_author(self.author);
        let raw_body = self.body.unwrap_or_default();
        let trimmed = raw_body.trim();
        let is_deleted = author == "[deleted]";
        let is_removed = trimmed.eq_ignore_ascii_case("[removed]");
        let body = if is_removed {
            String::from("[removed]")
        } else if trimmed.is_empty() {
            String::from("[deleted]")
        } else {
            raw_body
        };

        let comment = Comment {
            author,
            body,
            score: self.score.unwrap_or_default(),
            depth: self.depth,
            is_deleted,
            is_removed,
        };
        let replies = match self.replies {
            RawReplies::Empty(_) => None,
            RawReplies::Listing(listing) => Some(listing),
        };

        (comment, replies)
    }
}

pub fn posts_from_listing(listing: Listing) -> Vec<Post> {
    listing
        .data
        .children
        .into_iter()
        .filter_map(|child| match child {
            Child::Post { data } => data.into_post(),
            _ => None,
        })
        .collect()
}

pub fn comments_from_listings(listings: Vec<Listing>) -> Vec<Comment> {
    let mut comments = Vec::new();
    for listing in listings.into_iter().skip(1) {
        for child in listing.data.children {
            push_comment_child(child, &mut comments);
        }
    }
    comments
}

fn push_comment_child(child: Child, comments: &mut Vec<Comment>) {
    if let Child::Comment { data } = child {
        let (comment, replies) = data.into_parts();
        comments.push(comment);
        if let Some(listing) = replies {
            for reply in listing.data.children {
                push_comment_child(reply, comments);
            }
        }
    }
}

fn normalize_author(author: Option<String>) -> String {
    author
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| String::from("[deleted]"))
}

fn decode_reddit_url(url: &str) -> String {
    url.replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use crate::models::MediaKind;

    use super::{comments_from_listings, posts_from_listing, Listing};

    #[test]
    fn parses_post_fixture_into_domain_models() {
        let fixture = include_str!("../tests/fixtures/posts.json");
        let listing: Listing = serde_json::from_str(fixture).expect("valid posts fixture");
        let posts = posts_from_listing(listing);

        assert_eq!(posts.len(), 2);
        assert_eq!(posts[0].title, "Rust milestone planning");
        assert_eq!(posts[0].author, "alice");
        assert!(posts[0].is_self);
        assert!(posts[0].is_spoiler);
        assert!(!posts[0].url.is_empty());
        assert!(posts[0].media.is_none());

        assert_eq!(posts[1].author, "[deleted]");
        assert!(posts[1].is_stickied);
        assert!(posts[1].is_nsfw);
        assert_eq!(
            posts[1].media.as_ref().map(|media| media.kind),
            Some(MediaKind::Image)
        );
    }

    #[test]
    fn parses_comment_fixture_and_normalizes_deleted_removed_comments() {
        let fixture = include_str!("../tests/fixtures/comments.json");
        let listings: Vec<Listing> = serde_json::from_str(fixture).expect("valid comments fixture");
        let comments = comments_from_listings(listings);

        assert_eq!(comments.len(), 3);
        assert_eq!(comments[0].author, "bob");
        assert_eq!(comments[0].depth, 0);
        assert_eq!(comments[1].author, "[deleted]");
        assert_eq!(comments[1].body, "[deleted]");
        assert!(comments[1].is_deleted);
        assert_eq!(comments[2].body, "[removed]");
        assert!(comments[2].is_removed);
    }

    #[test]
    fn ignores_non_post_and_non_comment_listing_entries() {
        let post_fixture = include_str!("../tests/fixtures/posts.json");
        let listing: Listing = serde_json::from_str(post_fixture).expect("valid posts fixture");
        let posts = posts_from_listing(listing);

        assert!(posts.iter().all(|post| !post.title.is_empty()));
    }
}
