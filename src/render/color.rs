#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    #[inline]
    pub const fn argb(self) -> u32 {
        self.0
    }
}

pub const BLACK: Color = Color::rgb(0, 0, 0);
pub const WHITE: Color = Color::rgb(255, 255, 255);
pub const RED: Color = Color::rgb(255, 0, 0);
pub const GREEN: Color = Color::rgb(0, 255, 0);
pub const BLUE: Color = Color::rgb(0, 0, 255);
pub const GRAY: Color = Color::rgb(127, 127, 127);
pub const DARK_GRAY: Color = Color::rgb(48, 48, 48);
pub const LIGHT_GRAY: Color = Color::rgb(200, 200, 200);
