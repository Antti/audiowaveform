//! Headerless, interleaved PCM input. Chunk boundaries need not align to samples.

use std::io::{ErrorKind, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::str::FromStr;

use crate::error::poll;
use crate::reduce::{Reducer, Signal};
use crate::{Error, Options, Resolution, Statistics, Waveform};

/// PCM encodings named after FFmpeg's raw audio formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmFormat {
    U8,
    S8,
    S16Le,
    S16Be,
    S24Le,
    S24Be,
    S32Le,
    S32Be,
    F32Le,
    F32Be,
    F64Le,
    F64Be,
}

impl FromStr for PcmFormat {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(match value {
            "u8" => Self::U8,
            "s8" => Self::S8,
            "s16le" => Self::S16Le,
            "s16be" => Self::S16Be,
            "s24le" => Self::S24Le,
            "s24be" => Self::S24Be,
            "s32le" => Self::S32Le,
            "s32be" => Self::S32Be,
            "f32le" => Self::F32Le,
            "f32be" => Self::F32Be,
            "f64le" => Self::F64Le,
            "f64be" => Self::F64Be,
            _ => return Err(Error::InvalidOption("unsupported PCM format")),
        })
    }
}

impl PcmFormat {
    pub fn bytes_per_sample(self) -> usize {
        match self {
            Self::U8 | Self::S8 => 1,
            Self::S16Le | Self::S16Be => 2,
            Self::S24Le | Self::S24Be => 3,
            Self::S32Le | Self::S32Be | Self::F32Le | Self::F32Be => 4,
            Self::F64Le | Self::F64Be => 8,
        }
    }

    fn decode(self, bytes: &[u8]) -> f64 {
        match self {
            Self::U8 => (f64::from(bytes[0]) - 128.0) / 128.0,
            Self::S8 => f64::from(bytes[0] as i8) / 128.0,
            Self::S16Le => f64::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32768.0,
            Self::S16Be => f64::from(i16::from_be_bytes(bytes.try_into().unwrap())) / 32768.0,
            Self::S24Le => {
                f64::from(i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]])) / 2147483648.0
            }
            Self::S24Be => {
                f64::from(i32::from_be_bytes([bytes[0], bytes[1], bytes[2], 0])) / 2147483648.0
            }
            Self::S32Le => f64::from(i32::from_le_bytes(bytes.try_into().unwrap())) / 2147483648.0,
            Self::S32Be => f64::from(i32::from_be_bytes(bytes.try_into().unwrap())) / 2147483648.0,
            Self::F32Le => f64::from(f32::from_le_bytes(bytes.try_into().unwrap())),
            Self::F32Be => f64::from(f32::from_be_bytes(bytes.try_into().unwrap())),
            Self::F64Le => f64::from_le_bytes(bytes.try_into().unwrap()),
            Self::F64Be => f64::from_be_bytes(bytes.try_into().unwrap()),
        }
    }
}

/// One-pass PCM accumulator. Retains a fixed decoding buffer plus output peaks.
///
/// Exact point counts require knowing the full input length, so `Points` is
/// rejected. Use `FramesPerPoint` or `PointsPerSecond`. After a failed push the
/// stream cannot be reused. `finish` rejects incomplete samples/channel frames.
pub struct PcmStream {
    format: PcmFormat,
    channels: usize,
    reducer: Reducer,
    samples: Vec<f64>,
    block_samples: usize,
    pending: [u8; 8],
    pending_len: usize,
    failed: bool,
}

impl PcmStream {
    pub fn new(
        format: PcmFormat,
        sample_rate: u32,
        channels: u16,
        options: Options,
    ) -> Result<Self, Error> {
        options.validate()?;
        if sample_rate == 0 || channels == 0 {
            return Err(Error::InvalidOption(
                "PCM sample rate and channels must be positive",
            ));
        }
        if matches!(options.resolution, Resolution::Points(_)) {
            return Err(Error::InvalidOption(
                "exact points are unavailable for PCM streams",
            ));
        }
        let channels = usize::from(channels);
        let reducer = Reducer::new(
            options,
            Signal {
                rate: sample_rate,
                channels,
            },
            None,
        )?;
        let block_samples = (4096 / channels).max(1) * channels;
        let mut samples = Vec::new();
        samples.try_reserve_exact(block_samples)?;
        Ok(Self {
            format,
            channels,
            reducer,
            samples,
            block_samples,
            pending: [0; 8],
            pending_len: 0,
            failed: false,
        })
    }

    /// Heap bytes currently owned, including the growing output peaks.
    pub fn allocated_bytes(&self) -> usize {
        self.samples.capacity() * size_of::<f64>() + self.reducer.allocated_bytes()
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.push_with_cancel(bytes, || false)
    }

    pub fn push_with_cancel(
        &mut self,
        bytes: &[u8],
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<(), Error> {
        if self.failed {
            return Err(Error::InvalidAudio("PCM stream has failed"));
        }
        self.failed = true;
        let result = catch_unwind(AssertUnwindSafe(|| self.push_inner(bytes, &mut cancelled)))
            .map_err(|_| Error::InternalPanic)?;
        self.failed = result.is_err();
        result
    }

    fn push_inner(
        &mut self,
        mut bytes: &[u8],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<(), Error> {
        poll(cancelled)?;
        let width = self.format.bytes_per_sample();
        while !bytes.is_empty() {
            let sample = if self.pending_len > 0 || bytes.len() < width {
                let count = (width - self.pending_len).min(bytes.len());
                self.pending[self.pending_len..self.pending_len + count]
                    .copy_from_slice(&bytes[..count]);
                self.pending_len += count;
                bytes = &bytes[count..];
                if self.pending_len < width {
                    break;
                }
                self.pending_len = 0;
                self.format.decode(&self.pending[..width])
            } else {
                let sample = self.format.decode(&bytes[..width]);
                bytes = &bytes[width..];
                sample
            };
            if !sample.is_finite() {
                return Err(Error::InvalidAudio("nonfinite audio sample"));
            }
            self.samples.push(sample);
            if self.samples.len() == self.block_samples {
                self.reducer.feed(&self.samples, cancelled)?;
                self.samples.clear();
            }
        }
        poll(cancelled)
    }

    pub fn finish(self) -> Result<Waveform, Error> {
        self.finish_with_cancel(|| false)
    }

    pub fn finish_with_cancel(
        mut self,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Waveform, Error> {
        catch_unwind(AssertUnwindSafe(|| {
            poll(&mut cancelled)?;
            if self.failed {
                return Err(Error::InvalidAudio("PCM stream has failed"));
            }
            if self.pending_len != 0 || !self.samples.len().is_multiple_of(self.channels) {
                return Err(Error::InvalidAudio(
                    "incomplete PCM sample or channel frame",
                ));
            }
            self.reducer.feed(&self.samples, &mut cancelled)?;
            let statistics = Statistics {
                decode_passes: 1,
                scratch_capacity_bytes: self.samples.capacity() * size_of::<f64>(),
                ..Statistics::default()
            };
            self.reducer.finish(statistics, &mut cancelled)
        }))
        .map_err(|_| Error::InternalPanic)?
    }
}

/// Read headerless PCM in one pass without seeking. Pass `&mut reader` to
/// retain ownership of the input after generation.
pub fn generate_pcm(
    input: impl Read,
    format: PcmFormat,
    sample_rate: u32,
    channels: u16,
    options: Options,
) -> Result<Waveform, Error> {
    generate_pcm_with_cancel(input, format, sample_rate, channels, options, || false)
}

/// Cancellation is checked between reads and while aggregating samples.
/// The caller's `Read` implementation must arrange to interrupt blocked reads.
pub fn generate_pcm_with_cancel(
    mut input: impl Read,
    format: PcmFormat,
    sample_rate: u32,
    channels: u16,
    options: Options,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Waveform, Error> {
    catch_unwind(AssertUnwindSafe(|| {
        let mut stream = PcmStream::new(format, sample_rate, channels, options)?;
        let mut buffer = [0; 32768];
        loop {
            poll(&mut cancelled)?;
            match input.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => stream.push_with_cancel(&buffer[..count], &mut cancelled)?,
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        let mut waveform = stream.finish_with_cancel(cancelled)?;
        waveform.statistics.scratch_capacity_bytes += buffer.len();
        Ok(waveform)
    }))
    .map_err(|_| Error::InternalPanic)?
}
