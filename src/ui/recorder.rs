use std::ops::ControlFlow;
use std::time::Duration;

use audio::{MicrophoneOptions, SourceId};
use crownui::kit::{
    LevelHistory, LevelMeterOptions, icon_button, icons, level_meter, live_waveform, section,
    setting_row,
};
use crownui::prelude::{
    Cx, Listeners, Semantic, Signal, Styled, View, bind, hstack, keyed, vstack,
};

use super::clock::clock;
use super::microphone_status::microphone_status;
use super::pickers::{enhancement_picker, format_picker, input_picker};
use super::record_button::record_button;
use super::shortcuts::ESCAPE;
use crate::state::{AppState, MicrophoneInput, TakePhase};
use crate::util::{perceived_level, repeat_every};

const CONTENT_WIDTH: f32 = 600.0;
const CONTENT_PADDING: f32 = 32.0;
const SECTION_GAP: f32 = 24.0;
const WAVEFORM_HEIGHT: f32 = 120.0;
const METER_WIDTH: f32 = 320.0;
const CONTROL_GAP: f32 = 28.0;
const STOP_SLOT: f32 = 28.0;
const TICK: Duration = Duration::from_millis(16);
const MICROPHONE_UNAVAILABLE: &str = "The microphone is not available.";

/// Record, pause and stop, over the live input. The microphone is open while this is shown, and
/// reopens when another input is picked.
pub fn recorder_view(state: AppState) -> impl View {
    keyed(
        move |runtime| {
            state
                .preferences
                .with(runtime, |preferences| preferences.input.clone())
        },
        move |cx, input| recorder_body(cx, state, input),
    )
}

fn recorder_body(cx: &mut Cx, state: AppState, input: Option<SourceId>) -> impl View + use<> {
    let enhancement = state
        .preferences
        .with_untracked(cx, |preferences| preferences.enhancement);
    let options = MicrophoneOptions::default().enhancement(enhancement);
    let options = match input {
        Some(source) => options.source(source),
        None => options,
    };
    let microphone = MicrophoneInput::open(cx, state.capture, options);
    let history = cx.signal(LevelHistory::new());
    let level = microphone.level();
    let recording = cx.memo(move |runtime| state.take.is_active(runtime));
    vstack((
        microphone_status(microphone.state()),
        clock(cx, state.take.elapsed),
        live_waveform(history)
            .w_full()
            .h(WAVEFORM_HEIGHT)
            .label("Live waveform"),
        level_meter(bind(move |runtime| level.get(runtime).peak))
            .rms(bind(move |runtime| level.get(runtime).rms))
            .w(METER_WIDTH)
            .self_center()
            .label("Input level"),
        controls(cx, state, microphone, history),
        section(
            "Recording",
            (
                setting_row("Input", input_picker(cx, state).disabled(recording)),
                setting_row("Enhancement", enhancement_picker(cx, state, microphone)),
                setting_row("Format", format_picker(cx, state).disabled(recording)),
            ),
        )
        .self_stretch(),
    ))
    .items_center()
    .gap(SECTION_GAP)
    .p(CONTENT_PADDING)
    .w_full()
    .max_w(CONTENT_WIDTH)
    .self_center()
}

fn controls(
    cx: &mut Cx,
    state: AppState,
    microphone: MicrophoneInput,
    history: Signal<LevelHistory>,
) -> impl View + use<> {
    let take = state.take;
    let record = record_button(cx, take.phase, move |cx| match take.phase.get(cx) {
        TakePhase::Idle => start(cx, state, microphone, history),
        TakePhase::Recording | TakePhase::Paused => take.toggle_pause(cx),
    });
    let stop = icon_button(icons::SQUARE, "Stop")
        .disabled(bind(move |runtime| !take.is_active(runtime)))
        .shortcut(ESCAPE)
        .self_center()
        .on_click(move |cx, _| state.finish_take(cx));
    hstack((vstack(()).w(STOP_SLOT), record, stop))
        .items_center()
        .gap(CONTROL_GAP)
}

fn start(cx: &mut Cx, state: AppState, microphone: MicrophoneInput, history: Signal<LevelHistory>) {
    let Some(feed) = microphone.take_feed(cx) else {
        state.report(cx, MICROPHONE_UNAVAILABLE);
        return;
    };
    let format = state.preferences.with(cx, |preferences| preferences.format);
    let path = state.with_library(cx, |library| library.new_recording_path(format));
    if let Err(error) = state.take.begin(cx, path, format, feed) {
        state.report(cx, format!("{error:#}"));
        return;
    }
    history.update(cx, LevelHistory::clear);
    let take = state.take;
    let level = microphone.level();
    repeat_every(cx, TICK, move |cx| {
        if !history.is_alive(cx) || !take.is_active(cx) {
            return ControlFlow::Break(());
        }
        if take.phase.get(cx) == TakePhase::Recording {
            let peak = level.get(cx).peak;
            history.update(cx, |history| history.push(perceived_level(peak)));
        }
        take.sample_elapsed(cx);
        ControlFlow::Continue(())
    });
}
