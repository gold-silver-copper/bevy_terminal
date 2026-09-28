//! Box Drawing, U+2500–257F (Ghostty's `sprite/draw/box.zig`).

use super::canvas::{Canvas, Path, Shade};
use super::{Corner, Metrics, Thickness};
use bevy::math::DVec2;

/// The style of a line from the cell's center to one edge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Style {
    None,
    Light,
    Heavy,
    Double,
}

use Style::{Double as D, Heavy as H, Light as L, None as N};

pub(super) fn draw(codepoint: u32, canvas: &mut Canvas, metrics: Metrics) {
    let light = Thickness::Light.pixels(metrics.box_thickness);
    let heavy = Thickness::Heavy.pixels(metrics.box_thickness);
    match codepoint {
        0x2500 => lines(metrics, canvas, [N, L, N, L]),
        0x2501 => lines(metrics, canvas, [N, H, N, H]),
        0x2502 => lines(metrics, canvas, [L, N, L, N]),
        0x2503 => lines(metrics, canvas, [H, N, H, N]),
        0x250c => lines(metrics, canvas, [N, L, L, N]),
        0x250d => lines(metrics, canvas, [N, H, L, N]),
        0x250e => lines(metrics, canvas, [N, L, H, N]),
        0x250f => lines(metrics, canvas, [N, H, H, N]),
        0x2510 => lines(metrics, canvas, [N, N, L, L]),
        0x2511 => lines(metrics, canvas, [N, N, L, H]),
        0x2512 => lines(metrics, canvas, [N, N, H, L]),
        0x2513 => lines(metrics, canvas, [N, N, H, H]),
        0x2514 => lines(metrics, canvas, [L, L, N, N]),
        0x2515 => lines(metrics, canvas, [L, H, N, N]),
        0x2516 => lines(metrics, canvas, [H, L, N, N]),
        0x2517 => lines(metrics, canvas, [H, H, N, N]),
        0x2518 => lines(metrics, canvas, [L, N, N, L]),
        0x2519 => lines(metrics, canvas, [L, N, N, H]),
        0x251a => lines(metrics, canvas, [H, N, N, L]),
        0x251b => lines(metrics, canvas, [H, N, N, H]),
        0x251c => lines(metrics, canvas, [L, L, L, N]),
        0x251d => lines(metrics, canvas, [L, H, L, N]),
        0x251e => lines(metrics, canvas, [H, L, L, N]),
        0x251f => lines(metrics, canvas, [L, L, H, N]),
        0x2520 => lines(metrics, canvas, [H, L, H, N]),
        0x2521 => lines(metrics, canvas, [H, H, L, N]),
        0x2522 => lines(metrics, canvas, [L, H, H, N]),
        0x2523 => lines(metrics, canvas, [H, H, H, N]),
        0x2524 => lines(metrics, canvas, [L, N, L, L]),
        0x2525 => lines(metrics, canvas, [L, N, L, H]),
        0x2526 => lines(metrics, canvas, [H, N, L, L]),
        0x2527 => lines(metrics, canvas, [L, N, H, L]),
        0x2528 => lines(metrics, canvas, [H, N, H, L]),
        0x2529 => lines(metrics, canvas, [H, N, L, H]),
        0x252a => lines(metrics, canvas, [L, N, H, H]),
        0x252b => lines(metrics, canvas, [H, N, H, H]),
        0x252c => lines(metrics, canvas, [N, L, L, L]),
        0x252d => lines(metrics, canvas, [N, L, L, H]),
        0x252e => lines(metrics, canvas, [N, H, L, L]),
        0x252f => lines(metrics, canvas, [N, H, L, H]),
        0x2530 => lines(metrics, canvas, [N, L, H, L]),
        0x2531 => lines(metrics, canvas, [N, L, H, H]),
        0x2532 => lines(metrics, canvas, [N, H, H, L]),
        0x2533 => lines(metrics, canvas, [N, H, H, H]),
        0x2534 => lines(metrics, canvas, [L, L, N, L]),
        0x2535 => lines(metrics, canvas, [L, L, N, H]),
        0x2536 => lines(metrics, canvas, [L, H, N, L]),
        0x2537 => lines(metrics, canvas, [L, H, N, H]),
        0x2538 => lines(metrics, canvas, [H, L, N, L]),
        0x2539 => lines(metrics, canvas, [H, L, N, H]),
        0x253a => lines(metrics, canvas, [H, H, N, L]),
        0x253b => lines(metrics, canvas, [H, H, N, H]),
        0x253c => lines(metrics, canvas, [L, L, L, L]),
        0x253d => lines(metrics, canvas, [L, L, L, H]),
        0x253e => lines(metrics, canvas, [L, H, L, L]),
        0x253f => lines(metrics, canvas, [L, H, L, H]),
        0x2540 => lines(metrics, canvas, [H, L, L, L]),
        0x2541 => lines(metrics, canvas, [L, L, H, L]),
        0x2542 => lines(metrics, canvas, [H, L, H, L]),
        0x2543 => lines(metrics, canvas, [H, L, L, H]),
        0x2544 => lines(metrics, canvas, [H, H, L, L]),
        0x2545 => lines(metrics, canvas, [L, L, H, H]),
        0x2546 => lines(metrics, canvas, [L, H, H, L]),
        0x2547 => lines(metrics, canvas, [H, H, L, H]),
        0x2548 => lines(metrics, canvas, [L, H, H, H]),
        0x2549 => lines(metrics, canvas, [H, L, H, H]),
        0x254a => lines(metrics, canvas, [H, H, H, L]),
        0x254b => lines(metrics, canvas, [H, H, H, H]),
        0x2550 => lines(metrics, canvas, [N, D, N, D]),
        0x2551 => lines(metrics, canvas, [D, N, D, N]),
        0x2552 => lines(metrics, canvas, [N, D, L, N]),
        0x2553 => lines(metrics, canvas, [N, L, D, N]),
        0x2554 => lines(metrics, canvas, [N, D, D, N]),
        0x2555 => lines(metrics, canvas, [N, N, L, D]),
        0x2556 => lines(metrics, canvas, [N, N, D, L]),
        0x2557 => lines(metrics, canvas, [N, N, D, D]),
        0x2558 => lines(metrics, canvas, [L, D, N, N]),
        0x2559 => lines(metrics, canvas, [D, L, N, N]),
        0x255a => lines(metrics, canvas, [D, D, N, N]),
        0x255b => lines(metrics, canvas, [L, N, N, D]),
        0x255c => lines(metrics, canvas, [D, N, N, L]),
        0x255d => lines(metrics, canvas, [D, N, N, D]),
        0x255e => lines(metrics, canvas, [L, D, L, N]),
        0x255f => lines(metrics, canvas, [D, L, D, N]),
        0x2560 => lines(metrics, canvas, [D, D, D, N]),
        0x2561 => lines(metrics, canvas, [L, N, L, D]),
        0x2562 => lines(metrics, canvas, [D, N, D, L]),
        0x2563 => lines(metrics, canvas, [D, N, D, D]),
        0x2564 => lines(metrics, canvas, [N, D, L, D]),
        0x2565 => lines(metrics, canvas, [N, L, D, L]),
        0x2566 => lines(metrics, canvas, [N, D, D, D]),
        0x2567 => lines(metrics, canvas, [L, D, N, D]),
        0x2568 => lines(metrics, canvas, [D, L, N, L]),
        0x2569 => lines(metrics, canvas, [D, D, N, D]),
        0x256a => lines(metrics, canvas, [L, D, L, D]),
        0x256b => lines(metrics, canvas, [D, L, D, L]),
        0x256c => lines(metrics, canvas, [D, D, D, D]),
        0x2574 => lines(metrics, canvas, [N, N, N, L]),
        0x2575 => lines(metrics, canvas, [L, N, N, N]),
        0x2576 => lines(metrics, canvas, [N, L, N, N]),
        0x2577 => lines(metrics, canvas, [N, N, L, N]),
        0x2578 => lines(metrics, canvas, [N, N, N, H]),
        0x2579 => lines(metrics, canvas, [H, N, N, N]),
        0x257a => lines(metrics, canvas, [N, H, N, N]),
        0x257b => lines(metrics, canvas, [N, N, H, N]),
        0x257c => lines(metrics, canvas, [N, H, N, L]),
        0x257d => lines(metrics, canvas, [L, N, H, N]),
        0x257e => lines(metrics, canvas, [N, L, N, H]),
        0x257f => lines(metrics, canvas, [H, N, L, N]),
        0x2504 => dash_horizontal(metrics, canvas, 3, light, light.max(4)),
        0x2505 => dash_horizontal(metrics, canvas, 3, heavy, light.max(4)),
        0x2506 => dash_vertical(metrics, canvas, 3, light, light.max(4)),
        0x2507 => dash_vertical(metrics, canvas, 3, heavy, light.max(4)),
        0x2508 => dash_horizontal(metrics, canvas, 4, light, light.max(4)),
        0x2509 => dash_horizontal(metrics, canvas, 4, heavy, light.max(4)),
        0x250a => dash_vertical(metrics, canvas, 4, light, light.max(4)),
        0x250b => dash_vertical(metrics, canvas, 4, heavy, light.max(4)),
        0x254c => dash_horizontal(metrics, canvas, 2, light, light),
        0x254d => dash_horizontal(metrics, canvas, 2, heavy, heavy),
        0x254e => dash_vertical(metrics, canvas, 2, light, heavy),
        0x254f => dash_vertical(metrics, canvas, 2, heavy, heavy),
        0x256d => arc(metrics, canvas, Corner::BottomRight),
        0x256e => arc(metrics, canvas, Corner::BottomLeft),
        0x256f => arc(metrics, canvas, Corner::TopLeft),
        0x2570 => arc(metrics, canvas, Corner::TopRight),
        0x2571 => diagonal_rising(metrics, canvas),
        0x2572 => diagonal_falling(metrics, canvas),
        0x2573 => {
            diagonal_rising(metrics, canvas);
            diagonal_falling(metrics, canvas);
        }
        _ => unreachable!("box drawing codepoint {codepoint:x}"),
    }
}

/// Draws lines from the center to the edges in `[up, right, down, left]`
/// styles, joining heavy, light and doubled strokes as Ghostty does.
pub(super) fn lines(metrics: Metrics, canvas: &mut Canvas, [up, right, down, left]: [Style; 4]) {
    let light = Thickness::Light.pixels(metrics.box_thickness);
    let heavy = Thickness::Heavy.pixels(metrics.box_thickness);
    let (width, height) = (metrics.cell_width, metrics.cell_height);

    let h_light_top = height.saturating_sub(light) / 2;
    let h_light_bottom = h_light_top.saturating_add(light);
    let h_heavy_top = height.saturating_sub(heavy) / 2;
    let h_heavy_bottom = h_heavy_top.saturating_add(heavy);
    let h_double_top = h_light_top.saturating_sub(light);
    let h_double_bottom = h_light_bottom.saturating_add(light);

    let v_light_left = width.saturating_sub(light) / 2;
    let v_light_right = v_light_left.saturating_add(light);
    let v_heavy_left = width.saturating_sub(heavy) / 2;
    let v_heavy_right = v_heavy_left.saturating_add(heavy);
    let v_double_left = v_light_left.saturating_sub(light);
    let v_double_right = v_light_right.saturating_add(light);

    let up_bottom = if left == H || right == H {
        h_heavy_bottom
    } else if left != right || down == up {
        if left == D || right == D {
            h_double_bottom
        } else {
            h_light_bottom
        }
    } else if left == N && right == N {
        h_light_bottom
    } else {
        h_light_top
    };
    let down_top = if left == H || right == H {
        h_heavy_top
    } else if left != right || up == down {
        if left == D || right == D {
            h_double_top
        } else {
            h_light_top
        }
    } else if left == N && right == N {
        h_light_top
    } else {
        h_light_bottom
    };
    let left_right = if up == H || down == H {
        v_heavy_right
    } else if up != down || left == right {
        if up == D || down == D {
            v_double_right
        } else {
            v_light_right
        }
    } else if up == N && down == N {
        v_light_right
    } else {
        v_light_left
    };
    let right_left = if up == H || down == H {
        v_heavy_left
    } else if up != down || right == left {
        if up == D || down == D {
            v_double_left
        } else {
            v_light_left
        }
    } else if up == N && down == N {
        v_light_left
    } else {
        v_light_right
    };

    let mut rect = |x0: u32, y0: u32, x1: u32, y1: u32| {
        canvas.box_(x0 as i32, y0 as i32, x1 as i32, y1 as i32, Shade::On);
    };
    match up {
        N => {}
        L => rect(v_light_left, 0, v_light_right, up_bottom),
        H => rect(v_heavy_left, 0, v_heavy_right, up_bottom),
        D => {
            let left_bottom = if left == D { h_light_top } else { up_bottom };
            let right_bottom = if right == D { h_light_top } else { up_bottom };
            rect(v_double_left, 0, v_light_left, left_bottom);
            rect(v_light_right, 0, v_double_right, right_bottom);
        }
    }
    match right {
        N => {}
        L => rect(right_left, h_light_top, width, h_light_bottom),
        H => rect(right_left, h_heavy_top, width, h_heavy_bottom),
        D => {
            let top_left = if up == D { v_light_right } else { right_left };
            let bottom_left = if down == D { v_light_right } else { right_left };
            rect(top_left, h_double_top, width, h_light_top);
            rect(bottom_left, h_light_bottom, width, h_double_bottom);
        }
    }
    match down {
        N => {}
        L => rect(v_light_left, down_top, v_light_right, height),
        H => rect(v_heavy_left, down_top, v_heavy_right, height),
        D => {
            let left_top = if left == D { h_light_bottom } else { down_top };
            let right_top = if right == D { h_light_bottom } else { down_top };
            rect(v_double_left, left_top, v_light_left, height);
            rect(v_light_right, right_top, v_double_right, height);
        }
    }
    match left {
        N => {}
        L => rect(0, h_light_top, left_right, h_light_bottom),
        H => rect(0, h_heavy_top, left_right, h_heavy_bottom),
        D => {
            let top_right = if up == D { v_light_left } else { left_right };
            let bottom_right = if down == D { v_light_left } else { left_right };
            rect(0, h_double_top, top_right, h_light_top);
            rect(0, h_light_bottom, bottom_right, h_double_bottom);
        }
    }
}

/// The slope-preserving overshoot of a cell-spanning diagonal.
fn overshoot(metrics: Metrics) -> DVec2 {
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    DVec2::new((width / height).min(1.0), (height / width).min(1.0)) * 0.5
}

/// `╱`: from the upper right to the lower left corner.
pub(super) fn diagonal_rising(metrics: Metrics, canvas: &mut Canvas) {
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let slope = overshoot(metrics);
    canvas.line(
        DVec2::new(width + slope.x, -slope.y),
        DVec2::new(-slope.x, height + slope.y),
        f64::from(Thickness::Light.pixels(metrics.box_thickness)),
        Shade::On,
    );
}

/// `╲`: from the upper left to the lower right corner.
pub(super) fn diagonal_falling(metrics: Metrics, canvas: &mut Canvas) {
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let slope = overshoot(metrics);
    canvas.line(
        DVec2::new(-slope.x, -slope.y),
        DVec2::new(width + slope.x, height + slope.y),
        f64::from(Thickness::Light.pixels(metrics.box_thickness)),
        Shade::On,
    );
}

/// A rounded light corner joining the center lines towards `corner`'s two edges.
pub(super) fn arc(metrics: Metrics, canvas: &mut Canvas, corner: Corner) {
    let thickness = Thickness::Light.pixels(metrics.box_thickness);
    let (width, height) = (
        f64::from(metrics.cell_width),
        f64::from(metrics.cell_height),
    );
    let thick = f64::from(thickness);
    let cx = f64::from(metrics.cell_width.saturating_sub(thickness) / 2) + thick / 2.0;
    let cy = f64::from(metrics.cell_height.saturating_sub(thickness) / 2) + thick / 2.0;
    let r = width.min(height) / 2.0;
    // Fraction away from the center of the middle control points.
    let s = 0.25;
    let (vertical_edge, horizontal_edge, dy, dx) = match corner {
        Corner::TopLeft => (0.0, 0.0, -1.0, -1.0),
        Corner::TopRight => (0.0, width, -1.0, 1.0),
        Corner::BottomLeft => (height, 0.0, 1.0, -1.0),
        Corner::BottomRight => (height, width, 1.0, 1.0),
    };
    let mut path = Path::default();
    path.move_to(cx, vertical_edge)
        .line_to(cx, cy + dy * r)
        .curve_to(cx, cy + dy * s * r, cx + dx * s * r, cy, cx + dx * r, cy)
        .line_to(horizontal_edge, cy);
    canvas.stroke_path(&path, thick, Shade::On);
}

/// Evenly tiling horizontal dashes: `count` dashes with half gaps at both ends.
fn dash_horizontal(
    metrics: Metrics,
    canvas: &mut Canvas,
    count: u32,
    thick: u32,
    desired_gap: u32,
) {
    let width = metrics.cell_width;
    if width < 2 * count {
        let light = Thickness::Light.pixels(metrics.box_thickness);
        let y = metrics.cell_height.saturating_sub(light) / 2;
        canvas.box_(0, y as i32, width as i32, (y + light) as i32, Shade::On);
        return;
    }
    let gap = desired_gap.min(width / (2 * count)) as i32;
    let total_dash = width as i32 - count as i32 * gap;
    let dash = total_dash.div_euclid(count as i32);
    let mut extra = total_dash.rem_euclid(count as i32);
    let y = (metrics.cell_height.saturating_sub(thick) / 2) as i32;
    let mut x = gap.div_euclid(2);
    for _ in 0..count {
        let mut x1 = x + dash;
        if extra > 0 {
            extra -= 1;
            x1 += 1;
        }
        canvas.box_(x, y, x1, y + thick as i32, Shade::On);
        x = x1 + gap;
    }
}

/// Evenly tiling vertical dashes: `count` dashes and a full gap at the bottom.
fn dash_vertical(metrics: Metrics, canvas: &mut Canvas, count: u32, thick: u32, desired_gap: u32) {
    let height = metrics.cell_height;
    if height < 2 * count {
        let light = Thickness::Light.pixels(metrics.box_thickness);
        let x = metrics.cell_width.saturating_sub(light) / 2;
        canvas.box_(x as i32, 0, (x + light) as i32, height as i32, Shade::On);
        return;
    }
    let gap = desired_gap.min(height / (2 * count)) as i32;
    let total_dash = height as i32 - count as i32 * gap;
    let dash = total_dash.div_euclid(count as i32);
    let mut extra = total_dash.rem_euclid(count as i32);
    let x = (metrics.cell_width.saturating_sub(thick) / 2) as i32;
    let mut y = 0;
    for _ in 0..count {
        let mut y1 = y + dash;
        if extra > 0 {
            extra -= 1;
            y1 += 1;
        }
        canvas.box_(x, y, x + thick as i32, y1, Shade::On);
        y = y1 + gap;
    }
}
