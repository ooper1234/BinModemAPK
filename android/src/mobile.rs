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
}

impl Default for Mobile {
    fn default() -> Self {
        Self { page: Page::Line, fit_font: true, sound_started: false }
    }
}

/// What the layout wants of the platform this frame.
#[derive(Debug, Default)]
pub struct Requests {
    /// Raise (`Some(true)`) or drop (`Some(false)`) the soft keyboard.
    pub keyboard: Option<bool>,
}

const DIM: Color32 = Color32::from_rgb(140, 150, 165);
const BRIGHT: Color32 = Color32::from_rgb(220, 225, 235);
const GREEN: Color32 = Color32::from_rgb(90, 220, 130);
const AMBER: Color32 = Color32::from_rgb(245, 180, 70);

/// Height of a touch target, in points.
const TOUCH_H: f32 = 44.0;

impl ScopeApp {
    pub fn mobile_ui(&mut self, ui: &mut egui::Ui, m: &mut Mobile) -> Requests {
        let mut requests = Requests::default();

        self.poll();
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

        egui::Panel::top("m-status").show(ui, |ui| {
            ui.add_space(4.0);
            self.status_strip(ui);
            ui.add_space(4.0);
        });

        egui::Panel::bottom("m-tabs").show(ui, |ui| {
            ui.add_space(4.0);
            tab_bar(ui, &mut m.page);
            ui.add_space(4.0);
        });

        if m.page == Page::Terminal {
            egui::Panel::bottom("m-keys").show(ui, |ui| {
                ui.add_space(4.0);
                self.extra_keys(ui, &mut requests);
                ui.add_space(4.0);
            });
        }

        // The desktop's own pop-ups: the large constellation. The dialler,
        // transfers and network windows are drawn by the line controls.
        self.constellation_window(ui);

        egui::CentralPanel::default().show(ui, |ui| match m.page {
            Page::Line => self.page_line(ui),
            Page::Terminal => self.page_terminal(ui, m, &mut requests),
            Page::Scopes => self.page_scopes(ui),
            Page::Status => self.page_status(ui),
        });

        requests
    }

    /// One line of what the call is doing, always in view.
    fn status_strip(&mut self, ui: &mut egui::Ui) {
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
        scopes::faceplate(ui, &self.frame.leds);
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

        if self.tab == Tab::Transcript {
            self.transcript(ui);
            return;
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
                self.waterfall.paint(ui, 220.0, self.frame.modulation);
                ui.add_space(8.0);

                ui.label(RichText::new("spectrum").strong());
                scopes::spectrum(
                    ui,
                    &self.frame.spectrum_db,
                    self.frame.hz_per_bin,
                    170.0,
                    self.waterfall.floor_db,
                    self.waterfall.ceiling_db,
                    self.frame.modulation,
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
                        pairs: self.frame.symbol_label == "PCM",
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
                if let Some(drn) = self.status(ui)
                    && let Source::Live(session) = &self.source
                {
                    session.choose_rate(drn);
                }

                if let Source::Live(session) = &self.source {
                    let connected = self.frame.state == telemetry::CallState::Connected;
                    let v34 = self.frame.modulation == "V.34";
                    ui.add_space(6.0);
                    if ui
                        .add_enabled(
                            connected && v34,
                            egui::Button::new("Retrain (V.34)").min_size(egui::vec2(0.0, 36.0)),
                        )
                        .clicked()
                    {
                        session.retrain();
                    }
                }

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

/// Four tabs across the bottom, each a quarter of the width.
fn tab_bar(ui: &mut egui::Ui, page: &mut Page) {
    const TABS: [(Page, &str); 4] = [
        (Page::Line, "Line"),
        (Page::Terminal, "Terminal"),
        (Page::Scopes, "Scopes"),
        (Page::Status, "Status"),
    ];
    let gap = ui.spacing().item_spacing.x;
    let w = ((ui.available_width() - gap * 3.0) / 4.0).floor();
    ui.horizontal(|ui| {
        for (p, label) in TABS {
            let selected = *page == p;
            let text = RichText::new(label).size(17.0);
            let text = if selected { text.strong() } else { text };
            let button = egui::Button::selectable(selected, text).min_size(egui::vec2(w, TOUCH_H));
            if ui.add(button).clicked() {
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
            "fax" => self.fax.open = true,
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
