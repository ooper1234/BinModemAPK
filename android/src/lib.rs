//! BinModem on Android.
//!
//! The desktop program's window is reused as it is: every module of
//! `BinModem/crates/gui` is compiled in here unchanged, by path, and only the
//! entry point is new. What the entry point adds is what a phone needs and a
//! desktop does not:
//!
//! * a writable home for the settings and SIP account files, which the
//!   upstream code finds through `HOME`/`XDG_CONFIG_HOME` -- neither is set for
//!   an Android app, so both are pointed at the app's private storage;
//! * the microphone permission, which Android asks for at run time;
//! * a button to raise the on-screen keyboard, since the terminal reads keys
//!   and a phone has none until asked;
//! * a zoom that fits the desktop layout (900 points, its desktop minimum width) onto the
//!   screen.

#![allow(dead_code, clippy::all)]

#[path = "../../BinModem/crates/gui/src/answer.rs"]
mod answer;
#[path = "../../BinModem/crates/gui/src/app.rs"]
mod app;
#[path = "../../BinModem/crates/gui/src/console.rs"]
mod console;
#[path = "../../BinModem/crates/gui/src/crashlog.rs"]
mod crashlog;
#[path = "../../BinModem/crates/gui/src/dialin.rs"]
mod dialin;
#[path = "../../BinModem/crates/gui/src/engine.rs"]
mod engine;
#[path = "../../BinModem/crates/gui/src/faxwin.rs"]
mod faxwin;
#[path = "../../BinModem/crates/gui/src/live.rs"]
mod live;
#[path = "../../BinModem/crates/gui/src/net.rs"]
mod net;
#[path = "../../BinModem/crates/gui/src/network.rs"]
mod network;
#[path = "../../BinModem/crates/gui/src/remembered.rs"]
mod remembered;
#[path = "../../BinModem/crates/gui/src/scopes.rs"]
mod scopes;
#[path = "../../BinModem/crates/gui/src/speed.rs"]
mod speed;

use std::sync::Arc;

use eframe::egui;
use winit::platform::android::activity::AndroidApp;
use engine::{Control, SCOPE_LEN, SPECTRUM_BINS};
use line::AudioSink;

/// The rate a live modem runs at, as in the desktop `main.rs`.
const LIVE_FS: f64 = 16_000.0;

/// The narrowest the desktop layout goes (its minimum window width), which
/// is what a phone held upright has to fit.
const LAYOUT_W: f32 = 900.0;

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

    if let Err(e) = request_microphone() {
        log::warn!("could not ask for the microphone: {e}");
    }

    if let Err(e) = run(android_app) {
        log::error!("BinModem stopped: {e}");
    }
}

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
            let inner = app::ScopeApp::new(rx, ui_control, sink, sample_rate, source);
            Ok(Box::new(Phone {
                inner,
                android_app,
                keyboard: false,
                fitted_for: 0.0,
            }))
        }),
    )?;

    control.quit.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = engine.join();
    Ok(())
}

/// The desktop window, with what a phone needs around it.
struct Phone {
    inner: app::ScopeApp,
    android_app: AndroidApp,
    keyboard: bool,
    /// The physical width the zoom was last chosen for.
    fitted_for: f32,
}

impl eframe::App for Phone {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.fit(&ctx);
        self.inner.ui(ui, frame);
        self.keyboard_button(&ctx);
    }
}

impl Phone {
    /// Choose a zoom that fits the desktop layout across the screen, never
    /// larger than the screen's own density would give.
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
        let native = native.unwrap_or(1.0);
        let zoom = (physical / LAYOUT_W).clamp(0.5, native.max(0.5));
        ctx.set_pixels_per_point(zoom);
    }

    /// A small floating button that raises or drops the soft keyboard.
    fn keyboard_button(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("android-keyboard"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let label = if self.keyboard { "Hide keyboard" } else { "Keyboard" };
                if ui
                    .add(egui::Button::new(egui::RichText::new(label).size(16.0)))
                    .clicked()
                {
                    self.keyboard = !self.keyboard;
                    if self.keyboard {
                        self.android_app.show_soft_input(true);
                    } else {
                        self.android_app.hide_soft_input(false);
                    }
                }
            });
    }
}

/// Send panics to logcat as well as wherever the upstream hook puts them.
fn log_panics() {
    crashlog::install();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{info}");
        previous(info);
    }));
}

/// Ask for RECORD_AUDIO, which a sound-card modem cannot hear without.
///
/// `Activity.requestPermissions` through JNI; the answer comes back to the
/// activity, and the line simply opens once it has been given.
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
