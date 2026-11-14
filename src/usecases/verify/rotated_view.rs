use image::{GenericImageView, Rgb, RgbImage};

pub(super) struct RotatedView<'a> {
    image: &'a RgbImage,
    quarter_turns: usize,
}

impl<'a> RotatedView<'a> {
    pub(super) fn new(image: &'a RgbImage, quarter_turns: usize) -> Self {
        Self {
            image,
            quarter_turns: quarter_turns % 4,
        }
    }
}

impl GenericImageView for RotatedView<'_> {
    type Pixel = Rgb<u8>;

    fn dimensions(&self) -> (u32, u32) {
        let (width, height) = self.image.dimensions();
        if self.quarter_turns.is_multiple_of(2) {
            (width, height)
        } else {
            (height, width)
        }
    }

    fn get_pixel(&self, x: u32, y: u32) -> Rgb<u8> {
        let (width, height) = self.image.dimensions();
        let (sx, sy) = match self.quarter_turns {
            0 => (x, y),
            1 => (y, height - 1 - x),
            2 => (width - 1 - x, height - 1 - y),
            _ => (width - 1 - y, x),
        };
        *self.image.get_pixel(sx, sy)
    }
}

#[cfg(test)]
#[path = "rotated_view_tests.rs"]
mod tests;
