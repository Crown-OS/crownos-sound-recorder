use crownui::kit::{TEXT_SIZE, icons, sidebar_item, sidebar_section, use_theme};
use crownui::prelude::{
    Cx, Listeners, Semantic, Styled, ViewSeq, bind, for_each, scroll_view, show, text, vstack,
};

use super::library_row::library_row;
use super::nothing::nothing;
use crate::recording::Recording;
use crate::state::{AppState, Selection};
use crate::util::tint;

const ROW_GAP: f32 = 4.0;
const EMPTY_PADDING: f32 = 10.0;

/// The sidebar: a way back to the recorder, then every recording, newest first.
pub fn library_menus(cx: &mut Cx, state: AppState) -> impl ViewSeq + use<> {
    let theme = use_theme(cx);
    let selection = state.selection;
    let recordings = state.recordings;
    let on_recorder =
        bind(move |runtime| selection.with(runtime, |selection| *selection == Selection::Recorder));
    let empty_hint = move || {
        text("No recordings yet")
            .text_size(TEXT_SIZE - 1.0)
            .text_color(tint(theme, |theme| theme.sidebar.subitem_text))
            .px(EMPTY_PADDING)
    };
    (
        sidebar_item("New Recording", icons::MIC, on_recorder)
            .on_click(move |cx, _| state.show(cx, Selection::Recorder)),
        sidebar_section("Recordings", ()),
        scroll_view(
            vstack((
                for_each(
                    recordings,
                    |recording: &Recording| recording.path.clone(),
                    move |cx, recording| library_row(cx, state, recording),
                ),
                show(
                    bind(move |runtime| recordings.with(runtime, Vec::is_empty)),
                    empty_hint,
                    nothing,
                ),
            ))
            .gap(ROW_GAP)
            .role(crownui::prelude::Role::List)
            .label("Recordings"),
        )
        .flex_1(),
    )
}
