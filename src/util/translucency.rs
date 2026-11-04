use crownui::prelude::Color;

/// `color` with its translucency, `1 - alpha`, multiplied by `factor`.
pub fn with_translucency_factor(color: Color, factor: f32) -> Color {
    Color {
        a: 1.0 - (1.0 - color.a) * factor,
        ..color
    }
}
