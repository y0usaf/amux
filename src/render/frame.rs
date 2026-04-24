use crate::render::color::Color;
use crate::render::text::{blend_over, char_cell_width, TextRenderer};

pub struct Frame<'a> {
    pixels: &'a mut [u32],
    pub width: usize,
    pub height: usize,
}

impl<'a> Frame<'a> {
    pub fn new(pixels: &'a mut [u32], width: usize, height: usize) -> Self {
        assert_eq!(pixels.len(), width * height, "frame buffer size mismatch");
        Self {
            pixels,
            width,
            height,
        }
    }

    pub fn clear(&mut self, color: Color) {
        self.pixels.fill(color.argb());
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = x.saturating_add(w).min(self.width as i32).max(0) as usize;
        let y1 = y.saturating_add(h).min(self.height as i32).max(0) as usize;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for yy in y0..y1 {
            let row = yy * self.width;
            for xx in x0..x1 {
                self.pixels[row + xx] = color.argb();
            }
        }
    }

    pub fn stroke_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        let right = x.saturating_add(w.saturating_sub(1));
        let bottom = y.saturating_add(h.saturating_sub(1));
        self.hline(x, right, y, color);
        self.hline(x, right, bottom, color);
        self.vline(x, y, bottom, color);
        self.vline(right, y, bottom, color);
    }

    pub fn hline(&mut self, x0: i32, x1: i32, y: i32, color: Color) {
        if self.width == 0 || y < 0 || y >= self.height as i32 {
            return;
        }
        let start = x0.min(x1).max(0) as usize;
        let end = x0.max(x1).min(self.width as i32 - 1).max(0) as usize;
        if start > end {
            return;
        }
        let row = y as usize * self.width;
        for xx in start..=end {
            self.pixels[row + xx] = color.argb();
        }
    }

    pub fn vline(&mut self, x: i32, y0: i32, y1: i32, color: Color) {
        if self.height == 0 || x < 0 || x >= self.width as i32 {
            return;
        }
        let start = y0.min(y1).max(0) as usize;
        let end = y0.max(y1).min(self.height as i32 - 1).max(0) as usize;
        if start > end {
            return;
        }
        for yy in start..=end {
            self.pixels[yy * self.width + x as usize] = color.argb();
        }
    }

    pub fn text(&mut self, renderer: &mut TextRenderer, x: i32, y: i32, color: Color, text: &str) {
        let baseline = y + renderer.metrics.baseline;
        let mut pen_x = x;
        for ch in text.chars() {
            match ch {
                '\n' => break,
                '\t' => {
                    pen_x += renderer.metrics.cell_width * 4;
                }
                _ => {
                    let cell_w = renderer.metrics.cell_width * char_cell_width(ch);
                    self.draw_glyph(renderer, ch, pen_x, baseline, color);
                    pen_x += cell_w;
                }
            }
        }
    }

    pub fn text_cells(
        &mut self,
        renderer: &mut TextRenderer,
        col: i32,
        row: i32,
        color: Color,
        text: &str,
    ) {
        let x = col * renderer.metrics.cell_width;
        let y = row * renderer.metrics.cell_height;
        self.text(renderer, x, y, color, text);
    }

    fn draw_glyph(
        &mut self,
        renderer: &mut TextRenderer,
        ch: char,
        pen_x: i32,
        baseline: i32,
        color: Color,
    ) {
        let glyph = renderer.glyph(ch);
        if glyph.width == 0 || glyph.height == 0 {
            return;
        }
        let draw_x = pen_x + glyph.xmin;
        let draw_y = baseline - glyph.height as i32 - glyph.ymin;

        for gy in 0..glyph.height {
            let yy = draw_y + gy as i32;
            if yy < 0 || yy >= self.height as i32 {
                continue;
            }
            for gx in 0..glyph.width {
                let xx = draw_x + gx as i32;
                if xx < 0 || xx >= self.width as i32 {
                    continue;
                }
                let alpha = glyph.alpha[gy * glyph.width + gx];
                if alpha == 0 {
                    continue;
                }
                let idx = yy as usize * self.width + xx as usize;
                self.pixels[idx] = blend_over(self.pixels[idx], color.argb(), alpha);
            }
        }
    }
}
