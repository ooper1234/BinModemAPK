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
}

impl Default for Mobile {
    fn default() -> Self {
        Self { page: Page::Line, fit_font: true }
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
    fn status_strip(&self, ui: &mut egui::Ui) {
        let f = &self.frame;
        let colour = match f.state {
            telemetry::CallState::Connected => GREEN,
            telemetry::CallState::Idle => DIM,
            _ => AMBER,
        };
        ui.horizontal_wrapped(|ui| {
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
        ui.add_space(2.0);
        scopes::faceplate(ui, &self.frame.leds);
    }

    /// The line, the call and every setting: the desktop's top bar, which
    /// wraps itself to whatever width it is given.
    fn page_line(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // The desktop rows are wrapping rows, but egui's default is to
                // wrap a button's *text* into what is left of the row rather
                // than move the button down. Keep every widget whole, so the
                // row breaks between them instead.
                let width = ui.available_width();
                ui.set_max_width(width);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.spacing_mut().interact_size.y = 34.0;
                self.controls(ui);
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
            if ui
                .add(egui::Button::new(RichText::new("Keyboard").size(16.0)).min_size(egui::vec2(half, 38.0)))
                .clicked()
            {
                requests.keyboard = Some(true);
            }
            if ui
                .add(egui::Button::new(RichText::new("Hide keyboard").size(16.0)).min_size(egui::vec2(half, 38.0)))
                .clicked()
            {
                requests.keyboard = Some(false);
            }
        });
    }

    fn page_scopes(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
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
            contents(ui)
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
            _ => {}
        }
    }
}
