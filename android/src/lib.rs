//! BinModem on Android.
//!
//! The desktop program's modem, line and widgets are reused as they are:
//! every module of `BinModem/crates/gui` is compiled in here (build.rs copies
//! them, adding the phone layout inside `app`), and only the entry point and
//! the layout are new. What they add is what a phone needs and a desktop does
//! not:
//!
//! * a writable home for the settings and SIP account files, which the
//!   upstream code finds through `HOME`/`XDG_CONFIG_HOME` -- neither is set for
//!   an Android app, so both are pointed at the app's private storage;
//! * the microphone permission, which Android asks for at run time;
//! * a layout for a phone held upright (src/mobile.rs), with the keys a soft
//!   keyboard lacks and the keyboard raised when the terminal is tapped.

#![allow(dead_code, clippy::all)]

// Every module of the desktop GUI, copied by build.rs with the phone layout
// (src/mobile.rs) added inside `app`.
include!(concat!(env!("OUT_DIR"), "/gui/mods.rs"));

use std::sync::Arc;

use eframe::egui;
#[cfg(target_os = "android")]
use winit::platform::android::activity::AndroidApp;
use engine::{Control, SCOPE_LEN, SPECTRUM_BINS};
use line::AudioSink;

/// The rate a live modem runs at, as in the desktop `main.rs`.
const LIVE_FS: f64 = 16_000.0;

/// The narrowest a phone layout is drawn: a screen with fewer points across
/// than this is zoomed out until it has them.
const MIN_W: f32 = 380.0;

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(android_app: AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("BinModem"),
    );
    log_panics();

    if let Some(home) = android_app.internal_data_path() {
        let _ = std::fs::create_dir_all(&home);
        // SAFETY: nothing else is running yet; this is the first thing the
        // process does on the thread that owns it.
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::set_var("XDG_CONFIG_HOME", &home);
            std::env::set_var("TMPDIR", &home);
        }
        let _ = std::env::set_current_dir(&home);
    }
    app::mobile::load_muted();

    if let Err(e) = request_microphone() {
        log::warn!("could not ask for the microphone: {e}");
    }

    if let Err(e) = run(android_app) {
        log::error!("BinModem stopped: {e}");
    }
}

#[cfg(target_os = "android")]
fn run(android_app: AndroidApp) -> Result<(), Box<dyn std::error::Error>> {
    let control = Arc::new(Control::default());
    let sample_rate = LIVE_FS;
    let (tx, rx) = telemetry::channel(SCOPE_LEN, SPECTRUM_BINS, sample_rate);
    let sink = Arc::new(AudioSink::new((sample_rate * 0.25) as usize));

    // A modem, with the line chosen in the window -- what the desktop program
    // opens with when nothing is named on its command line.
    let session = Arc::new(live::Session::default());
    let engine = live::spawn(tx, control.clone(), session.clone(), sink.clone());
    let source = app::Source::Live(session);

    let options = eframe::NativeOptions {
        android_app: Some(android_app.clone()),
        ..Default::default()
    };

    let ui_control = control.clone();
    eframe::run_native(
        "BinModem",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            touch_style(&cc.egui_ctx);
            let inner = app::ScopeApp::new(rx, ui_control, sink, sample_rate, source);
            Ok(Box::new(Phone {
                inner,
                mobile: app::mobile::Mobile::default(),
                android_app,
                keyboard: false,
                fitted_for: 0.0,
                last_page: app::mobile::Page::default(),
                pending_paste: None,
                text_focus: None,
            }))
        }),
    )?;

    control.quit.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = engine.join();
    Ok(())
}

/// Bigger targets and more room between them, for a finger.
fn touch_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.interact_size = egui::vec2(44.0, 32.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.slider_width = 160.0;
        style.spacing.combo_height = 400.0;
        style.spacing.scroll.bar_width = 8.0;
        style.interaction.tooltip_delay = f32::INFINITY;
    });
}

/// The window laid out for a phone, and the platform around it.
#[cfg(target_os = "android")]
struct Phone {
    inner: app::ScopeApp,
    mobile: app::mobile::Mobile,
    android_app: AndroidApp,
    /// Whether the soft keyboard was last asked up.
    keyboard: bool,
    /// The physical width the zoom was last chosen for.
    fitted_for: f32,
    last_page: app::mobile::Page,
    /// Clipboard text to hand egui as a paste at the start of the next frame.
    pending_paste: Option<String>,
    /// The widget that last had the keyboard, and when it lost it: the Paste
    /// button stays up a moment after, because pressing it takes the focus
    /// away from the box it is meant to paste into.
    text_focus: Option<(egui::Id, Option<f64>)>,
}

#[cfg(target_os = "android")]
impl eframe::App for Phone {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(text) = self.pending_paste.take() {
            raw_input.events.push(egui::Event::Paste(text));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.fit(&ctx);
        self.track_focus(&ctx);
        let requests = self.inner.mobile_ui(ui, &mut self.mobile);

        if requests.paste {
            self.paste(&ctx, None);
        }
        if self.mobile.page != self.last_page {
            self.text_focus = None;
        }
        if self.mobile.page != app::mobile::Page::Terminal {
            self.paste_button(&ctx);
        }

        // The terminal asks for the keyboard itself; anywhere else it follows
        // whether a text box has the focus. Leaving the terminal puts it away.
        let page = self.mobile.page;
        let wanted = if page == app::mobile::Page::Terminal {
            requests.keyboard
        } else if self.last_page == app::mobile::Page::Terminal {
            Some(false)
        } else {
            requests.keyboard.or(Some(ctx.egui_wants_keyboard_input()))
        };
        self.last_page = page;
        if let Some(want) = wanted {
            self.set_keyboard(want);
        }
    }
}

#[cfg(target_os = "android")]
impl Phone {
    /// The screen's own density, unless that leaves fewer than [`MIN_W`]
    /// points across, in which case as many as it takes to have them.
    fn fit(&mut self, ctx: &egui::Context) {
        // Not the viewport's inner_rect: winit cannot give a window position
        // on Android, so egui-winit leaves that None there. The screen rect
        // comes from the surface size and is always known.
        let native = ctx.input(|i| i.viewport().native_pixels_per_point);
        let physical = ctx.viewport_rect().width() * ctx.pixels_per_point();
        if physical <= 0.0 || (physical - self.fitted_for).abs() < 1.0 {
            return;
        }
        self.fitted_for = physical;
        let native = native.unwrap_or(2.0);
        let zoom = native.min(physical / MIN_W).max(0.75);
        ctx.set_pixels_per_point(zoom);
    }

    /// Remember which widget has the keyboard, and when it let go of it.
    fn track_focus(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        match ctx.memory(|m| m.focused()) {
            Some(id) => self.text_focus = Some((id, None)),
            None => {
                if let Some((_, lost @ None)) = &mut self.text_focus {
                    *lost = Some(now);
                }
                if let Some((_, Some(at))) = self.text_focus
                    && now - at > 3.0
                {
                    self.text_focus = None;
                }
            }
        }
    }

    /// A Paste button over the bottom right while a text box is in use.
    fn paste_button(&mut self, ctx: &egui::Context) {
        let Some((id, _)) = self.text_focus else { return };
        egui::Area::new(egui::Id::new("android-paste"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -72.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let button = egui::Button::new(egui::RichText::new("Paste").strong().size(17.0))
                    .min_size(egui::vec2(96.0, 42.0));
                if ui.add(button).clicked() {
                    self.paste(ctx, Some(id));
                }
            });
    }

    /// Read the system clipboard and paste it into `into` (or whatever has
    /// the focus) on the next frame.
    fn paste(&mut self, ctx: &egui::Context, into: Option<egui::Id>) {
        match read_clipboard() {
            Ok(Some(text)) if !text.is_empty() => {
                if let Some(id) = into {
                    ctx.memory_mut(|m| m.request_focus(id));
                }
                self.pending_paste = Some(text);
                ctx.request_repaint();
            }
            Ok(_) => log::info!("nothing to paste"),
            Err(e) => log::warn!("could not read the clipboard: {e}"),
        }
    }

    fn set_keyboard(&mut self, want: bool) {
        if want == self.keyboard {
            return;
        }
        self.keyboard = want;
        if want {
            self.android_app.show_soft_input(true);
        } else {
            self.android_app.hide_soft_input(false);
        }
    }
}

/// Send panics to logcat as well as wherever the upstream hook puts them.
#[cfg(target_os = "android")]
fn log_panics() {
    crashlog::install();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{info}");
        previous(info);
    }));
}

/// The system clipboard's text, through `ClipboardManager`: egui-winit has
/// no clipboard on Android, so without this nothing copied in another app can
/// be pasted here.
#[cfg(target_os = "android")]
fn read_clipboard() -> Result<Option<String>, Box<dyn std::error::Error>> {
    use jni::objects::{JObject, JString, JValue};
    let ctx = ndk_context::android_context();
    // SAFETY: both pointers come from the running activity via ndk-context.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast())? };
    let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
    let mut env = vm.attach_current_thread()?;

    let result = (|| -> jni::errors::Result<Option<String>> {
        let name = env.new_string("clipboard")?;
        let manager = env
            .call_method(
                &activity,
                "getSystemService",
                "(Ljava/lang/String;)Ljava/lang/Object;",
                &[JValue::Object(&name)],
            )?
            .l()?;
        if manager.is_null() {
            return Ok(None);
        }
        let clip = env
            .call_method(&manager, "getPrimaryClip", "()Landroid/content/ClipData;", &[])?
            .l()?;
        if clip.is_null() {
            return Ok(None);
        }
        let count = env.call_method(&clip, "getItemCount", "()I", &[])?.i()?;
        if count < 1 {
            return Ok(None);
        }
        let item = env
            .call_method(&clip, "getItemAt", "(I)Landroid/content/ClipData$Item;", &[JValue::Int(0)])?
            .l()?;
        let text = env
            .call_method(
                &item,
                "coerceToText",
                "(Landroid/content/Context;)Ljava/lang/CharSequence;",
                &[JValue::Object(&activity)],
            )?
            .l()?;
        if text.is_null() {
            return Ok(None);
        }
        let string = env.call_method(&text, "toString", "()Ljava/lang/String;", &[])?.l()?;
        let string: String = env.get_string(&JString::from(string))?.into();
        Ok(Some(string))
    })();
    if result.is_err() {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    Ok(result?)
}

/// Ask for RECORD_AUDIO, which a sound-card modem cannot hear without.
///
/// `Activity.requestPermissions` through JNI; the answer comes back to the
/// activity, and the line simply opens once it has been given.
#[cfg(target_os = "android")]
fn request_microphone() -> Result<(), Box<dyn std::error::Error>> {
    const PERMISSION: &str = "android.permission.RECORD_AUDIO";
    let ctx = ndk_context::android_context();
    // SAFETY: both pointers come from the running activity via ndk-context.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast())? };
    let activity = unsafe { jni::objects::JObject::from_raw(ctx.context().cast()) };
    let mut env = vm.attach_current_thread()?;

    let name = env.new_string(PERMISSION)?;
    let granted = env
        .call_method(
            &activity,
            "checkSelfPermission",
            "(Ljava/lang/String;)I",
            &[(&name).into()],
        )?
        .i()?;
    if granted == 0 {
        return Ok(());
    }
    let array = env.new_object_array(1, "java/lang/String", &name)?;
    env.call_method(
        &activity,
        "requestPermissions",
        "([Ljava/lang/String;I)V",
        &[(&array).into(), jni::objects::JValue::Int(1)],
    )
    .map(|_| ())
    .or_else(|_| {
        // Missing (API < 23) means the permission was granted at install.
        let _ = env.exception_clear();
        Ok::<(), jni::errors::Error>(())
    })?;
    Ok(())
}

/// The phone layout in a desktop window the size of a phone, for working on
/// it without one. `BINMODEM_PAGE` picks the tab it opens on.
#[cfg(not(target_os = "android"))]
pub fn preview() -> Result<(), Box<dyn std::error::Error>> {
    let control = Arc::new(Control::default());
    let (tx, rx) = telemetry::channel(SCOPE_LEN, SPECTRUM_BINS, LIVE_FS);
    let sink = Arc::new(AudioSink::new((LIVE_FS * 0.25) as usize));
    let session = Arc::new(live::Session::default());
    let engine = live::spawn(tx, control.clone(), session.clone(), sink.clone());
    let source = app::Source::Live(session);

    app::mobile::load_muted();
    let ppp: f32 = std::env::var("BINMODEM_PPP").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let mut mobile = app::mobile::Mobile::default();
    mobile.page = match std::env::var("BINMODEM_PAGE").as_deref() {
        Ok("terminal") => app::mobile::Page::Terminal,
        Ok("scopes") => app::mobile::Page::Scopes,
        Ok("status") => app::mobile::Page::Status,
        _ => app::mobile::Page::Line,
    };

    struct Preview(app::ScopeApp, app::mobile::Mobile);
    impl eframe::App for Preview {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let _ = self.0.mobile_ui(ui, &mut self.1);
        }
    }

    let ui_control = control.clone();
    eframe::run_native(
        "BinModem phone preview",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([412.0 * ppp, 915.0 * ppp])
                .with_decorations(false),
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            cc.egui_ctx.set_pixels_per_point(ppp);
            touch_style(&cc.egui_ctx);
            let mut inner = app::ScopeApp::new(rx, ui_control, sink, LIVE_FS, source);
            if let Ok(name) = std::env::var("BINMODEM_OPEN") {
                inner.open_window(&name);
            }
            Ok(Box::new(Preview(inner, mobile)))
        }),
    )?;
    control.quit.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = engine.join();
    Ok(())
}
