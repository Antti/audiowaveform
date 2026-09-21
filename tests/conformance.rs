#![cfg(feature = "wav")]

use serde_json::Value;
use std::path::PathBuf;
use waveform_core::{
    ChannelMode, Error, Gain, Options, Resolution, generate, generate_with_cancel,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/generated")
        .join(format!("{name}.wav"))
}

fn options(value: &Value) -> Options {
    let resolution = if let Some(points) = value["points"].as_u64() {
        Resolution::Points(points as u32)
    } else if let Some(scale) = value["samples_per_pixel"].as_u64() {
        Resolution::FramesPerPoint(scale as u32)
    } else if let Some(rate) = value["pixels_per_second"].as_u64() {
        Resolution::PointsPerSecond(rate as u32)
    } else {
        Options::default().resolution
    };
    Options {
        resolution,
        channels: if value["split_channels"].as_bool().unwrap_or(false) {
            ChannelMode::Split
        } else {
            ChannelMode::Mono
        },
        gain: if value["amplitude_scale"] == "auto" {
            Gain::Normalize
        } else {
            Gain::Fixed(value["amplitude_scale"].as_f64().unwrap_or(1.0))
        },
    }
}

#[test]
fn all_contract_vectors_decode_to_literal_expectations() {
    let corpus: Value = serde_json::from_str(include_str!("../contract/vectors.json")).unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let waveform = generate(
            fixture(case["fixture"].as_str().unwrap()),
            options(&case["options"]),
        )
        .unwrap_or_else(|error| panic!("{id}: {error}"));
        let expected = &case["expected"];
        assert_eq!(
            waveform.sample_rate() as u64,
            expected["sample_rate"].as_u64().unwrap(),
            "{id}"
        );
        assert_eq!(
            waveform.channels() as u64,
            expected["channels"].as_u64().unwrap(),
            "{id}"
        );
        assert_eq!(
            waveform.source_frames(),
            expected["frames"].as_u64().unwrap(),
            "{id}"
        );
        assert_eq!(
            waveform.frames_per_point(),
            expected["samples_per_pixel"].as_u64().unwrap(),
            "{id}"
        );
        assert_eq!(
            waveform.len() as u64,
            expected["length"].as_u64().unwrap(),
            "{id}"
        );
        assert_eq!(
            waveform.is_empty(),
            expected["length"].as_u64().unwrap() == 0,
            "{id}"
        );
        assert_eq!(
            waveform.duration(),
            waveform.source_frames() as f64 / f64::from(waveform.sample_rate()),
            "{id}"
        );
        let expected16: Vec<_> = expected["data16"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i16)
            .collect();
        let expected8: Vec<_> = expected["data8"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap() as i8)
            .collect();
        assert_eq!(waveform.data16(), expected16, "{id}");
        assert_eq!(waveform.data8().collect::<Vec<_>>(), expected8, "{id}");
        for index in 0..waveform.len() {
            for channel in 0..waveform.channels() {
                let offset = 2 * (index * waveform.channels() + channel);
                assert_eq!(
                    waveform.point(index, channel),
                    Some([expected16[offset], expected16[offset + 1]]),
                    "{id}"
                );
            }
        }
        assert_eq!(waveform.point(usize::MAX, 0), None);
        assert_eq!(waveform.point(0, usize::MAX), None);
        // PCM/float WAV has an exact data extent: points need only one decode.
        assert_eq!(waveform.statistics().decode_passes, 1, "{id}");
    }
}

#[test]
fn invalid_audio_does_not_return_successful_peaks() {
    for name in ["invalid_header", "nonfinite_nan", "nonfinite_inf", "absent"] {
        for resolution in [Resolution::Points(3), Resolution::FramesPerPoint(3)] {
            assert!(
                generate(
                    fixture(name),
                    Options {
                        resolution,
                        ..Options::default()
                    }
                )
                .is_err(),
                "{name}"
            );
        }
    }
    // A declared payload whose end is missing must not be silently accepted.
    let data = std::fs::read(fixture("stereo")).unwrap();
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), &data[..data.len() - 5]).unwrap();
    assert!(generate(temp.path(), Options::default()).is_err());
}

#[test]
fn malformed_extensible_channel_masks_fail_without_panicking() {
    let mut data = std::fs::read(fixture("surround")).unwrap();
    data[40..44].copy_from_slice(&3_u32.to_le_bytes()); // two speakers, six declared channels
    let temp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(temp.path(), &data).unwrap();
    assert!(generate(temp.path(), Options::default()).is_err());
}

#[test]
fn validation_precedes_file_access() {
    for resolution in [
        Resolution::Points(0),
        Resolution::FramesPerPoint(0),
        Resolution::FramesPerPoint(1),
        Resolution::PointsPerSecond(0),
    ] {
        assert!(matches!(
            generate(
                fixture("absent"),
                Options {
                    resolution,
                    ..Options::default()
                }
            ),
            Err(Error::InvalidOption(_))
        ));
    }
    for gain in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(matches!(
            generate(
                fixture("absent"),
                Options {
                    gain: Gain::Fixed(gain),
                    ..Options::default()
                }
            ),
            Err(Error::InvalidOption(_))
        ));
    }
}

#[test]
fn cancellation_works_before_input_and_during_generation() {
    assert!(matches!(
        generate_with_cancel(fixture("absent"), Options::default(), || true),
        Err(Error::Cancelled)
    ));
    let mut calls = 0;
    let result = generate_with_cancel(
        fixture("stereo"),
        Options {
            resolution: Resolution::Points(110),
            ..Options::default()
        },
        || {
            calls += 1;
            calls >= 5
        },
    );
    assert!(matches!(result, Err(Error::Cancelled)));
}

#[test]
fn panics_cannot_escape_the_generation_boundary() {
    let result = generate_with_cancel(fixture("absent"), Options::default(), || {
        panic!("injected processing panic")
    });
    assert!(matches!(result, Err(Error::InternalPanic)));
}

#[test]
fn empty_exact_output_does_not_reserve_the_requested_point_count() {
    let waveform = generate(
        fixture("empty"),
        Options {
            resolution: Resolution::Points(u32::MAX),
            ..Options::default()
        },
    )
    .unwrap();
    assert!(waveform.is_empty());
    assert_eq!(waveform.allocated_bytes(), 0);
    assert_eq!(waveform.statistics().decode_passes, 1);
}
