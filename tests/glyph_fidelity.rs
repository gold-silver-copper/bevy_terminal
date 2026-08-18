//! Runs the `glyph_fidelity` harness's automated check for every vendored font
//! family at every raster scale. Needs a GPU; run with
//! `cargo test -p bevy_terminal_ratatui --test glyph_fidelity -- --ignored`.

use std::process::Command;

#[test]
#[ignore = "requires a GPU and the vendored fonts under assets/fonts"]
fn no_primary_glyph_is_clipped_and_tiles_have_no_seams() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "run",
            "--quiet",
            "--example",
            "glyph_fidelity",
            "--",
            "--check",
            "--font",
            "all",
            "--scale",
            "all",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .expect("cargo runs");
    assert!(status.success(), "glyph fidelity check failed: {status}");
}
