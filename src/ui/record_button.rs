use crownui::kit::{icon_colored, icons, use_theme};
use crownui::prelude::{Cx, Listeners, Signal, Styled, View, bind, show, vstack};

use super::round_button::round_button;
use super::shortcuts::SPACE;
use crate::state::TakePhase;
use crate::util::tint;

const DIAMETER: f32 = 76.0;
const DOT_DIAMETER: f32 = 26.0;
const PAUSE_SIZE: f32 = 30.0;
const PAUSE_STROKE: f32 = 2.6;

/// The big red button: record, then pause and resume. Space presses it.
pub fn record_button<OnPress: FnMut(&mut Cx) + 'static>(
    cx: &mut Cx,
    phase: Signal<TakePhase>,
    mut on_press: OnPress,
) -> impl View + use<OnPress> {
    let theme = use_theme(cx);
    let glyph = show(
        bind(move |runtime| phase.get(runtime) == TakePhase::Recording),
        || {
            icon_colored(icons::PAUSE, PAUSE_STROKE, |theme| theme.status.on_danger)
                .size(PAUSE_SIZE)
        },
        move || {
            vstack(())
                .size(DOT_DIAMETER)
                .rounded(DOT_DIAMETER / 2.0)
                .bg(tint(theme, |theme| theme.status.on_danger))
        },
    );
    round_button(
        cx,
        DIAMETER,
        |theme| theme.status.danger,
        glyph,
        bind(move |runtime| phase.get(runtime).action().to_owned()),
    )
    .shortcut(SPACE)
    .on_click(move |cx, _| on_press(cx))
}
