//! Compile the upstream GUI with a phone layout grafted onto it.
//!
//! The phone layout needs the window's private state and helpers, which only
//! code inside the `app` module (or below it) can reach. Rather than edit the
//! submodule, the sources are copied into OUT_DIR with four swaps -- rows
//! held to the screen, combo boxes and sliders that wrap, and windows that open as full-screen sheets
//! (mobile::wrapped, Combo, Slider, Window) -- live.rs's two ways out
//! to a speaker go through the sound switch (mobile::muted), its recordings go
//! to mobile::captures_dir, and app.rs gets one
//! line added: `mod mobile;`, pointing at src/mobile.rs here. Upstream stays
//! byte-for-byte as checked out: the modem itself, V.92 included, is
//! upstream's, with no patches.

use std::fs;
use std::path::PathBuf;

fn main() {
    let here = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let upstream = here.join("../BinModem/crates/gui/src").canonicalize().unwrap();
    let mobile = here.join("src/mobile.rs").canonicalize().unwrap();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("gui");
    fs::create_dir_all(&out).unwrap();

    // Which upstream build this is, for the masthead (upstream's gui/build.rs
    // does the same for the desktop program): the submodule's commit.
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(here.join("../BinModem"))
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let commit = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let when = git(&["log", "-1", "--format=%cd", "--date=format:%Y-%m-%d %H:%M"]).unwrap_or_default();
    println!("cargo:rustc-env=BINMODEM_BUILD={commit}{} {when}", if dirty { "+" } else { "" });

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
        // Listen gives way to the Caller and Answerer switches, which keep
        // the monitor open themselves (with Sound).
        let patched = if name == "app" {
            let from = "        let listening = self.monitor.is_some();
        if ui
            .selectable_label(listening, if listening { \"Listening\" } else { \"Listen\" })
            .on_hover_text(\"Play the line audio through an output device\")
            .clicked()
        {
            self.set_listening(!listening);
        }
";
            assert_eq!(patched.matches(from).count(), 1, "app.rs: the Listen button has changed upstream");
            let mut patched = patched.replace(from, "        self.side_buttons(ui);\n");
            // Two rows of the faceplate that are fixed rows on the desktop
            // wrap on a phone: the name with the build and the line's state,
            // and the keys a call is made with.
            for (from, to) in [
                (
                    "        ui.horizontal(|ui| {\n            ui.label(RichText::new(\"BINMODEM\")",
                    "        crate::app::mobile::wrapped(ui, |ui| {\n            ui.label(RichText::new(\"BINMODEM\")",
                ),
                // The line's state and the Setup key on a row of their own,
                // not squeezed to "..." beside the build.
                (
                    "            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {\n                let Some(state) = state else {",
                    "            ui.end_row();\n            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {\n                let Some(state) = state else {",
                ),
                (
                    "        // The keys a call is made with.\n        ui.horizontal(|ui| {",
                    "        // The keys a call is made with.\n        crate::app::mobile::wrapped(ui, |ui| {",
                ),
            ] {
                assert_eq!(patched.matches(from).count(), 1, "app.rs: `{from}` has changed upstream");
                patched = patched.replace(from, to);
            }
            // The faceplate is a desktop's width (lamps beside a readout,
            // 520 points at least); a phone shows the readout across the
            // screen instead (mobile::phone_faceplate).
            let from = "        self.faceplate(ui);\n";
            assert_eq!(patched.matches(from).count(), 2, "app.rs: the faceplate calls have changed upstream");
            patched.replace(from, "        self.phone_faceplate(ui);\n")
        } else {
            patched
        };
        let patched = if name == "live" {
            let mut patched = patched;
            for (from, to) in [
                (
                    "Self::Audio(line) => line.transmit(samples),",
                    "Self::Audio(line) => crate::app::mobile::to_speaker(line, samples),",
                ),
                (
                    "sink.push(&from_line);",
                    "crate::app::mobile::monitor(&sink, &from_line, &to_line, if modem.exact_levels() { 1.0 } else { drive }, audio.sip().is_some(), modem.role() == Role::Answering);",
                ),
                // Recordings where they can be got at (mobile::captures_dir).
                (
                    "let dir = std::path::Path::new(\"captures\");",
                    "let dir = &crate::app::mobile::captures_dir();",
                ),
                // The spectrum and waterfall show the sides that are on.
                (
                    "spectrum.push(heard);",
                    "spectrum.push(crate::app::mobile::seen(heard, f64::from(*to_line.last().unwrap_or(&0.0)), modem.role() == Role::Answering));",
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
