use crownui::kit::{INTER_FAMILY, TEXT_SIZE, use_theme};
use crownui::prelude::{
    CursorIcon, Cx, Listeners, Role, Runtime, Semantic, Signal, Spring, Styled, TRANSPARENT, View,
    bind, text, vstack,
};

use crate::recording::Recording;
use crate::state::{AppState, Selection};
use crate::util::{date_text, duration_text, tint};

const ROW_RADIUS: f32 = 14.0;
const PADDING_X: f32 = 10.0;
const PADDING_Y: f32 = 7.0;
const LINE_GAP: f32 = 2.0;
const DETAIL_TEXT_SIZE: f32 = TEXT_SIZE - 2.0;

/// A recording in the sidebar: its title over when it was made and how long it is.
pub fn library_row(
    cx: &mut Cx,
    state: AppState,
    recording: Signal<Recording>,
) -> impl View + use<> {
    let theme = use_theme(cx);
    let path = recording.with_untracked(cx, |recording| recording.path.clone());
    let selection = state.selection;
    let selected = cx.memo({
        let path = path.clone();
        move |runtime| {
            selection.with(
                runtime,
                |selection| matches!(selection, Selection::Recording(shown) if *shown == path),
            )
        }
    });
    let title =
        move |runtime: &mut Runtime| recording.with(runtime, |recording| recording.title.clone());
    let detail = move |runtime: &mut Runtime| {
        recording.with(runtime, |recording| {
            format!(
                "{} · {}",
                date_text(recording.created),
                duration_text(recording.duration)
            )
        })
    };
    let title_color = bind(move |runtime| {
        let selected = selected.get(runtime);
        theme.with(runtime, |theme| {
            if selected {
                theme.sidebar.selected_text
            } else {
                theme.sidebar.text
            }
        })
    });
    let fill = move |hovered: bool| {
        bind(move |runtime| {
            let selected = selected.get(runtime);
            theme.with(runtime, |theme| match (selected, hovered) {
                (true, _) => theme.sidebar.selected_bg,
                (false, true) => theme.sidebar.hover_bg,
                (false, false) => TRANSPARENT,
            })
        })
    };
    vstack((
        text(bind(title))
            .font_medium()
            .truncate()
            .text_color(title_color),
        text(bind(detail))
            .text_size(DETAIL_TEXT_SIZE)
            .truncate()
            .text_color(tint(theme, |theme| theme.sidebar.subitem_text)),
    ))
    .gap(LINE_GAP)
    .px(PADDING_X)
    .py(PADDING_Y)
    .rounded(ROW_RADIUS)
    .font_family(INTER_FAMILY)
    .bg(fill(false))
    .hover(|style| style.bg(fill(true)))
    .transition(Spring::snappy())
    .cursor(CursorIcon::Pointer)
    .role(Role::ListItem)
    .focusable(true)
    .label(bind(title))
    .value(bind(detail))
    .on_click(move |cx, _| state.show(cx, Selection::Recording(path.clone())))
}
