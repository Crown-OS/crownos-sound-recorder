use crownui::kit::{CalloutOptions, Tone, callout, icon_button, icons};
use crownui::prelude::{Listeners, Styled, View, hstack, keyed};

use super::nothing::nothing;
use crate::state::AppState;

const MARGIN: f32 = 16.0;

/// The last thing that went wrong, until dismissed or the view changes.
pub fn notice_banner(state: AppState) -> impl View {
    let notice = state.notice;
    keyed(
        move |runtime| notice.get(runtime),
        move |_, message| match message {
            Some(message) => hstack((
                callout(Tone::Warning, "Something went wrong")
                    .body(message)
                    .flex_1(),
                icon_button(icons::X, "Dismiss").on_click(move |cx, _| notice.set(cx, None)),
            ))
            .items_start()
            .gap(8.0)
            .m(MARGIN)
            .boxed(),
            None => nothing().boxed(),
        },
    )
}
