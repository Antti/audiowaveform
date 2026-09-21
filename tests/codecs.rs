use serde_json::Value;
use std::path::PathBuf;
use waveform_core::{ChannelMode, Options, Resolution, generate};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/codecs")
        .join(name)
}

fn enabled(feature: &str) -> bool {
    match feature {
        "wav" => cfg!(feature = "wav"),
        "aiff" => cfg!(feature = "aiff"),
        "caf" => cfg!(feature = "caf"),
        "flac" => cfg!(feature = "flac"),
        "ogg" => cfg!(feature = "ogg"),
        "aac" => cfg!(feature = "aac"),
        "m4a" => cfg!(feature = "m4a"),
        "mkv" => cfg!(feature = "mkv"),
        "mp1" => cfg!(feature = "mp1"),
        "mp2" => cfg!(feature = "mp2"),
        "mp3" => cfg!(feature = "mp3"),
        _ => panic!("unknown feature {feature}"),
    }
}

#[test]
fn synthetic_codec_matrix() {
    let manifest: Value =
        serde_json::from_str(include_str!("fixtures/codecs/manifest.json")).unwrap();
    for case in manifest["cases"].as_array().unwrap() {
        if !enabled(case["feature"].as_str().unwrap()) {
            continue;
        }
        let file = case["file"].as_str().unwrap();
        for (mode, channels) in [("mono", ChannelMode::Mono), ("split", ChannelMode::Split)] {
            let waveform = generate(
                fixture(file),
                Options {
                    channels,
                    resolution: Resolution::Points(110),
                    ..Options::default()
                },
            )
            .unwrap_or_else(|error| panic!("{file} {mode}: {error}"));
            assert_eq!(waveform.len(), 110, "{file}");
            assert_eq!(
                waveform.statistics().decode_passes,
                if matches!(file, "pcm.wav" | "audio.flac") {
                    1
                } else {
                    2
                },
                "{file} {mode}"
            );
            assert_eq!(waveform.sample_rate(), 48000, "{file}");
            assert!(waveform.source_frames() > 0, "{file}");
            assert!(
                waveform
                    .data16()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|pair| pair[0] <= pair[1])
            );
            if case["lossless"] == true {
                assert_eq!(waveform.source_frames(), 12000, "{file}");
                let expected: Vec<_> = manifest["expected16"][mode]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_i64().unwrap() as i16)
                    .collect();
                assert_eq!(waveform.data16(), expected, "{file} {mode}");
            }
            if case["silent"] == true {
                assert!(waveform.data16().iter().all(|value| *value == 0));
            }
        }
    }
}

#[test]
#[cfg(feature = "mkv")]
fn default_audio_track_is_selected_after_video_and_unsupported_default_is_an_error() {
    let waveform = generate(fixture("default-track.mkv"), Options::default()).unwrap();
    assert_eq!(waveform.source_frames(), 12000);
    assert!(waveform.data16().iter().all(|value| *value == 0));
    assert!(generate(fixture("unsupported-default.mkv"), Options::default()).is_err());
}

#[test]
#[cfg(feature = "mkv")]
fn exact_points_do_not_require_declared_container_duration() {
    use symphonia::core::{
        formats::{TrackType, probe::Hint},
        io::MediaSourceStream,
    };
    {
        let file = "live.webm";
        let stream = MediaSourceStream::new(
            Box::new(std::fs::File::open(fixture(file)).unwrap()),
            Default::default(),
        );
        let reader = symphonia::default::get_probe()
            .probe(&Hint::new(), stream, Default::default(), Default::default())
            .unwrap();
        assert!(
            reader
                .default_track(TrackType::Audio)
                .unwrap()
                .duration
                .is_none(),
            "{file}"
        );
        let waveform = generate(
            fixture(file),
            Options {
                resolution: Resolution::Points(110),
                ..Options::default()
            },
        )
        .unwrap();
        assert_eq!(waveform.len(), 110);
        assert_eq!(waveform.statistics().decode_passes, 2);
        assert_eq!(
            waveform.duration(),
            waveform.source_frames() as f64 / 48000.0
        );
    }
}

#[test]
#[cfg(feature = "m4a")]
fn mp4_audio_is_found_after_video_with_demuxer_default_selection() {
    // Symphonia does not expose MP4 enabled/default flags: its default is the
    // first audio track. Matroska default-flag behavior is tested separately.
    let waveform = generate(fixture("default-track.mp4"), Options::default()).unwrap();
    assert_eq!(waveform.source_frames(), 12000);
    assert!(waveform.data16().iter().any(|value| *value != 0));
    assert!(generate(fixture("unsupported-default.mp4"), Options::default()).is_err());
}

#[test]
#[cfg(feature = "mkv")]
fn live_webm_truncated_packet_is_not_treated_as_ordinary_eof() {
    let source = std::fs::read(fixture("live.webm")).unwrap();
    for missing in [1, 2, 7, 23] {
        let temp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(temp.path(), &source[..source.len() - missing]).unwrap();
        assert!(
            generate(temp.path(), Options::default()).is_err(),
            "missing={missing}"
        );
    }
}

#[test]
fn gapless_decoding_removes_leading_delay_before_placing_buckets() {
    // The fixture source contains 12,000 frames, starting immediately with a
    // tone. MP3 carries exact delay/padding metadata. Vorbis container tail
    // granularity can retain up to one 256-frame block; it must not retain the
    // extra priming blocks that shifted the waveform in 0.3.0.
    for (feature, file) in [
        ("mp3", "audio.mp3"),
        ("ogg", "vorbis.ogg"),
        ("mkv", "live.webm"),
    ] {
        if !enabled(feature) {
            continue;
        }
        for resolution in [Resolution::Points(110), Resolution::FramesPerPoint(256)] {
            let waveform = generate(
                fixture(file),
                Options {
                    resolution,
                    ..Options::default()
                },
            )
            .unwrap();
            if feature == "mp3" {
                assert_eq!(waveform.source_frames(), 12000, "{file}");
            } else {
                assert!(
                    (12000..=12288).contains(&waveform.source_frames()),
                    "{file}: {}",
                    waveform.source_frames()
                );
            }
            let [min, max] = waveform.point(0, 0).unwrap();
            assert!(
                min < -8000 && max > 8000,
                "{file}: first bucket contains priming silence: {min}/{max}"
            );
        }
    }
}

#[test]
#[cfg(feature = "flac")]
fn missing_or_understated_flac_frame_counts_fall_back_without_changing_peaks() {
    use waveform_core::Gain;
    let original = std::fs::read(fixture("audio.flac")).unwrap();
    assert_eq!(&original[..4], b"fLaC");
    assert_eq!(original[4] & 0x7f, 0); // first block is STREAMINFO
    let packed = u64::from_be_bytes(original[18..26].try_into().unwrap());
    let mask = (1_u64 << 36) - 1;
    assert_eq!(packed & mask, 12000);
    for declared in [0_u64, 1, 6000] {
        let mut bytes = original.clone();
        bytes[18..26].copy_from_slice(&((packed & !mask) | declared).to_be_bytes());
        let input = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(input.path(), bytes).unwrap();
        for gain in [Gain::Fixed(1.0), Gain::Normalize] {
            for channels in [ChannelMode::Mono, ChannelMode::Split] {
                for count in [3, 110, 12001] {
                    let options = Options {
                        resolution: Resolution::Points(count),
                        channels,
                        gain,
                    };
                    let fast = generate(fixture("audio.flac"), options).unwrap();
                    let fallback = generate(input.path(), options).unwrap();
                    assert_eq!(fast.statistics().decode_passes, 1);
                    assert_eq!(
                        fallback.statistics().decode_passes,
                        2,
                        "declared={declared}"
                    );
                    assert_eq!(fallback.source_frames(), 12000);
                    assert_eq!(fallback.data16(), fast.data16());
                    assert_eq!(fallback.frames_per_point(), fast.frames_per_point());
                    assert_eq!(fallback.duration(), fast.duration());
                }
            }
        }
    }
}

#[test]
#[cfg(feature = "flac")]
fn flac_fast_path_does_not_hide_corruption_or_truncation() {
    let original = std::fs::read(fixture("audio.flac")).unwrap();
    let mut overstated = original.clone();
    let packed = u64::from_be_bytes(overstated[18..26].try_into().unwrap());
    let mask = (1_u64 << 36) - 1;
    overstated[18..26].copy_from_slice(&((packed & !mask) | 24000).to_be_bytes());
    let mut wrong_checksum = original.clone();
    wrong_checksum[26] ^= 1; // STREAMINFO's expected MD5 of decoded samples
    for bytes in [
        &original[..original.len() - 40],
        &overstated,
        &wrong_checksum,
    ] {
        let input = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(input.path(), bytes).unwrap();
        assert!(
            generate(
                input.path(),
                Options {
                    resolution: Resolution::Points(110),
                    ..Options::default()
                }
            )
            .is_err()
        );
    }
}

#[test]
#[cfg(feature = "wav")]
fn wav_fast_path_uses_detected_format_and_not_filename_or_fact_duration() {
    let original = std::fs::read(fixture("pcm.wav")).unwrap();
    // Inject a bogus FACT count before fmt/data; PCM length comes from data.
    let mut bytes = original[..12].to_vec();
    bytes.extend_from_slice(b"fact");
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&original[12..]);
    let size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    let input = tempfile::Builder::new().suffix(".mp3").tempfile().unwrap();
    std::fs::write(input.path(), bytes).unwrap();
    let options = Options {
        resolution: Resolution::Points(110),
        ..Options::default()
    };
    let expected = generate(fixture("pcm.wav"), options).unwrap();
    let actual = generate(input.path(), options).unwrap();
    assert_eq!(actual.statistics().decode_passes, 1);
    assert_eq!(actual.source_frames(), 12000);
    assert_eq!(actual.data16(), expected.data16());
}
