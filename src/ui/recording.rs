use std::path::PathBuf;

use crownui::kit::{TEXT_SIZE, icon_button, icons, use_theme};
use crownui::prelude::{
    AnyView, Cx, Listeners, Role, Semantic, Styled, View, hstack, spacer, text, vstack,
};

use super::delete_confirmation::delete_confirmation;
use super::enhance::enhance_panel;
use super::title_editor::title_editor;
use super::transport::transport;
use crate::state::AppState;
use crate::util::{date_text, duration_text, tint};

const CONTENT_WIDTH: f32 = 680.0;
const CONTENT_PADDING: f32 = 32.0;
const SECTION_GAP: f32 = 24.0;
const HEADER_GAP: f32 = 4.0;
const MISSING: &str = "This recording is no longer in the library.";

/// One saved recording: its title, player and enhancement.
pub fn recording_view(cx: &mut Cx, state: AppState, path: PathBuf) -> AnyView {
    let theme = use_theme(cx);
    let Some(recording) = state.recording(cx, &path) else {
        return text(MISSING).p(CONTENT_PADDING).boxed();
    };
    let details = format!(
        "{} · {} · {}",
        date_text(recording.created),
        duration_text(recording.duration),
        recording.format.extension().to_uppercase()
    );
    let confirming_delete = cx.signal(false);
    let title = recording.title.clone();
    let transport = transport(cx, state, &recording);
    let enhance = enhance_panel(cx, state, &recording.path);
    let recording = cx.store(recording);
    vstack((
        vstack((
            hstack((
                title_editor(cx, state, recording),
                spacer(),
                icon_button(icons::TRASH_2, "Delete")
                    .on_click(move |cx, _| confirming_delete.set(cx, true)),
            ))
            .items_center()
            .w_full(),
            text(details)
                .text_size(TEXT_SIZE - 1.0)
                .text_color(tint(theme, |theme| theme.text.muted)),
        ))
        .gap(HEADER_GAP),
        delete_confirmation(cx, state, recording, confirming_delete),
        transport,
        enhance,
    ))
    .gap(SECTION_GAP)
    .p(CONTENT_PADDING)
    .w_full()
    .max_w(CONTENT_WIDTH)
    .self_center()
    .role(Role::Group)
    .label(title)
    .boxed()
}
