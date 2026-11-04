use std::path::Path;

use audio::Enhancement;
use crownui::kit::{
    ButtonOptions, ButtonVariant, CalloutOptions, TEXT_SIZE, Tone, button, callout, progress,
    section, setting_row, use_theme,
};
use crownui::prelude::{
    Cx, Listeners, Semantic, Styled, View, bind, hstack, keyed, spacer, text, vstack,
};

use super::pickers::{choice_picker, enhancement_name};
use crate::state::{AppState, EnhancePhase, EnhanceTask};
use crate::util::tint;

const OFFLINE_ENHANCEMENTS: [Enhancement; 3] = [
    Enhancement::Light,
    Enhancement::NoiseSuppression,
    Enhancement::StudioVoice,
];
const PROGRESS_WIDTH: f32 = 220.0;
const ROW_PADDING: f32 = 14.0;
const GAP: f32 = 12.0;
const HINT: &str = "Saves an enhanced copy next to the original.";

/// Enhances a copy of the recording through the voice service, with progress and cancel.
pub fn enhance_panel(cx: &mut Cx, state: AppState, source: &Path) -> impl View + use<> {
    let theme = use_theme(cx);
    let task = EnhanceTask::new(cx);
    let preferred = state
        .preferences
        .with_untracked(cx, |preferences| preferences.enhancement);
    let initial = if OFFLINE_ENHANCEMENTS.contains(&preferred) {
        preferred
    } else {
        Enhancement::NoiseSuppression
    };
    let choice = cx.signal(initial);
    let source = cx.store(source.to_path_buf());
    let running = cx.memo(move |runtime| task.phase.get(runtime) == EnhancePhase::Running);
    let start = move |cx: &mut Cx| {
        let enhancement = choice.get(cx);
        let path = source.with(cx, Clone::clone);
        task.start(cx, state, &path, enhancement);
    };
    let picker = choice_picker(
        cx,
        "Enhancement",
        &OFFLINE_ENHANCEMENTS,
        initial,
        enhancement_name,
        move |cx, enhancement| choice.set(cx, enhancement),
    )
    .disabled(running);
    let action = keyed(
        move |runtime| task.phase.get(runtime),
        move |_, phase| match phase {
            EnhancePhase::Idle => hstack((
                text(HINT)
                    .text_size(TEXT_SIZE - 1.0)
                    .text_color(tint(theme, |theme| theme.text.muted)),
                spacer(),
                button("Enhance")
                    .variant(ButtonVariant::Primary)
                    .on_click(move |cx, _| start(cx)),
            ))
            .items_center()
            .gap(GAP)
            .boxed(),
            EnhancePhase::Running => hstack((
                progress(task.progress)
                    .w(PROGRESS_WIDTH)
                    .label("Enhancement progress"),
                text(bind(move |runtime| {
                    format!("{:.0}%", task.progress.get(runtime) * 100.0)
                }))
                .text_color(tint(theme, |theme| theme.text.muted)),
                spacer(),
                button("Cancel").on_click(move |cx, _| task.cancel(cx)),
            ))
            .items_center()
            .gap(GAP)
            .boxed(),
            EnhancePhase::Failed(reason) => vstack((
                callout(Tone::Danger, "Enhancement failed").body(reason),
                button("Try Again").on_click(move |cx, _| start(cx)),
            ))
            .gap(GAP)
            .boxed(),
        },
    );
    section(
        "Enhance",
        (
            setting_row("Enhancement", picker),
            vstack(action).p(ROW_PADDING),
        ),
    )
    .self_stretch()
}
