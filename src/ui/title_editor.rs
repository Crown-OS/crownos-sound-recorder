use crownui::ext::{Key, NamedKey};
use crownui::kit::{
    ButtonOptions, ButtonVariant, TEXT_SIZE, TextInputOptions, button, icon_button, icons,
    text_input, use_theme,
};
use crownui::prelude::{
    Cx, Listeners, Role, Semantic, Signal, Store, Styled, View, bind, hstack, show, text, vstack,
};

use super::nothing::nothing;
use crate::recording::Recording;
use crate::state::AppState;
use crate::util::tint;

const TITLE_SIZE: f32 = TEXT_SIZE + 9.0;
const FIELD_WIDTH: f32 = 320.0;
const GAP: f32 = 8.0;

#[derive(Debug, Clone, Copy)]
struct Draft {
    recording: Store<Recording>,
    title: Signal<String>,
    editing: Signal<bool>,
    failure: Signal<Option<String>>,
}

impl Draft {
    fn begin(self, cx: &mut Cx) {
        let title = self.recording.with(cx, |recording| recording.title.clone());
        self.title.set(cx, title);
        self.failure.set(cx, None);
        self.editing.set(cx, true);
    }

    fn cancel(self, cx: &mut Cx) {
        self.editing.set(cx, false);
    }

    fn commit(self, cx: &mut Cx, state: AppState) {
        let title = self.title.get(cx);
        let recording = self.recording.with(cx, Recording::clone);
        if title.trim() == recording.title {
            return self.cancel(cx);
        }
        if let Err(error) = state.rename(cx, &recording, &title) {
            self.failure.set(cx, Some(error.to_string()));
        }
    }
}

/// The recording's title, renamed in place.
pub fn title_editor(
    cx: &mut Cx,
    state: AppState,
    recording: Store<Recording>,
) -> impl View + use<> {
    let theme = use_theme(cx);
    let title = recording.with(cx, |recording| recording.title.clone());
    let draft = Draft {
        recording,
        title: cx.signal(title.clone()),
        editing: cx.signal(false),
        failure: cx.signal(None),
    };
    let editor = move || {
        vstack((
            hstack((
                text_input(draft.title)
                    .width(FIELD_WIDTH)
                    .label("Title")
                    .on_submit(move |cx, _| draft.commit(cx, state))
                    .on_key_down(move |cx, event| {
                        if event.key == Key::Named(NamedKey::Escape) {
                            draft.cancel(cx);
                        }
                    }),
                button("Save")
                    .variant(ButtonVariant::Primary)
                    .on_click(move |cx, _| draft.commit(cx, state)),
                button("Cancel").on_click(move |cx, _| draft.cancel(cx)),
            ))
            .items_center()
            .gap(GAP),
            show(
                bind(move |runtime| draft.failure.with(runtime, Option::is_some)),
                move || {
                    text(bind(move |runtime| {
                        draft.failure.get(runtime).unwrap_or_default()
                    }))
                    .text_color(tint(theme, |theme| theme.status.danger))
                },
                nothing,
            ),
        ))
        .gap(GAP)
    };
    let heading = move || {
        hstack((
            text(title.clone())
                .text_size(TITLE_SIZE)
                .font_semibold()
                .truncate()
                .text_color(tint(theme, |theme| theme.text.primary))
                .role(Role::Heading),
            icon_button(icons::PENCIL, "Rename").on_click(move |cx, _| draft.begin(cx)),
        ))
        .items_center()
        .gap(GAP)
    };
    show(draft.editing, editor, heading)
}
