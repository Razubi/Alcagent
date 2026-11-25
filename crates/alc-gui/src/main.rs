//! Native entry point for the Alcagent observer.
//!
//! Usage: `cargo run -p alc-gui -- [seed]` (seed defaults to `0xA1CA`).

fn main() -> eframe::Result<()> {
    let seed = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse::<u64>().ok())
        .unwrap_or(0xA1CA);
    alc_gui::launch(seed)
}
