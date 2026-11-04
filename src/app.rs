use anyhow::Result;
use crownui::AppConfig;
use crownui::kit::{
    INTER_FAMILY, inter_font, provide_configured_theme, sidebar_with_header, title_bar_spacer,
    use_theme, window_controls,
};
use crownui::prelude::{
    BLACK, Curve, Cx, Styled, View, component, keyed, scroll_view, vstack, zstack,
};

use crate::state::{AppState, Environment, Selection};
use crate::ui::{library_menus, notice_banner, recorder_view, recording_view};
use crate::util::{tint, with_translucency_factor};

const TITLE: &str = "Voice Recorder";
const APP_ID: &str = "crownos-sound-recorder";
const INITIAL_SIZE: (u32, u32) = (980, 700);
const MIN_SIZE: (u32, u32) = (760, 560);
const CHROME_TRANSLUCENCY: f32 = 0.5;
const THEME_FADE_MILLIS: u64 = 240;

/// Opens the recorder window on the user's library and runs it until it closes.
pub fn run() -> Result<()> {
    let environment = Environment::system()?;
    let config = AppConfig::new(TITLE)
        .app_id(APP_ID)
        .size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .min_size(MIN_SIZE.0, MIN_SIZE.1)
        .clear_color(BLACK)
        .mcp(cfg!(debug_assertions))
        .font(inter_font().to_vec(), Some(INTER_FAMILY));
    let root = component(move |cx| {
        provide_configured_theme(cx);
        let state = AppState::new(cx, environment);
        shell(cx, state)
    });
    Ok(crownui::run(root, config)?)
}

/// Everything the window draws: the library beside the recorder or the selected recording,
/// under a title bar ending in the window controls, over a translucent backdrop in the theme's
/// surface color.
pub fn shell(cx: &mut Cx, state: AppState) -> impl View + use<> {
    let theme = use_theme(cx);
    let backdrop = vstack(())
        .w_full()
        .h_full()
        .bg(tint(theme, |theme| {
            with_translucency_factor(theme.surface.bg, CHROME_TRANSLUCENCY)
        }))
        .transition(Curve::ease_in_out(THEME_FADE_MILLIS));
    let selection = state.selection;
    let content = keyed(
        move |runtime| selection.get(runtime),
        move |cx, selection| match selection {
            Selection::Recorder => recorder_view(state).boxed(),
            Selection::Recording(path) => recording_view(cx, state, path),
        },
    )
    .w_full();
    let menus = library_menus(cx, state);
    zstack((
        backdrop,
        sidebar_with_header(
            menus,
            (title_bar_spacer(), window_controls()),
            (notice_banner(state), scroll_view(content).flex_1()),
        ),
    ))
}
