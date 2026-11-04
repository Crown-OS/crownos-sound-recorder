use crownui::prelude::{Styled, View, vstack};

/// A view that takes no space and no gap.
pub fn nothing() -> impl View {
    vstack(()).display_none()
}
