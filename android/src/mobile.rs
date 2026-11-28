//! The window laid out for a phone held upright.
//!
//! Compiled as a child of the upstream `app` module (see build.rs), so it
//! uses the desktop window's own state and widgets: nothing about the modem
//! is reimplemented here, only where things go. One column, a strip of call
//! state along the top, and a row of tabs along the bottom choosing what the
//! rest of the screen is for.

use super::*;

/// What the screen below the status strip is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Line,
    Terminal,
    Scopes,
    Status,
}

/// State the phone layout keeps that the desktop window has no field for.
#[derive(Debug)]
pub struct Mobile {
    pub page: Page,
    /// Size the terminal's font to fit its columns across the screen.
    pub fit_font: bool,
    /// Whether the monitor has been opened for the Sound switch yet.
    pub sound_started: bool,
    /// The file browser standing in for a file dialog, while it is open.
    pub picker: Option<Picker>,
    /// Files are picked with Android's own picker (Google Photos, Drive,
    /// Downloads...) rather than the in-app browser. Set on a phone.
    pub system_picker: bool,
    /// What the system picker was opened for, while it is out.
    pub waiting: Option<rfd::Request>,
    /// The platform's answer: the picked file copied into the app's folder,
    /// or an empty string for cancelled.
    pub picked: Option<String>,
    /// Points at the top and bottom covered by the system bars or the
    /// keyboard, which the layout keeps clear of.
    pub insets: (f32, f32),
}

/// A file dialog, drawn by the app: Android has none a native app can use
/// without Java. Opened when the GUI asks the rfd stand-in for a file.
#[derive(Debug)]
pub struct Picker {
    request: rfd::Request,
    dir: std::path::PathBuf,
}

impl Default for Mobile {
    fn default() -> Self {
        Self { page: Page::Line, fit_font: true, sound_started: false, picker: None, system_picker: false, waiting: None, picked: None, insets: (0.0, 0.0) }
    }
}

/// What the layout wants of the platform this frame.
#[derive(Debug, Default)]
pub struct Requests {
    /// Raise (`Some(true)`) or drop (`Some(false)`) the soft keyboard.
    pub keyboard: Option<bool>,
    /// The terminal itself is on screen and takes the keys: it raises the
    /// keyboard when tapped. Anywhere else -- the fax number, a SIP
    /// password -- the keyboard follows whether a text box has the focus.
    pub terminal: bool,
    /// Ask for permission to read shared storage, for the file browser.
    pub storage: bool,
    /// Open the system file picker for these MIME types (empty: any).
    pub system_pick: Option<Vec<String>>,
}

const DIM: Color32 = Color32::from_rgb(140, 150, 165);
const BRIGHT: Color32 = Color32::from_rgb(220, 225, 235);
const GREEN: Color32 = Color32::from_rgb(90, 220, 130);
const AMBER: Color32 = Color32::from_rgb(245, 180, 70);
/// The colour of a chosen tab.
const ACCENT: Color32 = Color32::from_rgb(90, 170, 255);

/// Height of a touch target, in points.
const TOUCH_H: f32 = 44.0;

impl ScopeApp {
    pub fn mobile_ui(&mut self, ui: &mut egui::Ui, m: &mut Mobile) -> Requests {
        let mut requests = Requests::default();

        self.poll();
        self.note_last_call();
        self.remember();
        scopes::request_animation(ui.ctx());

        // Sound starts on, which means hearing the call: the monitor opened
        // the first time round, as if Listen had been pressed.
        if !m.sound_started {
            m.sound_started = true;
            if !muted() && self.monitor.is_none() {
                self.set_listening(true);
            }
        }

        // Clear of the status and navigation bars, and of the keyboard: a
        // phone with on-screen buttons draws them over the bottom of the app,
        // which is where the tabs are.
        let (top, bottom) = m.insets;
        // With the keyboard up the room left is small, and what it is for
        // is the terminal: no tabs, no lamps, and the extra keys in one row.
        let typing = bottom > 150.0;
        if top > 0.5 {
            egui::Panel::top("m-inset-top")
                .exact_size(top)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(ui.visuals().panel_fill))
                .show(ui, |_| {});
        }
        if bottom > 0.5 {
            egui::Panel::bottom("m-inset-bottom")
                .exact_size(bottom)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(Color32::BLACK))
                .show(ui, |_| {});
        }

        egui::Panel::top("m-status").show(ui, |ui| {
            ui.add_space(4.0);
            self.status_strip(ui, !typing);
            ui.add_space(4.0);
        });

        if !typing {
            egui::Panel::bottom("m-tabs").show(ui, |ui| {
                ui.add_space(4.0);
                tab_bar(ui, &mut m.page);
                ui.add_space(4.0);
            });
        }

        requests.terminal = m.page == Page::Terminal && self.tab == Tab::Terminal && m.picker.is_none();
        // The extra keys type into the terminal, so only beside it.
        if requests.terminal {
            egui::Panel::bottom("m-keys").show(ui, |ui| {
                ui.add_space(4.0);
                if typing {
                    self.extra_keys_compact(ui, &mut requests);
                } else {
                    self.extra_keys(ui, &mut requests);
                }
                ui.add_space(4.0);
            });
        }

        // The desktop's own pop-ups: the large constellation. The dialler,
        // transfers and network windows are drawn by the line controls.
        self.constellation_window(ui);

        let tab_was = self.tab;
        if let Some(path) = m.picked.take()
            && let Some(request) = m.waiting.take()
            && !path.is_empty()
        {
            self.deliver(&request, std::path::Path::new(&path));
        }
        if let Some(request) = rfd::take_request() {
            if m.system_picker && !request.folder {
                // Android's own picker; the answer comes back through
                // `picked` a few frames later.
                requests.system_pick = Some(mime_types(&request.extensions()));
                m.waiting = Some(request);
            } else {
                m.picker = Some(Picker { request, dir: picker_roots()[0].1.clone() });
                requests.storage = true;
            }
        }
        egui::CentralPanel::default().show(ui, |ui| {
            // The page under an id of its own. The central panel's is made
            // from a count of what was laid out before it, and the panels
            // before it come and go -- the tabs step aside for the keyboard
            // -- so every widget on the page was a different widget the
            // moment the keyboard came up. A text box that had just been
            // given the focus was gone, egui dropped the focus, and nothing
            // could be typed into the fax number.
            ui.scope_builder(egui::UiBuilder::new().id(egui::Id::new("m-page")), |ui| {
            // Upstream's line controls do more than draw: they start a fax
            // that Send fax or Wait for a fax asked for, take the pages that
            // arrive, and put up the prompt for an incoming SIP call. On the
            // desktop they are always on screen; here only the Line page
            // draws them, so whenever it is not showing they run unseen --
            // or a fax pressed on the Terminal page never started.
            if m.picker.is_some() || m.page != Page::Line {
                let mut hidden = ui.new_child(egui::UiBuilder::new().invisible());
                self.controls(&mut hidden);
            }
            if m.picker.is_some() {
                return self.picker(ui, m);
            }
            match m.page {
            Page::Line => self.page_line(ui),
            Page::Terminal => self.page_terminal(ui, m, &mut requests),
            Page::Scopes => self.page_scopes(ui),
            Page::Status => self.page_status(ui),
            }
            });
        });
        // The faceplate's Scopes and Fax keys pick a desktop tab; here they
        // go to the page that shows it.
        if self.tab != tab_was {
            match self.tab {
                Tab::Scopes => {
                    m.page = Page::Scopes;
                    self.tab = Tab::Terminal;
                }
                Tab::Fax if m.page == Page::Line => m.page = Page::Terminal,
                _ => {}
            }
        }

        requests
    }

    /// One line of what the call is doing, always in view.
    fn status_strip(&mut self, ui: &mut egui::Ui, lamps: bool) {
        let f = &self.frame;
        let colour = match f.state {
            telemetry::CallState::Connected => GREEN,
            telemetry::CallState::Idle => DIM,
            _ => AMBER,
        };
        let mut sound_clicked = false;
        ui.horizontal_wrapped(|ui| {
            sound_clicked = sound_label(ui);
            dot(ui, colour);
            ui.label(RichText::new(f.state.label()).strong().color(colour));
            if !f.modulation.is_empty() && f.modulation != "-" {
                ui.label(RichText::new(f.modulation).monospace().color(BRIGHT));
            }
            if let Some(rx) = f.bit_rate {
                let tx = f.tx_bit_rate.map_or_else(|| "-".to_owned(), |r| r.to_string());
                ui.label(RichText::new(format!("{rx}/{tx} bps")).monospace().color(BRIGHT));
            }
            ui.label(
                RichText::new(format!("{:.0} dBFS", f.rx_level_db))
                    .monospace()
                    .color(DIM),
            );
            let mode = match self.console.mode {
                Mode::Command => ("command", DIM),
                Mode::Online => ("online", GREEN),
            };
            ui.label(RichText::new(mode.0).monospace().color(mode.1));
        });
        if sound_clicked {
            self.toggle_sound();
        }
        ui.add_space(2.0);
        if lamps {
            scopes::faceplate(ui, &self.frame.leds);
        }
    }

    /// One switch for every sound, lit while it is on -- the same kind of
    /// toggle as V.8 and Listen. On is hearing the call, both sides of it:
    /// it opens the monitor if Listen is not already on. Off is silence: the
    /// monitor, and the modem's tones on a sound-card line.
    /// Caller and Answerer, in place of the desktop's Listen: each side of
    /// the call on or off, for what is heard and what the spectrum and
    /// waterfall draw. "you" marks our own modem's side once a call is placed.
    pub fn side_buttons(&mut self, ui: &mut egui::Ui) {
        let answering = self.frame.leds.aa;
        let placed = self.frame.state != telemetry::CallState::Idle;
        for caller in [true, false] {
            let on = side_on(caller);
            let name = if caller { "Caller" } else { "Answerer" };
            let ours = placed && caller != answering;
            let label = if ours { format!("{name} (you)") } else { name.to_owned() };
            if ui
                .selectable_label(on, label)
                .on_hover_text(if caller {
                    "Hear and see the side that dialled"
                } else {
                    "Hear and see the side that answered"
                })
                .clicked()
            {
                set_side(caller, !on);
                // Turning a side on is asking to hear it.
                if !on && !muted() && self.monitor.is_none() {
                    self.set_listening(true);
                }
            }
        }
    }

    fn toggle_sound(&mut self) {
        let was_on = !muted();
        set_muted(was_on);
        if was_on {
            // Off: close the monitor outright, so nothing it had queued
            // plays on after the press.
            if self.monitor.is_some() {
                self.set_listening(false);
            }
        } else if self.monitor.is_none() {
            self.set_listening(true);
        }
    }

    /// The line, the call and every setting: the desktop's top bar, which
    /// wraps itself to whatever width it is given.
    fn page_line(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.spacing_mut().interact_size.y = 34.0;
                within_screen(ui, |ui| {
                    if LONG_ROW.load(std::sync::atomic::Ordering::Relaxed) {
                        // What a registered SIP line shows, for the preview.
                        wrapped(ui, |ui| {
                            let _ = ui.button("Record");
                            ui.label(RichText::new("registered").monospace().color(GREEN));
                            ui.label(RichText::new("up with sip:16169882103@coopervoip-6745.sip.signalwire.com in PCMU").monospace().color(DIM));
                            ui.label(RichText::new("1 gaps").monospace().color(AMBER));
                        });
                    }
                    self.controls(ui)
                });
                ui.add_space(12.0);
                ui.label(RichText::new("receive level").strong());
                scopes::level_meter(ui, self.frame.rx_level_db);
                ui.add_space(24.0);
            });
    }

    fn page_terminal(&mut self, ui: &mut egui::Ui, m: &mut Mobile, requests: &mut Requests) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tab, Tab::Terminal, "terminal");
            ui.selectable_value(&mut self.tab, Tab::Transcript, "transcript");
            if self.source.is_live() {
                ui.selectable_value(&mut self.tab, Tab::Fax, "fax");
            }
            ui.selectable_value(&mut self.tab, Tab::Review, "review");
            if self.tab == Tab::Terminal {
                ui.separator();
                if ui.button("reset").clicked() {
                    self.console.term.reset();
                }
                ui.checkbox(&mut m.fit_font, "fit width");
                if !m.fit_font {
                    ui.add(egui::Slider::new(&mut self.font_size, 6.0..=22.0).text("font"));
                }
            }
        });
        ui.separator();

        match self.tab {
            Tab::Transcript => return self.transcript(ui),
            // Upstream's Fax and Review tabs, as they are on the desktop.
            Tab::Fax => return self.page_fax(ui),
            Tab::Review => return self.page_review(ui),
            // Scopes have a page of their own here.
            Tab::Scopes => self.tab = Tab::Terminal,
            Tab::Terminal => {}
        }

        if m.fit_font {
            let (cols, _) = self.console.term.size();
            let probe = FontId::monospace(10.0);
            let w10 = ui.ctx().fonts_mut(|f| f.glyph_width(&probe, 'M'));
            if w10 > 0.0 {
                let fit = 10.0 * (ui.available_width() - 4.0) / (cols as f32 * w10);
                self.font_size = fit.clamp(4.0, 22.0);
            }
        }

        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let view = console::view(ui, &self.console.term, self.font_size);
                let response = view.response;
                if response.clicked() {
                    requests.keyboard = Some(true);
                }
                for event in view.mouse {
                    self.console.term.mouse(event);
                }
                // The terminal is the only thing on this page that reads keys,
                // and the buttons below it would otherwise take the focus away
                // every time one is pressed.
                if !response.has_focus() {
                    response.request_focus();
                }
                ui.memory_mut(|mem| {
                    mem.set_focus_lock_filter(
                        response.id,
                        egui::EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: true,
                        },
                    );
                });
                let typed = console::keys_to_bytes(ui);
                if !typed.is_empty() {
                    self.source.send(&typed);
                }
            });
    }

    /// The file browser: places to start from, the folder's folders and the
    /// files the request allows, and the chosen one handed to whatever asked.
    fn picker(&mut self, ui: &mut egui::Ui, m: &mut Mobile) {
        let Some(picker) = m.picker.as_mut() else { return };
        let folder = picker.request.folder;
        let mut exts = picker.request.extensions();
        exts.sort();
        exts.dedup();
        let mut chosen = None;
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(if folder { "choose a folder" } else { "choose a file" }).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        crate::app::mobile::wrapped(ui, |ui| {
            for (name, dir) in picker_roots() {
                if ui.selectable_label(picker.dir == dir, name).clicked() {
                    let _ = std::fs::create_dir_all(&dir);
                    picker.dir = dir;
                }
            }
        });
        ui.label(RichText::new(picker.dir.display().to_string()).monospace().small().color(DIM));
        if !exts.is_empty() {
            ui.label(RichText::new(format!("showing {}", exts.join(", "))).small().color(DIM));
        }
        ui.separator();
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        let listing = std::fs::read_dir(&picker.dir);
        match listing {
            Ok(entries) => {
                for e in entries.flatten() {
                    let path = e.path();
                    let name = e.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    if path.is_dir() {
                        dirs.push((name, path));
                    } else if !folder
                        && (exts.is_empty()
                            || path.extension().is_some_and(|x| exts.contains(&x.to_string_lossy().to_ascii_lowercase())))
                    {
                        files.push((name, path));
                    }
                }
            }
            Err(e) => {
                ui.label(RichText::new(format!("cannot read this folder: {e}")).color(AMBER));
            }
        }
        dirs.sort_by_key(|(n, _)| n.to_lowercase());
        files.sort_by_key(|(n, _)| n.to_lowercase());
        if folder && ui.button("Use this folder").clicked() {
            chosen = Some(picker.dir.clone());
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if let Some(parent) = picker.dir.parent()
                && ui.add(egui::Button::new(RichText::new("..  up").monospace()).min_size(egui::vec2(ui.available_width(), 36.0))).clicked()
            {
                picker.dir = parent.to_path_buf();
            }
            for (name, path) in dirs {
                let b = egui::Button::new(RichText::new(format!("{name}/")).monospace().color(BRIGHT))
                    .min_size(egui::vec2(ui.available_width(), 36.0));
                if ui.add(b).clicked() {
                    picker.dir = path;
                }
            }
            for (name, path) in files {
                let b = egui::Button::new(RichText::new(name).monospace().color(GREEN))
                    .min_size(egui::vec2(ui.available_width(), 36.0));
                if ui.add(b).clicked() {
                    chosen = Some(path);
                }
            }
            if files_hint_needed(&picker.dir) {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Android shows other apps' PDFs and documents only to the system file \
                         picker. Copy them to \"app files/fax\" (Android/data/io.github.binmodem/\
                         files/fax, reachable over USB) to send them.",
                    )
                    .small()
                    .color(DIM),
                );
            }
        });
        if let Some(path) = chosen {
            let request = m.picker.take().map(|p| p.request);
            if let Some(request) = request {
                self.deliver(&request, &path);
            }
        } else if close {
            m.picker = None;
        }
    }

    /// The chosen file to whichever dialog asked, known by its filters.
    fn deliver(&mut self, request: &rfd::Request, path: &std::path::Path) {
        if request.folder {
            // ZMODEM's folder for received files: the only folder asked for.
            self.receive_dir = path.display().to_string();
            return;
        }
        match request.first_filter() {
            Some("recordings") => self.review.open(path),
            Some(_) => {
                // The fax document: pictures and PDFs.
                self.fax.load(path);
                self.fax.view = crate::faxwin::View::Sending;
            }
            // No filter: a file for ZMODEM to send.
            None => self.send_path = path.display().to_string(),
        }
    }

    /// The faceplate on a phone: upstream's readout -- the rate, and what the
    /// call is doing -- across the whole width. The lamps beside it on the
    /// desktop are in the strip at the top here, and the receive level is
    /// at the foot of the Line page.
    pub fn phone_faceplate(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        self.readout(ui, egui::vec2(width, 96.0));
    }

    /// Upstream's Review tab, with the recordings Record made listed above
    /// it: its Open button is a file dialog, which Android does not have.
    fn page_review(&mut self, ui: &mut egui::Ui) {
        let dir = captures_dir();
        let mut wavs: Vec<_> = std::fs::read_dir(&dir)
            .map(|d| {
                d.filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
                    .collect()
            })
            .unwrap_or_default();
        wavs.sort();
        wavs.reverse();
        egui::CollapsingHeader::new(format!("recordings ({})", wavs.len()))
            .id_salt("m-recordings")
            .default_open(true)
            .show(ui, |ui| {
                if wavs.is_empty() {
                    ui.label(RichText::new("None yet: Record on the Line page while a line is open.").small().color(DIM));
                }
                egui::ScrollArea::vertical().id_salt("m-wavs").max_height(140.0).show(ui, |ui| {
                    for path in &wavs {
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        if ui.button(RichText::new(name).monospace().size(12.0)).clicked() {
                            self.review.open(path);
                        }
                    }
                });
            });
        ui.separator();
        self.review.ui(ui);
    }

    /// Upstream's Fax tab in one column: what to send or receive, the page,
    /// and what the far machine said. The desktop puts these side by side
    /// with the call card, which the Status page has here.
    fn page_fax(&mut self, ui: &mut egui::Ui) {
        let on_hook = self.frame.state == telemetry::CallState::Idle;
        egui::ScrollArea::vertical()
            .id_salt("m-fax")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                within_screen(ui, |ui| {
                    if let Some(start) = self.fax.side_panel(ui, on_hook, &mut self.dial_number) {
                        self.fax_start = Some(start);
                    }
                    ui.add_space(8.0);
                    let page = egui::vec2(ui.available_width(), ui.available_width() * 1.3);
                    ui.allocate_ui(page, |ui| {
                        ui.set_min_size(page);
                        self.fax.page_panel(ui);
                    });
                    ui.add_space(8.0);
                    self.fax.far_panel(ui);
                    ui.add_space(24.0);
                });
            });
    }

    /// The extra keys in one row, for while the keyboard is up, with a key
    /// that puts it away.
    fn extra_keys_compact(&mut self, ui: &mut egui::Ui, requests: &mut Requests) {
        const KEYS: [(&str, &[u8]); 8] = [
            ("Esc", b"\x1b"),
            ("Tab", b"\t"),
            ("^C", b"\x03"),
            ("\u{23F4}", b"\x1b[D"),
            ("\u{23F6}", b"\x1b[A"),
            ("\u{23F7}", b"\x1b[B"),
            ("\u{23F5}", b"\x1b[C"),
            ("+++", b"+++"),
        ];
        ui.spacing_mut().button_padding.x = 2.0;
        ui.spacing_mut().item_spacing.x = 4.0;
        let gap = ui.spacing().item_spacing.x;
        let n = KEYS.len() as f32 + 1.0;
        let key_w = ((ui.available_width() - gap * (n - 1.0)) / n).floor();
        let size = egui::vec2(key_w, 36.0);
        ui.horizontal(|ui| {
            for (label, bytes) in KEYS {
                if ui.add(egui::Button::new(RichText::new(label).size(15.0)).min_size(size)).clicked() {
                    self.source.send(bytes);
                }
            }
            if ui
                .add(egui::Button::new(RichText::new("\u{2328}").size(15.0)).min_size(size))
                .on_hover_text("Hide the keyboard")
                .clicked()
            {
                requests.keyboard = Some(false);
            }
        });
    }

    /// The keys a phone keyboard does not have, and a way to raise it.
    fn extra_keys(&mut self, ui: &mut egui::Ui, requests: &mut Requests) {
        const KEYS: [(&str, &[u8]); 10] = [
            ("Esc", b"\x1b"),
            ("Tab", b"\t"),
            ("^C", b"\x03"),
            ("Left", b"\x1b[D"),
            ("Up", b"\x1b[A"),
            ("Down", b"\x1b[B"),
            ("Right", b"\x1b[C"),
            ("Del", b"\x7f"),
            ("+++", b"+++"),
            ("Enter", b"\r"),
        ];
        let width = ui.available_width();
        let per_row = 5.0;
        let gap = ui.spacing().item_spacing.x;
        let key_w = ((width - gap * (per_row - 1.0)) / per_row).floor();
        for row in KEYS.chunks(5) {
            ui.horizontal(|ui| {
                for (label, bytes) in row {
                    let button = egui::Button::new(RichText::new(*label).size(16.0))
                        .min_size(egui::vec2(key_w, 38.0));
                    if ui.add(button).clicked() {
                        self.source.send(bytes);
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            let half = ((width - gap) / 2.0).floor();
            let size = egui::vec2(half, 38.0);
            if ui.add(egui::Button::new(RichText::new("Keyboard").size(16.0)).min_size(size)).clicked() {
                requests.keyboard = Some(true);
            }
            if ui.add(egui::Button::new(RichText::new("Hide keyboard").size(16.0)).min_size(size)).clicked() {
                requests.keyboard = Some(false);
            }
        });
    }

    fn page_scopes(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("showing").monospace().color(DIM));
                    self.side_buttons(ui);
                });
                ui.add_space(4.0);
                ui.label(RichText::new("waterfall  (0 - 4000 Hz)").strong());
                self.waterfall.paint(ui, 220.0, self.frame.modulation, self.row_secs);
                ui.add_space(8.0);

                ui.label(RichText::new("spectrum").strong());
                let measured = scopes::measure(&self.frame.spectrum_db, self.frame.hz_per_bin, self.frame.display_hz);
                scopes::spectrum(
                    ui,
                    170.0,
                    scopes::Trace {
                        bins: &self.frame.spectrum_db,
                        hold: &self.hold,
                        hz_per_bin: self.frame.hz_per_bin,
                        span_hz: self.frame.display_hz,
                        floor: self.waterfall.floor_db,
                        ceiling: self.waterfall.ceiling_db,
                        modulation: self.frame.modulation,
                        measured,
                    },
                );
                ui.add_space(8.0);

                ui.label(RichText::new("symbols").strong());
                let label = self.symbol_label();
                let width = ui.available_width().min(420.0);
                let scope = scopes::symbol_scope(
                    ui,
                    &self.frame.symbols,
                    scopes::Constellation {
                        points: &self.frame.constellation,
                        tones: self.frame.tones,
                        peak: self.frame.constellation_peak,
                        pcm: (self.frame.symbol_label == "PCM").then_some(self.pcm_view),
                    },
                    &label,
                    self.frame.symbol_quality(),
                    width,
                );
                if scope.clicked() {
                    self.constellation_open = !self.constellation_open;
                }
                ui.add_space(8.0);

                ui.label(RichText::new("receive level").strong());
                scopes::level_meter(ui, self.frame.rx_level_db);
                ui.add_space(8.0);
                ui.add(
                    egui::Slider::new(&mut self.waterfall.floor_db, -140.0..=-40.0)
                        .text("floor")
                        .suffix(" dB"),
                );
                ui.add(
                    egui::Slider::new(&mut self.waterfall.ceiling_db, -60.0..=0.0)
                        .text("ceiling")
                        .suffix(" dB"),
                );
                ui.add_space(24.0);
            });
    }

    fn page_status(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(RichText::new("call").strong());
                // Upstream's call card: state, rates, and the PCM down and
                // (V.92) PCM up rate menus with Retrain beside them.
                let measured = scopes::measure(&self.frame.spectrum_db, self.frame.hz_per_bin, self.frame.display_hz);
                self.call_card(ui, measured);

                ui.add_space(12.0);
                ui.separator();
                self.distant(ui);
                ui.add_space(24.0);
            });
    }
}

/// Whether everything the phone would play out loud is silenced: the modem's
/// own tones on a sound-card line, and the monitor that plays the caller.
/// Where Record puts its files: the app's external files folder on the
/// phone (Android/data/io.github.binmodem/files/captures), which a computer
/// on USB or a file manager can reach, so a recording can be sent on; its
/// private data folder cannot be reached at all.
static CAPTURES: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn set_captures_dir(dir: std::path::PathBuf) {
    let _ = CAPTURES.set(dir);
}

pub fn captures_dir() -> std::path::PathBuf {
    CAPTURES.get().cloned().unwrap_or_else(|| std::path::PathBuf::from("captures"))
}

static MUTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Where the choice is kept between runs: a file that exists while muted, in
/// the app's private directory (the working directory on Android).
const MUTE_FILE: &str = "muted";

pub fn muted() -> bool {
    MUTED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_muted(on: bool) {
    MUTED.store(on, std::sync::atomic::Ordering::Relaxed);
    let _ = if on {
        std::fs::write(MUTE_FILE, b"")
    } else {
        std::fs::remove_file(MUTE_FILE)
    };
}

/// Put back what the last run chose.
pub fn load_muted() {
    use std::sync::atomic::Ordering::Relaxed;
    use std::path::Path;
    MUTED.store(Path::new(MUTE_FILE).exists(), Relaxed);
    HEAR_CALLER.store(!Path::new(CALLER_OFF_FILE).exists(), Relaxed);
    HEAR_ANSWERER.store(!Path::new(ANSWERER_OFF_FILE).exists(), Relaxed);
}

/// How loud our own modem plays in the monitor, as a fraction of its full
/// output: about -16 dB, a little above where a far end usually arrives.
const OWN_SIDE: f32 = 0.16;

/// Which sides of the call are on: heard (the monitor) and drawn (the
/// spectrum and waterfall). Both start on.
static HEAR_CALLER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static HEAR_ANSWERER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Files that exist while a side is off, beside the Sound one.
const CALLER_OFF_FILE: &str = "caller-off";
const ANSWERER_OFF_FILE: &str = "answerer-off";

fn side(caller: bool) -> (&'static std::sync::atomic::AtomicBool, &'static str) {
    if caller {
        (&HEAR_CALLER, CALLER_OFF_FILE)
    } else {
        (&HEAR_ANSWERER, ANSWERER_OFF_FILE)
    }
}

pub fn side_on(caller: bool) -> bool {
    side(caller).0.load(std::sync::atomic::Ordering::Relaxed)
}

fn set_side(caller: bool, on: bool) {
    let (flag, file) = side(caller);
    flag.store(on, std::sync::atomic::Ordering::Relaxed);
    let _ = if on { std::fs::remove_file(file) } else { std::fs::write(file, b"") };
}

/// Whether our own modem's side, and the far end's, are on -- which of the
/// two is the caller depends on which end dialled.
fn own_and_far(answering: bool) -> (bool, bool) {
    let caller = side_on(true);
    let answerer = side_on(false);
    if answering { (answerer, caller) } else { (caller, answerer) }
}

/// What the spectrum and waterfall are given for one sample: the sides that
/// are on, at the level each is on the line. build.rs puts this in place of
/// live.rs's `spectrum.push(heard)`, which drew only the far end.
pub fn seen(heard: f64, sent: f64, answering: bool) -> f64 {
    let (own, far) = own_and_far(answering);
    (if far { heard } else { 0.0 }) + (if own { sent } else { 0.0 })
}

/// What the monitor plays: the sides of the call that are on.
///
/// build.rs routes live.rs's monitor through here. Upstream plays only what
/// arrived. Over SIP our own modem's tones go to the network and nothing on
/// the phone plays them, so they are mixed in here when our side is on. On a
/// sound-card line they are already coming out of the phone, as the line, so
/// they are not played twice.
///
/// What was sent has the drive in it (-20 dB by default, a tenth), which is
/// the level for the line and nothing to do with how loud it should be here,
/// so it is taken back out and our side played at [`OWN_SIDE`]. The block
/// keeps the length of what arrived, so the monitor keeps the line's timing.
pub fn monitor(
    sink: &line::AudioSink,
    heard: &[f32],
    sent: &[f32],
    drive: f32,
    sip: bool,
    answering: bool,
) {
    if muted() {
        return;
    }
    let (own, far) = own_and_far(answering);
    let own = own && sip && !sent.is_empty();
    let gain = OWN_SIDE / drive.abs().max(0.001);
    let mixed: Vec<f32> = heard
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let theirs = if far { *h } else { 0.0 };
            let ours = if own { gain * sent.get(i).copied().unwrap_or(0.0) } else { 0.0 };
            (theirs + ours).clamp(-1.0, 1.0)
        })
        .collect();
    sink.push(&mixed);
}

/// The modem's transmit to a sound card, silenced while muted.
///
/// build.rs routes live.rs's sound-card transmit through here. Silence is
/// sent rather than nothing, so the output keeps its timing and the modem
/// carries on exactly as it would; the speaker just has nothing to play.
/// A SIP line does not come through here: what it sends goes to the network,
/// not out of the phone.
pub fn to_speaker(line: &line::Duplex, samples: &[f32]) {
    if muted() {
        line.transmit(&vec![0.0; samples.len()]);
    } else {
        line.transmit(samples);
    }
}

/// The Sound switch itself; whether it was pressed.
fn sound_label(ui: &mut egui::Ui) -> bool {
    ui.selectable_label(!muted(), "Sound")
        .on_hover_text("Hear the call -- the caller and the modem -- or nothing at all")
        .clicked()
}

/// Four tabs across the bottom, each a quarter of the width: an icon over
/// its name, the chosen one lit.
fn tab_bar(ui: &mut egui::Ui, page: &mut Page) {
    const TABS: [(Page, &str, &str); 4] = [
        (Page::Line, "\u{260E}", "Line"),
        (Page::Terminal, "\u{2328}", "Terminal"),
        (Page::Scopes, "\u{1F4C8}", "Scopes"),
        (Page::Status, "\u{2139}", "Status"),
    ];
    let gap = ui.spacing().item_spacing.x;
    let w = ((ui.available_width() - gap * 3.0) / 4.0).floor();
    ui.horizontal(|ui| {
        for (p, icon, label) in TABS {
            let selected = *page == p;
            let (rect, response) = ui.allocate_exact_size(egui::vec2(w, 52.0), egui::Sense::click());
            let colour = if selected { ACCENT } else if response.hovered() { BRIGHT } else { DIM };
            let painter = ui.painter();
            if selected {
                painter.rect_filled(rect.shrink2(egui::vec2(6.0, 2.0)), 10.0, ACCENT.gamma_multiply(0.18));
            }
            painter.text(
                rect.center_top() + egui::vec2(0.0, 6.0),
                egui::Align2::CENTER_TOP,
                icon,
                egui::FontId::proportional(20.0),
                colour,
            );
            painter.text(
                rect.center_bottom() - egui::vec2(0.0, 5.0),
                egui::Align2::CENTER_BOTTOM,
                label,
                egui::FontId::proportional(13.0),
                colour,
            );
            if response.clicked() {
                *page = p;
            }
        }
    });
}

/// `egui::ComboBox`, for a row that wraps.
///
/// build.rs puts this in place of every `egui::ComboBox` in the copy of
/// app.rs. A combo box cannot tell a wrapping row how wide it is going to be
/// before it is drawn, so the row never moves it down a line: it runs off the
/// right edge, and the row is wider than the screen from then on. This one
/// starts a new row itself when it will not fit on this one, and is never made
/// wider than the screen.
pub struct Combo {
    inner: egui::ComboBox,
    width: Option<f32>,
}

impl Combo {
    pub fn from_id_salt(id_salt: impl egui::AsIdSalt) -> Self {
        Self { inner: egui::ComboBox::from_id_salt(id_salt), width: None }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.inner = self.inner.height(height);
        self
    }

    pub fn selected_text(mut self, text: impl Into<egui::WidgetText>) -> Self {
        self.inner = self.inner.selected_text(text);
        self
    }

    pub fn close_behavior(mut self, behavior: egui::PopupCloseBehavior) -> Self {
        self.inner = self.inner.close_behavior(behavior);
        self
    }

    pub fn show_ui<R>(
        self,
        ui: &mut egui::Ui,
        contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> egui::InnerResponse<Option<R>> {
        let spacing = ui.spacing();
        let wanted = self.width.unwrap_or(spacing.combo_width);
        // The box, its arrow, and the padding either side.
        let overhead = spacing.icon_width + spacing.button_padding.x * 2.0 + spacing.item_spacing.x;
        if ui.layout().main_wrap && ui.available_size_before_wrap().x < wanted + overhead {
            ui.end_row();
        }
        let room = (ui.available_size_before_wrap().x - overhead).max(40.0);
        self.inner
            .width(wanted.min(room))
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_ui(ui, contents)
    }
}

/// `egui::Slider`, for a row that wraps; see [`Combo`] for why. Only the
/// builder methods app.rs uses are here.
pub struct Slider<'a> {
    inner: egui::Slider<'a>,
    text: String,
    suffix: String,
}

impl<'a> Slider<'a> {
    pub fn new(value: &'a mut f32, range: std::ops::RangeInclusive<f32>) -> Self {
        Self { inner: egui::Slider::new(value, range), text: String::new(), suffix: String::new() }
    }

    pub fn text(mut self, text: &str) -> Self {
        self.text = text.to_owned();
        self.inner = self.inner.text(text);
        self
    }

    pub fn suffix(mut self, suffix: &str) -> Self {
        self.suffix = suffix.to_owned();
        self.inner = self.inner.suffix(suffix);
        self
    }

    pub fn show_value(mut self, on: bool) -> Self {
        self.inner = self.inner.show_value(on);
        self
    }

    pub fn logarithmic(mut self, on: bool) -> Self {
        self.inner = self.inner.logarithmic(on);
        self
    }
}

impl egui::Widget for Slider<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let measure = |ui: &egui::Ui, s: &str| {
            let font = egui::TextStyle::Body.resolve(ui.style());
            ui.fonts_mut(|f| f.layout_no_wrap(s.to_owned(), font, Color32::WHITE).size().x)
        };
        let spacing = ui.spacing();
        let needed = spacing.slider_width
            + spacing.interact_size.x
            + measure(ui, &format!("-000.0{}", self.suffix))
            + measure(ui, &self.text)
            + spacing.item_spacing.x * 3.0;
        if ui.layout().main_wrap && ui.available_size_before_wrap().x < needed {
            ui.end_row();
        }
        ui.add(self.inner)
    }
}

/// `egui::Window`, as a sheet over the whole screen.
///
/// A floating window the size of a desktop one does not fit on a phone, and
/// dragging one about with a finger is no way to use it. This keeps the
/// title and the close button and fills the screen with the rest, scrolling
/// if it has to. Only the builder methods the GUI uses are here; sizes are
/// taken and ignored, since the screen decides.
pub struct Window<'open> {
    title: egui::WidgetText,
    open: Option<&'open mut bool>,
}

impl<'open> Window<'open> {
    pub fn new(title: impl Into<egui::WidgetText>) -> Self {
        Self { title: title.into(), open: None }
    }

    pub fn open(mut self, open: &'open mut bool) -> Self {
        self.open = Some(open);
        self
    }

    /// The sheet is the whole screen whatever it is called, so a title that
    /// changes needs no id of its own to stay the same sheet.
    pub fn id(self, _id: egui::Id) -> Self {
        self
    }

    pub fn resizable(self, _resizable: bool) -> Self {
        self
    }

    pub fn default_width(self, _width: f32) -> Self {
        self
    }

    pub fn default_size(self, _size: impl Into<egui::Vec2>) -> Self {
        self
    }

    pub fn show<R>(
        self,
        ctx: &egui::Context,
        contents: impl FnOnce(&mut egui::Ui) -> R,
    ) -> Option<egui::InnerResponse<Option<R>>> {
        let screen = ctx.content_rect().shrink(4.0);
        // Room for the window's own frame and title bar.
        let size = egui::vec2(screen.width() - 16.0, screen.height() - 56.0);
        let mut window = egui::Window::new(self.title)
            .collapsible(false)
            .resizable(false)
            .movable(false)
            .fixed_pos(screen.min)
            .fixed_size(size)
            // Sideways too, for the few grids that are wider than a phone.
            .scroll([true, true]);
        if let Some(open) = self.open {
            window = window.open(open);
        }
        window.show(ctx, |ui| {
            ui.set_max_width(size.x);
            within_screen(ui, contents)
        })
    }
}

impl ScopeApp {
    /// Open one of the windows, for the preview: `BINMODEM_OPEN=dial` and so on.
    pub fn open_window(&mut self, name: &str) {
        match name {
            "dial" => self.dial_open = true,
            "network" => self.network_open = true,
            "advanced" => self.advanced = true,
            "files" => self.transfer_open = true,
            "fax" => self.tab = Tab::Fax,
            "error" => self.protection_open = true,
            // Not windows: the long V.90 name in the call box, and SIP chosen.
            "v90" => self.carrier = 5,
            "sip" => self.chosen_line = 1,
            "long" => LONG_ROW.store(true, std::sync::atomic::Ordering::Relaxed),
            _ => {}
        }
    }
}

/// Preview only: draw a registered SIP line's long status row.
static LONG_ROW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

thread_local! {
    /// The right-hand edge rows have to stay inside, while one is being laid
    /// out by [`within_screen`].
    static RIGHT_EDGE: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

/// Lay out `add` with every wrapping row held to the width `ui` has now.
pub fn within_screen<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let before = RIGHT_EDGE.replace(Some(ui.max_rect().right()));
    let result = add(ui);
    RIGHT_EDGE.set(before);
    result
}

/// `Ui::horizontal_wrapped`, held to the screen; build.rs puts it in place of
/// every one in the copied sources.
///
/// In egui a row that something overflows -- a SIP address, a long
/// modulation name -- makes its parent wider, and every row after it then
/// wraps against that wider edge and runs off the screen too. Here the row is
/// laid out against the real edge, and only as much of it as fits is
/// reported back to the parent, so one long line cannot widen the rest.
pub fn wrapped<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    let Some(right) = RIGHT_EDGE.get() else {
        return ui.horizontal_wrapped(add);
    };
    let top_left = ui.cursor().min;
    let width = (right - top_left.x).max(60.0);
    // One row's height, as egui's own horizontal_wrapped starts with: rows
    // are centred across it, and further rows grow the child downwards.
    let room = egui::Rect::from_min_size(top_left, egui::vec2(width, ui.spacing().interact_size.y));
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(room)
            .layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true)),
    );
    // One line per widget, shortened with "..." when it is wider than the
    // screen: a SIP address has nowhere to break, and wrapped it was drawn
    // from left of the screen's edge, taking the page with it.
    row.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    let inner = add(&mut row);
    let used = row.min_rect();
    let kept = egui::Rect::from_min_max(
        egui::pos2(used.min.x.max(top_left.x), used.min.y),
        egui::pos2(used.max.x.min(right), used.max.y),
    );
    let response = ui.allocate_rect(kept, egui::Sense::hover());
    egui::InnerResponse::new(inner, response)
}

/// Where the file browser starts: the app's own folder (which needs no
/// permission and can be reached over USB), then shared storage.
fn picker_roots() -> Vec<(&'static str, std::path::PathBuf)> {
    let mut roots = Vec::new();
    let captures = captures_dir();
    if let Some(app) = captures.parent().filter(|p| !p.as_os_str().is_empty()) {
        roots.push(("app files/fax", app.join("fax")));
        roots.push(("app files", app.to_path_buf()));
    } else {
        roots.push(("app files/fax", std::path::PathBuf::from("fax")));
    }
    let shared = std::path::PathBuf::from("/storage/emulated/0");
    if shared.is_dir() {
        roots.push(("Download", shared.join("Download")));
        roots.push(("Pictures", shared.join("Pictures")));
        roots.push(("DCIM", shared.join("DCIM")));
        roots.push(("phone", shared));
    } else if let Some(home) = std::env::var_os("HOME") {
        roots.push(("home", home.into()));
    }
    roots
}

/// Whether the browser is in shared storage, where Android hides other apps'
/// non-media files.
fn files_hint_needed(dir: &std::path::Path) -> bool {
    dir.starts_with("/storage/emulated/0") && !dir.starts_with("/storage/emulated/0/Android")
}

/// MIME types for the system picker, from the extensions a dialog allows.
fn mime_types(extensions: &[String]) -> Vec<String> {
    let mut types: Vec<String> = Vec::new();
    for e in extensions {
        let t: &[&str] = match e.as_str() {
            "png" => &["image/png"],
            "jpg" | "jpeg" => &["image/jpeg"],
            "bmp" => &["image/bmp", "image/x-ms-bmp"],
            "gif" => &["image/gif"],
            "pdf" => &["application/pdf"],
            "wav" => &["audio/wav", "audio/x-wav", "audio/wave"],
            _ => &[],
        };
        for t in t {
            if !types.iter().any(|x| x == t) {
                types.push((*t).to_owned());
            }
        }
    }
    types
}
