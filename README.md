# reddit-tui

A terminal Reddit browser written in Rust with background loading, a split-pane post reader, and keyboard-first navigation. Fully vibecoded.

## Features

- Browse any public subreddit with `hot`, `new`, `top`, or `rising` sorting
- Keep the UI responsive while posts and comments load in the background
- Read posts in a split pane with metadata and preview content
- Open threaded comments with wrapped, depth-aware rendering
- Reopen recent subreddits from the input screen
- View an in-app help overlay with `?`

## Requirements

- Rust stable toolchain
- Network access to `reddit.com`

## Run

```bash
cargo run --release
```

On startup, enter a subreddit name such as `rust`, `programming`, or `news`.

## Usage Notes

- Fetches run on a background worker thread, so navigation and redraws stay active during slow network requests.
- The post list uses a split layout: the left pane is the navigable list, and the right pane previews the selected post.
- Self posts show selftext in the preview pane. Link posts show the outbound URL preview instead.
- Use `?` on any screen to toggle the help modal.

## Keybindings

### Global

| Key | Action |
| --- | --- |
| `?` | Toggle help |

### Subreddit Input

| Key | Action |
| --- | --- |
| Type text | Edit subreddit name |
| `Enter` | Load subreddit |
| `Tab` | Cycle backward through recent subreddits |
| `Shift+Tab` | Cycle forward through recent subreddits |
| `Esc` | Cancel input and return |
| `q` | Quit |

### Post List

| Key | Action |
| --- | --- |
| `Up` / `k` | Move selection up |
| `Down` / `j` | Move selection down |
| `PageUp` / `u` | Page up |
| `PageDown` / `d` | Page down |
| `Enter` | Open comments |
| `s` | Cycle sort |
| `1` | Set sort to `hot` |
| `2` | Set sort to `new` |
| `3` | Set sort to `top` |
| `4` | Set sort to `rising` |
| `/` | Open subreddit input with current subreddit prefilled |
| `r` | Refresh posts |
| `q` / `Esc` | Quit |

### Comments

| Key | Action |
| --- | --- |
| `Up` / `k` | Move selection up |
| `Down` / `j` | Move selection down |
| `PageUp` / `u` | Page up |
| `PageDown` / `d` | Page down |
| `r` | Reload comments |
| `Esc` / `b` | Return to posts |
| `q` | Quit |

## Development

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test
```

## Stack

- [`ratatui`](https://github.com/ratatui/ratatui) for terminal UI rendering
- [`crossterm`](https://github.com/crossterm-rs/crossterm) for input and terminal control
- [`reqwest`](https://github.com/seanmonstar/reqwest) for async HTTP requests
- [`tokio`](https://github.com/tokio-rs/tokio) for the background async runtime
- [`serde`](https://github.com/serde-rs/serde) and [`serde_json`](https://github.com/serde-rs/json) for JSON parsing
