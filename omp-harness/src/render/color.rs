#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color(pub u32);

impl Color {
    const ANSI_INDEX_ALPHA: u32 = 0x01;

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32)
    }

    pub const fn ansi_index(index: u8) -> Self {
        Self((Self::ANSI_INDEX_ALPHA << 24) | index as u32)
    }

    pub const fn ansi_index_value(self) -> Option<u8> {
        if self.0 >> 24 == Self::ANSI_INDEX_ALPHA {
            Some((self.0 & 0xff) as u8)
        } else {
            None
        }
    }

    #[inline]
    pub const fn argb(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn rgb_components(self) -> (u8, u8, u8) {
        let value = self.0;
        (
            ((value >> 16) & 0xff) as u8,
            ((value >> 8) & 0xff) as u8,
            (value & 0xff) as u8,
        )
    }

    #[inline]
    pub const fn negative(self) -> Self {
        if self.0 >> 24 == Self::ANSI_INDEX_ALPHA {
            return self;
        }

        let (r, g, b) = self.rgb_components();
        let alpha = ((self.0 >> 24) & 0xff) as u8;
        Self::rgba(u8::MAX - r, u8::MAX - g, u8::MAX - b, alpha)
    }
}
