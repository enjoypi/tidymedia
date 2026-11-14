use image::{GenericImageView, Rgb, RgbImage, imageops};

use super::RotatedView;
use crate::usecases::verify::content_diff_io::resize;
use crate::usecases::verify::phash::phash;

fn noise(width: u32, height: u32) -> RgbImage {
    let mut state = u64::from(width) * 7919 + u64::from(height);
    RgbImage::from_fn(width, height, |_, _| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        let bytes = state.to_be_bytes();
        Rgb([bytes[0], bytes[1], bytes[2]])
    })
}

fn reference(image: &RgbImage, quarter_turns: usize) -> RgbImage {
    match quarter_turns {
        0 => image.clone(),
        1 => imageops::rotate90(image),
        2 => imageops::rotate180(image),
        _ => imageops::rotate270(image),
    }
}

#[test]
fn view_matches_materialized_rotation_pixel_for_pixel() {
    for (width, height) in [(1, 1), (1, 7), (7, 1), (5, 3), (4, 4), (17, 33)] {
        let image = noise(width, height);
        for quarter_turns in 0..4 {
            let rotated = reference(&image, quarter_turns);
            let view = RotatedView::new(&image, quarter_turns);
            assert_eq!(view.dimensions(), rotated.dimensions());
            for (x, y, pixel) in rotated.enumerate_pixels() {
                assert_eq!(view.get_pixel(x, y), *pixel);
            }
        }
    }
}

#[test]
fn quarter_turns_wrap_around() {
    let image = noise(5, 3);
    let view = RotatedView::new(&image, 5);
    assert_eq!(view.dimensions(), (3, 5));
}

#[test]
fn resize_and_phash_on_view_equal_materialized_rotation_bit_for_bit() {
    for (width, height) in [(513, 300), (512, 512), (700, 1025), (33, 31)] {
        let image = noise(width, height);
        for quarter_turns in 0..4 {
            let rotated = reference(&image, quarter_turns);
            let view = RotatedView::new(&image, quarter_turns);
            assert_eq!(resize(&view, 512).as_raw(), resize(&rotated, 512).as_raw());
            assert_eq!(phash(&view), phash(&rotated));
        }
    }
}
