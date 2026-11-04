use std::time::Duration;

use crownui::kit::{INTER_FAMILY, use_theme};
use crownui::prelude::{Cx, Role, Semantic, Signal, Styled, View, bind, hstack, keyed, text};

use crate::util::{duration_text, tint};

const TEXT_SIZE: f32 = 56.0;
const DIGIT_WIDTH: f32 = TEXT_SIZE * 0.62;
const SEPARATOR_WIDTH: f32 = TEXT_SIZE * 0.3;
const SEPARATOR: char = ':';

/// The elapsed time with every digit in a cell of its own width, so it never jitters.
pub fn clock(cx: &mut Cx, elapsed: Signal<Duration>) -> impl View + use<> {
    let theme = use_theme(cx);
    let reading = cx.memo(move |runtime| duration_text(elapsed.get(runtime)));
    let shape = move |runtime: &mut _| {
        reading.with(runtime, |reading: &String| {
            reading
                .chars()
                .map(|glyph| glyph == SEPARATOR)
                .collect::<Vec<_>>()
        })
    };
    keyed(shape, move |_, separators| {
        let cells: Vec<_> = separators
            .into_iter()
            .enumerate()
            .map(|(index, is_separator)| {
                text(bind(move |runtime| {
                    reading.with(runtime, |reading| {
                        reading.get(index..=index).unwrap_or_default().to_owned()
                    })
                }))
                .w(if is_separator {
                    SEPARATOR_WIDTH
                } else {
                    DIGIT_WIDTH
                })
                .text_center()
            })
            .collect();
        hstack(cells)
    })
    .text_size(TEXT_SIZE)
    .font_family(INTER_FAMILY)
    .font_semibold()
    .text_color(tint(theme, |theme| theme.text.primary))
    .role(Role::Text)
    .label("Elapsed time")
    .value(reading)
}
