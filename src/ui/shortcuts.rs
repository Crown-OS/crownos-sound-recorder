use crownui::ext::{Key, Keybind, Modifiers, NamedKey};

const fn bare(key: NamedKey) -> Keybind {
    Keybind {
        key: Key::Named(key),
        modifiers: Modifiers::NONE,
    }
}

pub const SPACE: Keybind = bare(NamedKey::Space);
pub const ESCAPE: Keybind = bare(NamedKey::Escape);
