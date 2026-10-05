//! The microphone, as 16 kHz mono chunks.

use std::sync::mpsc::Sender;

use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::listen::RATE;

/// Turns any rate into 16 kHz by linear interpolation; enough for speech.
struct Resampler {
    step: f64,
    position: f64,
    last: f32,
}

impl Resampler {
    fn new(from: u32) -> Self {
        Self {
            step: f64::from(from) / RATE as f64,
            position: 0.0,
            last: 0.0,
        }
    }

    fn run(&mut self, input: &[f32], out: &mut Vec<f32>) {
        // `position` is relative to the sample before `input` (`last`, at -1).
        while self.position < input.len() as f64 {
            let i = self.position.floor() as isize - 1;
            let frac = (self.position - self.position.floor()) as f32;
            let a = if i < 0 { self.last } else { input[i as usize] };
            let b = input[(i + 1) as usize];
            out.push(a + (b - a) * frac);
            self.position += self.step;
        }
        self.position -= input.len() as f64;
        if let Some(last) = input.last() {
            self.last = *last;
        }
    }
}

/// Opens the microphone (the first input whose name contains `name`, or the default) and sends
/// 16 kHz mono audio. The stream records while it's kept alive.
pub fn open(name: Option<&str>, chunks: Sender<Vec<f32>>) -> anyhow::Result<cpal::Stream> {
    let host = cpal::default_host();
    let device = match name {
        Some(name) => host
            .input_devices()?
            .find(|d| {
                d.name()
                    .is_ok_and(|n| n.to_lowercase().contains(&name.to_lowercase()))
            })
            .with_context(|| format!("no microphone named {name:?}"))?,
        None => host.default_input_device().context("no microphone")?,
    };
    let config = device.default_input_config()?;
    tracing::info!(
        "microphone: {} at {} Hz, {} channel(s)",
        device.name().unwrap_or_default(),
        config.sample_rate().0,
        config.channels()
    );
    let channels = usize::from(config.channels());
    let mut resampler = Resampler::new(config.sample_rate().0);
    let mut mono = Vec::new();
    let mut handle = move |samples: &mut dyn Iterator<Item = f32>| {
        mono.clear();
        let all: Vec<f32> = samples.collect();
        mono.extend(
            all.chunks(channels)
                .map(|c| c.iter().sum::<f32>() / c.len() as f32),
        );
        let mut out = Vec::with_capacity(mono.len());
        resampler.run(&mono, &mut out);
        let _ = chunks.send(out);
    };
    let error = |e| tracing::warn!("microphone: {e}");
    let stream_config = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| handle(&mut data.iter().copied()),
            error,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| handle(&mut data.iter().map(|s| *s as f32 / 32768.0)),
            error,
            None,
        )?,
        cpal::SampleFormat::I32 => device.build_input_stream(
            &stream_config,
            move |data: &[i32], _| handle(&mut data.iter().map(|s| *s as f32 / 2_147_483_648.0)),
            error,
            None,
        )?,
        format => anyhow::bail!("unsupported microphone format {format:?}"),
    };
    stream.play()?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_length_and_shape_across_chunks() {
        let mut r = Resampler::new(48_000);
        let input: Vec<f32> = (0..48_000)
            .map(|i| (i as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU).sin())
            .collect();
        let mut out = Vec::new();
        for chunk in input.chunks(1_000) {
            r.run(chunk, &mut out);
        }
        assert!((15_990..=16_010).contains(&out.len()), "{}", out.len());
        // Every third input sample, roughly.
        assert!((out[300] - input[900]).abs() < 0.05);
        let mut same = Resampler::new(16_000);
        let mut copy = Vec::new();
        same.run(&[0.1, 0.2, 0.3], &mut copy);
        same.run(&[0.4], &mut copy);
        assert_eq!(copy.len(), 4);
    }
}
