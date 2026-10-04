//! SVG → small RGBA square, with resvg.

use resvg::{tiny_skia, usvg};

/// A square of straight (not premultiplied) RGBA pixels, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconPixels {
    pub size: u32,
    pub rgba: Vec<[u8; 4]>,
}

impl IconPixels {
    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        self.rgba[(y * self.size + x) as usize]
    }
}

/// How much larger than the target the SVG is drawn before averaging down; smooths small shapes
/// better than drawing at 16×16 directly (the spike used a large render + downscale too).
const SUPERSAMPLE: u32 = 4;

/// Renders `svg` centered into a `size`×`size` square, keeping its aspect ratio.
pub fn render_svg(svg: &[u8], size: u32) -> Option<IconPixels> {
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let big = size * SUPERSAMPLE;
    let mut pixmap = tiny_skia::Pixmap::new(big, big)?;
    let svg_size = tree.size();
    let scale = big as f32 / svg_size.width().max(svg_size.height());
    let dx = (big as f32 - svg_size.width() * scale) / 2.0;
    let dy = (big as f32 - svg_size.height() * scale) / 2.0;
    let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // Box-filter each SUPERSAMPLE×SUPERSAMPLE block (premultiplied, so edges blend correctly).
    let src = pixmap.pixels();
    let mut rgba = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0u32; 4];
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let p = src[((y * SUPERSAMPLE + sy) * big + x * SUPERSAMPLE + sx) as usize];
                    for (s, v) in sum
                        .iter_mut()
                        .zip([p.red(), p.green(), p.blue(), p.alpha()])
                    {
                        *s += u32::from(v);
                    }
                }
            }
            let n = SUPERSAMPLE * SUPERSAMPLE;
            let a = sum[3] / n;
            // Undo premultiplication; fully transparent pixels stay black.
            let straight = |c: u32| ((c / n) * 255).checked_div(a).unwrap_or(0).min(255) as u8;
            rgba.push([
                straight(sum[0]),
                straight(sum[1]),
                straight(sum[2]),
                a as u8,
            ]);
        }
    }
    Some(IconPixels { size, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED_SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
        <rect x="0" y="0" width="10" height="5" fill="#ff0000"/></svg>"##;

    #[test]
    fn renders_and_keeps_transparency() {
        let px = render_svg(RED_SQUARE.as_bytes(), 16).unwrap();
        assert_eq!(px.rgba.len(), 256);
        assert_eq!(px.get(8, 2), [255, 0, 0, 255]); // top half is red
        assert_eq!(px.get(8, 13)[3], 0); // bottom half is transparent
    }

    #[test]
    fn garbage_is_none() {
        assert!(render_svg(b"not svg", 16).is_none());
    }
}
