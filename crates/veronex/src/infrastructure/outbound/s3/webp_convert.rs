use anyhow::Context;
use base64::Engine as _;
use image::ImageReader;

fn decode_base64_image(b64: &str) -> anyhow::Result<image::DynamicImage> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("base64 decode failed")?;
    ImageReader::new(std::io::Cursor::new(&raw))
        .with_guessed_format()
        .context("image format detection failed")?
        .decode()
        .context("image decode failed")
}

/// Decode a base64-encoded image (JPEG/PNG) and re-encode as WebP.
/// Returns `(full_webp, thumbnail_webp)`.
///
/// - Full: re-encoded as WebP (default quality)
/// - Thumbnail: resized to fit 128×128 (aspect-preserved)
pub fn base64_to_webp_pair(b64: &str) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let img = decode_base64_image(b64)?;

    // Full-size WebP
    let full = encode_webp(&img)?;

    // Thumbnail (128px max dimension, aspect-preserved)
    let thumb_img = img.thumbnail(128, 128);
    let thumb = encode_webp(&thumb_img)?;

    Ok((full, thumb))
}

/// Encode image as WebP using the `image` crate's default quality.
/// The `image` crate's WebP encoder does not expose a quality parameter;
/// its default quality is sufficient for both storage and inference.
fn encode_webp(img: &image::DynamicImage) -> anyhow::Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::WebP)
        .context("WebP encode failed")?;
    Ok(buf.into_inner())
}

/// Compress a base64-encoded image to WebP, resizing the longest edge to `max_longest_edge`.
///
/// - Landscape (W>H): width → `max_longest_edge`, height scaled proportionally
/// - Portrait  (H>W): height → `max_longest_edge`, width scaled proportionally
/// - Square:           both → `max_longest_edge`
/// - Already smaller:  no resize, only re-encode to WebP
///
/// Always re-encodes to WebP (~25-35% smaller than JPEG at equivalent quality).
/// Vision models internally resize to 336–1414px — caller controls the edge limit.
pub fn compress_base64_image(b64: &str, max_longest_edge: u32) -> anyhow::Result<String> {
    let img = decode_base64_image(b64)?;
    let longest = img.width().max(img.height());
    let resized = if longest > max_longest_edge {
        // thumbnail() constrains the longest edge and preserves aspect ratio
        img.thumbnail(max_longest_edge, max_longest_edge)
    } else {
        img
    };

    let webp = encode_webp(&resized)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(webp))
}
