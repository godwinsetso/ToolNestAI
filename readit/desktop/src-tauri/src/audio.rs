//! Audio output. `DeviceSink` plays through the default sound device
//! (speakers, headphones, Bluetooth) via cpal; `WavSink` records to a file
//! for end-to-end tests and "save as audio".

use crate::player::AudioSink;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::{Condvar, Mutex};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

struct Shared {
    buf: Mutex<VecDeque<f32>>,
    space: Condvar,
    paused: AtomicBool,
    volume: AtomicU32, // f32 bits
    device_rate: AtomicU32,
    /// Incremented by flush, so a write blocked across a flush drops its stale audio.
    epoch: AtomicU32,
}

/// Linear resampler that keeps its phase across chunks so sentence audio stays seamless.
struct Resampler {
    from: u32,
    to: u32,
    pos: f64,
    last: f32,
}

impl Resampler {
    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.from == self.to || self.from == 0 {
            out.extend_from_slice(input);
            return;
        }
        let step = self.from as f64 / self.to as f64;
        // `pos` is measured from the sample before input[0] (self.last).
        while self.pos < input.len() as f64 {
            let i = self.pos.floor() as isize - 1;
            let frac = (self.pos - self.pos.floor()) as f32;
            let a = if i < 0 { self.last } else { input[i as usize] };
            let b = input[(i + 1) as usize];
            out.push(a + (b - a) * frac);
            self.pos += step;
        }
        self.pos -= input.len() as f64;
        if let Some(&l) = input.last() {
            self.last = l;
        }
    }
}

pub struct DeviceSink {
    shared: Arc<Shared>,
    resampler: Mutex<Resampler>,
    started: Mutex<bool>,
}

impl Default for DeviceSink {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                buf: Mutex::new(VecDeque::new()),
                space: Condvar::new(),
                paused: AtomicBool::new(false),
                volume: AtomicU32::new(1.0f32.to_bits()),
                device_rate: AtomicU32::new(0),
                epoch: AtomicU32::new(0),
            }),
            resampler: Mutex::new(Resampler { from: 0, to: 0, pos: 0.0, last: 0.0 }),
            started: Mutex::new(false),
        }
    }
}

impl DeviceSink {
    pub fn set_volume(&self, v: f32) {
        self.shared.volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::SeqCst);
    }

    /// Start the output stream on its own thread (cpal streams are not Send on every platform).
    fn start(&self) -> Result<(), String> {
        let mut started = self.started.lock();
        if *started {
            return Ok(());
        }
        let shared = self.shared.clone();
        let (tx, rx) = std::sync::mpsc::channel::<Result<u32, String>>();
        std::thread::Builder::new()
            .name("readit-audio".into())
            .spawn(move || {
                let host = cpal::default_host();
                let Some(device) = host.default_output_device() else {
                    let _ = tx.send(Err("No sound output was found. Connect speakers or headphones and try again.".into()));
                    return;
                };
                let config = match device.default_output_config() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = tx.send(Err(format!("The sound device could not be used: {e}")));
                        return;
                    }
                };
                let rate = config.sample_rate().0;
                let channels = config.channels() as usize;
                let err_fn = |e| eprintln!("audio stream error: {e}");
                let s = shared.clone();
                macro_rules! build {
                    ($t:ty, $conv:expr) => {
                        device.build_output_stream(
                            &config.clone().into(),
                            move |data: &mut [$t], _| fill(&s, data, channels, $conv),
                            err_fn,
                            None,
                        )
                    };
                }
                let stream = match config.sample_format() {
                    cpal::SampleFormat::F32 => build!(f32, |x: f32| x),
                    cpal::SampleFormat::I16 => build!(i16, |x: f32| (x * i16::MAX as f32) as i16),
                    cpal::SampleFormat::U16 => build!(u16, |x: f32| ((x * 0.5 + 0.5) * u16::MAX as f32) as u16),
                    other => {
                        let _ = tx.send(Err(format!("Unsupported sound format {other:?}")));
                        return;
                    }
                };
                let stream = match stream {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(Err(format!("The sound device could not be opened: {e}")));
                        return;
                    }
                };
                if let Err(e) = stream.play() {
                    let _ = tx.send(Err(format!("The sound device could not start: {e}")));
                    return;
                }
                let _ = tx.send(Ok(rate));
                // Keep the stream alive for the life of the app.
                loop {
                    std::thread::park();
                }
            })
            .map_err(|e| e.to_string())?;
        let rate = rx.recv().map_err(|_| "The audio thread stopped unexpectedly.".to_string())??;
        self.shared.device_rate.store(rate, Ordering::SeqCst);
        *started = true;
        Ok(())
    }
}

fn fill<T: Copy>(s: &Shared, data: &mut [T], channels: usize, conv: impl Fn(f32) -> T) {
    let vol = f32::from_bits(s.volume.load(Ordering::Relaxed));
    let paused = s.paused.load(Ordering::Relaxed);
    let mut buf = s.buf.lock();
    for frame in data.chunks_mut(channels) {
        let v = if paused { 0.0 } else { buf.pop_front().unwrap_or(0.0) * vol };
        let out = conv(v.clamp(-1.0, 1.0));
        for c in frame.iter_mut() {
            *c = out;
        }
    }
    drop(buf);
    s.space.notify_all();
}

impl AudioSink for DeviceSink {
    fn open(&self, sample_rate: u32) -> Result<(), String> {
        self.start()?;
        let to = self.shared.device_rate.load(Ordering::SeqCst);
        *self.resampler.lock() = Resampler { from: sample_rate, to, pos: 0.0, last: 0.0 };
        Ok(())
    }

    fn write(&self, samples: &[f32]) {
        let mut out = Vec::with_capacity(samples.len() * 2);
        self.resampler.lock().push(samples, &mut out);
        let rate = self.shared.device_rate.load(Ordering::SeqCst).max(8000) as usize;
        let limit = rate / 4; // keep about 250 ms queued: pause and skip feel immediate
        let epoch = self.shared.epoch.load(Ordering::SeqCst);
        let mut buf = self.shared.buf.lock();
        while buf.len() > limit {
            self.shared.space.wait_for(&mut buf, std::time::Duration::from_millis(100));
            if self.shared.epoch.load(Ordering::SeqCst) != epoch {
                return; // flushed while waiting: this audio belongs to the old position
            }
        }
        buf.extend(out);
    }

    fn pause(&self) {
        self.shared.paused.store(true, Ordering::SeqCst);
    }

    fn resume(&self) {
        self.shared.paused.store(false, Ordering::SeqCst);
    }

    fn flush(&self) {
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        self.shared.buf.lock().clear();
        self.shared.space.notify_all();
    }
}

/// Collects audio in memory and writes a 16-bit WAV file.
#[derive(Default)]
pub struct WavSink {
    pub samples: Mutex<Vec<f32>>,
    pub rate: AtomicU32,
}

impl AudioSink for WavSink {
    fn open(&self, sample_rate: u32) -> Result<(), String> {
        self.rate.store(sample_rate, Ordering::SeqCst);
        Ok(())
    }
    fn write(&self, s: &[f32]) {
        self.samples.lock().extend_from_slice(s);
    }
    fn pause(&self) {}
    fn resume(&self) {}
    fn flush(&self) {}
}

impl WavSink {
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        let s = self.samples.lock();
        let rate = self.rate.load(Ordering::SeqCst);
        let mut out = Vec::with_capacity(44 + s.len() * 2);
        let data_len = (s.len() * 2) as u32;
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for v in s.iter() {
            out.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        std::fs::write(path, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_is_continuous_across_chunks() {
        // A 24 kHz ramp resampled to 48 kHz in two chunks must equal one pass.
        let input: Vec<f32> = (0..1000).map(|i| i as f32 / 1000.0).collect();
        let mut whole = Vec::new();
        Resampler { from: 24000, to: 48000, pos: 0.0, last: 0.0 }.push(&input, &mut whole);
        let mut split = Vec::new();
        let mut r = Resampler { from: 24000, to: 48000, pos: 0.0, last: 0.0 };
        r.push(&input[..333], &mut split);
        r.push(&input[333..], &mut split);
        assert_eq!(whole.len(), split.len());
        assert!(whole.iter().zip(&split).all(|(a, b)| (a - b).abs() < 1e-6));
        assert!((whole.len() as i64 - 2000).abs() <= 2);
    }
}
