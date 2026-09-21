use std::io::{self, Cursor, Read};
use waveform_core::{
    ChannelMode, Error, Gain, Options, PcmFormat, PcmStream, Resolution, generate_pcm,
    generate_pcm_with_cancel,
};

fn options() -> Options {
    Options {
        resolution: Resolution::FramesPerPoint(2),
        channels: ChannelMode::Split,
        ..Options::default()
    }
}

fn encodings() -> Vec<(&'static str, Vec<u8>)> {
    // Six exactly representable values, arranged as three stereo frames.
    let s = [-128_i32, 64, 0, -64, 32, -32];
    vec![
        ("u8", s.iter().map(|&v| (v + 128) as u8).collect()),
        ("s8", s.iter().map(|&v| v as i8 as u8).collect()),
        (
            "s16le",
            s.iter()
                .flat_map(|&v| ((v * 256) as i16).to_le_bytes())
                .collect(),
        ),
        (
            "s16be",
            s.iter()
                .flat_map(|&v| ((v * 256) as i16).to_be_bytes())
                .collect(),
        ),
        (
            "s24le",
            s.iter()
                .flat_map(|&v| (v * 65536).to_le_bytes()[..3].to_vec())
                .collect(),
        ),
        (
            "s24be",
            s.iter()
                .flat_map(|&v| (v * 65536).to_be_bytes()[1..].to_vec())
                .collect(),
        ),
        (
            "s32le",
            s.iter()
                .flat_map(|&v| (v * 16777216).to_le_bytes())
                .collect(),
        ),
        (
            "s32be",
            s.iter()
                .flat_map(|&v| (v * 16777216).to_be_bytes())
                .collect(),
        ),
        (
            "f32le",
            s.iter()
                .flat_map(|&v| (v as f32 / 128.0).to_le_bytes())
                .collect(),
        ),
        (
            "f32be",
            s.iter()
                .flat_map(|&v| (v as f32 / 128.0).to_be_bytes())
                .collect(),
        ),
        (
            "f64le",
            s.iter()
                .flat_map(|&v| (f64::from(v) / 128.0).to_le_bytes())
                .collect(),
        ),
        (
            "f64be",
            s.iter()
                .flat_map(|&v| (f64::from(v) / 128.0).to_be_bytes())
                .collect(),
        ),
    ]
}

#[test]
fn all_encodings_preserve_frames_across_arbitrary_byte_boundaries() {
    for (name, bytes) in encodings() {
        let format: PcmFormat = name.parse().unwrap();
        for chunk in 1..=17 {
            let mut stream = PcmStream::new(format, 48000, 2, options()).unwrap();
            for part in bytes.chunks(chunk) {
                stream.push(part).unwrap();
                stream.push(&[]).unwrap();
            }
            let w = stream.finish().unwrap();
            assert_eq!(
                w.data16(),
                [-32768, 0, -16384, 16384, 8192, 8192, -8192, -8192],
                "{name}/{chunk}"
            );
            assert_eq!(
                w.data8().collect::<Vec<_>>(),
                [-128, 0, -64, 64, 32, 32, -32, -32]
            );
            assert_eq!(w.source_frames(), 3);
            assert_eq!(w.duration(), 3.0 / 48000.0);
            assert_eq!(w.statistics().decode_passes, 1);
        }
        let w = generate_pcm(
            Cursor::new(bytes),
            format,
            48000,
            2,
            Options {
                channels: ChannelMode::Mono,
                ..options()
            },
        )
        .unwrap();
        assert_eq!(w.data16(), [-8192, -8192, 0, 0], "{name}");
    }
}

#[test]
fn gain_and_normalization_preserve_precision_before_quantization() {
    let bytes: Vec<_> = [0.0000152587890625_f64, -0.0000152587890625]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let w = generate_pcm(
        Cursor::new(bytes),
        PcmFormat::F64Le,
        8000,
        1,
        Options {
            gain: Gain::Normalize,
            ..options()
        },
    )
    .unwrap();
    assert_eq!(w.data16(), [-32767, 32767]);
    let bytes: Vec<_> = [-2.0_f32, 2.0]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let w = generate_pcm(
        Cursor::new(bytes),
        PcmFormat::F32Le,
        8000,
        1,
        Options {
            gain: Gain::Fixed(0.5),
            ..options()
        },
    )
    .unwrap();
    assert_eq!(w.data16(), [-32768, 32767]);
}

#[test]
fn eof_rejects_partial_samples_and_partial_channel_frames() {
    for (name, bytes) in encodings() {
        let width = name.parse::<PcmFormat>().unwrap().bytes_per_sample();
        for missing in 1..(2 * width) {
            let mut stream = PcmStream::new(name.parse().unwrap(), 8000, 2, options()).unwrap();
            stream.push(&bytes[..bytes.len() - missing]).unwrap();
            assert!(
                matches!(stream.finish(), Err(Error::InvalidAudio(_))),
                "{name}/{missing}"
            );
        }
    }
    let w = generate_pcm(io::empty(), PcmFormat::S16Le, 8000, 2, options()).unwrap();
    assert!(w.is_empty());
    assert_eq!(w.channels(), 2);
    assert_eq!(w.duration(), 0.0);
}

#[test]
fn validation_precedes_reads_and_rejects_exact_points() {
    struct NoReads;
    impl Read for NoReads {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("read before validation")
        }
    }
    for (rate, channels, opts) in [
        (0, 1, options()),
        (8000, 0, options()),
        (
            8000,
            1,
            Options {
                resolution: Resolution::Points(110),
                ..options()
            },
        ),
        (
            8000,
            1,
            Options {
                gain: Gain::Fixed(f64::NAN),
                ..options()
            },
        ),
    ] {
        assert!(matches!(
            generate_pcm(NoReads, PcmFormat::S16Le, rate, channels, opts),
            Err(Error::InvalidOption(_))
        ));
    }
    assert!("wav".parse::<PcmFormat>().is_err());
}

#[test]
fn read_errors_cancellation_and_panics_do_not_return_partial_waveforms() {
    struct Failure;
    impl Read for Failure {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("source failed"))
        }
    }
    assert!(matches!(
        generate_pcm(Failure, PcmFormat::U8, 8000, 1, options()),
        Err(Error::Io(_))
    ));
    let mut calls = 0;
    assert!(matches!(
        generate_pcm_with_cancel(io::repeat(128), PcmFormat::U8, 8000, 1, options(), || {
            calls += 1;
            calls > 8
        }),
        Err(Error::Cancelled)
    ));
    let mut stream = PcmStream::new(PcmFormat::U8, 8000, 1, options()).unwrap();
    assert!(matches!(
        stream.push_with_cancel(&[128; 8192], || panic!("cancel callback")),
        Err(Error::InternalPanic)
    ));
    assert!(stream.push(&[]).is_err());
    assert!(stream.finish().is_err());
}

#[test]
fn interrupted_and_short_reads_preserve_every_byte() {
    let data: Vec<u8> = (0..=255).cycle().take(10001).collect();
    struct Fragmented<'a> {
        remaining: &'a [u8],
        interrupt: bool,
    }
    impl Read for Fragmented<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.interrupt = !self.interrupt;
            if self.interrupt {
                return Err(ErrorKind::Interrupted.into());
            }
            let n = self.remaining.len().min(buffer.len()).min(7);
            buffer[..n].copy_from_slice(&self.remaining[..n]);
            self.remaining = &self.remaining[n..];
            Ok(n)
        }
    }
    use std::io::ErrorKind;
    let a = generate_pcm(Cursor::new(&data), PcmFormat::U8, 8000, 1, options()).unwrap();
    let b = generate_pcm(
        Fragmented {
            remaining: &data,
            interrupt: false,
        },
        PcmFormat::U8,
        8000,
        1,
        options(),
    )
    .unwrap();
    assert_eq!(a.data16(), b.data16());
    assert_eq!(b.source_frames(), 10001);
}

#[test]
fn nonfinite_pcm_is_rejected_without_poisoned_results() {
    for sample in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut stream = PcmStream::new(PcmFormat::F64Le, 8000, 1, options()).unwrap();
        assert!(matches!(
            stream.push(&sample.to_le_bytes()),
            Err(Error::InvalidAudio(_))
        ));
        assert!(stream.finish().is_err());
    }
}

#[test]
fn longer_input_with_the_same_point_count_uses_the_same_buffer_capacities() {
    for gain in [Gain::Fixed(1.0), Gain::Normalize] {
        let run = |frames| {
            generate_pcm(
                io::repeat(128).take(frames),
                PcmFormat::U8,
                8000,
                1,
                Options {
                    resolution: Resolution::FramesPerPoint((frames / 110) as u32),
                    gain,
                    ..options()
                },
            )
            .unwrap()
        };
        let short = run(110_000);
        let long = run(11_000_000);
        assert_eq!(short.len(), 110);
        assert_eq!(long.len(), 110);
        assert_eq!(short.statistics(), long.statistics());
    }
}
