use crownui::kit::{ButtonOptions, ButtonVariant, button, use_theme};
use crownui::prelude::{
    Cx, Listeners, Role, Semantic, Signal, Store, Styled, View, hstack, show, text,
};

use super::nothing::nothing;
use crate::recording::Recording;
use crate::state::AppState;
use crate::util::tint;

const RADIUS: f32 = 14.0;
const PADDING: f32 = 12.0;
const GAP: f32 = 8.0;

/// Asks before moving the recording to the trash.
pub fn delete_confirmation(
    cx: &mut Cx,
    state: AppState,
    recording: Store<Recording>,
    confirming: Signal<bool>,
) -> impl View + use<> {
    let theme = use_theme(cx);
    let question = recording.with(cx, |recording| {
        format!("Move “{}” to the trash?", recording.title)
    });
    let prompt = move || {
        hstack((
            text(question.clone())
                .flex_1()
                .text_color(tint(theme, |theme| theme.text.primary)),
            button("Cancel").on_click(move |cx, _| confirming.set(cx, false)),
            button("Move to Trash")
                .variant(ButtonVariant::Destructive)
                .on_click(move |cx, _| {
                    let doomed = recording.with(cx, Recording::clone);
                    state.delete(cx, &doomed);
                }),
        ))
        .items_center()
        .gap(GAP)
        .p(PADDING)
        .rounded(RADIUS)
        .bg(tint(theme, |theme| theme.status.danger_bg))
        .role(Role::Dialog)
        .label("Delete recording")
    };
    show(confirming, prompt, nothing)
}
