use super::*;
use std::io::{Cursor, Write};

fn atom(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut result = ((data.len() + 8) as u32).to_be_bytes().to_vec();
    result.extend(kind);
    result.extend(data);
    result
}

fn header(kind: &[u8; 4], version: u8, value: u32) -> Vec<u8> {
    let mut data = vec![0; if version == 0 { 12 } else { 20 }];
    data[0] = version;
    data.extend(value.to_be_bytes());
    atom(kind, &data)
}

fn edit(version: u8, entries: &[(u64, i64, u32)]) -> Vec<u8> {
    let mut data = vec![version, 0, 0, 0];
    data.extend((entries.len() as u32).to_be_bytes());
    for &(duration, start, rate) in entries {
        if version == 0 {
            data.extend((duration as u32).to_be_bytes());
            data.extend((start as i32).to_be_bytes());
        } else {
            data.extend(duration.to_be_bytes());
            data.extend(start.to_be_bytes());
        }
        data.extend(rate.to_be_bytes());
    }
    atom(b"edts", &atom(b"elst", &data))
}

fn track(id: u32, version: u8, scale: u32, timing: &[(u32, u32)], edits: &[u8]) -> Vec<u8> {
    let mut times = vec![0; 4];
    times.extend((timing.len() as u32).to_be_bytes());
    for (count, delta) in timing {
        times.extend(count.to_be_bytes());
        times.extend(delta.to_be_bytes());
    }
    let mut media = header(b"mdhd", version, scale);
    media.extend(atom(b"minf", &atom(b"stbl", &atom(b"stts", &times))));
    let mut data = header(b"tkhd", version, id);
    data.extend(atom(b"mdia", &media));
    data.extend(edits);
    atom(b"trak", &data)
}

fn movie(version: u8, scale: u32, tracks: &[u8]) -> Vec<u8> {
    let mut data = header(b"mvhd", version, scale);
    data.extend(tracks);
    atom(b"moov", &data)
}

fn parse(bytes: &[u8], id: u32, rate: u32) -> Result<Playback, Error> {
    Parser {
        input: &mut Cursor::new(bytes),
        cancelled: &mut || false,
    }
    .playback(bytes.len() as u64, id, rate)
    .map(Option::unwrap)
}

#[test]
fn versions_clocks_and_selected_track() {
    for version in [0, 1] {
        // An unrelated track has an unsupported edit. Only id 7 applies.
        let mut tracks = track(
            3,
            version,
            48000,
            &[(1, 500)],
            &edit(version, &[(20, -1, 65536)]),
        );
        tracks.extend(track(
            7,
            version,
            88200,
            &[(3, 2048), (1, 314)],
            &edit(version, &[(51, 2048, 65536)]),
        ));
        let bounds = parse(&movie(version, 1000, &tracks), 7, 44100).unwrap();
        assert_eq!(
            (bounds.start, bounds.end, bounds.minimum_media_frames),
            (1024, 3229, 3229)
        );
        assert_eq!(bounds.slice(0, 1024), 1024..1024);
        assert_eq!(bounds.slice(1024, 1024), 0..1024);
        assert_eq!(bounds.slice(3072, 1024), 0..157);
        assert_eq!(bounds.slice(4096, 1024), 0..0);
        assert!(bounds.packet(1024, 1024, 1024, 2048).is_err());
        assert!(bounds.packet(0, 1024, -1, 2048).is_err());
        assert!(bounds.verify(4096, 2205).is_ok());
        assert!(bounds.verify(3072, 2205).is_err());
        assert!(bounds.verify(4096, 2204).is_err());
    }
}

#[test]
fn packet_slots_clip_overlaps_and_fill_only_observed_gaps() {
    let timing = [(1, 1024), (1, 712), (1, 6144), (1, 1000), (1, 352)];
    let bytes = movie(
        0,
        48000,
        &track(1, 0, 48000, &timing, &edit(0, &[(8208, 1024, 65536)])),
    );
    let bounds = parse(&bytes, 1, 48000).unwrap();
    let mut position = 0;
    let mut retained = 0;
    for (pts, duration, samples, silence, end) in [
        (0, 1024, 1024..1024, 0, 1024),
        (1024, 712, 0..712, 0, 1736),
        (1736, 6144, 0..1024, 0, 2760),
        (7880, 1000, 0..1000, 5120, 8880),
        (8880, 352, 0..352, 0, 9232),
    ] {
        let part = bounds.packet(position, 1024, pts, duration).unwrap();
        assert_eq!(
            (part.samples.clone(), part.silence, part.end),
            (samples, silence, end)
        );
        position = part.end;
        retained += part.silence + part.samples.len() as u64;
    }
    assert!(bounds.verify(position, retained).is_ok());
    // The last decoded packet cannot invent a trailing gap to satisfy metadata.
    assert!(bounds.verify(2760, 1736).is_err());
    assert!(bounds.packet(1024, 1024, -1, 1024).is_err());
    assert!(bounds.packet(1024, 1024, 0, 1024).is_err());
    assert!(matches!(
        bounds.packet(1024, 1024, 1024, u64::MAX),
        Err(Error::SizeOverflow)
    ));

    // An edit entirely inside a gap retains only that portion of silence.
    let bytes = movie(
        0,
        48000,
        &track(1, 0, 48000, &timing, &edit(0, &[(500, 3500, 65536)])),
    );
    let bounds = parse(&bytes, 1, 48000).unwrap();
    let part = bounds.packet(2760, 1024, 7880, 1000).unwrap();
    assert_eq!(part.silence, 500);
    assert!(part.samples.is_empty());

    // A zero-duration slot is decoded for AAC state but contributes no samples.
    let part = bounds.packet(1024, 1024, 1024, 0).unwrap();
    assert_eq!(part.end, 1024);
    assert_eq!(part.silence, 0);
    assert!(part.samples.is_empty());
}

#[test]
fn edit_end_is_added_before_rounding_and_can_cut_within_a_packet() {
    let bytes = movie(
        0,
        10,
        &track(1, 0, 3, &[(3, 3)], &edit(0, &[(1, 1, 65536)])),
    );
    let bounds = parse(&bytes, 1, 10).unwrap();
    assert_eq!((bounds.start, bounds.end), (4, 5)); // ceil(10/3), ceil(10/3 + 1)
    assert_eq!(bounds.slice(0, 10), 4..5);
}

#[test]
fn absent_and_empty_edit_use_sample_table_duration() {
    for edits in [vec![], edit(0, &[])] {
        let bytes = movie(
            0,
            44100,
            &track(1, 0, 44100, &[(3, 1024), (1, 157)], &edits),
        );
        let bounds = parse(&bytes, 1, 44100).unwrap();
        assert_eq!((bounds.start, bounds.end), (0, 3229));
    }
}

#[test]
fn complex_edits_and_ranges_beyond_media_are_rejected() {
    for version in [0, 1] {
        for entries in [
            vec![(50, -1, 65536)],
            vec![(50, 1024, 0)],
            vec![(50, 1024, 131072)],
            vec![(50, 0, 65536), (50, 1024, 65536)],
            vec![(50, 1024, 65536), (50, -1, 65536)],
            vec![(50, -2, 65536), (50, 1024, 65536)],
            vec![(50, 5000, 65536)],
        ] {
            let bytes = movie(
                version,
                1000,
                &track(1, version, 44100, &[(4, 1024)], &edit(version, &entries)),
            );
            assert!(matches!(
                parse(&bytes, 1, 44100),
                Err(Error::InvalidAudio(_))
            ));
        }
    }
}

#[test]
fn leading_empty_edits_advance_movie_time_without_changing_media_range() {
    for version in [0, 1] {
        let edits = edit(
            version,
            &[(33, -1, 65536), (67, -1, 65536), (50, 1024, 65536)],
        );
        let bytes = movie(
            version,
            1000,
            &track(1, version, 44100, &[(3, 1024), (1, 157)], &edits),
        );
        let bounds = parse(&bytes, 1, 44100).unwrap();
        assert_eq!(
            (bounds.start, bounds.end, bounds.leading_frames),
            (1024, 3229, 4410)
        );
        assert!(bounds.verify(4096, 6615).is_ok());
        assert!(bounds.verify(4096, 2205).is_err());
    }
    // Sum movie ticks before converting so several short gaps do not each add
    // another rounded frame. Metadata remains constant-size for many edits.
    let bytes = movie(
        0,
        3,
        &track(
            1,
            0,
            10,
            &[(1, 10)],
            &edit(0, &[(1, -1, 65536), (1, -1, 65536), (1, 0, 65536)]),
        ),
    );
    assert_eq!(parse(&bytes, 1, 10).unwrap().leading_frames, 7);

    let huge = movie(
        1,
        1,
        &track(
            1,
            1,
            44100,
            &[(1, 1024)],
            &edit(1, &[(u64::MAX, -1, 65536), (1, -1, 65536), (1, 0, 65536)]),
        ),
    );
    assert!(matches!(parse(&huge, 1, 44100), Err(Error::SizeOverflow)));
}

#[test]
fn rounded_media_timestamps_allow_less_than_one_tick_without_accumulating_drift() {
    let bytes = movie(
        0,
        1000,
        &track(1, 0, 1000, &[(2, 23), (1, 24), (1, 3)], &[]),
    );
    let bounds = parse(&bytes, 1, 44100).unwrap();
    for (position, timestamp) in [(0, 0), (1024, 23), (2048, 46), (3072, 70)] {
        assert_eq!(bounds.boundary(timestamp, position).unwrap(), position);
    }
    assert_eq!(bounds.boundary(24, 1024).unwrap(), 1024); // rounded upward
    assert_eq!(bounds.boundary(22, 1024).unwrap(), 971);
    assert_eq!(bounds.boundary(25, 1024).unwrap(), 1103);
    assert_eq!(bounds.boundary(1001, 44100).unwrap(), 44145); // one tick
    assert_eq!(bounds.boundary(69, 3072).unwrap(), 3072);
    assert_eq!(bounds.boundary(92, 4096).unwrap(), 4096);
    assert_eq!(bounds.boundary(115, 5120).unwrap(), 5072); // real drift
    assert_eq!(bounds.end, 3220);
    assert!(bounds.verify(4096, 3220).is_ok());
}

#[test]
fn rounded_media_end_is_clamped_to_decoded_frames_but_truncation_is_rejected() {
    let bytes = movie(
        0,
        1000,
        &track(1, 0, 1000, &[(2, 23), (1, 24), (1, 23)], &[]),
    );
    let bounds = parse(&bytes, 1, 44100).unwrap();
    assert_eq!(bounds.end, 4102); // 93 ms, rounded beyond 4,096 decoded frames
    assert!(bounds.verify(4096, 4096).is_ok());
    assert!(bounds.verify(3072, 3072).is_err());
    assert!(bounds.verify(4096, 4102).is_err());
    let bytes = movie(0, 1000, &track(1, 0, 1000, &[(4, 24)], &[]));
    assert!(parse(&bytes, 1, 44100).unwrap().verify(4096, 4096).is_err());
    let exact = movie(0, 44100, &track(1, 0, 44100, &[(4, 1024)], &[]));
    assert!(parse(&exact, 1, 44100).unwrap().verify(4095, 4095).is_err());
}

#[test]
fn extended_and_open_ended_atoms_and_parent_bounds() {
    let valid = movie(0, 44100, &track(1, 0, 44100, &[(2, 1024)], &[]));
    let mut extended = 1_u32.to_be_bytes().to_vec();
    extended.extend(b"moov");
    extended.extend((valid.len() as u64 + 8).to_be_bytes());
    extended.extend(&valid[8..]);
    assert_eq!(parse(&extended, 1, 44100).unwrap().end, 2048);
    let mut open = valid.clone();
    open[..4].copy_from_slice(&0_u32.to_be_bytes());
    assert_eq!(parse(&open, 1, 44100).unwrap().end, 2048);
    for length in 0..valid.len() {
        assert!(
            parse(&valid[..length], 1, 44100).is_err(),
            "length={length}"
        );
    }
    for size in [4_u32, 7, u32::MAX] {
        let mut bad = valid.clone();
        bad[8..12].copy_from_slice(&size.to_be_bytes());
        assert!(parse(&bad, 1, 44100).is_err());
    }
    let duplicate = [valid.clone(), valid].concat();
    assert!(parse(&duplicate, 1, 44100).is_err());
}

#[test]
fn invalid_timing_tables_and_overflow_are_errors() {
    for scale in [0, 44100] {
        let bytes = movie(0, scale, &track(1, 0, 0, &[], &[]));
        assert!(parse(&bytes, 1, 44100).is_err());
    }
    let overflow = movie(
        0,
        44100,
        &track(1, 0, 44100, &[(u32::MAX, u32::MAX); 3], &[]),
    );
    assert!(matches!(
        parse(&overflow, 1, 44100),
        Err(Error::SizeOverflow)
    ));
    let mut bad = movie(0, 44100, &track(1, 0, 44100, &[(2, 1024)], &[]));
    let position = bad.windows(4).position(|x| x == b"stts").unwrap();
    bad[position + 8..position + 12].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(parse(&bad, 1, 44100).is_err());
    let duplicate = movie(
        0,
        44100,
        &[track(1, 0, 44100, &[], &[]), track(1, 0, 44100, &[], &[])].concat(),
    );
    assert!(parse(&duplicate, 1, 44100).is_err());
}

#[test]
fn metadata_scan_restores_the_shared_file_position_on_success_error_and_cancellation() {
    let bytes = movie(0, 44100, &track(1, 0, 44100, &[(4, 1024)], &[]));
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    file.seek(SeekFrom::Start(11)).unwrap();
    let mut clone = file.try_clone().unwrap();
    assert!(playback(&mut clone, 1, 44100, &mut || false).is_ok());
    assert_eq!(file.stream_position().unwrap(), 11);
    assert!(playback(&mut clone, 9, 44100, &mut || false).is_err());
    assert_eq!(file.stream_position().unwrap(), 11);
    let mut calls = 0;
    assert!(matches!(
        playback(&mut clone, 1, 44100, &mut || {
            calls += 1;
            calls == 5
        }),
        Err(Error::Cancelled)
    ));
    assert_eq!(file.stream_position().unwrap(), 11);
}
