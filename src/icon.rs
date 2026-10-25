//! The Snapt icon, drawn in code so the binary needs no image assets.
//!
//! It matches `assets/logo.svg`: a violet rounded square with white
//! viewfinder corners and a dot in the middle.

pub const SIZE: u32 = 64;

const TOP: [f32; 3] = [139.0, 92.0, 246.0]; // #8B5CF6
const BOTTOM: [f32; 3] = [91.0, 33.0, 182.0]; // #5B21B6

/// Renders the icon as RGBA pixels, `SIZE` by `SIZE`.
pub fn rgba() -> Vec<u8> {
    let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    let s = SIZE as f32;
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Sample the pixel centre in a 0..1 coordinate space.
            let (u, v) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
            let aa = 1.0 / s;

            let bg = coverage(rounded_square(u, v, 0.5, 0.22), aa);
            let mark = coverage(viewfinder(u, v), aa);

            let mut rgb = [0.0; 3];
            for (i, c) in rgb.iter_mut().enumerate() {
                let base = TOP[i] + (BOTTOM[i] - TOP[i]) * v;
                *c = base + (255.0 - base) * mark;
            }
            pixels.extend(rgb.map(|c| c.round() as u8));
            pixels.push((bg * 255.0).round() as u8);
        }
    }
    pixels
}

/// Turns a signed distance into an anti-aliased coverage value in 0..1.
fn coverage(distance: f32, aa: f32) -> f32 {
    (0.5 - distance / aa).clamp(0.0, 1.0)
}

/// Signed distance to a rounded square centred at (0.5, 0.5).
fn rounded_square(u: f32, v: f32, half: f32, radius: f32) -> f32 {
    let qx = (u - 0.5).abs() - (half - radius);
    let qy = (v - 0.5).abs() - (half - radius);
    let outside = qx.max(0.0).hypot(qy.max(0.0));
    outside + qx.max(qy).min(0.0) - radius
}

/// Signed distance to the four corner brackets and the centre dot.
fn viewfinder(u: f32, v: f32) -> f32 {
    const EDGE: f32 = 0.24; // distance of the brackets from the icon edge
    const ARM: f32 = 0.16; // bracket arm length
    const WIDTH: f32 = 0.035; // half stroke width

    // Fold into the top-left quadrant: all four corners are mirror images.
    let (fu, fv) = (0.5 - (u - 0.5).abs(), 0.5 - (v - 0.5).abs());
    let horizontal = segment(fu, fv, (EDGE, EDGE), (EDGE + ARM, EDGE));
    let vertical = segment(fu, fv, (EDGE, EDGE), (EDGE, EDGE + ARM));
    let brackets = horizontal.min(vertical) - WIDTH;

    let dot = (u - 0.5).hypot(v - 0.5) - 0.07;
    brackets.min(dot)
}

fn segment(u: f32, v: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py) = (u - a.0, v - a.1);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    (px - dx * t).hypot(py - dy * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
        let i = ((y * SIZE + x) * 4) as usize;
        pixels[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn has_expected_size() {
        assert_eq!(rgba().len(), (SIZE * SIZE * 4) as usize);
    }

    #[test]
    fn corners_are_transparent_and_centre_is_white() {
        let pixels = rgba();
        assert_eq!(pixel(&pixels, 0, 0)[3], 0);
        assert_eq!(pixel(&pixels, SIZE - 1, SIZE - 1)[3], 0);
        assert_eq!(pixel(&pixels, SIZE / 2, SIZE / 2), [255, 255, 255, 255]);
    }
}
