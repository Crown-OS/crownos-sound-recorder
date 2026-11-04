use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use pipewire as pw;
use pw::properties::properties;
use pw::spa::param::ParamType;
use pw::spa::param::audio::{AudioFormat, AudioInfoRaw};
use pw::spa::pod::serialize::PodSerializer;
use pw::spa::pod::{Object, Pod, Value};
use pw::spa::utils::{Direction, SpaTypes};
use pw::stream::{StreamFlags, StreamRc};
use rtrb::Consumer;

use super::pcm_chunk::{PcmChunk, PlaybackClock};
use crate::encode::SAMPLE_RATE;

const STREAM_NAME: &str = "crownos-sound-recorder";
const SAMPLE_BYTES: usize = size_of::<f32>();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamCommand {
    Resume,
    Pause,
    Quit,
}

/// A mono PipeWire playback stream fed from a lock-free chunk ring.
pub struct OutputStream {
    commands: pw::channel::Sender<StreamCommand>,
    thread: Option<JoinHandle<()>>,
}

struct RealtimeState {
    chunks: Consumer<PcmChunk>,
    clock: Arc<PlaybackClock>,
    offset: usize,
}

impl OutputStream {
    pub fn start(chunks: Consumer<PcmChunk>, clock: Arc<PlaybackClock>) -> Result<Self> {
        let (commands, command_receiver) = pw::channel::channel();
        let (ready_sender, ready) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("pipewire-playback".into())
            .spawn(move || {
                let state = RealtimeState {
                    chunks,
                    clock,
                    offset: 0,
                };
                if let Err(error) = run_loop(state, command_receiver, &ready_sender) {
                    let _ = ready_sender.send(Err(error));
                }
            })?;
        ready
            .recv()
            .context("PipeWire thread exited")?
            .context("opening PipeWire playback")?;
        Ok(Self {
            commands,
            thread: Some(thread),
        })
    }

    pub fn resume(&self) {
        let _ = self.commands.send(StreamCommand::Resume);
    }

    pub fn pause(&self) {
        let _ = self.commands.send(StreamCommand::Pause);
    }
}

impl Drop for OutputStream {
    fn drop(&mut self) {
        let _ = self.commands.send(StreamCommand::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_loop(
    state: RealtimeState,
    commands: pw::channel::Receiver<StreamCommand>,
    ready: &mpsc::SyncSender<Result<(), pw::Error>>,
) -> Result<(), pw::Error> {
    pw::init();
    let main_loop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&main_loop, None)?;
    let core = context.connect_rc(None)?;
    let stream = StreamRc::new(
        core,
        STREAM_NAME,
        properties! {
            *pw::keys::MEDIA_TYPE => "Audio",
            *pw::keys::MEDIA_CATEGORY => "Playback",
            *pw::keys::MEDIA_ROLE => "Music",
            *pw::keys::NODE_NAME => STREAM_NAME,
        },
    )?;
    let _listener = stream
        .add_local_listener_with_user_data(state)
        .process(play_chunks)
        .register()?;
    let format = format_pod()?;
    let mut parameters = [Pod::from_bytes(&format).ok_or(pw::Error::CreationFailed)?];
    stream.connect(
        Direction::Output,
        None,
        StreamFlags::AUTOCONNECT
            | StreamFlags::MAP_BUFFERS
            | StreamFlags::RT_PROCESS
            | StreamFlags::INACTIVE,
        &mut parameters,
    )?;
    let _commands = commands.attach(main_loop.loop_(), {
        let main_loop = main_loop.clone();
        let stream = stream.clone();
        move |command| match command {
            StreamCommand::Resume => {
                let _ = stream.set_active(true);
            }
            StreamCommand::Pause => {
                let _ = stream.set_active(false);
            }
            StreamCommand::Quit => main_loop.quit(),
        }
    });
    let _ = ready.send(Ok(()));
    main_loop.run();
    Ok(())
}

fn play_chunks(stream: &pw::stream::Stream, state: &mut RealtimeState) {
    let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
    };
    let requested = usize::try_from(buffer.requested()).unwrap_or_default();
    let Some(data) = buffer.datas_mut().first_mut() else {
        return;
    };
    let frames = data.data().map_or(0, |bytes| {
        let (samples, _) = bytes.as_chunks_mut::<SAMPLE_BYTES>();
        let frames = match requested {
            0 => samples.len(),
            requested => requested.min(samples.len()),
        };
        state.fill(&mut samples[..frames]);
        frames
    });
    let chunk = data.chunk_mut();
    *chunk.offset_mut() = 0;
    *chunk.stride_mut() = SAMPLE_BYTES as i32;
    *chunk.size_mut() = u32::try_from(frames * SAMPLE_BYTES).unwrap_or_default();
}

impl RealtimeState {
    fn fill(&mut self, output: &mut [[u8; SAMPLE_BYTES]]) {
        let epoch = self.clock.epoch.load(Ordering::Acquire);
        let mut filled = 0;
        while filled < output.len() {
            let Ok(chunk) = self.chunks.peek() else {
                break;
            };
            if chunk.epoch != epoch {
                self.discard_front();
                continue;
            }
            let available = &chunk.samples[self.offset..chunk.len];
            let count = available.len().min(output.len() - filled);
            output[filled..filled + count]
                .iter_mut()
                .zip(available)
                .for_each(|(target, sample)| *target = sample.to_le_bytes());
            filled += count;
            self.offset += count;
            let position = chunk.start + self.offset as u64;
            let finished = self.offset == chunk.len;
            self.clock.position.store(position, Ordering::Relaxed);
            if finished {
                self.discard_front();
            }
        }
        output[filled..].fill([0; SAMPLE_BYTES]);
    }

    fn discard_front(&mut self) {
        if let Ok(front) = self.chunks.read_chunk(1) {
            front.commit_all();
        }
        self.offset = 0;
    }
}

fn format_pod() -> Result<Vec<u8>, pw::Error> {
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_rate(SAMPLE_RATE);
    info.set_channels(1);
    let mut position = [0; pw::spa::sys::SPA_AUDIO_MAX_CHANNELS as usize];
    position[0] = pw::spa::sys::SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let object = Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    PodSerializer::serialize(Cursor::new(Vec::new()), &Value::Object(object))
        .map(|(cursor, _)| cursor.into_inner())
        .map_err(|_| pw::Error::CreationFailed)
}
