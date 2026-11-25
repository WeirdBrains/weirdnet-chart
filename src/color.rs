//! Colours.

/// A straight (non-premultiplied) sRGB colour with alpha, each channel from 0.0 to 1.0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    /// Red.
    pub r: f32,
    /// Green.
    pub g: f32,
    /// Blue.
    pub b: f32,
    /// Alpha: 0.0 is transparent, 1.0 opaque.
    pub a: f32,
}

impl Rgba {
    /// Fully transparent.
    pub const TRANSPARENT: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    /// An opaque colour from a `0xRRGGBB` value.
    pub const fn hex(rgb: u32) -> Rgba {
        Rgba { r: ((rgb >> 16) & 0xff) as f32 / 255.0, g: ((rgb >> 8) & 0xff) as f32 / 255.0, b: (rgb & 0xff) as f32 / 255.0, a: 1.0 }
    }

    /// The same colour with a different alpha.
    pub const fn with_alpha(self, a: f32) -> Rgba {
        Rgba { a, ..self }
    }

    /// This colour composited over an opaque `background`; the result is opaque.
    /// Useful for backends without alpha blending, such as terminals.
    pub fn over(self, background: Rgba) -> Rgba {
        let a = self.a.clamp(0.0, 1.0);
        Rgba { r: self.r * a + background.r * (1.0 - a), g: self.g * a + background.g * (1.0 - a), b: self.b * a + background.b * (1.0 - a), a: 1.0 }
    }

    /// The channels as 8-bit values `(r, g, b)`, ignoring alpha.
    pub fn to_rgb8(self) -> (u8, u8, u8) {
        let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        (c(self.r), c(self.g), c(self.b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_and_blend() {
        let red = Rgba::hex(0xff0000);
        assert_eq!(red.to_rgb8(), (255, 0, 0));
        let half = red.with_alpha(0.5).over(Rgba::hex(0x000000));
        assert_eq!(half.to_rgb8(), (128, 0, 0));
        assert_eq!(half.a, 1.0);
    }
}
