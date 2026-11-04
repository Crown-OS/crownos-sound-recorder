use std::iter;

use audio::{Enhancement, SourceInfo};
use crownui::kit::{SelectKind, select};
use crownui::prelude::{Cx, Element, Listeners, Semantic};

use crate::encode::RecordingFormat;
use crate::state::{AppState, Capture, MicrophoneInput};
use crate::util::follow_watch;

const SYSTEM_DEFAULT_INPUT: &str = "System default";

pub const fn enhancement_name(enhancement: Enhancement) -> &'static str {
    match enhancement {
        Enhancement::Off => "Off",
        Enhancement::Light => "Light",
        Enhancement::NoiseSuppression => "Noise reduction",
        Enhancement::StudioVoice => "Studio voice",
    }
}

const fn format_name(format: RecordingFormat) -> &'static str {
    match format {
        RecordingFormat::Opus => "Opus",
        RecordingFormat::Flac => "FLAC",
    }
}

/// A dropdown over `choices`, starting at `current`, calling `on_pick` with each new choice.
pub fn choice_picker<Choice: Copy + PartialEq + 'static>(
    cx: &mut Cx,
    label: &str,
    choices: &'static [Choice],
    current: Choice,
    name: fn(Choice) -> &'static str,
    mut on_pick: impl FnMut(&mut Cx, Choice) + 'static,
) -> Element<SelectKind> {
    let names = choices
        .iter()
        .map(|&choice| name(choice).to_owned())
        .collect();
    let options = cx.signal(names);
    let selected = cx.signal(choices.iter().position(|&choice| choice == current));
    select(options, selected)
        .label(label.to_owned())
        .on_change(move |cx, _| {
            if let Some(&choice) = selected.get(cx).and_then(|index| choices.get(index)) {
                on_pick(cx, choice);
            }
        })
}

pub fn enhancement_picker(
    cx: &mut Cx,
    state: AppState,
    microphone: MicrophoneInput,
) -> Element<SelectKind> {
    let current = state
        .preferences
        .with_untracked(cx, |preferences| preferences.enhancement);
    choice_picker(
        cx,
        "Enhancement",
        &Enhancement::ALL,
        current,
        enhancement_name,
        move |cx, enhancement| {
            state.update_preferences(cx, |preferences| preferences.enhancement = enhancement);
            microphone.set_enhancement(cx, enhancement);
        },
    )
}

pub fn format_picker(cx: &mut Cx, state: AppState) -> Element<SelectKind> {
    let current = state
        .preferences
        .with_untracked(cx, |preferences| preferences.format);
    choice_picker(
        cx,
        "Format",
        &RecordingFormat::ALL,
        current,
        format_name,
        move |cx, format| state.update_preferences(cx, |preferences| preferences.format = format),
    )
}

/// The capture devices as they come and go, the system default first.
pub fn input_picker(cx: &mut Cx, state: AppState) -> Element<SelectKind> {
    let sources = cx.signal(Vec::<SourceInfo>::new());
    if state.capture == Capture::Microphone {
        let receiver = audio::sources();
        sources.set(cx, receiver.borrow().clone());
        follow_watch(cx, sources, receiver);
    }
    let options = cx.signal(Vec::new());
    let selected = cx.signal(None);
    cx.effect(move |runtime| {
        let chosen = state
            .preferences
            .with(runtime, |preferences| preferences.input.clone());
        let (names, index) = sources.with(runtime, |sources| {
            let names = iter::once(SYSTEM_DEFAULT_INPUT.to_owned())
                .chain(sources.iter().map(|source| source.description.clone()))
                .collect::<Vec<_>>();
            let index = match &chosen {
                None => Some(0),
                Some(id) => sources
                    .iter()
                    .position(|source| source.id == *id)
                    .map(|position| position + 1),
            };
            (names, index)
        });
        options.set(runtime, names);
        selected.set(runtime, index);
    });
    select(options, selected)
        .label("Input")
        .on_change(move |cx, _| {
            let Some(index) = selected.get(cx) else {
                return;
            };
            let input = sources.with(cx, |sources| {
                let source = index
                    .checked_sub(1)
                    .and_then(|position| sources.get(position));
                source.map(|source| source.id.clone())
            });
            state.update_preferences(cx, |preferences| preferences.input = input);
        })
}
