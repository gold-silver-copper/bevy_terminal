//! Application-owned grid fitting and backend geometry adoption.

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
    let other = RatatuiTerminal::new(4, 2);
    let foreign = measure(other.surface(), Vec2::new(10.0, 20.0));
    assert!(!app::fit_grid(
        &mut terminal,
        &foreign,
        Vec2::new(805.0, 245.0)
    ));
    assert_eq!(terminal.size().unwrap(), Size::new(4, 2));
    let texture = measure(terminal.surface(), Vec2::new(10.0, 20.0));
    assert!(app::fit_grid(
        &mut terminal,
        &texture,
        Vec2::new(805.0, 245.0)
    ));
    assert_eq!(terminal.size().unwrap(), Size::new(80, 12));
    // Old geometry is no longer authoritative after fitting resized the grid.
    assert!(!app::fit_grid(&mut terminal, &texture, Vec2::new(5.0, 5.0)));
    let texture = measure(terminal.surface(), Vec2::new(10.0, 20.0));
    assert!(!app::fit_grid(
        &mut terminal,
        &texture,
        Vec2::new(809.0, 259.0)
    ));
    assert!(app::fit_grid(&mut terminal, &texture, Vec2::new(5.0, 5.0)));
    assert_eq!(terminal.size().unwrap(), Size::new(1, 1));
}

#[test]
fn fitting_adopts_fractional_pixel_geometry_without_resizing() {
    use ratatui::backend::Backend;

    let mut terminal = RatatuiTerminal::new(4, 2);
    let initial = measure(terminal.surface(), Vec2::new(10.0, 20.0));
    assert!(!app::fit_grid(
        &mut terminal,
        &initial,
        Vec2::new(40.0, 40.0)
    ));
    assert_eq!(
        terminal.backend_mut().window_size().unwrap().pixels,
        Size::new(40, 40)
    );

    let mut changed = common::measure_at_scale(terminal.surface(), Vec2::new(10.5, 20.5), 2.0);
    let geometry = changed.measured().unwrap();
    assert_eq!(geometry.cell_size(), Vec2::new(10.5, 20.5));
    assert!(!app::fit_grid(
        &mut terminal,
        &changed,
        Vec2::new(42.0, 41.0)
    ));
    assert_eq!(
        terminal.backend_mut().window_size().unwrap().pixels,
        Size::new(84, 82)
    );
    assert_eq!(terminal.surface().size(), GridSize::new(4, 2));

    for status in [TerminalStatus::Loading, TerminalStatus::FontFailed] {
        changed.status = status;
        assert!(!app::fit_grid(&mut terminal, &changed, Vec2::splat(1000.0)));
        assert_eq!(terminal.surface().size(), GridSize::new(4, 2));
        assert_eq!(
            terminal.backend_mut().window_size().unwrap().pixels,
            Size::new(84, 82)
        );
    }
}
