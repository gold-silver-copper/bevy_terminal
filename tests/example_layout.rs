//! Application-owned grid fitting.

#[path = "../examples/common/app.rs"]
mod app;
mod common;

use bevy::prelude::*;
use bevy_terminal_ratatui::prelude::*;
use common::measure;
use ratatui::layout::Size;

#[test]
fn fit_to_resizes_the_grid_exactly_when_the_fit_changes() {
    let mut terminal = RatatuiTerminal::new(4, 2);
    let texture = measure(terminal.grid().clone(), Vec2::new(10.0, 20.0));
    assert!(app::fit_grid(
        &mut terminal,
        &texture,
        Vec2::new(805.0, 245.0)
    ));
    assert_eq!(terminal.size().unwrap(), Size::new(80, 12));
    // Old geometry is no longer authoritative after fitting resized the grid.
    assert!(!app::fit_grid(&mut terminal, &texture, Vec2::new(5.0, 5.0)));
    let texture = measure(terminal.grid().clone(), Vec2::new(10.0, 20.0));
    assert!(!app::fit_grid(
        &mut terminal,
        &texture,
        Vec2::new(809.0, 259.0)
    ));
    assert!(app::fit_grid(&mut terminal, &texture, Vec2::new(5.0, 5.0)));
    assert_eq!(terminal.size().unwrap(), Size::new(1, 1));
}

#[test]
fn fitting_fractional_cells_keeps_a_grid_that_already_fits() {
    let mut terminal = RatatuiTerminal::new(4, 2);
    let changed = common::measure_at_scale(terminal.grid().clone(), Vec2::new(10.5, 20.5), 2.0);
    let geometry = changed.measured().unwrap();
    assert_eq!(geometry.cell_size(), Vec2::new(10.5, 20.5));
    assert!(!app::fit_grid(
        &mut terminal,
        &changed,
        Vec2::new(42.0, 41.0)
    ));
    assert_eq!(terminal.grid().size(), GridSize::new(4, 2));
}
