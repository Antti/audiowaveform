use crate::error::{elements, poll};
use crate::{ChannelMode, Error, Gain, Options, Resolution, Statistics, Waveform};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Signal {
    pub rate: u32,
    pub channels: usize,
}

enum Peaks {
    Integer(Vec<i16>),
    Unscaled(Vec<f64>),
}

pub(crate) struct Reducer {
    options: Options,
    signal: Signal,
    channels: usize,
    total: Option<u64>,
    scale: u64,
    frames: u64,
    points: u64,
    window_start: u128,
    window_end: u128,
    used: bool,
    current: Vec<f64>,
    peaks: Peaks,
}

impl Reducer {
    pub fn new(options: Options, signal: Signal, total: Option<u64>) -> Result<Self, Error> {
        options.validate()?;
        if signal.rate == 0 || signal.channels == 0 {
            return Err(Error::InvalidAudio("zero sample rate or channel count"));
        }
        let channels = if options.channels == ChannelMode::Mono {
            1
        } else {
            signal.channels
        };
        let (scale, capacity) = match options.resolution {
            Resolution::Points(count) => {
                let frames =
                    total.ok_or(Error::InvalidAudio("exact points require a frame count"))?;
                (
                    (frames / u64::from(count)).max(2),
                    if frames == 0 {
                        0
                    } else {
                        elements(u64::from(count), channels)?
                    },
                )
            }
            Resolution::FramesPerPoint(scale) => (u64::from(scale), 0),
            Resolution::PointsPerSecond(rate) => (u64::from((signal.rate / rate).max(2)), 0),
        };
        let mut current = Vec::new();
        let width = elements(1, channels)?;
        current.try_reserve_exact(width)?;
        current.resize(width, 0.0);
        let peaks = if options.gain == Gain::Normalize {
            let mut data = Vec::new();
            data.try_reserve_exact(capacity)?;
            Peaks::Unscaled(data)
        } else {
            let mut data = Vec::new();
            data.try_reserve_exact(capacity)?;
            Peaks::Integer(data)
        };
        let mut reducer = Self {
            options,
            signal,
            channels,
            total,
            scale,
            frames: 0,
            points: 0,
            window_start: 0,
            window_end: 0,
            used: false,
            current,
            peaks,
        };
        (reducer.window_start, reducer.window_end) = reducer.window();
        Ok(reducer)
    }

    pub(crate) fn allocated_bytes(&self) -> usize {
        self.current.capacity() * size_of::<f64>()
            + match &self.peaks {
                Peaks::Integer(data) => data.capacity() * size_of::<i16>(),
                Peaks::Unscaled(data) => data.capacity() * size_of::<f64>(),
            }
    }

    fn window(&self) -> (u128, u128) {
        let index = u128::from(self.points);
        match self.options.resolution {
            Resolution::Points(count) => {
                let frames = u128::from(self.total.unwrap_or(0));
                let start = index * frames / u128::from(count);
                let end = (index + 1) * frames / u128::from(count);
                (start, end.max(start + 1))
            }
            _ => (
                index * u128::from(self.scale),
                (index + 1) * u128::from(self.scale),
            ),
        }
    }

    pub fn feed(
        &mut self,
        samples: &[f64],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<(), Error> {
        if !samples.len().is_multiple_of(self.signal.channels) {
            return Err(Error::InvalidAudio("incomplete audio frame"));
        }
        for (index, frame) in samples.chunks_exact(self.signal.channels).enumerate() {
            if index.is_multiple_of(1024) {
                poll(cancelled)?;
            }
            if frame.iter().any(|sample| !sample.is_finite()) {
                return Err(Error::InvalidAudio("nonfinite audio sample"));
            }
            if self.total.is_some_and(|total| self.frames >= total) {
                return Err(Error::InputChanged);
            }
            let mono = if self.options.channels == ChannelMode::Mono {
                mean(frame)
            } else {
                0.0
            };
            loop {
                for (channel, &source_sample) in frame.iter().take(self.channels).enumerate() {
                    let sample = if self.options.channels == ChannelMode::Mono {
                        mono
                    } else {
                        source_sample
                    };
                    let pair = &mut self.current[channel * 2..channel * 2 + 2];
                    if self.used {
                        pair[0] = pair[0].min(sample);
                        pair[1] = pair[1].max(sample);
                    } else {
                        pair.copy_from_slice(&[sample, sample]);
                    }
                }
                self.used = true;
                if u128::from(self.frames) + 1 < self.window_end {
                    break;
                }
                self.emit()?;
                if let Resolution::Points(count) = self.options.resolution
                    && self.points == u64::from(count)
                {
                    break;
                }
                if self.window_start > u128::from(self.frames) {
                    break;
                }
                if self.points.is_multiple_of(1024) {
                    poll(cancelled)?;
                }
            }
            self.frames = self.frames.checked_add(1).ok_or(Error::SizeOverflow)?;
        }
        Ok(())
    }

    fn emit(&mut self) -> Result<(), Error> {
        match &mut self.peaks {
            Peaks::Integer(data) => {
                let Gain::Fixed(gain) = self.options.gain else {
                    unreachable!()
                };
                data.try_reserve(self.current.len())?;
                data.extend(
                    self.current
                        .iter()
                        .map(|&sample| (sample * gain * 32768.0) as i16),
                );
            }
            Peaks::Unscaled(data) => {
                data.try_reserve(self.current.len())?;
                data.extend_from_slice(&self.current);
            }
        }
        self.points = self.points.checked_add(1).ok_or(Error::SizeOverflow)?;
        (self.window_start, self.window_end) = self.window();
        self.used = false;
        Ok(())
    }

    pub fn finish(
        mut self,
        mut statistics: Statistics,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Waveform, Error> {
        if self.total.is_some_and(|total| total != self.frames) {
            return Err(Error::InputChanged);
        }
        if self.used {
            self.emit()?;
        }
        let scratch_bytes = self.current.capacity() * size_of::<f64>();
        let (data, bytes) = match self.peaks {
            Peaks::Integer(data) => {
                let bytes = data.capacity() * size_of::<i16>();
                (data, bytes)
            }
            Peaks::Unscaled(unscaled) => {
                let mut maximum = 0.0_f64;
                for chunk in unscaled.chunks(1024) {
                    poll(cancelled)?;
                    maximum = chunk.iter().fold(maximum, |m, sample| m.max(sample.abs()));
                }
                let mut data = Vec::new();
                data.try_reserve_exact(unscaled.len())?;
                for chunk in unscaled.chunks(1024) {
                    poll(cancelled)?;
                    data.extend(chunk.iter().map(|sample| {
                        if maximum == 0.0 {
                            0
                        } else {
                            (sample / maximum * 32767.0) as i16
                        }
                    }));
                }
                let bytes =
                    unscaled.capacity() * size_of::<f64>() + data.capacity() * size_of::<i16>();
                (data, bytes)
            }
        };
        statistics.peak_capacity_bytes = bytes + scratch_bytes;
        Ok(Waveform {
            data,
            sample_rate: self.signal.rate,
            channels: self.channels,
            source_frames: self.frames,
            frames_per_point: self.scale,
            statistics,
        })
    }
}

fn mean(frame: &[f64]) -> f64 {
    let sum: f64 = frame.iter().sum();
    if sum.is_finite() {
        return sum / frame.len() as f64;
    }
    // Finite float PCM can exceed full scale. Keep the mean finite even when
    // its unscaled sum overflows, without clipping the individual channels.
    let maximum = frame.iter().fold(0.0_f64, |m, sample| m.max(sample.abs()));
    let scaled: f64 = frame
        .iter()
        .map(|sample| sample / maximum / frame.len() as f64)
        .sum();
    scaled.clamp(-1.0, 1.0) * maximum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(samples: &[f64], channels: usize, points: u32, block: usize) -> Waveform {
        let frames = (samples.len() / channels) as u64;
        let options = Options {
            resolution: Resolution::Points(points),
            channels: ChannelMode::Split,
            ..Options::default()
        };
        let mut reducer = Reducer::new(
            options,
            Signal {
                rate: 48000,
                channels,
            },
            Some(frames),
        )
        .unwrap();
        for part in samples.chunks(block * channels) {
            reducer.feed(part, &mut || false).unwrap();
        }
        reducer
            .finish(Statistics::default(), &mut || false)
            .unwrap()
    }

    #[test]
    fn boundaries_are_independent_of_block_size_and_match_batch_extrema() {
        for channels in [1, 2, 6] {
            for frames in 1..32 {
                let input: Vec<f64> = (0..frames * channels)
                    .map(|i| ((i * 37 % 97) as f64 - 48.0) / 128.0)
                    .collect();
                for points in [1, 3, 11, 40] {
                    let mut expected = Vec::new();
                    for point in 0..points as usize {
                        let lo = point * frames / points as usize;
                        let hi = ((point + 1) * frames / points as usize).max(lo + 1);
                        for channel in 0..channels {
                            let values: Vec<_> =
                                (lo..hi).map(|i| input[i * channels + channel]).collect();
                            expected.push(
                                (values.iter().copied().fold(f64::INFINITY, f64::min) * 32768.0)
                                    as i16,
                            );
                            expected.push(
                                (values.iter().copied().fold(f64::NEG_INFINITY, f64::max) * 32768.0)
                                    as i16,
                            );
                        }
                    }
                    for block in [1, 2, 7, 128] {
                        assert_eq!(
                            run(&input, channels, points, block).data16(),
                            expected,
                            "channels={channels} frames={frames} points={points} block={block}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn exact_count_reuses_reserved_peak_and_channel_buffers() {
        let mut reducer = Reducer::new(
            Options {
                resolution: Resolution::Points(110),
                ..Options::default()
            },
            Signal {
                rate: 48000,
                channels: 1,
            },
            Some(10000),
        )
        .unwrap();
        let current = reducer.current.as_ptr();
        let Peaks::Integer(data) = &reducer.peaks else {
            panic!()
        };
        let output = data.as_ptr();
        let capacity = data.capacity();
        for _ in 0..100 {
            reducer.feed(&[0.25; 100], &mut || false).unwrap();
        }
        let Peaks::Integer(data) = &reducer.peaks else {
            panic!()
        };
        assert_eq!(data.as_ptr(), output);
        assert_eq!(data.capacity(), capacity);
        assert_eq!(reducer.current.as_ptr(), current);
        assert_eq!(
            reducer
                .finish(Statistics::default(), &mut || false)
                .unwrap()
                .len(),
            110
        );
    }

    #[test]
    fn replay_cannot_silently_drop_or_add_frames() {
        let make = || {
            Reducer::new(
                Options {
                    resolution: Resolution::Points(2),
                    ..Options::default()
                },
                Signal {
                    rate: 8000,
                    channels: 1,
                },
                Some(4),
            )
            .unwrap()
        };
        let mut short = make();
        short.feed(&[0.0; 3], &mut || false).unwrap();
        assert!(matches!(
            short.finish(Statistics::default(), &mut || false),
            Err(Error::InputChanged)
        ));
        assert!(matches!(
            make().feed(&[0.0; 5], &mut || false),
            Err(Error::InputChanged)
        ));
    }

    #[test]
    fn huge_dimensions_fail_without_allocation() {
        assert!(matches!(
            elements(u64::MAX, usize::MAX),
            Err(Error::SizeOverflow)
        ));
        assert!(
            Reducer::new(
                Options {
                    resolution: Resolution::Points(u32::MAX),
                    channels: ChannelMode::Split,
                    ..Options::default()
                },
                Signal {
                    rate: 1,
                    channels: usize::MAX
                },
                Some(1)
            )
            .is_err()
        );
        let mut allocation = Vec::<f64>::new();
        assert!(allocation.try_reserve_exact(usize::MAX).is_err());
    }

    #[test]
    fn finite_float_mixing_handles_overflowing_sums() {
        assert_eq!(mean(&[f64::MAX, f64::MAX]), f64::MAX);
        assert_eq!(mean(&[f64::MAX, -f64::MAX]), 0.0);
        assert_eq!(mean(&[f64::MAX, f64::MAX, -f64::MAX, -f64::MAX]), 0.0);
    }

    #[test]
    fn cancellation_interrupts_repetition_of_one_frame() {
        let mut reducer = Reducer::new(
            Options {
                resolution: Resolution::Points(10000),
                ..Options::default()
            },
            Signal {
                rate: 8000,
                channels: 1,
            },
            Some(1),
        )
        .unwrap();
        let mut calls = 0;
        assert!(matches!(
            reducer.feed(&[0.5], &mut || {
                calls += 1;
                calls == 2
            }),
            Err(Error::Cancelled)
        ));
    }
}
