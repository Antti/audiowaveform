//! AAC playback bounds from ISO BMFF timing, without loading media or tables.
//! The supported edit is one contiguous, unit-rate media segment. References:
//! <https://developer.apple.com/documentation/quicktime-file-format/edit_list_atom>
//! <https://developer.apple.com/documentation/quicktime-file-format/time-to-sample_atom>

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    ops::Range,
};

use crate::{Error, error::poll};

const INVALID: Error = Error::InvalidAudio("invalid MP4 playback timing");
const UNSUPPORTED_EDIT: Error = Error::InvalidAudio(
    "AAC/MP4 requires a single unit-rate media edit; decode complex edits to PCM",
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Playback {
    pub rate: u32,
    timescale: u32,
    start: u64,
    end: u64,
    media_frames: u64,
}

impl Playback {
    pub fn slice(self, position: u64, frames: usize, pts: i64) -> Result<Range<usize>, Error> {
        let timestamp = u64::try_from(pts).map_err(|_| INVALID)?;
        // This range is indexed in decoded frames. Do not silently apply it to
        // discontinuous or retimed packets, or to an upstream pre-trimmed stream.
        if u128::from(timestamp) * u128::from(self.rate)
            != u128::from(position) * u128::from(self.timescale)
        {
            return Err(Error::InvalidAudio("non-contiguous AAC/MP4 sample timing"));
        }
        let length = u64::try_from(frames).map_err(|_| Error::SizeOverflow)?;
        Ok(self.start.saturating_sub(position).min(length) as usize
            ..self.end.saturating_sub(position).min(length) as usize)
    }

    pub fn verify(self, decoded: u64, retained: u64) -> Result<(), Error> {
        if decoded < self.media_frames || retained != self.end - self.start {
            return Err(Error::InvalidAudio("truncated AAC/MP4 playback range"));
        }
        Ok(())
    }
}

/// `file` is a duplicate of the decoder's open file description. Restore its
/// physical cursor before decoding resumes, including when metadata is invalid.
/// No pathname is reopened, so both decoding passes keep using the same file.
pub(crate) fn playback(
    file: &mut File,
    track: u32,
    rate: u32,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<Playback>, Error> {
    let saved = file.stream_position()?;
    let length = file.metadata()?.len();
    let result = Parser {
        input: &mut *file,
        cancelled,
    }
    .playback(length, track, rate);
    let restored = file.seek(SeekFrom::Start(saved));
    let result = result?;
    restored?;
    Ok(result)
}

#[derive(Clone, Copy)]
struct Atom {
    kind: [u8; 4],
    data: u64,
    end: u64,
}

struct Parser<'a, R, C> {
    input: &'a mut R,
    cancelled: &'a mut C,
}

impl<R: Read + Seek, C: FnMut() -> bool> Parser<'_, R, C> {
    fn playback(&mut self, length: u64, track: u32, rate: u32) -> Result<Option<Playback>, Error> {
        let root = Atom {
            kind: *b"root",
            data: 0,
            end: length,
        };
        let moov = self.required(root, *b"moov")?;
        // Fragment sample timing lives in moof/trun, not this initial stts.
        // Leave fragmented inputs on the existing decoded-frame timeline.
        if self.find(moov, *b"mvex")?.is_some() {
            return Ok(None);
        }
        let movie = self.required(moov, *b"mvhd")?;
        let movie_scale = self.timescale(movie)?;
        let mut selected = None;
        let mut position = moov.data;
        while position < moov.end {
            let atom = self.atom(position, moov.end)?;
            if atom.kind == *b"trak" {
                let header = self.required(atom, *b"tkhd")?;
                let offset = match self.version(header)? {
                    0 => 12,
                    1 => 20,
                    _ => return Err(INVALID),
                };
                if u32::from_be_bytes(self.bytes(header, offset)?) == track
                    && selected.replace(atom).is_some()
                {
                    return Err(INVALID);
                }
            }
            position = atom.end;
        }
        let selected = selected.ok_or(INVALID)?;
        let media = self.required(selected, *b"mdia")?;
        let header = self.required(media, *b"mdhd")?;
        let media_scale = self.timescale(header)?;
        let info = self.required(media, *b"minf")?;
        let table = self.required(info, *b"stbl")?;
        let stts = self.required(table, *b"stts")?;
        let media_duration = self.duration(stts)?;
        let media_frames = frames(u128::from(media_duration), media_scale, rate)?;
        let edit = match self.find(selected, *b"edts")? {
            Some(edts) => match self.find(edts, *b"elst")? {
                Some(elst) => self.edit(elst)?,
                None => None,
            },
            None => None,
        };
        let (start, end) = match edit {
            Some((start, duration)) => {
                let start_frame = frames(u128::from(start), media_scale, rate)?;
                // Add in rational time before rounding, and clamp rounded movie
                // duration to the sample table's precise end of media.
                let end_time = u128::from(start) * u128::from(movie_scale)
                    + u128::from(duration) * u128::from(media_scale);
                let numerator = end_time
                    .checked_mul(u128::from(rate))
                    .ok_or(Error::SizeOverflow)?;
                let denominator = u128::from(movie_scale) * u128::from(media_scale);
                let end_frame = u64::try_from(numerator.div_ceil(denominator))
                    .map_err(|_| Error::SizeOverflow)?
                    .min(media_frames);
                (start_frame, end_frame)
            }
            None => (0, media_frames),
        };
        if start > end {
            return Err(INVALID);
        }
        Ok(Some(Playback {
            rate,
            timescale: media_scale,
            start,
            end,
            media_frames,
        }))
    }

    fn atom(&mut self, position: u64, limit: u64) -> Result<Atom, Error> {
        poll(self.cancelled)?;
        let remaining = limit.checked_sub(position).ok_or(INVALID)?;
        if remaining < 8 {
            return Err(INVALID);
        }
        self.input.seek(SeekFrom::Start(position))?;
        let mut header = [0; 8];
        self.input.read_exact(&mut header)?;
        let short = u32::from_be_bytes(header[..4].try_into().unwrap());
        let (size, width) = match short {
            0 => (remaining, 8),
            1 => {
                if remaining < 16 {
                    return Err(INVALID);
                }
                let mut size = [0; 8];
                self.input.read_exact(&mut size)?;
                (u64::from_be_bytes(size), 16)
            }
            n => (u64::from(n), 8),
        };
        if size < width || size > remaining {
            return Err(INVALID);
        }
        Ok(Atom {
            kind: header[4..].try_into().unwrap(),
            data: position + width,
            end: position + size,
        })
    }

    fn find(&mut self, parent: Atom, kind: [u8; 4]) -> Result<Option<Atom>, Error> {
        let mut found = None;
        let mut position = parent.data;
        while position < parent.end {
            let atom = self.atom(position, parent.end)?;
            if atom.kind == kind && found.replace(atom).is_some() {
                return Err(INVALID);
            }
            position = atom.end;
        }
        Ok(found)
    }

    fn required(&mut self, parent: Atom, kind: [u8; 4]) -> Result<Atom, Error> {
        self.find(parent, kind)?.ok_or(INVALID)
    }

    fn bytes<const N: usize>(&mut self, atom: Atom, offset: u64) -> Result<[u8; N], Error> {
        let position = atom.data.checked_add(offset).ok_or(INVALID)?;
        if position
            .checked_add(N as u64)
            .is_none_or(|end| end > atom.end)
        {
            return Err(INVALID);
        }
        self.input.seek(SeekFrom::Start(position))?;
        let mut bytes = [0; N];
        self.input.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    fn version(&mut self, atom: Atom) -> Result<u8, Error> {
        let header = self.bytes::<4>(atom, 0)?;
        if header[1..] != [0, 0, 0] && atom.kind != *b"tkhd" {
            return Err(INVALID);
        }
        Ok(header[0])
    }

    fn timescale(&mut self, atom: Atom) -> Result<u32, Error> {
        let offset = match self.version(atom)? {
            0 => 12,
            1 => 20,
            _ => return Err(INVALID),
        };
        let value = u32::from_be_bytes(self.bytes(atom, offset)?);
        if value == 0 {
            return Err(INVALID);
        }
        Ok(value)
    }

    fn duration(&mut self, atom: Atom) -> Result<u64, Error> {
        if self.version(atom)? != 0 {
            return Err(INVALID);
        }
        let entries = u32::from_be_bytes(self.bytes(atom, 4)?);
        if atom.end - atom.data != 8 + u64::from(entries) * 8 {
            return Err(INVALID);
        }
        let mut duration = 0_u64;
        for index in 0..entries {
            poll(self.cancelled)?;
            let entry = self.bytes::<8>(atom, 8 + u64::from(index) * 8)?;
            let count = u32::from_be_bytes(entry[..4].try_into().unwrap());
            let delta = u32::from_be_bytes(entry[4..].try_into().unwrap());
            duration = duration
                .checked_add(u64::from(count) * u64::from(delta))
                .ok_or(Error::SizeOverflow)?;
        }
        Ok(duration)
    }

    fn edit(&mut self, atom: Atom) -> Result<Option<(u64, u64)>, Error> {
        let version = self.version(atom)?;
        let entries = u32::from_be_bytes(self.bytes(atom, 4)?);
        let width = match version {
            0 => 12,
            1 => 20,
            _ => return Err(INVALID),
        };
        if atom.end - atom.data != 8 + u64::from(entries) * width {
            return Err(INVALID);
        }
        if entries == 0 {
            return Ok(None);
        }
        if entries != 1 {
            return Err(UNSUPPORTED_EDIT);
        }
        let (duration, start, rate) = match version {
            0 => (
                u64::from(u32::from_be_bytes(self.bytes(atom, 8)?)),
                i64::from(i32::from_be_bytes(self.bytes(atom, 12)?)),
                u32::from_be_bytes(self.bytes(atom, 16)?),
            ),
            _ => (
                u64::from_be_bytes(self.bytes(atom, 8)?),
                i64::from_be_bytes(self.bytes(atom, 16)?),
                u32::from_be_bytes(self.bytes(atom, 24)?),
            ),
        };
        if start < 0 || rate != 0x0001_0000 {
            return Err(UNSUPPORTED_EDIT);
        }
        Ok(Some((start as u64, duration)))
    }
}

fn frames(time: u128, scale: u32, rate: u32) -> Result<u64, Error> {
    u64::try_from((time * u128::from(rate)).div_ceil(u128::from(scale)))
        .map_err(|_| Error::SizeOverflow)
}

#[cfg(test)]
mod tests;
