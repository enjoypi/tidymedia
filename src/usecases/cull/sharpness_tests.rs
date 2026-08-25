use super::*;

#[test]
fn variance_zero_on_uniform_image() {
    let img = image::GrayImage::from_pixel(10, 10, image::Luma([128]));
    assert!(laplacian_variance(&img).abs() < f32::EPSILON);
}

#[test]
fn variance_zero_on_empty_image() {
    let img = image::GrayImage::new(0, 0);
    assert!(laplacian_variance(&img).abs() < f32::EPSILON);
}

#[test]
fn variance_zero_on_zero_height_image() {
    let img = image::GrayImage::new(5, 0);
    assert!(laplacian_variance(&img).abs() < f32::EPSILON);
}

#[test]
fn variance_zero_on_zero_width_image() {
    let img = image::GrayImage::new(0, 5);
    assert!(laplacian_variance(&img).abs() < f32::EPSILON);
}

#[test]
fn variance_high_on_checker_pattern() {
    let mut img = image::GrayImage::new(8, 8);
    for y in 0..8 {
        for x in 0..8 {
            let v = if (x + y) % 2 == 0 { 0 } else { 255 };
            img.put_pixel(x, y, image::Luma([v]));
        }
    }
    let var = laplacian_variance(&img);
    assert!(var > 1000.0, "got: {var}");
}
