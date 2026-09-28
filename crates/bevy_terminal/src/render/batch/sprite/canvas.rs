//! An 8-bit coverage canvas with rectangles and anti-aliased paths, after
//! Ghostty's `font/sprite/canvas.zig` (which draws paths with z2d).
//!
//! Paths are flattened to polygons and filled with exact area coverage
//! (non-zero winding for non-overlapping contours); strokes are filled
//! outlines with butt caps and miter joins. The canvas keeps a quarter-cell
//! margin on each side so drawings may overshoot the cell, as in Ghostty.

use bevy::math::{DVec2, IVec2, UVec2};

/// Alpha of a shaded fill.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Shade {
    Light = 0x40,
    Medium = 0x80,
    Dark = 0xc0,
    On = 0xff,
}

/// A path of straight and cubic segments in cell pixels.
#[derive(Clone, Debug, Default)]
pub(crate) struct Path {
    contours: Vec<(Vec<DVec2>, bool)>,
}

impl Path {
    pub(crate) fn move_to(&mut self, x: f64, y: f64) -> &mut Self {
        self.contours.push((vec![DVec2::new(x, y)], false));
        self
    }

    pub(crate) fn line_to(&mut self, x: f64, y: f64) -> &mut Self {
        if self.contours.is_empty() {
            return self.move_to(x, y);
        }
        self.current().push(DVec2::new(x, y));
        self
    }

    pub(crate) fn curve_to(
        &mut self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        x3: f64,
        y3: f64,
    ) -> &mut Self {
        let points = self.current();
        let p0 = *points.last().expect("curve after a point");
        let (p1, p2, p3) = (DVec2::new(x1, y1), DVec2::new(x2, y2), DVec2::new(x3, y3));
        let length = p0.distance(p1) + p1.distance(p2) + p2.distance(p3);
        let steps = (length * 2.0).ceil().clamp(4.0, 256.0) as usize;
        for step in 1..=steps {
            let t = step as f64 / steps as f64;
            let u = 1.0 - t;
            points.push(
                p0 * (u * u * u)
                    + p1 * (3.0 * u * u * t)
                    + p2 * (3.0 * u * t * t)
                    + p3 * (t * t * t),
            );
        }
        self
    }

    /// A full circle as a closed contour.
    pub(crate) fn circle(&mut self, center: DVec2, radius: f64) -> &mut Self {
        let steps = (radius * std::f64::consts::TAU).ceil().clamp(16.0, 512.0) as usize;
        let points = (0..steps)
            .map(|step| {
                let angle = step as f64 / steps as f64 * std::f64::consts::TAU;
                center + DVec2::new(angle.cos(), angle.sin()) * radius
            })
            .collect();
        self.contours.push((points, true));
        self
    }

    pub(crate) fn close(&mut self) -> &mut Self {
        if let Some(contour) = self.contours.last_mut() {
            contour.1 = true;
        }
        self
    }

    fn current(&mut self) -> &mut Vec<DVec2> {
        &mut self.contours.last_mut().expect("path started").0
    }
}

/// An alpha-only drawing surface for one sprite.
pub(crate) struct Canvas {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) padding: UVec2,
    /// Margins excluded from the output, in canvas pixels per side
    /// (left, top, right, bottom).
    pub(crate) clip: [u32; 4],
    pub(crate) pixels: Vec<u8>,
}

/// A drawn sprite: its trimmed coverage and where its top-left pixel lies
/// relative to the cell's top-left corner.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Sprite {
    pub(crate) offset: IVec2,
    pub(crate) size: UVec2,
    pub(crate) alpha: Vec<u8>,
}

impl Canvas {
    /// A canvas for a `width` × `height` cell with a quarter-cell margin.
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let padding = UVec2::new(width / 4, height / 4);
        let size = UVec2::new(width, height) + 2 * padding;
        Self {
            width,
            height,
            padding,
            clip: [0; 4],
            pixels: vec![0; (size.x * size.y) as usize],
        }
    }

    fn stride(&self) -> u32 {
        self.width + 2 * self.padding.x
    }

    fn rows(&self) -> u32 {
        self.height + 2 * self.padding.y
    }

    /// Excludes everything outside the cell from the output.
    pub(crate) fn clip_to_cell(&mut self) {
        self.clip = [
            self.padding.x,
            self.padding.y,
            self.padding.x,
            self.padding.y,
        ];
    }

    /// Sets one pixel (cell coordinates) to `alpha`.
    pub(crate) fn pixel(&mut self, x: i32, y: i32, alpha: u8) {
        let x = x + self.padding.x as i32;
        let y = y + self.padding.y as i32;
        if x >= 0 && y >= 0 && (x as u32) < self.stride() && (y as u32) < self.rows() {
            let stride = self.stride();
            self.pixels[(y as u32 * stride + x as u32) as usize] = alpha;
        }
    }

    /// Sets the pixels of the rectangle between two corners.
    pub(crate) fn box_(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, shade: Shade) {
        for y in y0.min(y1)..y0.max(y1) {
            for x in x0.min(x1)..x0.max(x1) {
                self.pixel(x, y, shade as u8);
            }
        }
    }

    /// Coverage of `path` in canvas pixels, clamped to one.
    fn coverage(&self, path: &Path) -> Vec<f32> {
        let (stride, rows) = (self.stride() as usize, self.rows() as usize);
        // Signed area per pixel; a prefix sum along each row gives coverage.
        let mut accumulation = vec![0.0f32; stride * rows];
        let offset = self.padding.as_dvec2();
        for (points, _) in &path.contours {
            if points.len() < 2 {
                continue;
            }
            for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
                accumulate(&mut accumulation, stride, rows, *a + offset, *b + offset);
            }
        }
        let mut coverage = vec![0.0f32; stride * rows];
        for y in 0..rows {
            let mut sum = 0.0;
            for x in 0..stride {
                sum += accumulation[y * stride + x];
                coverage[y * stride + x] = sum.abs().min(1.0);
            }
        }
        coverage
    }

    /// Composites coverage over the canvas.
    fn composite(&mut self, coverage: &[f32], shade: Shade) {
        let alpha = f32::from(shade as u8) / 255.0;
        for (pixel, coverage) in self.pixels.iter_mut().zip(coverage) {
            let source = coverage * alpha;
            let destination = f32::from(*pixel) / 255.0;
            *pixel = ((source + destination * (1.0 - source)) * 255.0).round() as u8;
        }
    }

    /// Fills `path` (every contour closed) with non-zero winding.
    pub(crate) fn fill_path(&mut self, path: &Path, shade: Shade) {
        let coverage = self.coverage(path);
        self.composite(&coverage, shade);
    }

    /// Strokes `path` with butt caps and miter joins.
    pub(crate) fn stroke_path(&mut self, path: &Path, width: f64, shade: Shade) {
        let coverage = self.coverage(&stroke_outline(path, width));
        self.composite(&coverage, shade);
    }

    /// Strokes `path` inside its own outline: the band of the given width
    /// inside the filled (closed) path along its edges.
    pub(crate) fn inner_stroke_path(&mut self, path: &Path, width: f64, shade: Shade) {
        let band = self.coverage(&stroke_outline(path, 2.0 * width));
        let inside = self.coverage(path);
        let coverage: Vec<f32> = band.iter().zip(&inside).map(|(a, b)| a * b).collect();
        self.composite(&coverage, shade);
    }

    pub(crate) fn triangle(&mut self, points: [DVec2; 3], shade: Shade) {
        let mut path = Path::default();
        path.move_to(points[0].x, points[0].y)
            .line_to(points[1].x, points[1].y)
            .line_to(points[2].x, points[2].y)
            .close();
        self.fill_path(&path, shade);
    }

    /// Strokes a straight line with butt caps.
    pub(crate) fn line(&mut self, from: DVec2, to: DVec2, width: f64, shade: Shade) {
        let mut path = Path::default();
        path.move_to(from.x, from.y).line_to(to.x, to.y);
        self.stroke_path(&path, width, shade);
    }

    pub(crate) fn invert(&mut self) {
        for pixel in &mut self.pixels {
            *pixel = 255 - *pixel;
        }
    }

    pub(crate) fn flip_horizontal(&mut self) {
        let stride = self.stride() as usize;
        for row in self.pixels.chunks_mut(stride) {
            row.reverse();
        }
        self.clip.swap(0, 2);
    }

    /// The drawing, trimmed to its non-transparent pixels inside the clip.
    pub(crate) fn finish(&self) -> Option<Sprite> {
        let (stride, rows) = (self.stride(), self.rows());
        let [left, top, right, bottom] = self.clip;
        let (x0, y0, x1, y1) = (
            left,
            top,
            stride.saturating_sub(right),
            rows.saturating_sub(bottom),
        );
        let inked = |x: u32, y: u32| self.pixels[(y * stride + x) as usize] != 0;
        let columns: Vec<u32> = (x0..x1)
            .filter(|x| (y0..y1).any(|y| inked(*x, y)))
            .collect();
        let rows_inked: Vec<u32> = (y0..y1)
            .filter(|y| (x0..x1).any(|x| inked(x, *y)))
            .collect();
        let (&min_x, &max_x) = (columns.first()?, columns.last()?);
        let (&min_y, &max_y) = (rows_inked.first()?, rows_inked.last()?);
        let size = UVec2::new(max_x + 1 - min_x, max_y + 1 - min_y);
        let alpha = (min_y..=max_y)
            .flat_map(|y| {
                let start = (y * stride + min_x) as usize;
                self.pixels[start..start + size.x as usize].iter().copied()
            })
            .collect();
        Some(Sprite {
            offset: IVec2::new(min_x as i32, min_y as i32) - self.padding.as_ivec2(),
            size,
            alpha,
        })
    }
}

/// Adds the signed area of the edge `a → b` to the accumulation buffer, the
/// way font-rs accumulates outlines: each pixel receives the area between the
/// edge and its right side, and the pixel to its right the remaining cover.
fn accumulate(accumulation: &mut [f32], stride: usize, rows: usize, a: DVec2, b: DVec2) {
    if (a.y - b.y).abs() < 1e-12 {
        return;
    }
    let (direction, top, bottom) = if a.y < b.y { (1.0, a, b) } else { (-1.0, b, a) };
    let dxdy = (bottom.x - top.x) / (bottom.y - top.y);
    let y_start = top.y.max(0.0);
    let y_end = bottom.y.min(rows as f64);
    if y_start >= y_end {
        return;
    }
    let mut x = top.x + (y_start - top.y) * dxdy;
    for row in y_start.floor() as usize..(y_end.ceil() as usize).min(rows) {
        let row_top = (row as f64).max(y_start);
        let row_bottom = ((row + 1) as f64).min(y_end);
        let dy = row_bottom - row_top;
        let x_next = x + dy * dxdy;
        let d = (dy * direction) as f32;
        let (x0, x1) = if x < x_next { (x, x_next) } else { (x_next, x) };
        let base = row * stride;
        let x0_floor = x0.floor();
        let x0_int = x0_floor as isize;
        let x1_ceil = x1.ceil() as isize;
        if x1_ceil <= x0_int + 1 {
            // The edge stays within one pixel column.
            let xmf = (0.5 * (x + x_next) - x0_floor) as f32;
            add(accumulation, base, stride, x0_int, d - d * xmf);
            add(accumulation, base, stride, x0_int + 1, d * xmf);
        } else {
            let s = (1.0 / (x1 - x0)) as f32;
            let x0f = (x0 - x0_floor) as f32;
            let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
            let x1f = (x1 - x1.ceil() + 1.0) as f32;
            let am = 0.5 * s * x1f * x1f;
            add(accumulation, base, stride, x0_int, d * a0);
            if x1_ceil == x0_int + 2 {
                add(accumulation, base, stride, x0_int + 1, d * (1.0 - a0 - am));
            } else {
                let a1 = s * (1.5 - x0f);
                add(accumulation, base, stride, x0_int + 1, d * (a1 - a0));
                for column in x0_int + 2..x1_ceil - 1 {
                    add(accumulation, base, stride, column, d * s);
                }
                let a2 = a1 + (x1_ceil - x0_int - 3) as f32 * s;
                add(accumulation, base, stride, x1_ceil - 1, d * (1.0 - a2 - am));
            }
            add(accumulation, base, stride, x1_ceil, d * am);
        }
        x = x_next;
    }
}

fn add(accumulation: &mut [f32], base: usize, stride: usize, column: isize, value: f32) {
    // Area left of the canvas still counts toward the row's running sum;
    // area right of it no longer matters.
    if column < stride as isize {
        accumulation[base + column.max(0) as usize] += value;
    }
}

/// The outline of `path` stroked with `width`: butt caps, miter joins
/// (bevelled past a miter limit of 10, z2d's default).
fn stroke_outline(path: &Path, width: f64) -> Path {
    let half = width / 2.0;
    let mut outline = Path::default();
    for (points, closed) in &path.contours {
        let mut points: Vec<DVec2> = points.clone();
        points.dedup_by(|a, b| a.distance(*b) < 1e-9);
        if *closed && points.len() > 2 && points[0].distance(*points.last().unwrap()) < 1e-9 {
            points.pop();
        }
        if points.len() < 2 {
            continue;
        }
        let n = points.len();
        let segments = if *closed { n } else { n - 1 };
        let normal = |i: usize| {
            let d = (points[(i + 1) % n] - points[i]).normalize();
            DVec2::new(-d.y, d.x)
        };
        // Offset points on one side at each vertex.
        let side = |sign: f64| -> Vec<DVec2> {
            let mut result = Vec::with_capacity(n + 2);
            for (i, point) in points.iter().enumerate() {
                let before = if i > 0 {
                    Some(normal(i - 1))
                } else if *closed {
                    Some(normal(segments - 1))
                } else {
                    None
                };
                let after = (i < segments).then(|| normal(i));
                match (before, after) {
                    (Some(a), Some(b)) => {
                        let miter = a + b;
                        let cos = a.dot(b);
                        if miter.length_squared() > 1e-12 && (2.0 / (1.0 + cos)).sqrt() <= 10.0 {
                            let miter = miter.normalize() * (half / ((1.0 + cos) / 2.0).sqrt());
                            result.push(*point + miter * sign);
                        } else {
                            result.push(*point + a * half * sign);
                            result.push(*point + b * half * sign);
                        }
                    }
                    (Some(a), None) => result.push(*point + a * half * sign),
                    (None, Some(b)) => result.push(*point + b * half * sign),
                    (None, None) => {}
                }
            }
            result
        };
        let left = side(1.0);
        let right = side(-1.0);
        if *closed {
            outline.contours.push((left, true));
            outline
                .contours
                .push((right.into_iter().rev().collect(), true));
        } else {
            let mut contour = left;
            contour.extend(right.into_iter().rev());
            outline.contours.push((contour, true));
        }
    }
    outline
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_polygons_have_exact_area_coverage() {
        let mut canvas = Canvas::new(8, 8);
        let mut path = Path::default();
        // A pixel-aligned square is fully opaque with crisp edges.
        path.move_to(1.0, 1.0)
            .line_to(5.0, 1.0)
            .line_to(5.0, 5.0)
            .line_to(1.0, 5.0)
            .close();
        canvas.fill_path(&path, Shade::On);
        let sprite = canvas.finish().unwrap();
        assert_eq!(sprite.offset, IVec2::new(1, 1));
        assert_eq!(sprite.size, UVec2::new(4, 4));
        assert!(sprite.alpha.iter().all(|a| *a == 255));

        // A half-pixel offset halves the edge coverage.
        let mut canvas = Canvas::new(8, 8);
        let mut path = Path::default();
        path.move_to(1.5, 1.0)
            .line_to(3.5, 1.0)
            .line_to(3.5, 2.0)
            .line_to(1.5, 2.0)
            .close();
        canvas.fill_path(&path, Shade::On);
        let sprite = canvas.finish().unwrap();
        assert_eq!(sprite.alpha, vec![128, 255, 128]);

        // A diagonal splits pixels by area.
        let mut canvas = Canvas::new(8, 8);
        canvas.triangle(
            [
                DVec2::new(0.0, 0.0),
                DVec2::new(2.0, 0.0),
                DVec2::new(0.0, 2.0),
            ],
            Shade::On,
        );
        let sprite = canvas.finish().unwrap();
        assert_eq!(sprite.alpha, vec![255, 128, 128, 0]);
    }

    #[test]
    fn strokes_and_inner_strokes_cover_their_bands() {
        let mut canvas = Canvas::new(8, 8);
        canvas.line(DVec2::new(0.0, 4.0), DVec2::new(8.0, 4.0), 2.0, Shade::On);
        let sprite = canvas.finish().unwrap();
        assert_eq!(
            (sprite.offset, sprite.size),
            (IVec2::new(0, 3), UVec2::new(8, 2))
        );
        assert!(sprite.alpha.iter().all(|a| *a == 255));

        // A ring: the stroke of a closed contour leaves its middle empty.
        let mut canvas = Canvas::new(8, 8);
        let mut square = Path::default();
        square
            .move_to(0.0, 0.0)
            .line_to(8.0, 0.0)
            .line_to(8.0, 8.0)
            .line_to(0.0, 8.0)
            .close();
        canvas.inner_stroke_path(&square, 1.0, Shade::On);
        let sprite = canvas.finish().unwrap();
        assert_eq!(sprite.size, UVec2::new(8, 8));
        assert_eq!(sprite.alpha[0], 255);
        assert_eq!(sprite.alpha[9], 0, "inside the one-pixel band");
    }

    #[test]
    fn clipping_and_flipping_keep_the_cell_frame() {
        let mut canvas = Canvas::new(4, 4);
        canvas.box_(-1, 0, 2, 1, Shade::On);
        let unclipped = canvas.finish().unwrap();
        assert_eq!(unclipped.offset, IVec2::new(-1, 0));
        canvas.clip_to_cell();
        assert_eq!(canvas.finish().unwrap().offset, IVec2::new(0, 0));
        canvas.flip_horizontal();
        let flipped = canvas.finish().unwrap();
        assert_eq!(
            (flipped.offset, flipped.size),
            (IVec2::new(2, 0), UVec2::new(2, 1))
        );
    }
}
