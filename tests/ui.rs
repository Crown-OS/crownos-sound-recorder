use std::path::Path;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use crownos_sound_recorder::app::shell;
use crownos_sound_recorder::encode::RecordingFormat;
use crownos_sound_recorder::recording::{
    Library, PlaybackState, Player, RecordingSession, ToneGenerator,
};
use crownos_sound_recorder::settings::{Persistence, Preferences};
use crownos_sound_recorder::state::{AppState, Capture, Environment};
use crownos_sound_recorder::util::Trash;
use crowntest::{Role, TestApp, role};
use crownui::kit::{Theme, provide_theme};
use crownui::prelude::component;

const WIDTH: f32 = 980.0;
const HEIGHT: f32 = 700.0;

fn library_in(directory: &Path) -> Result<Library> {
    Library::open(
        directory.join("Recordings"),
        Trash::at(directory.join("Trash")),
    )
}

fn record(library: &Library, title: &str, seconds: f32) -> Result<()> {
    let path = library.root().join(format!("{title}.ogg"));
    let tone = ToneGenerator::with_seconds(220.0, seconds);
    RecordingSession::start(path, RecordingFormat::Opus, tone)?.stop()?;
    Ok(())
}

fn window_with(library: Library, capture: Capture) -> TestApp {
    let environment = Environment {
        library,
        preferences: Preferences::default(),
        capture,
        persistence: Persistence::Memory,
    };
    TestApp::with_size(
        component(move |cx| {
            provide_theme(cx, Theme::dark());
            let state = AppState::new(cx, environment);
            shell(cx, state)
        }),
        WIDTH,
        HEIGHT,
    )
}

fn window(library: Library) -> TestApp {
    window_with(library, Capture::Tone)
}

#[test]
fn the_library_lists_recordings_newest_first() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let library = library_in(directory.path())?;
    record(&library, "Older memo", 0.5)?;
    thread::sleep(Duration::from_millis(20));
    record(&library, "Newer memo", 0.5)?;

    let mut app = window(library);
    assert_eq!(app.count(role(Role::ListItem)), 2);
    let older = app.find(role(Role::ListItem).name("Older memo"));
    let newer = app.find(role(Role::ListItem).name("Newer memo"));
    assert!(newer.bounds.origin.y < older.bounds.origin.y);
    assert!(newer.value.is_some_and(|detail| detail.ends_with("0:00")));
    Ok(())
}

#[test]
fn selecting_a_recording_shows_its_player() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let library = library_in(directory.path())?;
    record(&library, "Interview", 1.0)?;

    let mut app = window(library);
    assert!(app.exists(role(Role::Button).name("Record")));
    app.click(role(Role::ListItem).name("Interview"));
    app.settle();
    assert!(app.exists(role(Role::Group).name("Interview")));
    assert!(app.exists(role(Role::Button).name("Play")));
    assert!(app.exists(role(Role::Slider).name("Playback position")));
    assert!(!app.exists(role(Role::Button).name("Record")));

    app.click(role(Role::MenuItem).name("New Recording"));
    app.settle();
    assert!(app.exists(role(Role::Button).name("Record")));
    Ok(())
}

#[test]
fn the_record_button_names_what_it_does_next() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let mut app = window(library_in(directory.path())?);
    let stop = || role(Role::Button).name("Stop");
    assert!(app.find(stop()).states.disabled);

    app.click(role(Role::Button).name("Record"));
    assert!(app.exists(role(Role::Button).name("Pause")));
    assert!(!app.find(stop()).states.disabled);
    app.click(role(Role::Button).name("Pause"));
    assert!(app.exists(role(Role::Button).name("Resume")));
    app.press_key("Space");
    assert!(app.exists(role(Role::Button).name("Pause")));

    app.press_key("Escape");
    app.settle();
    assert_eq!(app.count(role(Role::ListItem)), 1);
    assert!(app.exists(role(Role::Button).name("Play")));
    Ok(())
}

#[test]
fn renaming_and_deleting_update_the_library() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let library = library_in(directory.path())?;
    record(&library, "Draft", 0.5)?;

    let mut app = window(library);
    app.click(role(Role::ListItem).name("Draft"));
    app.click(role(Role::Button).name("Rename"));
    app.click(role(Role::TextField).name("Title"));
    app.press_key("Ctrl+A");
    app.type_text("Final");
    app.press_key("Enter");
    app.settle();
    assert!(app.exists(role(Role::ListItem).name("Final")));
    assert!(app.exists(role(Role::Group).name("Final")));

    app.click(role(Role::Button).name("Delete"));
    app.click(role(Role::Button).name("Move to Trash"));
    app.settle();
    assert_eq!(app.count(role(Role::ListItem)), 0);
    assert!(app.exists(role(Role::Button).name("Record")));
    assert!(directory.path().join("Trash/files/Final.ogg").exists());
    Ok(())
}

#[test]
#[ignore = "needs PipeWire and a microphone"]
fn records_three_seconds_from_the_microphone_and_plays_them_back() -> Result<()> {
    const TAKE: Duration = Duration::from_secs(3);
    let directory = tempfile::tempdir()?;
    let library = library_in(directory.path())?;
    let root = library.root().to_path_buf();
    let mut app = window_with(library, Capture::Microphone);
    thread::sleep(Duration::from_millis(500));

    app.click(role(Role::Button).name("Record"));
    thread::sleep(TAKE);
    app.click(role(Role::Button).name("Stop"));
    app.settle();
    assert!(app.exists(role(Role::Button).name("Play")));

    let recording = Library::open(&root, Trash::at(directory.path().join("Trash")))?
        .list()?
        .remove(0);
    assert!(
        recording.duration.abs_diff(TAKE) < Duration::from_millis(300),
        "{:?}",
        recording.duration
    );
    let player = Player::open(&recording.path)?;
    player.play();
    let deadline = std::time::Instant::now() + TAKE * 2;
    while player.state() != PlaybackState::Ended && std::time::Instant::now() < deadline {
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(player.state(), PlaybackState::Ended);
    Ok(())
}
