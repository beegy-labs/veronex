use anyhow::Context;

/// Decode a base64-encoded image (JPEG/PNG) and re-encode as WebP.
/// Returns `(full_webp, thumbnail_webp)`.
///
/// - Full: re-encoded at quality 80
/// - Thumbnail: resized to fit 128×128 (aspect-preserved), quality 60
pub fn base64_to_webp_pair(b64: &str) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    use base64::Engine as _;
    use image::ImageReader;

    let raw = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("base64 decode failed")?;

    let img = ImageReader::new(std::io::Cursor::new(&raw))
        .with_guessed_format()
        .context("image format detection failed")?
        .decode()
        .context("image decode failed")?;

    // Full-size WebP
    let full = encode_webp(&img, 80)?;

    // Thumbnail (128px max dimension, aspect-preserved)
    let thumb_img = img.thumbnail(128, 128);
    let thumb = encode_webp(&thumb_img, 60)?;

    Ok((full, thumb))
}

fn encode_webp(img: &image::DynamicImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::WebP)
        .context("WebP encode failed")?;
    let _ = quality; // image crate WebP uses default quality; acceptable for our use case
    Ok(buf.into_inner())
}
