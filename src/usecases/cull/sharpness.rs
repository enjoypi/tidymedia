//! 全图/局部清晰度估算：3×3 离散 Laplacian 卷积后取响应方差。
//!
//! 方差越高边缘信息越丰富即越清晰；连拍/相似组内对方差排序即可选最清晰一张。
//! 边界 clamp 镜像（避免引入边缘人工 0 影响低分辨率小图）。

/// `kernel = [0,-1,0; -1,4,-1; 0,-1,0]` 对灰度图卷积，输出响应方差。
/// `variance = E[x²] - E[x]²`。空图返 0.0。
#[must_use]
pub(crate) fn laplacian_variance(luma: &image::GrayImage) -> f32 {
    let width = luma.width();
    let height = luma.height();
    if width == 0 || height == 0 {
        return 0.0;
    }
    let mut sum: f64 = 0.0;
    let mut sum_sq: f64 = 0.0;
    let total = f64::from(width) * f64::from(height);
    for y in 0..height {
        for x in 0..width {
            let center = f64::from(sample(luma, x, y, width, height));
            let north = f64::from(sample(luma, x, y.wrapping_sub(1), width, height));
            let south = f64::from(sample(luma, x, y.saturating_add(1), width, height));
            let left = f64::from(sample(luma, x.wrapping_sub(1), y, width, height));
            let right = f64::from(sample(luma, x.saturating_add(1), y, width, height));
            let response = 4.0 * center - north - south - left - right;
            sum += response;
            sum_sq += response * response;
        }
    }
    let mean = sum / total;
    let variance = (sum_sq / total) - mean * mean;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "方差通常 < 1e5，f32 精度足够"
    )]
    let result = variance.max(0.0) as f32;
    result
}

/// clamp 边界采样（`wrapping_sub` 让 y=0 时邻居 y-1 = `u32::MAX` → clamp 到 0）。
fn sample(luma: &image::GrayImage, x: u32, y: u32, width: u32, height: u32) -> u8 {
    let clamped_x = x.min(width - 1);
    let clamped_y = y.min(height - 1);
    luma.get_pixel(clamped_x, clamped_y).0[0]
}

#[cfg(test)]
mod tests {
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
}
