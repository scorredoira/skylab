//! The pictures the cards of foliage are drawn with: leaves, twigs and grass cut out of CC0 scans
//! (`assets/textures/foliage_*.png`, from Poly Haven's plant models; `CREDITS.md`) and composed
//! here into the sprays each kind of foliage is made of: a twig with broad leaves, a dense spray of
//! small leathery ones (holm oaks, the maquis), the narrow two-toned leaves of an olive, a spruce's
//! branchlet, a pine's tufts of needles, a palm's frond, a cypress's scales, a larch's rosettes,
//! a tuft of grass. Made on the CPU when the renderer starts, on a thread (`load`): a quarter of a
//! second on an Apple M5 Pro, nothing downloaded.
//!
//! Two texture arrays of one layer per `Layer`, `SIZE` a side, with their mip levels:
//! - colour (`Rgba8UnormSrgb`): the picture scaled so that its mean is half grey; the shaders
//!   multiply it by twice the species' leaf colour (`species::Look::leaf`), so that one picture
//!   serves several species. Alpha is the leaf's outline, with the same share of the card over
//!   the cut-off (one half) at every level: a tree keeps its density with distance.
//! - detail (`Rgba8Unorm`): the leaf's own slope across the card (x and y of its normal, as 0..1),
//!   how much light it lets through (0 for twigs and stems), and its outline again.
//!
//! A card's base, where it is attached to its branch, is the middle of its bottom edge; it grows
//! upwards in the picture.

use engine::texture::{Kind, PreparedLayers};
use glam::{Vec2, Vec3};
use std::path::Path;

/// Side of a layer, texels.
pub const SIZE: usize = 512;
/// Drawn at this many times the size and shrunk: the edges of leaves smooth.
const OVER: usize = 2;

/// The kinds of foliage, layers of the arrays in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Broadleaf,
    SmallLeaf,
    Olive,
    Spruce,
    Pine,
    Palm,
    Scale,
    Larch,
    Grass,
}

pub const LAYERS: usize = 9;

impl Layer {
    pub const ALL: [Layer; LAYERS] = [Layer::Broadleaf, Layer::SmallLeaf, Layer::Olive, Layer::Spruce, Layer::Pine, Layer::Palm, Layer::Scale, Layer::Larch, Layer::Grass];
}

/// The scans, in `assets/textures`, and whether the picture's base is on its left (it is turned to
/// stand on its bottom) rather than on its bottom.
const SCANS: [(&str, bool); 5] = [("foliage_leaves.png", false), ("foliage_spruce.png", false), ("foliage_pine_1.png", false), ("foliage_pine_2.png", true), ("foliage_grass.png", false)];

/// A leaf, twig or tuft cut from a scan: premultiplied linear RGBA, its base at the middle of its
/// bottom edge, growing up; with its smaller copies.
#[derive(Clone)]
struct Sprite {
    levels: Vec<Picture>,
}

#[derive(Clone)]
struct Picture {
    w: usize,
    h: usize,
    px: Vec<[f32; 4]>,
}

impl Picture {
    /// Bilinear, nothing outside.
    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |i: i64, j: i64| -> [f32; 4] { if i < 0 || j < 0 || i >= self.w as i64 || j >= self.h as i64 { [0.0; 4] } else { self.px[j as usize * self.w + i as usize] } };
        let (i, j) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i, j + 1), at(i + 1, j + 1));
        std::array::from_fn(|k| (a[k] * (1.0 - fx) + b[k] * fx) * (1.0 - fy) + (c[k] * (1.0 - fx) + d[k] * fx) * fy)
    }

    fn halved(&self) -> Picture {
        let (w, h) = ((self.w / 2).max(1), (self.h / 2).max(1));
        let mut px = vec![[0.0f32; 4]; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut sum = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((2 * x + dx).min(self.w - 1), (2 * y + dy).min(self.h - 1));
                    let p = self.px[sy * self.w + sx];
                    sum = std::array::from_fn(|k| sum[k] + p[k] * 0.25);
                }
                px[y * w + x] = sum;
            }
        }
        Picture { w, h, px }
    }
}

impl Sprite {
    fn new(picture: Picture) -> Sprite {
        let mut levels = vec![picture];
        while levels.last().is_some_and(|p| p.w.min(p.h) > 4) {
            let next = levels.last().map(Picture::halved).unwrap_or_else(|| unreachable!());
            levels.push(next);
        }
        Sprite { levels }
    }

    /// The copy to draw from at `scale` (canvas pixels per pixel of the full one), and its scale.
    fn at(&self, scale: f32) -> (&Picture, f32) {
        let level = ((1.0 / scale.max(1e-3)).log2().floor().max(0.0) as usize).min(self.levels.len() - 1);
        (&self.levels[level], scale * (1u32 << level) as f32)
    }

    fn height(&self) -> f32 {
        self.levels[0].h as f32
    }

    fn width(&self) -> f32 {
        self.levels[0].w as f32
    }
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// The separate pieces of a scan (8-connected, where it is more than an eighth opaque), the
/// smaller than a fifth of the biggest left out (specks, stray lines).
fn pieces(w: usize, h: usize, rgba: &[u8], turned: bool) -> Vec<Sprite> {
    let solid: Vec<bool> = rgba.as_chunks::<4>().0.iter().map(|p| p[3] > 32).collect();
    let mut label = vec![u32::MAX; w * h];
    let mut found: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if !solid[start] || label[start] != u32::MAX {
            continue;
        }
        let id = found.len() as u32;
        let (mut x0, mut y0, mut x1, mut y1, mut count) = (w, h, 0, 0, 0);
        label[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1, count) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y), count + 1);
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                        let j = ny as usize * w + nx as usize;
                        if solid[j] && label[j] == u32::MAX {
                            label[j] = id;
                            stack.push(j);
                        }
                    }
                }
            }
        }
        found.push((x0, y0, x1, y1, count));
    }
    let biggest = found.iter().map(|f| f.4).max().unwrap_or(0);
    let table: Vec<f32> = (0..256).map(|v| srgb_to_linear(v as f32 / 255.0)).collect();
    found
        .iter()
        .enumerate()
        .filter(|(_, f)| f.4 * 5 >= biggest)
        .map(|(id, &(x0, y0, x1, y1, _))| {
            let (pw, ph) = (x1 - x0 + 1, y1 - y0 + 1);
            let mut px = vec![[0.0f32; 4]; pw * ph];
            for y in 0..ph {
                for x in 0..pw {
                    let i = (y0 + y) * w + x0 + x;
                    // Its own texels, and the faint edge round them that belongs to no piece.
                    let mine = label[i] == id as u32 || (label[i] == u32::MAX && rgba[i * 4 + 3] > 0);
                    if mine {
                        let p = &rgba[i * 4..i * 4 + 4];
                        let a = p[3] as f32 / 255.0;
                        px[y * pw + x] = [table[p[0] as usize] * a, table[p[1] as usize] * a, table[p[2] as usize] * a, a];
                    }
                }
            }
            let picture = Picture { w: pw, h: ph, px };
            Sprite::new(if turned { turn(&picture) } else { picture })
        })
        .collect()
}

/// A quarter turn that puts the left edge at the bottom.
fn turn(p: &Picture) -> Picture {
    let (w, h) = (p.h, p.w);
    let mut px = vec![[0.0f32; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            px[y * w + x] = p.px[x * p.w + (p.w - 1 - y)];
        }
    }
    Picture { w, h, px }
}

/// A small generator of numbers, so that the pictures are the same every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }

    fn pick<'a, T>(&mut self, list: &'a [T]) -> &'a T {
        &list[((self.next() * list.len() as f32) as usize).min(list.len() - 1)]
    }
}

/// A picture being composed, premultiplied: colour and alpha, and the detail (normal x and y,
/// what it lets through, alpha).
struct Canvas {
    side: usize,
    colour: Vec<[f32; 4]>,
    detail: Vec<[f32; 4]>,
}

/// How a piece is laid on the canvas: its base, the way it grows (radians from up, clockwise), its
/// length and how much wider or narrower than the scan, its tint, how much light it lets through,
/// how much it is folded along its middle.
#[derive(Clone, Copy)]
struct Lay {
    base: Vec2,
    angle: f32,
    length: f32,
    width: f32,
    tint: Vec3,
    through: f32,
    fold: f32,
}

impl Canvas {
    fn new() -> Canvas {
        let side = SIZE * OVER;
        Canvas { side, colour: vec![[0.0; 4]; side * side], detail: vec![[0.0; 4]; side * side] }
    }

    /// One texel over what is there.
    fn over(&mut self, i: usize, colour: [f32; 4], detail: [f32; 3]) {
        let a = colour[3];
        let keep = 1.0 - a;
        let c = &mut self.colour[i];
        *c = std::array::from_fn(|k| colour[k] + c[k] * keep);
        let d = &mut self.detail[i];
        *d = [detail[0] * a + d[0] * keep, detail[1] * a + d[1] * keep, detail[2] * a + d[2] * keep, a + d[3] * keep];
    }

    /// Lays a piece; positions in units of the card (0 to 1, y down).
    fn lay(&mut self, sprite: &Sprite, l: Lay) {
        let side = self.side as f32;
        let (base, length) = (l.base * side, l.length * side);
        let up = Vec2::new(l.angle.sin(), -l.angle.cos());
        let across = Vec2::new(-up.y, up.x);
        let scale = length / sprite.height();
        let (picture, s) = sprite.at(scale.min(scale * l.width));
        // Canvas pixels per pixel of `picture`, along and across.
        let (along_scale, across_scale) = (s * scale / scale.min(scale * l.width), s * scale * l.width / scale.min(scale * l.width));
        let half = sprite.width() * scale * l.width / 2.0;
        let corners = [base + across * half, base - across * half, base + across * half + up * length, base - across * half + up * length];
        let (lo, hi) = corners.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
        let (x0, y0) = (lo.x.floor().max(0.0) as usize, lo.y.floor().max(0.0) as usize);
        let (x1, y1) = ((hi.x.ceil() as usize).min(self.side - 1), (hi.y.ceil() as usize).min(self.side - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - base;
                let (t, c) = (d.dot(up), d.dot(across));
                let (px, py) = (c / across_scale + picture.w as f32 / 2.0, picture.h as f32 - t / along_scale);
                let p = picture.sample(px, py);
                if p[3] <= 1e-3 {
                    continue;
                }
                let colour = [p[0] * l.tint.x, p[1] * l.tint.y, p[2] * l.tint.z, p[3]];
                // Folded along its middle: the two halves face apart.
                let q = (c / half.max(1.0)).clamp(-1.0, 1.0);
                let n = Vec2::new(across.x, -across.y) * q * l.fold;
                self.over(y * self.side + x, colour, [n.x, n.y, l.through]);
            }
        }
    }

    /// A stem from `a` to `b` (card units), `width` texels of the final picture wide at `a` and
    /// half that at `b`.
    fn stem(&mut self, a: Vec2, b: Vec2, width: f32, colour: Vec3) {
        let side = self.side as f32;
        let (a, b) = (a * side, b * side);
        let w = width * OVER as f32;
        let (lo, hi) = (a.min(b) - Vec2::splat(w), a.max(b) + Vec2::splat(w));
        let along = b - a;
        let length2 = along.length_squared().max(1e-6);
        for y in lo.y.max(0.0) as usize..(hi.y.ceil() as usize).min(self.side) {
            for x in lo.x.max(0.0) as usize..(hi.x.ceil() as usize).min(self.side) {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let t = ((p - a).dot(along) / length2).clamp(0.0, 1.0);
                let r = w * (1.0 - 0.5 * t) / 2.0;
                let d = (p - (a + along * t)).length();
                let cover = (r + 0.5 - d).clamp(0.0, 1.0);
                if cover > 0.0 {
                    self.over(y * self.side + x, [colour.x * cover, colour.y * cover, colour.z * cover, cover], [0.0, 0.0, 0.0]);
                }
            }
        }
    }
}

/// The pieces of the scans, by kind.
struct Scans {
    leaves: Vec<Sprite>,
    spruce: Vec<Sprite>,
    pine: Vec<Sprite>,
    grass: Vec<Sprite>,
}

const TWIG: Vec3 = Vec3::new(0.07, 0.055, 0.035);

/// A spray of leaves: a twig with side shoots, and `leaves` laid by `leaf` over the rounded clump
/// they make (given where, and the way out of the clump from its base), the deepest first, so that
/// the card is mostly leaf with ragged gaps, as a clump of a crown is: broadleaves, small leaves
/// and olives.
fn spray(canvas: &mut Canvas, rng: &mut Rng, leaves: usize, mut leaf: impl FnMut(&mut Canvas, &mut Rng, Vec2, f32)) {
    let (base, top) = (Vec2::new(0.5, 0.99), Vec2::new(0.5 + rng.range(-0.05, 0.05), 0.1));
    canvas.stem(base, top, 3.0, TWIG);
    for k in 0..6 {
        let t = 0.25 + 0.6 * (k as f32 + rng.range(0.0, 0.6)) / 6.0;
        let from = base.lerp(top, t);
        let angle = if k % 2 == 0 { 1.0 } else { -1.0 } * rng.range(0.6, 1.05) * (1.0 - 0.4 * t);
        let to = from + Vec2::new(angle.sin(), -angle.cos()) * rng.range(0.22, 0.32) * (1.0 - 0.4 * t);
        canvas.stem(from, to, 1.8, TWIG);
    }
    let middle = Vec2::new(0.5, 0.48);
    for _ in 0..leaves {
        // Over an ellipse round the clump's middle, thinning out towards its rim.
        let (r, a) = (rng.next().powf(0.7), rng.range(0.0, std::f32::consts::TAU));
        let at = middle + Vec2::new(a.cos() * 0.44, a.sin() * 0.45) * r;
        let out = at - Vec2::new(0.5, 0.95);
        let angle = out.x.atan2(-out.y) + rng.range(-0.7, 0.7);
        leaf(canvas, rng, at, angle);
    }
}

fn tint(rng: &mut Rng, spread: f32) -> Vec3 {
    let b = rng.range(1.0 - spread, 1.0 + spread);
    let hue = rng.range(-spread, spread) * 0.5;
    Vec3::new(b * (1.0 + hue), b, b * (1.0 - hue))
}

/// Composes the picture of one kind of foliage.
fn compose(layer: Layer, scans: &Scans) -> Canvas {
    let mut c = Canvas::new();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (layer as u64 + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
    let rng = &mut rng;
    match layer {
        Layer::Broadleaf => spray(&mut c, rng, 140, |c, rng, at, angle| {
            let length = rng.range(0.14, 0.2);
            let base = at - Vec2::new(angle.sin(), -angle.cos()) * length * 0.5;
            let lay = Lay { base, angle, length, width: rng.range(0.85, 1.1), tint: tint(rng, 0.15), through: 0.8, fold: 0.35 };
            c.lay(rng.pick(&scans.leaves), lay);
        }),
        Layer::SmallLeaf => spray(&mut c, rng, 480, |c, rng, at, angle| {
            let length = rng.range(0.07, 0.1);
            let base = at - Vec2::new(angle.sin(), -angle.cos()) * length * 0.5;
            let lay = Lay { base, angle, length, width: rng.range(1.0, 1.25), tint: tint(rng, 0.12), through: 0.45, fold: 0.5 };
            c.lay(rng.pick(&scans.leaves), lay);
        }),
        Layer::Olive => spray(&mut c, rng, 420, |c, rng, at, angle| {
            // The underside of an olive's leaf is silver, and the wind shows it.
            let silver = if rng.next() < 0.4 { Vec3::new(1.5, 1.55, 1.6) } else { Vec3::ONE };
            let length = rng.range(0.12, 0.16);
            let base = at - Vec2::new(angle.sin(), -angle.cos()) * length * 0.5;
            let lay = Lay { base, angle, length, width: rng.range(0.26, 0.32), tint: tint(rng, 0.1) * silver, through: 0.4, fold: 0.6 };
            c.lay(rng.pick(&scans.leaves), lay);
        }),
        Layer::Spruce => {
            let mut big = scans.spruce.clone();
            big.sort_by(|a, b| (b.width() * b.height()).total_cmp(&(a.width() * a.height())));
            for (k, angle) in [-0.55f32, 0.55, 0.0].into_iter().enumerate() {
                let lay = Lay { base: Vec2::new(0.5, 0.99), angle, length: if angle == 0.0 { 0.97 } else { 0.72 }, width: 1.0, tint: tint(rng, 0.1), through: 0.5, fold: 0.25 };
                c.lay(&big[k % big.len().min(3)], lay);
            }
            for _ in 0..18 {
                let lay = Lay {
                    base: Vec2::new(rng.range(0.25, 0.75), rng.range(0.3, 0.8)),
                    angle: rng.range(-1.0, 1.0),
                    length: rng.range(0.25, 0.4),
                    width: 1.0,
                    tint: tint(rng, 0.12),
                    through: 0.5,
                    fold: 0.25,
                };
                c.lay(rng.pick(&scans.spruce), lay);
            }
        }
        Layer::Pine => {
            for k in 0..9 {
                let angle = -1.1 + 2.2 * (k as f32 + rng.range(0.2, 0.8)) / 9.0;
                let lay = Lay {
                    base: Vec2::new(0.5 + rng.range(-0.03, 0.03), 0.99),
                    angle,
                    length: rng.range(0.6, 0.95) * (1.0 - 0.25 * angle.abs()),
                    width: rng.range(0.9, 1.3),
                    tint: tint(rng, 0.12),
                    through: 0.45,
                    fold: 0.2,
                };
                c.lay(rng.pick(&scans.pine), lay);
            }
        }
        Layer::Palm => {
            let (base, top) = (Vec2::new(0.5, 1.0), Vec2::new(0.5, 0.01));
            c.stem(base, top, 5.0, Vec3::new(0.16, 0.15, 0.08));
            for k in 0..40 {
                let t = 0.08 + 0.9 * k as f32 / 40.0;
                let at = base.lerp(top, t);
                for side in [-1.0f32, 1.0] {
                    let lay = Lay {
                        base: at,
                        angle: side * (1.0 - 0.45 * t),
                        length: (0.46 - 0.34 * t) * rng.range(0.9, 1.05),
                        width: rng.range(0.11, 0.14),
                        tint: tint(rng, 0.08) * Vec3::new(1.05, 1.0, 0.85),
                        through: 0.6,
                        fold: 0.6,
                    };
                    c.lay(rng.pick(&scans.leaves), lay);
                }
            }
        }
        Layer::Scale => {
            for _ in 0..46 {
                let base = Vec2::new(rng.range(0.3, 0.7), rng.range(0.35, 0.99));
                let lay = Lay { base, angle: rng.range(-0.9, 0.9), length: rng.range(0.3, 0.55), width: rng.range(0.8, 1.2), tint: tint(rng, 0.12), through: 0.3, fold: 0.15 };
                c.lay(rng.pick(&scans.spruce), lay);
            }
        }
        Layer::Larch => {
            let (base, top) = (Vec2::new(0.5, 0.99), Vec2::new(0.52, 0.05));
            c.stem(base, top, 2.0, TWIG);
            for k in 0..40 {
                let at = base.lerp(top, 0.1 + 0.88 * k as f32 / 40.0) + Vec2::new(rng.range(-0.2, 0.2), 0.0);
                for _ in 0..4 {
                    let lay = Lay { base: at, angle: rng.range(-1.6, 1.6), length: rng.range(0.09, 0.14), width: rng.range(1.2, 1.6), tint: tint(rng, 0.1), through: 0.6, fold: 0.1 };
                    c.lay(rng.pick(&scans.pine), lay);
                }
            }
        }
        Layer::Grass => {
            for k in 0..4 {
                let lay = Lay {
                    base: Vec2::new(0.2 + 0.2 * k as f32 + rng.range(-0.04, 0.04), 1.0),
                    angle: rng.range(-0.15, 0.15),
                    length: rng.range(0.6, 0.95),
                    width: rng.range(0.9, 1.1),
                    tint: tint(rng, 0.12),
                    through: 0.7,
                    fold: 0.2,
                };
                let tuft = rng.pick(&scans.grass).clone();
                // As wide as a quarter of the card at most: the grass scans are wide tufts.
                let width = lay.width.min(0.3 / (tuft.width() / tuft.height() * lay.length));
                c.lay(&tuft, Lay { width, ..lay });
            }
        }
    }
    c
}

/// A picture's mip levels, largest first, as the GPU format holds them.
type Levels = Vec<Vec<u8>>;

/// The mip levels of a composed picture, as the two arrays store them.
fn levels(canvas: &Canvas) -> (Levels, Levels) {
    // Shrunk to its size, premultiplied, then each smaller level halved from it.
    let shrink = |src: &[[f32; 4]], side: usize, factor: usize| -> Vec<[f32; 4]> {
        let out = side / factor;
        let mut px = vec![[0.0f32; 4]; out * out];
        let w = 1.0 / (factor * factor) as f32;
        for y in 0..out {
            for x in 0..out {
                let mut sum = [0.0f32; 4];
                for dy in 0..factor {
                    for dx in 0..factor {
                        let p = src[(y * factor + dy) * side + x * factor + dx];
                        sum = std::array::from_fn(|k| sum[k] + p[k] * w);
                    }
                }
                px[y * out + x] = sum;
            }
        }
        px
    };
    let mut colour = vec![shrink(&canvas.colour, canvas.side, OVER)];
    let mut detail = vec![shrink(&canvas.detail, canvas.side, OVER)];
    let mut side = SIZE;
    while side > 1 {
        colour.push(shrink(colour.last().map_or(&[][..], |v| v), side, 2));
        detail.push(shrink(detail.last().map_or(&[][..], |v| v), side, 2));
        side /= 2;
    }
    // The picture's mean colour where it covers: it is scaled to half grey.
    let first = &colour[0];
    let covered: f32 = first.iter().map(|p| p[3]).sum::<f32>().max(1e-6);
    let mean: [f32; 3] = std::array::from_fn(|k| (first.iter().map(|p| p[k]).sum::<f32>() / covered).max(1e-4));
    // What share of the card the full picture covers past the cut-off; each level's alpha is
    // scaled to cover as much (Castaño, "Computing Alpha Mipmaps", 2010).
    let share = |level: &[[f32; 4]], scale: f32| level.iter().filter(|p| p[3] * scale >= 0.5).count() as f32 / level.len() as f32;
    let target = share(first, 1.0);
    let srgb: Vec<u8> = (0..4096).map(|i| (linear_to_srgb(i as f32 / 4095.0) * 255.0 + 0.5) as u8).collect();
    let to_srgb = |v: f32| srgb[((v.clamp(0.0, 1.0)) * 4095.0) as usize];
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let mut colour_levels = Vec::new();
    let mut detail_levels = Vec::new();
    for (level, (c, d)) in colour.iter().zip(&detail).enumerate() {
        let scale = if level == 0 || target <= 0.0 {
            1.0
        } else {
            let (mut lo, mut hi) = (0.5f32, 8.0f32);
            for _ in 0..14 {
                let mid = (lo + hi) / 2.0;
                if share(c, mid) < target { lo = mid } else { hi = mid }
            }
            (lo + hi) / 2.0
        };
        let mut cl = Vec::with_capacity(c.len() * 4);
        let mut dl = Vec::with_capacity(d.len() * 4);
        for (p, q) in c.iter().zip(d) {
            let a = p[3];
            // Where nothing covers, the mean: a filter reaching past a leaf's edge brings in no dark.
            let rgb: [f32; 3] = if a > 1e-4 { std::array::from_fn(|k| p[k] / a / mean[k] * 0.5) } else { [0.5; 3] };
            cl.extend_from_slice(&[to_srgb(rgb[0]), to_srgb(rgb[1]), to_srgb(rgb[2]), byte(a * scale)]);
            let n: [f32; 3] = if q[3] > 1e-4 { [q[0] / q[3], q[1] / q[3], q[2] / q[3]] } else { [0.0; 3] };
            dl.extend_from_slice(&[byte(n[0] * 0.5 + 0.5), byte(n[1] * 0.5 + 0.5), byte(n[2]), byte(a * scale)]);
        }
        colour_levels.push(cl);
        detail_levels.push(dl);
    }
    (colour_levels, detail_levels)
}

/// The two arrays, ready to go to the GPU (`Gpu::upload_layers`): colour, then detail.
pub struct Prepared {
    pub colour: PreparedLayers,
    pub detail: PreparedLayers,
}

/// Reads the scans from `dir` (`assets/textures`) and composes every layer.
pub fn load(dir: &Path) -> Result<Prepared, String> {
    let mut pieces_of = Vec::new();
    for (file, turned) in SCANS {
        let (w, h, rgba) = engine::texture::decode(&dir.join(file)).map_err(|e| e.to_string())?;
        let found = pieces(w as usize, h as usize, &rgba, turned);
        if found.is_empty() {
            return Err(format!("{}: nothing in it", dir.join(file).display()));
        }
        pieces_of.push(found);
    }
    let mut it = pieces_of.into_iter();
    let mut next = || it.next().unwrap_or_default();
    let (leaves, spruce, mut pine, pine_2, grass) = (next(), next(), next(), next(), next());
    pine.extend(pine_2);
    let scans = Scans { leaves, spruce, pine, grass };
    Ok(compose_all(&scans))
}

fn compose_all(scans: &Scans) -> Prepared {
    let made: Vec<(Levels, Levels)> = Layer::ALL.iter().map(|layer| levels(&compose(*layer, scans))).collect();
    let (colour, detail): (Vec<_>, Vec<_>) = made.into_iter().map(|(c, d)| ((SIZE as u32, SIZE as u32, c), (SIZE as u32, SIZE as u32, d))).unzip();
    Prepared { colour: PreparedLayers::from_levels(Kind::Color, colour), detail: PreparedLayers::from_levels(Kind::Data, detail) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scans made up: ellipses for leaves, a fan of lines for twigs.
    fn made_up() -> Scans {
        let ellipse = |w: usize, h: usize| {
            let px = (0..w * h)
                .map(|i| {
                    let (x, y) = ((i % w) as f32 / w as f32 - 0.5, (i / w) as f32 / h as f32 - 0.5);
                    if x * x * 4.0 + y * y * 4.0 < 1.0 { [0.05, 0.1, 0.02, 1.0] } else { [0.0; 4] }
                })
                .collect();
            Sprite::new(Picture { w, h, px })
        };
        Scans { leaves: vec![ellipse(40, 100)], spruce: vec![ellipse(80, 90)], pine: vec![ellipse(30, 120)], grass: vec![ellipse(120, 60)] }
    }

    #[test]
    fn every_layer_covers_part_of_its_card_at_every_level() {
        let scans = made_up();
        for layer in Layer::ALL {
            let (colour, detail) = levels(&compose(layer, &scans));
            assert_eq!(colour.len(), SIZE.trailing_zeros() as usize + 1);
            let share = |level: &[u8]| level.as_chunks::<4>().0.iter().filter(|p| p[3] >= 128).count() as f32 / (level.len() / 4) as f32;
            let full = share(&colour[0]);
            assert!(full > 0.05 && full < 0.9, "{layer:?} covers {full}");
            // The density is kept down to 8 x 8.
            for level in &colour[1..SIZE.trailing_zeros() as usize - 2] {
                let s = share(level);
                assert!((s - full).abs() < 0.08 + 0.1 * full, "{layer:?}: {s} of the card at a smaller level, {full} at the full one");
            }
            assert_eq!(detail[0].len(), colour[0].len());
        }
    }

    #[test]
    fn pieces_are_found_apart_and_specks_left_out() {
        // Two blobs and a speck on a 40 x 20 picture.
        let (w, h) = (40, 20);
        let mut rgba = vec![0u8; w * h * 4];
        let mut fill = |x0: usize, y0: usize, size: usize| {
            for y in y0..y0 + size {
                for x in x0..x0 + size {
                    rgba[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[90, 120, 60, 255]);
                }
            }
        };
        fill(2, 2, 10);
        fill(20, 5, 12);
        fill(35, 15, 2);
        let found = pieces(w, h, &rgba, false);
        assert_eq!(found.len(), 2);
        assert_eq!((found[0].levels[0].w, found[0].levels[0].h), (10, 10));
        let turned = pieces(w, h, &rgba, true);
        assert_eq!((turned[1].levels[0].w, turned[1].levels[0].h), (12, 12));
    }

    #[test]
    fn the_scanned_foliage_fills_its_cards() {
        // The real scans: a crown seen from far is as dense as its cards are.
        let dir = crate::materials::find_textures().expect("assets/textures");
        let mut pieces_of = Vec::new();
        for (file, turned) in SCANS {
            let (w, h, rgba) = engine::texture::decode(&dir.join(file)).expect("a scan");
            pieces_of.push(pieces(w as usize, h as usize, &rgba, turned));
        }
        let mut it = pieces_of.into_iter();
        let mut next = || it.next().unwrap_or_default();
        let (leaves, spruce, mut pine, pine_2, grass) = (next(), next(), next(), next(), next());
        pine.extend(pine_2);
        let scans = Scans { leaves, spruce, pine, grass };
        for layer in Layer::ALL {
            let (colour, _) = levels(&compose(layer, &scans));
            let share = colour[0].as_chunks::<4>().0.iter().filter(|p| p[3] >= 128).count() as f32 / (SIZE * SIZE) as f32;
            let least = match layer {
                Layer::Pine | Layer::Larch | Layer::Grass | Layer::Palm => 0.18,
                _ => 0.3,
            };
            assert!(share > least && share < 0.85, "{layer:?} covers {share} of its card");
        }
    }
}
