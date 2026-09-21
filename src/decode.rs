use std::{
    fs::File,
    io::{Seek, SeekFrom},
    path::Path,
};

use symphonia::core::{
    codecs::audio::{
        AudioDecoder, AudioDecoderOptions,
        well_known::{
            CODEC_ID_AAC, CODEC_ID_FLAC, CODEC_ID_PCM_F32LE, CODEC_ID_PCM_F64LE,
            CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24LE, CODEC_ID_PCM_S32LE, CODEC_ID_PCM_U8,
        },
    },
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
    let metadata = file.try_clone()?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }
    let mut source = Source { hint, metadata };
    let session = Session::open(stream, &mut source, &mut cancelled)?;
    let mut scratch = Vec::new();
    let (reducer, decode_passes) = match options.resolution {
        Resolution::Points(_) => {
            reduce_points(session, options, &mut source, &mut scratch, &mut cancelled)?
        }
        _ => (
            reduce_fixed(session, options, &mut scratch, &mut cancelled)?,
            1,
        ),
    };
    let statistics = Statistics {
        decode_passes,
        scratch_capacity_bytes: scratch.capacity() * size_of::<f64>(),
        ..Statistics::default()
    };
    poll(&mut cancelled)?;
    reducer.finish(statistics, &mut cancelled)
}

fn reduce_fixed(
    session: Session,
    options: Options,
    scratch: &mut Vec<f64>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Reducer, Error> {
    let mut reducer = None;
    let (_, summary) = session.scan(scratch, cancelled, |progress, samples, cancel| {
        let reducer = match &mut reducer {
            Some(reducer) => reducer,
            slot @ None => slot.insert(Reducer::new(options, progress.signal, None)?),
        };
        reducer.feed(samples, cancel)
    })?;
    reducer.map_or_else(|| Reducer::new(options, summary.signal, None), Ok)
}

fn reduce_points(
    session: Session,
    options: Options,
    source: &mut Source,
    scratch: &mut Vec<f64>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(Reducer, u8), Error> {
    let mut provisional = session
        .exact_frame_count()
        .map(|expected| ProvisionalPoints {
            expected,
            reducer: None,
        });
    let (stream, counted) = session.scan(scratch, cancelled, |progress, samples, cancel| {
        // Discard the hint and its peaks together before feeding mismatched
        // channel dimensions or frames beyond the provisional last bucket.
        if provisional.as_ref().is_some_and(|state| {
            progress.signal != state.expected.signal || progress.frames > state.expected.frames
        }) {
            provisional = None;
        }
        match &mut provisional {
            Some(state) => state.feed(options, samples, cancel),
            None => validate_samples(samples),
        }
    })?;
    if let Some(reducer) = provisional
        .filter(|state| state.expected == counted)
        .and_then(|state| state.reducer)
    {
        return Ok((reducer, 1));
    }

    // Provisional peaks have been dropped before allocating their replacement.
    // The completed first pass already supplies the actual count for replay.
    let mut reducer = Reducer::new(options, counted.signal, Some(counted.frames))?;
    if counted.frames == 0 {
        return Ok((reducer, 1));
    }
    replay(stream, source, counted, &mut reducer, scratch, cancelled)?;
    Ok((reducer, 2))
}

struct ProvisionalPoints {
    expected: Scan,
    reducer: Option<Reducer>,
}

impl ProvisionalPoints {
    fn feed(
        &mut self,
        options: Options,
        samples: &[f64],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<(), Error> {
        // Wait for actual audio so an empty input never allocates point storage.
        let reducer = match &mut self.reducer {
            Some(reducer) => reducer,
            slot @ None => slot.insert(Reducer::new(
                options,
                self.expected.signal,
                Some(self.expected.frames),
            )?),
        };
        reducer.feed(samples, cancelled)
    }
}

fn replay(
    mut stream: MediaSourceStream<'static>,
    source: &mut Source,
    counted: Scan,
    reducer: &mut Reducer,
    scratch: &mut Vec<f64>,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<(), Error> {
    poll(cancelled)?;
    stream.seek(SeekFrom::Start(0))?;
    let session = Session::open(stream, source, cancelled)?;
    let (_, generated) = session.scan(scratch, cancelled, |progress, samples, cancel| {
        if progress.signal != counted.signal {
            return Err(Error::InputChanged);
        }
        reducer.feed(samples, cancel)
    })?;
    if generated != counted {
        return Err(Error::InputChanged);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Scan {
    signal: Signal,
    frames: u64,
    track: u32,
}

struct Source {
    hint: Hint,
    metadata: File,
}

struct Session {
    playback: Option<crate::mp4::Playback>,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
}

impl Session {
    fn open(
        stream: MediaSourceStream<'static>,
        source: &mut Source,
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Self, Error> {
        let format = symphonia::default::get_probe().probe(
            &source.hint,
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
        let playback =
            if format.format_info().short_name == "isomp4" && params.codec == CODEC_ID_AAC {
                crate::mp4::playback(
                    &mut source.metadata,
                    track.id,
                    params
                        .sample_rate
                        .filter(|rate| *rate > 0)
                        .ok_or(Error::InvalidAudio("AAC/MP4 has no sample rate"))?,
                    cancelled,
                )?
            } else {
                None
            };
        let decoder = symphonia::default::get_codecs().make_audio_decoder(
            params,
            &AudioDecoderOptions::default()
                .gapless(playback.is_none())
                .verify(true),
        )?;
        let track = track.id;
        Ok(Self {
            playback,
            format,
            decoder,
            track,
        })
    }

    /// Only native FLAC STREAMINFO and WAV PCM data extents have been audited
    /// as exact frame counts here. Container durations, ADPCM block counts,
    /// and counts affected by lossy codec delay/padding are not trusted.
    /// Even these hints are checked against all frames actually decoded.
    fn exact_frame_count(&self) -> Option<Scan> {
        let track = self
            .format
            .tracks()
            .iter()
            .find(|track| track.id == self.track)?;
        let params = track.codec_params.as_ref()?.audio()?;
        let eligible = match self.format.format_info().short_name {
            "flac" => params.codec == CODEC_ID_FLAC,
            "wave" => matches!(
                params.codec,
                CODEC_ID_PCM_U8
                    | CODEC_ID_PCM_S16LE
                    | CODEC_ID_PCM_S24LE
                    | CODEC_ID_PCM_S32LE
                    | CODEC_ID_PCM_F32LE
                    | CODEC_ID_PCM_F64LE
            ),
            _ => false,
        };
        if !eligible || track.delay.unwrap_or(0) != 0 || track.padding.unwrap_or(0) != 0 {
            return None;
        }
        Some(Scan {
            signal: Signal {
                rate: params.sample_rate.filter(|rate| *rate > 0)?,
                channels: params.channels.as_ref()?.count(),
            },
            frames: track.num_frames?,
            track: self.track,
        })
        .filter(|scan| scan.signal.channels > 0)
    }

    fn scan<C: FnMut() -> bool>(
        mut self,
        scratch: &mut Vec<f64>,
        cancelled: &mut C,
        mut consume: impl FnMut(Scan, &[f64], &mut C) -> Result<(), Error>,
    ) -> Result<(MediaSourceStream<'static>, Scan), Error> {
        let mut observed = None;
        let mut frames = 0_u64;
        let mut decoded_frames = 0_u64;
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
            let range = match self.playback {
                Some(playback) => {
                    if signal.rate != playback.rate {
                        return Err(Error::InvalidAudio("AAC/MP4 sample rate changed"));
                    }
                    playback.slice(decoded_frames, decoded.frames(), packet.pts.get())?
                }
                None => 0..decoded.frames(),
            };
            decoded_frames = decoded_frames
                .checked_add(u64::try_from(decoded.frames()).map_err(|_| Error::SizeOverflow)?)
                .ok_or(Error::SizeOverflow)?;
            if range.is_empty() {
                continue;
            }
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
                .checked_add(u64::try_from(range.len()).map_err(|_| Error::SizeOverflow)?)
                .ok_or(Error::SizeOverflow)?;
            consume(
                Scan {
                    signal,
                    frames,
                    track: self.track,
                },
                &scratch[range.start * signal.channels..range.end * signal.channels],
                cancelled,
            )?;
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
        if let Some(playback) = self.playback {
            playback.verify(decoded_frames, frames)?;
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
