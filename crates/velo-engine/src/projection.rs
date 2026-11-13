//! An output's corner pin and masks: where the picture's four corners land on the projector, so
//! it falls on a cloth that isn't square to it, and black polygons over it, so no light spills
//! past the cloth. Points are parts of the output's width and height (0..1, y down), so they
//! hold at any resolution. The output blit draws the picture through the homography the corners
//! make and blacks out the masks (`runtime.rs`, `Warp`); untouched, it isn't drawn at all.

use serde::{Deserialize, Serialize};

/// The most masks drawn, and points over all of them: what the warp shader's uniform holds.
pub const MAX_MASKS: usize = 8;
pub const MAX_POINTS: usize = 60;

/// The output's corners, as they are untouched: top left, top right, bottom right, bottom left.
pub const SQUARE: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// A 3×3 matrix, rows first, applied to (x, y, 1).
pub type Matrix = [[f32; 3]; 3];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Projection {
    /// Where the picture's corners land on the output: top left, top right, bottom right,
    /// bottom left.
    pub corners: [[f32; 2]; 4],
    /// Black polygons over the picture, any shape (even-odd: one crossing itself leaves a hole).
    pub masks: Vec<Vec<[f32; 2]>>,
    /// How far inside a mask's edge the black fades in, as a part of the output's height:
    /// 0 is a hard edge. The edge stays where it's drawn, so no light passes it either way.
    pub soft: f32,
}

impl Default for Projection {
    fn default() -> Self {
        Self { corners: SQUARE, masks: Vec::new(), soft: 0.0 }
    }
}

impl Projection {
    /// Nothing to do: the output is drawn as it always was.
    pub fn is_identity(&self) -> bool {
        self.corners == SQUARE && self.masks.iter().all(|m| m.len() < 3)
    }

    /// Every point that can be picked and moved, in order: the four corners, then each mask's.
    pub fn handles(&self) -> impl Iterator<Item = [f32; 2]> + '_ {
        self.corners.iter().copied().chain(self.masks.iter().flatten().copied())
    }

    pub fn handle_mut(&mut self, i: usize) -> Option<&mut [f32; 2]> {
        self.corners.iter_mut().chain(self.masks.iter_mut().flatten()).nth(i)
    }

    /// The mask handle `i` belongs to (None for a corner).
    pub fn mask_of(&self, i: usize) -> Option<usize> {
        let mut first = self.corners.len();
        for (m, mask) in self.masks.iter().enumerate() {
            if (first..first + mask.len()).contains(&i) {
                return Some(m);
            }
            first += mask.len();
        }
        None
    }

    /// The handle of a mask's first point.
    pub fn first_of(&self, mask: usize) -> usize {
        self.corners.len() + self.masks[..mask].iter().map(Vec::len).sum::<usize>()
    }

    /// What the warp shader's uniform holds, as f32s (see its `U`), for a target `size` pixels
    /// large; with `guides`, the handles and edges drawn over it, the picked one (if any) lit.
    pub fn uniform(&self, size: [f32; 2], decode: bool, guides: Option<Option<usize>>) -> Vec<f32> {
        // a corner pin that makes no homography (three corners on a line): the picture as it is
        let inv = homography(&self.corners).and_then(|h| invert(&h)).unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        let mut u = Vec::with_capacity(4 * (5 + MAX_MASKS + MAX_POINTS.div_ceil(2) + 2));
        // a mat3x3's columns, each padded to four
        u.extend((0..3).flat_map(|c| [inv[0][c], inv[1][c], inv[2][c], 0.0]));
        let mut masks = Vec::new();
        let mut points: Vec<[f32; 2]> = self.corners.to_vec();
        for mask in self.masks.iter().take(MAX_MASKS) {
            if points.len() + mask.len() > MAX_POINTS + 4 {
                break;
            }
            masks.push([points.len() as f32, mask.len() as f32, 0.0, 0.0]);
            points.extend(mask);
        }
        let selected = guides.flatten().map_or(-1.0, |i| i as f32);
        u.extend([f32::from(u8::from(decode)), self.soft * size[1], masks.len() as f32, selected]);
        u.extend([size[0], size[1], f32::from(u8::from(guides.is_some())), points.len() as f32]);
        masks.resize(MAX_MASKS, [0.0; 4]);
        u.extend(masks.into_iter().flatten());
        points.resize(MAX_POINTS + 4, [0.0; 2]);
        u.extend(points.into_iter().flatten());
        u
    }
}

/// The homography that takes the unit square's corners (0,0), (1,0), (1,1), (0,1) to `quad`'s,
/// in that order (Heckbert's square-to-quad). None when the quad is degenerate.
pub fn homography(quad: &[[f32; 2]; 4]) -> Option<Matrix> {
    let [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = quad.map(|p| p.map(f64::from));
    let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
    let m = if sx.abs() < 1e-12 && sy.abs() < 1e-12 {
        // a parallelogram: affine
        [[x1 - x0, x3 - x0, x0], [y1 - y0, y3 - y0, y0], [0.0, 0.0, 1.0]]
    } else {
        let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
        let det = dx1 * dy2 - dx2 * dy1;
        if det.abs() < 1e-12 {
            return None;
        }
        let g = (sx * dy2 - dx2 * sy) / det;
        let h = (dx1 * sy - sx * dy1) / det;
        [[x1 - x0 + g * x1, x3 - x0 + h * x3, x0], [y1 - y0 + g * y1, y3 - y0 + h * y3, y0], [g, h, 1.0]]
    };
    let m = m.map(|r| r.map(|v| v as f32));
    invert(&m).map(|_| m)
}

/// The inverse of `m` (up to scale, which a homography ignores), None when it has none.
pub fn invert(m: &Matrix) -> Option<Matrix> {
    let m = m.map(|r| r.map(f64::from));
    let c = |r: usize, k: usize| {
        let (r0, r1, k0, k1) = ((r + 1) % 3, (r + 2) % 3, (k + 1) % 3, (k + 2) % 3);
        m[r0][k0] * m[r1][k1] - m[r0][k1] * m[r1][k0]
    };
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    if det.abs() < 1e-12 {
        return None;
    }
    // the adjugate is the cofactors transposed
    Some(std::array::from_fn(|r| std::array::from_fn(|k| (c(k, r) / det) as f32)))
}

/// Where `m` takes `p`.
pub fn map(m: &Matrix, [x, y]: [f32; 2]) -> [f32; 2] {
    let w = m[2][0] * x + m[2][1] * y + m[2][2];
    [(m[0][0] * x + m[0][1] * y + m[0][2]) / w, (m[1][0] * x + m[1][1] * y + m[1][2]) / w]
}

/// Whether `p` is inside `polygon` (even-odd, as the warp shader draws it).
pub fn contains(polygon: &[[f32; 2]], [x, y]: [f32; 2]) -> bool {
    let mut inside = false;
    for (k, a) in polygon.iter().enumerate() {
        let b = polygon[(k + 1) % polygon.len()];
        if (a[1] > y) != (b[1] > y) && x < (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
    }
    polygon.len() >= 3 && inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-5 && (a[1] - b[1]).abs() < 1e-5
    }

    #[test]
    fn the_homography_takes_the_square_to_the_corners() {
        let quad = [[0.1, 0.05], [0.93, 0.12], [0.85, 0.9], [0.02, 0.97]];
        let h = homography(&quad).expect("a quad");
        for (corner, to) in SQUARE.iter().zip(quad) {
            assert!(near(map(&h, *corner), to), "{corner:?} to {:?}, not {to:?}", map(&h, *corner));
        }
        // and back
        let inv = invert(&h).expect("invertible");
        assert!(near(map(&inv, quad[2]), [1.0, 1.0]));
        assert!(near(map(&inv, map(&h, [0.3, 0.6])), [0.3, 0.6]));
        // three corners on a line make none
        assert_eq!(homography(&[[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [0.0, 1.0]]), None);
    }

    #[test]
    fn untouched_it_is_the_identity_and_costs_nothing() {
        let p = Projection::default();
        assert!(p.is_identity());
        let h = homography(&p.corners).expect("the square");
        assert!(near(map(&h, [0.25, 0.75]), [0.25, 0.75]));
        // a mask still being drawn (under three points) doesn't count
        let drawing = Projection { masks: vec![vec![[0.1, 0.1], [0.2, 0.1]]], ..Default::default() };
        assert!(drawing.is_identity());
        let pinned = Projection { corners: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.1, 1.0]], ..Default::default() };
        assert!(!pinned.is_identity());
    }

    #[test]
    fn a_mask_holds_the_points_inside_it() {
        let square = [[0.2, 0.2], [0.6, 0.2], [0.6, 0.6], [0.2, 0.6]];
        assert!(contains(&square, [0.4, 0.4]));
        assert!(!contains(&square, [0.7, 0.4]) && !contains(&square, [0.1, 0.1]));
        // a concave one: the notch is outside
        let notched = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.5, 0.3], [0.0, 1.0]];
        assert!(contains(&notched, [0.2, 0.2]));
        assert!(!contains(&notched, [0.5, 0.8]));
        assert!(!contains(&square[..2], [0.4, 0.2]), "two points mask nothing");
    }

    #[test]
    fn handles_run_over_the_corners_then_each_mask() {
        let mut p = Projection { masks: vec![vec![[0.1, 0.1]; 3], vec![[0.5, 0.5]; 4]], ..Default::default() };
        assert_eq!(p.handles().count(), 11);
        assert_eq!((p.mask_of(3), p.mask_of(4), p.mask_of(7)), (None, Some(0), Some(1)));
        assert_eq!(p.first_of(1), 7);
        *p.handle_mut(8).expect("a point") = [0.9, 0.9];
        assert_eq!(p.masks[1][1], [0.9, 0.9]);
        // the shader's uniform: 3 columns, 2 vec4s, the masks, the points (two to a vec4)
        assert_eq!(p.uniform([1920.0, 1080.0], false, None).len(), 4 * (3 + 2 + MAX_MASKS) + 2 * (MAX_POINTS + 4));
    }
}
