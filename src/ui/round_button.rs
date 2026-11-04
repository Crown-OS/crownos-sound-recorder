use crownui::kit::{Theme, use_theme};
use crownui::prelude::{
    Color, CursorIcon, Cx, IntoProp, Role, Semantic, Spring, Styled, View, vstack,
};

use crate::util::tint;

const HOVER_OPACITY: f32 = 0.88;
const PRESSED_SCALE: f32 = 0.94;

/// A filled circle around `glyph`, named `label` for assistive technology; attach `.on_click`.
pub fn round_button<Glyph: View, Label: IntoProp<String>>(
    cx: &mut Cx,
    diameter: f32,
    fill: fn(&Theme) -> Color,
    glyph: Glyph,
    label: Label,
) -> impl View + Styled + use<Glyph, Label> {
    let theme = use_theme(cx);
    vstack(glyph)
        .size(diameter)
        .shrink(0.0)
        .rounded(diameter / 2.0)
        .items_center()
        .justify_center()
        .bg(tint(theme, fill))
        .hover(|style| style.opacity(HOVER_OPACITY))
        .active(|style| style.scale(PRESSED_SCALE))
        .transition(Spring::snappy())
        .cursor(CursorIcon::Pointer)
        .role(Role::Button)
        .focusable(true)
        .label(label)
}
