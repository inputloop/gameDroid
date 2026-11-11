// SPDX-License-Identifier: GPL-3.0-or-later
//! The host's speakers and microphone, behind Android's sound card.
//!
//! Android's card hands over a period of sound at a time, at its own pace
//! and in its own format; the host's devices ask for sound when they want
//! it, in theirs. Between the two sits a short queue of samples, and a
//! converter that changes the rate and the number of channels as sound
//! leaves the queue.
//!
//! The microphone is opened only while Android is recording, and closed
//! when it stops.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

use bvmm::virtio::snd::SoundBackend;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Samples waiting to cross, with the format they are in.
#[derive(Default)]
pub struct Queue {
    samples: VecDeque<i16>,
    rate: u32,
    channels: usize,
    /// How far into the first frame the reader is, as a fraction.
    position: f64,
}

/// The most sound kept waiting: more is dropped, oldest first, so that
/// what is heard never lags far behind.
const MAX_QUEUED_MS: usize = 250;

impl Queue {
    fn reset(&mut self, rate: u32, channels: usize) {
        *self = Queue {
            rate,
            channels: channels.max(1),
            ..Default::default()
        };
    }

    fn push(&mut self, samples: impl Iterator<Item = i16>) {
        if self.channels == 0 {
            return;
        }
        self.samples.extend(samples);
        let most = self.rate as usize * self.channels * MAX_QUEUED_MS / 1000;
        if self.samples.len() > most {
            let drop = (self.samples.len() - most) / self.channels * self.channels;
            self.samples.drain(..drop);
        }
    }

    /// Take sound out as `frames` frames of `channels` channels at `rate`,
    /// giving each sample (frame, channel, value from -1 to 1) to `put`.
    /// Where the queue runs dry the rest is silence.
    pub fn pull(
        &mut self,
        rate: u32,
        channels: usize,
        frames: usize,
        mut put: impl FnMut(usize, usize, f32),
    ) {
        let have = self.samples.len().checked_div(self.channels).unwrap_or(0);
        let step = if rate == 0 {
            1.0
        } else {
            self.rate as f64 / rate as f64
        };
        let at = |frame: usize, channel: usize| -> f32 {
            // One channel into many: the same in each. Many into one: the mean.
            let value =
                |c: usize| self.samples[frame * self.channels + c.min(self.channels - 1)] as f32;
            if channels == 1 && self.channels > 1 {
                (0..self.channels).map(value).sum::<f32>() / self.channels as f32 / 32768.0
            } else {
                value(channel) / 32768.0
            }
        };
        let mut position = self.position;
        for frame in 0..frames {
            let whole = position as usize;
            // Two frames are needed to draw the line between.
            if whole + 1 >= have {
                for f in frame..frames {
                    for c in 0..channels {
                        put(f, c, 0.0);
                    }
                }
                break;
            }
            let between = (position - whole as f64) as f32;
            for c in 0..channels {
                let (a, b) = (at(whole, c), at(whole + 1, c));
                put(frame, c, a + (b - a) * between);
            }
            position += step;
        }
        let used = (position as usize).min(have.saturating_sub(1));
        self.samples.drain(..used * self.channels);
        self.position = position - used as f64;
        if have < 2 {
            self.position = 0.0;
        }
    }
}

enum Command {
    Play,
    Record,
    StopRecording,
}

/// The sound backend for Android's card. Cheap: the devices themselves are
/// on a thread of their own.
pub struct HostSound {
    to_speakers: Arc<Mutex<Queue>>,
    from_microphone: Arc<Mutex<Queue>>,
    commands: Sender<Command>,
    /// What Android asked to record in.
    recording: (u32, usize),
}

impl HostSound {
    /// `microphone`: part of the name of the microphone to use; the
    /// system's default one if none is given or none matches.
    pub fn new(microphone: Option<String>) -> HostSound {
        let to_speakers = Arc::new(Mutex::new(Queue::default()));
        let from_microphone = Arc::new(Mutex::new(Queue::default()));
        let (commands, rx) = std::sync::mpsc::channel();
        let (speakers, mic) = (to_speakers.clone(), from_microphone.clone());
        std::thread::spawn(move || devices(rx, speakers, mic, microphone));
        HostSound {
            to_speakers,
            from_microphone,
            commands,
            recording: (48000, 1),
        }
    }
}

impl SoundBackend for HostSound {
    fn playback_started(&mut self, rate: u32, channels: u8) {
        self.to_speakers
            .lock()
            .unwrap()
            .reset(rate, channels as usize);
        let _ = self.commands.send(Command::Play);
    }

    fn play(&mut self, samples: &[u8]) {
        let samples = samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b));
        self.to_speakers.lock().unwrap().push(samples);
    }

    fn capture_started(&mut self, rate: u32, channels: u8) {
        self.recording = (rate, channels.max(1) as usize);
        let _ = self.commands.send(Command::Record);
    }

    fn record(&mut self, out: &mut [u8]) {
        let (rate, channels) = self.recording;
        let frames = out.len() / (channels * 2);
        self.from_microphone.lock().unwrap().pull(
            rate,
            channels,
            frames,
            |frame, channel, value| {
                let at = (frame * channels + channel) * 2;
                let sample = (value * 32767.0).clamp(-32768.0, 32767.0) as i16;
                out[at..at + 2].copy_from_slice(&sample.to_le_bytes());
            },
        );
    }

    fn capture_stopped(&mut self) {
        let _ = self.commands.send(Command::StopRecording);
    }
}

/// The names of the host's microphones, the default one first.
pub fn microphones() -> Vec<String> {
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| d.name().ok());
    let mut names: Vec<String> = host
        .input_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    if let Some(default) = default {
        names.retain(|n| *n != default);
        names.insert(0, default);
    }
    names
}

fn complain(what: &str) -> impl FnMut(cpal::StreamError) + Send + 'static {
    let what = what.to_string();
    move |e| eprintln!("gameDroid: {what}: {e}")
}

/// Open the default speakers, fed from `queue`.
fn open_speakers(queue: Arc<Mutex<Queue>>) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("this computer has no sound output")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config: cpal::StreamConfig = supported.config();
    let (rate, channels) = (config.sample_rate.0, config.channels as usize);
    fn build<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        queue: Arc<Mutex<Queue>>,
        rate: u32,
        channels: usize,
    ) -> Result<cpal::Stream, String>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        device
            .build_output_stream(
                config,
                move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let frames = data.len() / channels;
                    queue
                        .lock()
                        .unwrap()
                        .pull(rate, channels, frames, |frame, channel, value| {
                            data[frame * channels + channel] = T::from_sample(value);
                        });
                },
                complain("sound output"),
                None,
            )
            .map_err(|e| e.to_string())
    }
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, queue, rate, channels),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, queue, rate, channels),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, queue, rate, channels),
        other => Err(format!(
            "the sound output wants a sample format gameDroid does not speak ({other})"
        )),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    eprintln!(
        "gameDroid: sound goes to {} ({rate} Hz, {channels} channel(s))",
        device
            .name()
            .unwrap_or_else(|_| "the default output".into())
    );
    Ok(stream)
}

/// Open a microphone, feeding `queue`.
fn open_microphone(queue: Arc<Mutex<Queue>>, wanted: Option<&str>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let named = wanted.and_then(|part| {
        let part = part.to_lowercase();
        host.input_devices()
            .ok()?
            .find(|d| d.name().is_ok_and(|n| n.to_lowercase().contains(&part)))
    });
    if wanted.is_some() && named.is_none() {
        eprintln!("gameDroid: no microphone is named like that; using the default one");
    }
    let device = named
        .or_else(|| host.default_input_device())
        .ok_or("this computer has no microphone")?;
    let supported = device.default_input_config().map_err(|e| e.to_string())?;
    let config: cpal::StreamConfig = supported.config();
    let (rate, channels) = (config.sample_rate.0, config.channels as usize);
    queue.lock().unwrap().reset(rate, channels);
    fn build<T>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        queue: Arc<Mutex<Queue>>,
    ) -> Result<cpal::Stream, String>
    where
        T: cpal::SizedSample,
        i16: cpal::FromSample<T>,
    {
        use cpal::Sample;
        device
            .build_input_stream(
                config,
                move |data: &[T], _: &cpal::InputCallbackInfo| {
                    queue
                        .lock()
                        .unwrap()
                        .push(data.iter().map(|s| i16::from_sample(*s)));
                },
                complain("the microphone"),
                None,
            )
            .map_err(|e| e.to_string())
    }
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, queue),
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, queue),
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, queue),
        other => Err(format!(
            "the microphone gives a sample format gameDroid does not speak ({other})"
        )),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    eprintln!(
        "gameDroid: Android is listening to {} ({rate} Hz, {channels} channel(s))",
        device
            .name()
            .unwrap_or_else(|_| "the default microphone".into())
    );
    Ok(stream)
}

/// The thread that owns the host's devices (they may not move between
/// threads). Trouble is said once and then lived with: Android goes on,
/// unheard or hearing silence.
fn devices(
    commands: Receiver<Command>,
    speakers: Arc<Mutex<Queue>>,
    mic: Arc<Mutex<Queue>>,
    wanted: Option<String>,
) {
    let mut output: Option<cpal::Stream> = None;
    let mut input: Option<cpal::Stream> = None;
    let (mut output_failed, mut input_failed) = (false, false);
    for command in commands {
        match command {
            Command::Play if output.is_none() && !output_failed => {
                match open_speakers(speakers.clone()) {
                    Ok(stream) => output = Some(stream),
                    Err(e) => {
                        output_failed = true;
                        eprintln!("gameDroid: no sound will be heard: {e}");
                    }
                }
            }
            Command::Record if input.is_none() && !input_failed => {
                match open_microphone(mic.clone(), wanted.as_deref()) {
                    Ok(stream) => input = Some(stream),
                    Err(e) => {
                        input_failed = true;
                        eprintln!("gameDroid: Android will record silence: {e}");
                    }
                }
            }
            Command::StopRecording => {
                if input.take().is_some() {
                    eprintln!("gameDroid: the microphone is closed");
                }
                mic.lock().unwrap().samples.clear();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull(q: &mut Queue, rate: u32, channels: usize, frames: usize) -> Vec<f32> {
        let mut out = vec![9.0; frames * channels];
        q.pull(rate, channels, frames, |f, c, v| out[f * channels + c] = v);
        out
    }

    #[test]
    fn the_same_format_passes_straight_through() {
        let mut q = Queue::default();
        q.reset(48000, 2);
        q.push([100i16, -100, 200, -200, 300, -300, 400, -400].into_iter());
        let out = pull(&mut q, 48000, 2, 3);
        let expect: Vec<f32> = [100, -100, 200, -200, 300, -300]
            .iter()
            .map(|v| *v as f32 / 32768.0)
            .collect();
        assert_eq!(out, expect);
        // The last frame stays, to draw the line to whatever comes next.
        assert_eq!(q.samples.len(), 2);
    }

    #[test]
    fn rates_and_channels_are_converted() {
        // 24 kHz mono into 48 kHz stereo: every sample twice, with the
        // halfway points in between, the same in both ears.
        let mut q = Queue::default();
        q.reset(24000, 1);
        q.push([0i16, 16384, 0].into_iter());
        let out = pull(&mut q, 48000, 2, 4);
        assert_eq!(out, vec![0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.25, 0.25]);
        // Stereo into mono: the mean of the two.
        let mut q = Queue::default();
        q.reset(48000, 2);
        q.push([16384i16, 0, 16384, 0, 0, 0].into_iter());
        assert_eq!(pull(&mut q, 48000, 1, 2), vec![0.25, 0.25]);
    }

    #[test]
    fn a_dry_queue_gives_silence_and_a_flooded_one_drops_the_oldest() {
        let mut q = Queue::default();
        assert_eq!(pull(&mut q, 48000, 2, 2), vec![0.0; 4], "before any sound");
        q.reset(8000, 1);
        q.push([1000i16, 1000].into_iter());
        let out = pull(&mut q, 8000, 1, 4);
        assert!(out[0] > 0.0 && out[1..] == [0.0, 0.0, 0.0], "{out:?}");
        // A second of sound into a queue that keeps a quarter of one.
        q.reset(8000, 1);
        q.push((0..8000).map(|i| i as i16));
        assert_eq!(q.samples.len(), 2000);
        assert_eq!(q.samples[0], 6000, "the oldest went");
    }

    #[test]
    fn the_backend_carries_android_sound_into_the_queue_and_back() {
        let mut host = HostSound::new(None);
        host.playback_started(16000, 1);
        host.play(&[0x34, 0x12, 0x78, 0x56]);
        assert_eq!(host.to_speakers.lock().unwrap().samples, [0x1234, 0x5678]);
        // Recording asks the queue in Android's format; dry, it is silence.
        host.recording = (16000, 1);
        let mut out = [0xffu8; 8];
        host.record(&mut out);
        assert_eq!(out, [0; 8]);
        host.from_microphone.lock().unwrap().reset(16000, 1);
        host.from_microphone
            .lock()
            .unwrap()
            .push([16384i16, 16384, 16384].into_iter());
        host.record(&mut out[..4]);
        assert_eq!(i16::from_le_bytes([out[0], out[1]]), 16383);
    }
}
