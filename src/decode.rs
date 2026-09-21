use std::{
    fs::File,
    io::{Seek, SeekFrom},
    path::Path,
};

use symphonia::core::{
    codecs::audio::{AudioDecoder, AudioDecoderOptions},
    formats::{FormatReader, TrackFlags, TrackType, probe::Hint},
    io::MediaSourceStream,
};

use crate::error::poll;
use crate::reduce::{Reducer, Signal};
use crate::{Error, Options, Resolution, Statistics, Waveform};

/// Generate peaks from a local, seekable audio file.
pub fn generate(path: impl AsRef<Path>, options: Options) -> Result<Waveform, Error> {
    generate_with_cancel(path, options, || false)
}

/// As [`generate`], with cooperative cancellation between packets and batches.
/// The callback runs synchronously on the decoding thread; it must not panic.
pub fn generate_with_cancel(
    path: impl AsRef<Path>,
    options: Options,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Waveform, Error> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        generate_inner(path.as_ref(), options, &mut cancelled)
    }))
    .map_err(|_| Error::InternalPanic)?
}

fn generate_inner(
    path: &Path,
    options: Options,
    mut cancelled: &mut impl FnMut() -> bool,
) -> Result<Waveform, Error> {
    options.validate()?;
    poll(&mut cancelled)?;
    if !path.metadata()?.is_file() {
        return Err(Error::InvalidAudio("input must be a regular file"));
    }
    let mut file = File::open(path)?;
    #[cfg(feature = "wav")]
    crate::wave_header::validate(&mut file, &mut cancelled)?;
    file.seek(SeekFrom::Start(0))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }
    let session = Session::open(stream, &hint)?;
    let mut scratch = Vec::new();
    let mut statistics = Statistics {
        decode_passes: 1,
        ..Statistics::default()
    };

    let mut reducer = None;
    let summary = if matches!(options.resolution, Resolution::Points(_)) {
        let (mut stream, counted) =
            session.scan(&mut scratch, &mut cancelled, |_, samples, _| {
                validate_samples(samples)
            })?;
        reducer = Some(Reducer::new(options, counted.signal, Some(counted.frames))?);
        if counted.frames > 0 {
            poll(&mut cancelled)?;
            stream.seek(SeekFrom::Start(0))?;
            let session = Session::open(stream, &hint)?;
            statistics.decode_passes = 2;
            let (_, generated) =
                session.scan(&mut scratch, &mut cancelled, |signal, samples, cancel| {
                    if signal != counted.signal {
                        return Err(Error::InputChanged);
                    }
                    reducer
                        .as_mut()
                        .expect("reducer initialized before replay")
                        .feed(samples, cancel)
                })?;
            if generated != counted {
                return Err(Error::InputChanged);
            }
        }
        counted
    } else {
        let (_, summary) =
            session.scan(&mut scratch, &mut cancelled, |signal, samples, cancel| {
                if reducer.is_none() {
                    reducer = Some(Reducer::new(options, signal, None)?);
                }
                reducer
                    .as_mut()
                    .expect("reducer initialized above")
                    .feed(samples, cancel)
            })?;
        summary
    };
    statistics.scratch_capacity_bytes = scratch.capacity() * size_of::<f64>();
    let reducer = match reducer {
        Some(reducer) => reducer,
        None => Reducer::new(options, summary.signal, None)?,
    };
    poll(&mut cancelled)?;
    reducer.finish(statistics, &mut cancelled)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scan {
    signal: Signal,
    frames: u64,
    track: u32,
}

struct Session {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
}

impl Session {
    fn open(stream: MediaSourceStream<'static>, hint: &Hint) -> Result<Self, Error> {
        let format = symphonia::default::get_probe().probe(
            hint,
            stream,
            Default::default(),
            Default::default(),
        )?;
        // Keep the first/default audio track even when its codec is unsupported.
        // Falling through to another codec could silently select different audio.
        let track = format
            .tracks()
            .iter()
            .find(|track| {
                track.track_type() == Some(TrackType::Audio)
                    && track.flags.contains(TrackFlags::DEFAULT)
            })
            .or_else(|| {
                format
                    .tracks()
                    .iter()
                    .find(|track| track.track_type() == Some(TrackType::Audio))
            })
            .ok_or(Error::InvalidAudio("no audio track"))?;
        let params = track
            .codec_params
            .as_ref()
            .and_then(|params| params.audio())
            .ok_or(Error::InvalidAudio("missing audio codec parameters"))?;
        let decoder = symphonia::default::get_codecs().make_audio_decoder(
            params,
            &AudioDecoderOptions::default().gapless(true).verify(true),
        )?;
        let track = track.id;
        Ok(Self {
            format,
            decoder,
            track,
        })
    }

    fn scan<C: FnMut() -> bool>(
        mut self,
        scratch: &mut Vec<f64>,
        cancelled: &mut C,
        mut consume: impl FnMut(Signal, &[f64], &mut C) -> Result<(), Error>,
    ) -> Result<(MediaSourceStream<'static>, Scan), Error> {
        let mut observed = None;
        let mut frames = 0_u64;
        let mut demux_error = None;
        loop {
            poll(cancelled)?;
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => break,
                Err(error) => {
                    demux_error = Some(error);
                    break;
                }
            };
            if packet.track_id != self.track {
                continue;
            }
            let decoded = self.decoder.decode(&packet)?;
            if decoded.frames() == 0 {
                continue;
            }
            let signal = Signal {
                rate: decoded.spec().rate(),
                channels: decoded.spec().channels().count(),
            };
            if signal.rate == 0 || signal.channels == 0 {
                return Err(Error::InvalidAudio("zero sample rate or channel count"));
            }
            if observed.is_some_and(|previous| previous != signal) {
                return Err(Error::InvalidAudio("sample rate or channel count changed"));
            }
            observed = Some(signal);
            let size = decoded
                .frames()
                .checked_mul(signal.channels)
                .ok_or(Error::SizeOverflow)?;
            if size > scratch.len() {
                scratch.try_reserve(size - scratch.len())?;
            }
            scratch.resize(size, 0.0);
            decoded.copy_to_slice_interleaved(scratch.as_mut_slice());
            frames = frames
                .checked_add(u64::try_from(decoded.frames()).map_err(|_| Error::SizeOverflow)?)
                .ok_or(Error::SizeOverflow)?;
            consume(signal, scratch, cancelled)?;
        }
        #[cfg(feature = "mkv")]
        let matroska = self.format.format_info().short_name == "matroska";
        let stream = self.format.into_inner();
        let stream = match demux_error {
            None => stream,
            Some(error) => {
                #[cfg(feature = "mkv")]
                if matroska
                    && matches!(&error, symphonia::core::errors::Error::IoError(io) if io.kind() == std::io::ErrorKind::UnexpectedEof)
                {
                    crate::matroska::complete_at_eof(stream, cancelled)?
                } else {
                    return Err(error.into());
                }
                #[cfg(not(feature = "mkv"))]
                return Err(error.into());
            }
        };
        if self.decoder.finalize().verify_ok == Some(false) {
            return Err(Error::InvalidAudio("decoder verification failed"));
        }
        let signal = match observed {
            Some(signal) => signal,
            None => {
                let params = self.decoder.codec_params();
                let rate = params
                    .sample_rate
                    .filter(|rate| *rate > 0)
                    .ok_or(Error::InvalidAudio("empty stream has no sample rate"))?;
                let channels = params
                    .channels
                    .as_ref()
                    .map(|channels| channels.count())
                    .filter(|count| *count > 0)
                    .ok_or(Error::InvalidAudio("empty stream has no channels"))?;
                Signal { rate, channels }
            }
        };
        Ok((
            stream,
            Scan {
                signal,
                frames,
                track: self.track,
            },
        ))
    }
}

fn validate_samples(samples: &[f64]) -> Result<(), Error> {
    if samples.iter().any(|sample| !sample.is_finite()) {
        Err(Error::InvalidAudio("nonfinite audio sample"))
    } else {
        Ok(())
    }
}
