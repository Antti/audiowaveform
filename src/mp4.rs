//! AAC playback bounds from ISO BMFF timing, without loading media or tables.
//! Supports leading empty edits and one contiguous, unit-rate media segment.
//! References:
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
    minimum_media_frames: u64,
    pub leading_frames: u64,
}

impl Playback {
    /// Expected playback length, verified against actual frames at EOF. A
    /// rounded media end can overstate this count and require peak generation
    /// to replay using the smaller observed count.
    pub fn frame_count(self) -> u64 {
        // Parsing already checked the ordering and this sum for overflow.
        self.end - self.start + self.leading_frames
    }

    /// Place decoded samples on the media timeline. A short sample-table slot
    /// clips the packet's tail; a long slot leaves silence before the next
    /// packet. Never fill an unverified trailing gap at EOF.
    pub fn packet(
        self,
        position: u64,
        decoded: usize,
        pts: i64,
        duration: u64,
    ) -> Result<PacketSlice, Error> {
        let timestamp = u64::try_from(pts).map_err(|_| INVALID)?;
        let start = self.boundary(timestamp, position)?;
        if start < position {
            return Err(Error::InvalidAudio("backwards AAC/MP4 sample timing"));
        }
        let decoded_end = start
            .checked_add(u64::try_from(decoded).map_err(|_| Error::SizeOverflow)?)
            .ok_or(Error::SizeOverflow)?;
        let timestamp_end = timestamp.checked_add(duration).ok_or(Error::SizeOverflow)?;
        let end = self
            .boundary(timestamp_end, decoded_end)?
            .min(decoded_end)
            .max(start);
        Ok(PacketSlice {
            silence: self.end.min(start).saturating_sub(self.start.max(position)),
            samples: self.slice(start, end - start),
            end,
        })
    }

    fn boundary(self, timestamp: u64, expected: u64) -> Result<u64, Error> {
        // Preserve continuous PCM when absolute boundaries differ by less than
        // one media-clock tick. Comparing absolute times avoids accumulating a
        // per-packet rounding allowance or independently rounding durations.
        if (u128::from(timestamp) * u128::from(self.rate))
            .abs_diff(u128::from(expected) * u128::from(self.timescale))
            < u128::from(self.rate)
        {
            Ok(expected)
        } else {
            frames(u128::from(timestamp), self.timescale, self.rate)
        }
    }

    fn slice(self, position: u64, length: u64) -> Range<usize> {
        // Only called for decoded buffers, so length already fits usize.
        self.start.saturating_sub(position).min(length) as usize
            ..self.end.saturating_sub(position).min(length) as usize
    }

    pub fn verify(self, media_end: u64, retained: u64) -> Result<(), Error> {
        // A coarse final timestamp can round slightly beyond the decoded end.
        // Clamp to the frames actually available, never synthesize AAC padding.
        let expected = self
            .end
            .min(media_end)
            .saturating_sub(self.start)
            .checked_add(self.leading_frames)
            .ok_or(Error::SizeOverflow)?;
        if media_end < self.minimum_media_frames || retained != expected {
            return Err(Error::InvalidAudio("truncated AAC/MP4 playback range"));
        }
        Ok(())
    }
}

pub(crate) struct PacketSlice {
    pub silence: u64,
    pub samples: Range<usize>,
    pub end: u64,
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

struct Edit {
    start: u64,
    duration: u64,
    leading: u64,
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
        let (start, end, leading_frames) = match edit {
            Some(Edit {
                start,
                duration,
                leading,
            }) => {
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
                (
                    start_frame,
                    end_frame,
                    frames(u128::from(leading), movie_scale, rate)?,
                )
            }
            None => (0, media_frames, 0),
        };
        if start > end {
            return Err(INVALID);
        }
        // Require available media to extend strictly past the preceding media tick.
        // With a sample-rate clock this still requires every declared frame.
        let minimum_media_frames = if media_duration == 0 {
            0
        } else {
            u64::try_from(
                u128::from(media_duration - 1) * u128::from(rate) / u128::from(media_scale),
            )
            .map_err(|_| Error::SizeOverflow)?
            .checked_add(1)
            .ok_or(Error::SizeOverflow)?
        };
        leading_frames
            .checked_add(end - start)
            .ok_or(Error::SizeOverflow)?;
        Ok(Some(Playback {
            rate,
            timescale: media_scale,
            start,
            end,
            minimum_media_frames,
            leading_frames,
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

    fn edit(&mut self, atom: Atom) -> Result<Option<Edit>, Error> {
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
        let mut leading = 0_u64;
        for index in 0..entries {
            poll(self.cancelled)?;
            let offset = 8 + u64::from(index) * width;
            let (duration, start, rate) = match version {
                0 => (
                    u64::from(u32::from_be_bytes(self.bytes(atom, offset)?)),
                    i64::from(i32::from_be_bytes(self.bytes(atom, offset + 4)?)),
                    u32::from_be_bytes(self.bytes(atom, offset + 8)?),
                ),
                _ => (
                    u64::from_be_bytes(self.bytes(atom, offset)?),
                    i64::from_be_bytes(self.bytes(atom, offset + 8)?),
                    u32::from_be_bytes(self.bytes(atom, offset + 16)?),
                ),
            };
            if rate != 0x0001_0000 {
                return Err(UNSUPPORTED_EDIT);
            }
            if start == -1 {
                leading = leading.checked_add(duration).ok_or(Error::SizeOverflow)?;
            } else if start >= 0 && index == entries - 1 {
                return Ok(Some(Edit {
                    start: start as u64,
                    duration,
                    leading,
                }));
            } else {
                return Err(UNSUPPORTED_EDIT);
            }
        }
        // A sequence of empty edits with no media segment has no audio range.
        Err(UNSUPPORTED_EDIT)
    }
}

fn frames(time: u128, scale: u32, rate: u32) -> Result<u64, Error> {
    u64::try_from((time * u128::from(rate)).div_ceil(u128::from(scale)))
        .map_err(|_| Error::SizeOverflow)
}

#[cfg(test)]
mod tests;
