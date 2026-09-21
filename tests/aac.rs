#![cfg(feature = "m4a")]

use std::{fs::File, path::Path};
use symphonia::core::{
    codecs::audio::AudioDecoderOptions,
    formats::{TrackType, probe::Hint},
    io::MediaSourceStream,
};
use waveform_core::{ChannelMode, Gain, Options, Resolution, generate};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/aac")
        .join(name)
}

// Small test-only oracle: decode every AAC packet without trimming, then use
// the known fixture range and independent batch min/max arithmetic.
fn raw(path: &Path) -> Vec<f64> {
    let stream = MediaSourceStream::new(Box::new(File::open(path).unwrap()), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(&Hint::new(), stream, Default::default(), Default::default())
        .unwrap();
    let track = format.default_track(TrackType::Audio).unwrap();
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(
            track.codec_params.as_ref().unwrap().audio().unwrap(),
            &AudioDecoderOptions::default().gapless(false),
        )
        .unwrap();
    let mut result = Vec::new();
    while let Some(packet) = format.next_packet().unwrap() {
        if packet.track_id != id {
            continue;
        }
        let decoded = decoder.decode(&packet).unwrap();
        let offset = result.len();
        result.resize(
            offset + decoded.frames() * decoded.spec().channels().count(),
            0.0,
        );
        decoded.copy_to_slice_interleaved(&mut result[offset..]);
    }
    result
}

fn expected(samples: &[f64], channels: usize, options: Options) -> Vec<i16> {
    let frames = samples.len() / channels;
    let windows: Vec<_> = match options.resolution {
        Resolution::Points(count) => (0..count as usize)
            .map(|i| {
                let start = i * frames / count as usize;
                start..((i + 1) * frames / count as usize).max(start + 1)
            })
            .collect(),
        Resolution::FramesPerPoint(scale) => (0..frames)
            .step_by(scale as usize)
            .map(|start| start..(start + scale as usize).min(frames))
            .collect(),
        _ => unreachable!(),
    };
    let mut peaks = Vec::new();
    for window in windows {
        for channel in 0..if options.channels == ChannelMode::Mono {
            1
        } else {
            channels
        } {
            let values: Vec<_> = window
                .clone()
                .map(|i| {
                    if options.channels == ChannelMode::Mono {
                        samples[i * channels..(i + 1) * channels]
                            .iter()
                            .sum::<f64>()
                            / channels as f64
                    } else {
                        samples[i * channels + channel]
                    }
                })
                .collect();
            peaks.extend([
                values.iter().copied().fold(f64::INFINITY, f64::min),
                values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            ]);
        }
    }
    let maximum = peaks.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    peaks
        .iter()
        .map(|x| match options.gain {
            Gain::Fixed(gain) => (x * gain * 32768.0) as i16,
            Gain::Normalize if maximum > 0.0 => (x / maximum * 32767.0) as i16,
            Gain::Normalize => 0,
        })
        .collect()
}

#[test]
fn playback_frames_and_peaks_exclude_only_the_declared_delay_and_padding() {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/aac/manifest.json")).unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        if case["fragmented"] == true {
            continue;
        }
        let name = case["file"].as_str().unwrap();
        let path = fixture(name);
        let channels = case["channels"].as_u64().unwrap() as usize;
        let frames = case["playback_frames"]
            .as_u64()
            .unwrap_or_else(|| case["frames"].as_u64().unwrap()) as usize;
        let rate = case["rate"].as_u64().unwrap() as u32;
        let decoded = raw(&path);
        let audible = &decoded[1024 * channels..(1024 + frames) * channels];
        for resolution in [Resolution::Points(110), Resolution::FramesPerPoint(23)] {
            for channels_mode in [ChannelMode::Mono, ChannelMode::Split] {
                for gain in [Gain::Fixed(1.0), Gain::Normalize] {
                    let options = Options {
                        resolution,
                        channels: channels_mode,
                        gain,
                    };
                    let waveform = generate(&path, options).unwrap();
                    assert_eq!(waveform.source_frames(), frames as u64, "{name}");
                    assert_eq!(waveform.sample_rate(), rate, "{name}");
                    assert_eq!(waveform.duration(), frames as f64 / f64::from(rate));
                    assert_eq!(
                        waveform.statistics().decode_passes,
                        if matches!(resolution, Resolution::Points(_)) {
                            2
                        } else {
                            1
                        }
                    );
                    let peaks = expected(audible, channels, options);
                    assert_eq!(waveform.data16(), peaks, "{name} {options:?}");
                    assert_eq!(
                        waveform.data8().collect::<Vec<_>>(),
                        peaks.iter().map(|x| (x / 256) as i8).collect::<Vec<_>>()
                    );
                    if name == "silence.m4a" {
                        assert!(peaks.iter().all(|x| *x == 0));
                    }
                    if name == "silent-edges.m4a" {
                        // Genuine silence remains part of the duration and the
                        // first/last peaks even when normalization is requested.
                        assert_eq!(peaks.first(), Some(&0));
                        assert_eq!(peaks.last(), Some(&0));
                    }
                }
            }
        }
    }
}

fn atom(bytes: &[u8], path: &[[u8; 4]]) -> usize {
    let mut start = 0;
    let mut end = bytes.len();
    for name in path {
        while &bytes[start + 4..start + 8] != name {
            start += u32::from_be_bytes(bytes[start..start + 4].try_into().unwrap()) as usize;
            assert!(start < end);
        }
        end = start + u32::from_be_bytes(bytes[start..start + 4].try_into().unwrap()) as usize;
        start += 8;
    }
    start
}

#[test]
fn edit_start_is_not_a_hardcoded_encoder_delay() {
    let original = std::fs::read(fixture("short-44100.m4a")).unwrap();
    let decoded = raw(&fixture("short-44100.m4a"));
    let edit = atom(&original, &[*b"moov", *b"trak", *b"edts", *b"elst"]);
    for (start, frames) in [(0_u32, 333_u32), (1537, 333), (2112, 1), (3229, 0)] {
        let mut bytes = original.clone();
        bytes[edit + 8..edit + 12].copy_from_slice(&frames.to_be_bytes());
        bytes[edit + 12..edit + 16].copy_from_slice(&start.to_be_bytes());
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), bytes).unwrap();
        for resolution in [Resolution::Points(110), Resolution::FramesPerPoint(23)] {
            let options = Options {
                resolution,
                ..Options::default()
            };
            let waveform = generate(file.path(), options).unwrap();
            assert_eq!(waveform.source_frames(), u64::from(frames));
            if frames == 0 {
                assert!(waveform.data16().is_empty());
            } else {
                assert_eq!(
                    waveform.data16(),
                    expected(
                        &decoded[start as usize..(start + frames) as usize],
                        1,
                        options
                    )
                );
            }
        }
    }
}

#[test]
fn missing_edit_does_not_guess_leading_delay_but_honors_sample_table_end() {
    let mut bytes = std::fs::read(fixture("short-44100.m4a")).unwrap();
    let edit = atom(&bytes, &[*b"moov", *b"trak", *b"edts"]);
    bytes[edit - 4..edit].copy_from_slice(b"free");
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), bytes).unwrap();
    let options = Options {
        resolution: Resolution::Points(110),
        ..Options::default()
    };
    let waveform = generate(file.path(), options).unwrap();
    assert_eq!(waveform.source_frames(), 3229);
    assert_eq!(
        waveform.data16(),
        expected(&raw(file.path())[..3229], 1, options)
    );
}

#[test]
fn selected_audio_track_is_trimmed_after_video() {
    let options = Options {
        resolution: Resolution::Points(110),
        ..Options::default()
    };
    let single = generate(fixture("short-44100.m4a"), options).unwrap();
    let tracks = generate(fixture("tracks.mp4"), options).unwrap();
    assert_eq!(tracks.source_frames(), 2205);
    assert_eq!(tracks.data16(), single.data16());
}

#[test]
fn fragmented_mp4_keeps_the_decoder_timeline() {
    let options = Options {
        resolution: Resolution::Points(110),
        ..Options::default()
    };
    let path = fixture("fragmented.m4a");
    let decoded = raw(&path);
    let waveform = generate(&path, options).unwrap();
    assert_eq!(waveform.source_frames(), decoded.len() as u64);
    assert_eq!(waveform.data16(), expected(&decoded, 1, options));
}

#[test]
fn impossible_sample_table_end_is_an_error_in_both_modes() {
    let mut bytes = std::fs::read(fixture("short-44100.m4a")).unwrap();
    let table = atom(
        &bytes,
        &[*b"moov", *b"trak", *b"mdia", *b"minf", *b"stbl", *b"stts"],
    );
    // The final packet still decodes 1,024 samples, but metadata now claims
    // another 100,000. A complete requested edit must not mask truncation.
    bytes[table + 20..table + 24].copy_from_slice(&100_000_u32.to_be_bytes());
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), bytes).unwrap();
    for resolution in [Resolution::Points(110), Resolution::FramesPerPoint(23)] {
        assert!(
            generate(
                file.path(),
                Options {
                    resolution,
                    ..Options::default()
                }
            )
            .is_err()
        );
    }
}
