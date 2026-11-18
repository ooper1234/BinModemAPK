//! The phone layout in a desktop window: `cargo run --example preview`.
//! `BINMODEM_PAGE=terminal|scopes|status` opens on another tab.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    binmodem::preview()
}
