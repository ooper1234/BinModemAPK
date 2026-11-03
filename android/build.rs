//! Compile the upstream GUI with a phone layout grafted onto it.
//!
//! The phone layout needs the window's private state and helpers, which only
//! code inside the `app` module (or below it) can reach. Rather than edit the
//! submodule, the sources are copied into OUT_DIR with three swaps -- combo
//! boxes and sliders that wrap, and windows that open as full-screen sheets
//! (mobile::Combo, mobile::Slider, mobile::Window) -- and app.rs gets one
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
            .replace("egui::Window::new(", "crate::app::mobile::Window::new(");
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
