use std::path::PathBuf;

use crownui::kit::{
    TEXT_SIZE, WaveformOptions, WaveformPeaks, icon_colored, icons, use_theme, waveform,
};
use crownui::prelude::{
    Cx, Listeners, Semantic, Signal, Styled, View, bind, hstack, show, spacer, text, vstack,
};

use super::round_button::round_button;
use crate::recording::{
    PlaybackState, PlaybackStatus, Recording, load_or_compute_peaks, read_peaks,
};
use crate::state::{AppState, Playback};
use crate::util::{duration_text, tint};

const WAVEFORM_HEIGHT: f32 = 120.0;
const PLAY_DIAMETER: f32 = 56.0;
const GLYPH_SIZE: f32 = 24.0;
const GLYPH_STROKE: f32 = 2.4;
const TIME_WIDTH: f32 = 64.0;
const GAP: f32 = 16.0;

/// The recording's waveform as a seek bar, with play and pause between its position and length.
pub fn transport(cx: &mut Cx, state: AppState, recording: &Recording) -> impl View + use<> {
    let theme = use_theme(cx);
    let playback = Playback::new(cx, recording.path.clone(), recording.duration);
    let status = playback.status;
    let peaks = cx.signal(WaveformPeaks::default());
    load_peaks(cx, peaks, recording.path.clone());
    let playing = cx
        .memo(move |runtime| status.with(runtime, |status| status.state == PlaybackState::Playing));
    let glyph = |body: &'static str| {
        move || icon_colored(body, GLYPH_STROKE, |theme| theme.text.on_accent).size(GLYPH_SIZE)
    };
    let time = move |read: fn(&PlaybackStatus) -> std::time::Duration| {
        text(bind(move |runtime| {
            status.with(runtime, |status| duration_text(read(status)))
        }))
        .w(TIME_WIDTH)
        .text_size(TEXT_SIZE - 1.0)
        .text_color(tint(theme, |theme| theme.text.muted))
    };
    let play_button = round_button(
        cx,
        PLAY_DIAMETER,
        |theme| theme.accent.start,
        show(playing, glyph(icons::PAUSE), glyph(icons::PLAY)),
        bind(move |runtime| {
            if playing.get(runtime) {
                "Pause"
            } else {
                "Play"
            }
            .to_owned()
        }),
    )
    .on_click(move |cx, _| {
        if let Err(error) = playback.toggle(cx) {
            state.report(cx, format!("{error:#}"));
        }
    });
    vstack((
        waveform(
            peaks,
            bind(move |runtime| status.with(runtime, PlaybackStatus::fraction) as f32),
        )
        .on_seeking(move |cx, position| {
            if let Err(error) = playback.seek(cx, position) {
                state.report(cx, format!("{error:#}"));
            }
        })
        .w_full()
        .h(WAVEFORM_HEIGHT)
        .label("Playback position"),
        hstack((
            time(|status| status.position).label("Position"),
            spacer(),
            play_button,
            spacer(),
            time(|status| status.duration)
                .text_right()
                .label("Duration"),
        ))
        .items_center()
        .w_full(),
    ))
    .gap(GAP)
    .w_full()
}

/// The saved peaks right away, or peaks decoded off the UI thread for files from elsewhere.
fn load_peaks(cx: &mut Cx, peaks: Signal<WaveformPeaks>, path: PathBuf) {
    if let Ok(saved) = read_peaks(&path) {
        peaks.set(cx, WaveformPeaks::new(saved));
        return;
    }
    let decode =
        async move { tokio::task::spawn_blocking(move || load_or_compute_peaks(&path)).await };
    cx.spawn(decode, move |cx, decoded| {
        if let Ok(Ok(decoded)) = decoded {
            peaks.try_set(cx, WaveformPeaks::new(decoded));
        }
    });
}
