use std::io::Cursor;

use image::{DynamicImage, Rgba, RgbaImage};
use mic_message::ImageFormat;

use super::*;

fn encode(image: DynamicImage, format: image::ImageFormat) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, format).unwrap();
    out.into_inner()
}

fn gradient(width: u32, height: u32, alpha: u8) -> DynamicImage {
    DynamicImage::ImageRgba8(RgbaImage::from_fn(width, height, |x, y| {
        Rgba([(x % 256) as u8, (y % 256) as u8, 128, alpha])
    }))
}

fn dimensions(bytes: &[u8]) -> (u32, u32) {
    let image = image::load_from_memory(bytes).unwrap();
    (image.width(), image.height())
}

#[test]
fn small_compliant_image_is_kept() {
    let bytes = encode(gradient(64, 48, 255), image::ImageFormat::Png);
    let image = normalize(bytes.clone()).unwrap();
    assert_eq!(image.format, ImageFormat::Png);
    assert_eq!(&*image.bytes, &bytes[..]);
}

#[test]
fn large_opaque_screenshot_becomes_bounded_jpeg() {
    let bytes = encode(gradient(2100, 1050, 255), image::ImageFormat::Png);
    let image = normalize(bytes).unwrap();
    assert_eq!(image.format, ImageFormat::Jpeg);
    assert_eq!(dimensions(&image.bytes), (2048, 1024));
    assert_eq!(normalize(image.bytes.to_vec()).unwrap().bytes, image.bytes);
}

#[test]
fn transparency_stays_png() {
    let bytes = encode(gradient(2100, 1050, 128), image::ImageFormat::Png);
    let image = normalize(bytes).unwrap();
    assert_eq!(image.format, ImageFormat::Png);
    assert_eq!(dimensions(&image.bytes), (2048, 1024));
}

#[test]
fn gif_becomes_first_frame_jpeg() {
    let bytes = encode(gradient(40, 30, 255), image::ImageFormat::Gif);
    let image = normalize(bytes).unwrap();
    assert_eq!(image.format, ImageFormat::Jpeg);
    assert_eq!(dimensions(&image.bytes), (40, 30));
}

#[test]
fn rejects_garbage_and_oversize() {
    assert_eq!(
        normalize(b"not an image".to_vec()).unwrap_err(),
        MediaError::Unsupported
    );
    assert_eq!(
        normalize(vec![0; MAX_IMAGE_BYTES + 1]).unwrap_err(),
        MediaError::TooManyBytes
    );
}
