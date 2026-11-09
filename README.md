# BinModemAPK
This is not official.

An Android build of [BinModem](https://github.com/CasualArclamp/BinModem), the
dial-up softmodem in Rust. The upstream source is the `BinModem/` submodule and
is not modified; `android/` borrows its GUI modules unchanged and adds an
Android entry point.

## What the Android build adds

![The four tabs: Line, Terminal, Scopes, Status](docs/phone.png)

A layout for a phone held upright (`android/src/mobile.rs`), built from the
desktop window's own controls:

- **Line**: the line, the call and every setting, wrapped into one column.
  Dial, Network, Advanced, Files, Fax and Error control open as full-screen
  sheets with a close button.
- **Terminal**: the terminal sized to fit 80 columns across (untick *fit
  width* for a font slider), with Esc, Tab, Ctrl-C, the arrows, Del, `+++`
  and Enter as buttons, since a phone keyboard has none of them. Tapping the
  terminal raises the keyboard.
- **Scopes**: waterfall, spectrum and constellation, stacked.
- **Status**: the call's state and rates, the V.90 rate menu, Retrain, and
  what the far end said about itself.
- A strip along the top with the call state and the modem's lights, always in
  view, and a **Sound** toggle (lit while on, like V.8). On, you hear the
  call and the monitor opens by itself; off is silence: it closes the
  monitor, and on a sound-card line silences the modem's own tones too, so
  there the far end hears nothing from this side while it is off. A SIP call
  carries on as normal either way.
- **Caller** and **Answerer** toggles in place of Listen (on the Line tab, and
  above the waterfall on Scopes): each side of the call on or off, both for
  what you hear and for what the spectrum and waterfall draw. Which one is
  your modem follows who dialled, and is marked "(you)" during a call. Over
  SIP your own side is mixed into what you hear at about the far end's level;
  on a sound-card line it is already coming out of the phone, so only the
  spectrum follows that switch. All three switches are remembered between runs.

Also: settings and SIP accounts kept in the app's private storage, and the
microphone permission asked for on first launch.

### The long DIL

Upstream's V.90 reads a short DIL (digital impairment learning, the end of
phase 3): one pass of the pattern it asks for, about 0.44 s. The server
repeats the DIL until the modem answers, so this build simply listens for
two passes (about 0.88 s) before choosing the rate: each codeword is read
twice as often and the noise estimate the rate is chosen from is firmer. The
pattern asked for is upstream's, so finding the DIL again after a jitter
buffer slip works as before. It is the one change to the modem itself, kept
as `android/patches/long-dil.patch` and applied to the submodule by
`build-apk.sh` (the build stops if it is missing).

The DIL is watched as it is read, V.90 or V.92. Once the two passes should
long since have come (about 2.4 s), a reading with all but a few of its
symbols read once goes on with that, as a one-pass DIL and a hardware modem
do. One with less than that after about 6 s fails, and the start-up retrains
at once instead of waiting for the 15 s B1d deadline. A reading that loses
its place on the line looks for it again, as upstream's does. The Line notes
say each of these: "reading the DIL", "lost the DIL's place ... looking for
it", "found the DIL again", "the DIL read once over ... going on with that".
This is in `android/patches/v92-quick-connect.patch` with the V.92 work.

### Recording a call

**Record** on the Line page, while a line is open, keeps the call as a stereo
WAV (what arrived on one channel, what was sent on the other) in
`Android/data/io.github.binmodem/files/captures/` on the phone, which a
computer reaches over USB. A recording of a call that went wrong can be run
through the receiver again, on a computer, to find out why.

### V.92: Quick Connect and PCM upstream

Upstream BinModem has no V.92. This build adds V.92's PCM upstream and its
Quick Connect for calls to a V.92 server. It is the **V.92** entry in the call
box (`AT+MS=V92`); **V.90** stays V.90 as upstream has it, saying nothing of
V.92, with V.34 upstream, never quick connecting.

**PCM upstream** (clause 8/V.92). A full V.92 start-up asks for it in
INFO1a (Table 18/V.92) of a server that offers it in INFO1d, and then:

- Phase 3 (9.5/V.92): Ru and TRN1u, Ja with the upstream rates, Su after Jd,
  and TRN1u again through the DIL, timed by Jp.
- Phase 4 (9.6/V.92): CPt, TRN2u, CPu and the SUVu/SUVd handshake; the
  server's CPd sets up this end's precoder, prefilter, gain and
  constellations; E2u, B1u, data.
- Data mode (6.4/V.92): modulus encoder, 16-state trellis, precoder choosing
  the smallest point of each class, prefilter; 8000 symbols a second put on
  the line by sinc interpolation.
- The upstream runs on the network's clock, not the sound card's. Once the
  downstream receiver has trained, a phase-locked loop holds the upstream to
  the receiver's sampling clock. In simulation a sound card 100 ppm fast or
  slow makes no difference.
- Phase 3 is watched a step at a time: Jp within 6 s of Su, Jp' within 2 s
  of Jp, the DIL read within 10 s, Ri within 3 s of CPt (each plus the round
  trip). The Line notes say which step a start-up stopped at ("found Jp'",
  "sent CPt", "found Ri", or "failed: V.92: no Ri ...").
- A start-up with PCM upstream that does not come up is retrained at once
  with V.34's upstream under V.90's downstream, and the next five calls to
  that number go with V.34's upstream from the start (kept in the same
  `quick-connect` file). A call that comes up with PCM upstream clears this.

In simulation (a V.92 server written for it here, which works out the
precoder and constellations from TRN1u and reads the upstream with a Viterbi
decoder), a 20 ms line comes up in 4.9 s at 52 000 down and 28 000 to
32 000 up, then 0 bits wrong in 15 s each way. On a noisy line it is 25 333
up. That makes PCM upstream about level with V.34's 31 200 here, not
V.92's 48 000: the server picks its constellation to leave a safe margin.
**None of this has met a real V.92 server yet.** A real server picks its own
constellation from what it hears, and where the spec leaves a choice (the bit
order of TRN2u's levels, for one) this follows its plainest reading.

**Quick Connect** (clause 9/V.92):

- A V.92 call says V.92 in INFO0a (Table 16/V.92). When the server says
  it too, the call remembers what its full start-up settled for the
  upstream, under the number dialled (`BinModem/quick-connect` in the app's
  settings directory).
- The next call to that number sends QC1a ahead of CM. A server that answers
  with QCA1d gets V.92's short Phase 1 (QTS, ANSpcm, TONEq) and short Phase 2
  (no line probing; INFO1a per Table 19/V.92), then V.90's phases 3 and 4
  with V.34 upstream: a Quick Connect does not ask for PCM upstream.
  LAPM is agreed in the short Phase 1, with no ODP/ADP.
- **Quick Connect 2** (9.2.1.2/V.92): a server that opens with V.8 bis's
  CRe (1375 + 2002 Hz, then 400 Hz) instead of ANSam is answered with QC2a, a
  V.8 bis frame on V.21(H). Its QCA2d (on V.21(L)) leads to the same QTS,
  ANSpcm, TONEq and short Phase 2 as QC1, a little sooner: in simulation
  7.2 s to online against 11.6 s in full. A CRe with no QCA2d after it is
  waited past for ANSam, and QC1 or V.8. The Line notes say which Quick
  Connect a call used. Only the V.8 bis this needs is here: CRe, and
  QC2a/QCA2d framed as 7.2/V.8 bis has it (flags, FCS, zero insertion).
- A server that ignores QC1a answers the CM with JM, and V.8 goes on as
  before. A Quick Connect that fails to reach data mode forgets the number,
  so the next call starts in full.

Not done: modem-on-hold, and PCM upstream with Quick Connect's short Phase 2.
A renegotiation during PCM upstream is a full retrain.
Kept as `android/patches/v92-quick-connect.patch`, applied after the long DIL.

### How it is put together

Upstream is not edited. `android/build.rs` copies the GUI files that need it
into the build directory and swaps four things in the copy: wrapping rows for ones held to the screen,
with text that does not fit shortened with "..." (in egui, one overflowing
row -- a SIP address, which has nowhere to break -- shifts or widens every
row after it), combo boxes and sliders for versions that move to a new row when
they do not fit (egui cannot wrap them otherwise), and windows for
full-screen sheets.
In `app.rs` the Listen button becomes the Caller/Answerer switches; in
`live.rs` the sound-card output, the monitor and the spectrum go through the
switches (mixing our own side in where it is on), and fails the build if those lines change upstream. It
also adds `mod mobile;` inside `app`, which is how the phone layout reaches
the window's private state.

To look at the layout without a phone (Linux, needs ALSA and X11):

    cd android && BINMODEM_PAGE=terminal cargo run --example preview

`BINMODEM_PAGE` is `line`, `terminal`, `scopes` or `status`;
`BINMODEM_OPEN=dial|network|advanced|files|fax|error` opens a window too
(and `v90`, `sip` pick those, `long` draws a SIP call's long status row; several can be given, comma-separated).

## Limits

- File dialogs (`Browse…` for ZMODEM and fax pictures) are stubbed out; type a
  path into the box beside them instead (e.g. under `/sdcard/Download`,
  which may need the app's storage access granted in Android settings).
- arm64-v8a only, Android 8.0 (API 26) or newer (AAudio).

## Building

    git clone --recursive https://github.com/ooper1234/BinModemAPK
    cd BinModemAPK
    ANDROID_HOME=~/Android/Sdk ./build-apk.sh    # -> BinModem.apk

Needs Rust, a JDK, and an Android SDK with build-tools, a platform and the NDK.
Without `KEYSTORE`/`KEYSTORE_PASS`/`KEY_ALIAS` set, a debug key is generated in
`android/debug.keystore`.

Every push also builds the APK in GitHub Actions (`Build APK` workflow); download
it from the run's artifacts.

## License

GPL-3.0-or-later, as upstream.
