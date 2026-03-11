use std::io::Cursor;
use std::sync::Mutex;

use image::{
    codecs::gif::GifDecoder,
    imageops::{self, FilterType},
    AnimationDecoder, DynamicImage, GenericImageView, ImageFormat, Rgba, RgbaImage,
};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::models::MediaKind;

#[derive(Debug)]
pub struct LoadedMedia {
    pub kind: MediaKind,
    pub frames: Vec<MediaFrame>,
    render_cache: Mutex<Vec<RenderedFrame>>,
}

#[derive(Debug, Clone)]
pub struct MediaFrame {
    pub image: RgbaImage,
    pub delay_ms: u32,
}

#[derive(Debug, Clone)]
struct RenderedFrame {
    width: u16,
    height: u16,
    frame_index: usize,
    lines: Vec<Line<'static>>,
}

impl Clone for LoadedMedia {
    fn clone(&self) -> Self {
        Self {
            kind: self.kind,
            frames: self.frames.clone(),
            render_cache: Mutex::new(Vec::new()),
        }
    }
}

pub fn decode_media(bytes: &[u8], kind: MediaKind) -> Result<LoadedMedia, String> {
    match kind {
        MediaKind::Gif => decode_gif(bytes),
        MediaKind::Image => decode_image(bytes),
    }
}

fn decode_image(bytes: &[u8]) -> Result<LoadedMedia, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|error| format!("unable to decode image: {error}"))?
        .to_rgba8();
    Ok(LoadedMedia {
        kind: MediaKind::Image,
        frames: vec![MediaFrame { image, delay_ms: 0 }],
        render_cache: Mutex::new(Vec::new()),
    })
}

fn decode_gif(bytes: &[u8]) -> Result<LoadedMedia, String> {
    let format =
        image::guess_format(bytes).map_err(|error| format!("invalid media format: {error}"))?;
    if format != ImageFormat::Gif {
        return decode_image(bytes);
    }

    let decoder = GifDecoder::new(Cursor::new(bytes))
        .map_err(|error| format!("unable to decode gif: {error}"))?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(|error| format!("unable to decode gif frames: {error}"))?;
    let frames = frames
        .into_iter()
        .map(|frame| MediaFrame {
            delay_ms: frame.delay().numer_denom_ms().0.max(80),
            image: frame.into_buffer(),
        })
        .collect::<Vec<_>>();

    if frames.is_empty() {
        return Err(String::from("gif contained no frames"));
    }

    Ok(LoadedMedia {
        kind: MediaKind::Gif,
        frames,
        render_cache: Mutex::new(Vec::new()),
    })
}

pub fn render_lines(
    media: &LoadedMedia,
    width: u16,
    height: u16,
    elapsed_ms: u128,
) -> Vec<Line<'static>> {
    if width == 0 || height == 0 || media.frames.is_empty() {
        return Vec::new();
    }

    let frame_index = select_frame_index(media, elapsed_ms);
    if let Ok(cache) = media.render_cache.lock() {
        if let Some(entry) = cache.iter().find(|entry| {
            entry.width == width && entry.height == height && entry.frame_index == frame_index
        }) {
            return entry.lines.clone();
        }
    }

    let frame = &media.frames[frame_index];
    let lines = render_half_block_image(frame, width, height, media.kind == MediaKind::Image);

    if let Ok(mut cache) = media.render_cache.lock() {
        cache.push(RenderedFrame {
            width,
            height,
            frame_index,
            lines: lines.clone(),
        });
        if cache.len() > 48 {
            let remove_count = cache.len().saturating_sub(48);
            cache.drain(0..remove_count);
        }
    }

    lines
}

fn render_half_block_image(
    frame: &MediaFrame,
    width: u16,
    height: u16,
    sharpen: bool,
) -> Vec<Line<'static>> {
    let mut resized = scale_frame(
        &frame.image,
        width.max(1) as u32,
        height.saturating_mul(2).max(2) as u32,
    );
    if sharpen {
        resized = imageops::unsharpen(&resized, 1.1, 2);
    }
    let rendered_height = resized.height().div_ceil(2) as usize;
    let horizontal_padding = width.saturating_sub(resized.width() as u16) as usize / 2;
    let vertical_padding = height.saturating_sub(rendered_height as u16) as usize / 2;
    let mut lines = Vec::new();

    for _ in 0..vertical_padding {
        lines.push(Line::from(" ".repeat(width as usize)));
    }

    let max_y = resized.height().max(2);
    for y in (0..max_y).step_by(2) {
        let mut spans = Vec::new();
        if horizontal_padding > 0 {
            spans.push(Span::raw(" ".repeat(horizontal_padding)));
        }
        for x in 0..resized.width() {
            let top = *resized.get_pixel(x, y);
            let bottom = *resized.get_pixel(x, (y + 1).min(resized.height().saturating_sub(1)));
            spans.push(pixel_span(top, bottom));
        }
        let used_width = horizontal_padding + resized.width() as usize;
        if used_width < width as usize {
            spans.push(Span::raw(" ".repeat(width as usize - used_width)));
        }
        lines.push(Line::from(spans));
    }

    while lines.len() < height as usize {
        lines.push(Line::from(" ".repeat(width as usize)));
    }

    lines
}

fn scale_frame(image: &RgbaImage, max_width: u32, max_height: u32) -> RgbaImage {
    let dynamic = DynamicImage::ImageRgba8(image.clone());
    let (source_width, source_height) = dynamic.dimensions();
    let width_ratio = max_width as f32 / source_width.max(1) as f32;
    let height_ratio = max_height as f32 / source_height.max(1) as f32;
    let scale = width_ratio.min(height_ratio).max(0.01);
    let filter = if scale >= 1.0 {
        FilterType::Nearest
    } else {
        FilterType::CatmullRom
    };

    let target_width = ((source_width as f32 * scale).round() as u32).clamp(1, max_width);
    let target_height = ((source_height as f32 * scale).round() as u32).clamp(1, max_height);
    dynamic
        .resize_exact(target_width, target_height, filter)
        .to_rgba8()
}

pub fn current_frame_delay_ms(media: &LoadedMedia, elapsed_ms: u128) -> u32 {
    media.frames[select_frame_index(media, elapsed_ms)]
        .delay_ms
        .max(80)
}

fn select_frame_index(media: &LoadedMedia, elapsed_ms: u128) -> usize {
    if media.kind != MediaKind::Gif || media.frames.len() == 1 {
        return 0;
    }

    let total_duration = media
        .frames
        .iter()
        .map(|frame| frame.delay_ms.max(1) as u128)
        .sum::<u128>()
        .max(1);
    let mut offset = elapsed_ms % total_duration;

    for (index, frame) in media.frames.iter().enumerate() {
        let delay = frame.delay_ms.max(1) as u128;
        if offset < delay {
            return index;
        }
        offset -= delay;
    }

    0
}

fn pixel_span(top: Rgba<u8>, bottom: Rgba<u8>) -> Span<'static> {
    if transparent(top) && transparent(bottom) {
        return Span::raw(" ");
    }

    Span::styled(
        "▀",
        Style::default()
            .fg(to_color(top))
            .bg(if transparent(bottom) {
                Color::Reset
            } else {
                to_color(bottom)
            }),
    )
}

fn transparent(pixel: Rgba<u8>) -> bool {
    pixel[3] == 0
}

fn to_color(pixel: Rgba<u8>) -> Color {
    if pixel[3] == 0 {
        Color::Reset
    } else {
        Color::Rgb(pixel[0], pixel[1], pixel[2])
    }
}
