use super::*;

#[test]
fn u32_from_f32_clamped_handles_nan_negative_inf_and_overflow() {
    assert_eq!(u32_from_f32_clamped(f32::NAN, 100), 0);
    assert_eq!(u32_from_f32_clamped(f32::INFINITY, 100), 100);
    assert_eq!(u32_from_f32_clamped(f32::NEG_INFINITY, 100), 0);
    assert_eq!(u32_from_f32_clamped(-1.0, 100), 0);
    assert_eq!(u32_from_f32_clamped(50.0, 100), 50);
    assert_eq!(u32_from_f32_clamped(150.0, 100), 100);
}

#[test]
fn crop_face_bbox_returns_1x1_when_x_or_y_inverted() {
    let image = image::RgbImage::from_pixel(20, 20, image::Rgb([0; 3]));
    let landmarks = [[0.0_f32; 2]; 5];
    let x_inverted = crate::FaceDetection {
        bbox: [10.0, 0.0, 5.0, 20.0],
        score: 0.9,
        landmarks_5pt: landmarks,
    };
    let y_inverted = crate::FaceDetection {
        bbox: [0.0, 10.0, 20.0, 5.0],
        score: 0.9,
        landmarks_5pt: landmarks,
    };
    assert_eq!(crop_face_bbox(&image, &x_inverted).dimensions(), (1, 1));
    assert_eq!(crop_face_bbox(&image, &y_inverted).dimensions(), (1, 1));
}

#[test]
fn crop_eye_around_returns_1x1_when_center_off_image() {
    let image = image::RgbImage::from_pixel(20, 20, image::Rgb([0; 3]));
    let off_x = crop_eye_around(&image, [-100.0, 10.0], 1.0);
    assert_eq!(off_x.dimensions(), (1, 1));
    let off_y = crop_eye_around(&image, [10.0, -100.0], 1.0);
    assert_eq!(off_y.dimensions(), (1, 1));
}

#[test]
fn crop_eye_around_returns_box_when_center_inside_image() {
    let image = image::RgbImage::from_pixel(40, 40, image::Rgb([0; 3]));
    let crop = crop_eye_around(&image, [20.0, 20.0], 5.0);
    assert_eq!(crop.dimensions(), (10, 10));
}
