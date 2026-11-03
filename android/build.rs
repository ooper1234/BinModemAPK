//! Compile the upstream GUI with a phone layout grafted onto it.
//!
//! The phone layout needs the window's private state and helpers, which only
//! code inside the `app` module (or below it) can reach. Rather than edit the
//! submodule, the sources are copied into OUT_DIR with four swaps -- rows
//! held to the screen, combo boxes and sliders that wrap, and windows that open as full-screen sheets
//! (mobile::wrapped, Combo, Slider, Window) -- live.rs's two ways out
//! to a speaker go through the sound switch (mobile::muted), and app.rs gets one
//! line added: `mod mobile;`, pointing at src/mobile.rs here. Upstream stays
//! byte-for-byte as checked out.

use std::fs;
use std::path::PathBuf;

fn main() {
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let upstream = here.join("../BinModem/crates/gui/src").canonicalize().unwrap();
    let mobile = here.join("src/mobile.rs").canonicalize().unwrap();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("gui");
    fs::create_dir_all(&out).unwrap();

    println!("cargo:rerun-if-changed={}", upstream.display());
    println!("cargo:rerun-if-changed={}", mobile.display());

    let mut mods = String::new();
    for entry in fs::read_dir(&upstream).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        println!("cargo:rerun-if-changed={}", path.display());
        // The desktop entry point is replaced by src/lib.rs.
        if name == "main" {
            continue;
        }
        // Files that make windows, combo boxes or sliders are copied with
        // those swapped for the phone versions in src/mobile.rs; app.rs also
        // gets `mod mobile;` added, so the layout can reach its private state.
        // Everything else is compiled where it is, so anything it includes by
        // relative path is still found.
        let text = fs::read_to_string(&path).unwrap();
        let patched = text
            .replace("egui::ComboBox::from_id_salt(", "crate::app::mobile::Combo::from_id_salt(")
            .replace("egui::Slider::new(", "crate::app::mobile::Slider::new(")
            .replace("egui::Window::new(", "crate::app::mobile::Window::new(")
            .replace("ui.horizontal_wrapped(", "crate::app::mobile::wrapped(ui, ");
        // The sound switch (mobile::muted): the modem's tones to a sound card,
        // and the monitor that plays the caller. Checked to apply exactly
        // once, so a change upstream fails the build instead of leaving the
        // switch quietly doing nothing.
        let patched = if name == "live" {
            let mut patched = patched;
            for (from, to) in [
                (
                    "Self::Audio(line) => line.transmit(samples),",
                    "Self::Audio(line) => crate::app::mobile::to_speaker(line, samples),",
                ),
                (
                    "sink.push(&from_line);",
                    "if !crate::app::mobile::muted() { sink.push(&from_line); }",
                ),
            ] {
                assert_eq!(patched.matches(from).count(), 1, "live.rs: `{from}` has changed upstream");
                patched = patched.replace(from, to);
            }
            patched
        } else {
            patched
        };
        let source = if name == "app" || patched != text {
            assert!(
                !text.contains("include_bytes!") && !text.contains("include_str!"),
                "{name}.rs includes a file by relative path and cannot be copied"
            );
            let mut patched = patched;
            if name == "app" {
                patched.push_str(&format!(
                    "\n#[path = {:?}]\npub mod mobile;\n",
                    mobile.display().to_string()
                ));
            }
            let copy = out.join(format!("{name}.rs"));
            fs::write(&copy, patched).unwrap();
            copy
        } else {
            path.clone()
        };
        mods.push_str(&format!(
            "#[path = {:?}]\nmod {name};\n",
            source.display().to_string()
        ));
    }
    fs::write(out.join("mods.rs"), mods).unwrap();
}
