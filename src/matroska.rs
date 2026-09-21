//! Verify an unknown-size Matroska document before accepting its physical EOF.
//! Symphonia 0.6.1 can report UnexpectedEof after the last complete element.
//! This is an extent/block-header check, not an alternate media decoder.
//! References: RFC 8794 section 6.2; RFC 9559 section 10 (block lacing).

use crate::{Error, error::poll};
use std::io::{Read, Seek, SeekFrom};
use symphonia::core::io::{MediaSource, MediaSourceStream, ReadBytes};

const INVALID: Error = Error::InvalidAudio("incomplete or invalid Matroska element");

pub(crate) fn complete_at_eof<C: FnMut() -> bool>(
    mut stream: MediaSourceStream<'static>,
    cancelled: &mut C,
) -> Result<MediaSourceStream<'static>, Error> {
    let length = stream.byte_len().ok_or(INVALID)?;
    if stream.pos() != length {
        return Err(INVALID);
    }
    stream.seek(SeekFrom::Start(0))?;
    let mut ends = [length; 4];
    let mut depth = 0;
    let mut unknown = false;
    while stream.pos() < length {
        poll(cancelled)?;
        while depth > 0 && stream.pos() == ends[depth] {
            depth -= 1;
        }
        let limit = ends[depth];
        let (id, _) = vint(&mut stream, limit, true)?;
        let (size, width) = vint(&mut stream, limit, false)?;
        let open = size == (1_u64 << (7 * width)) - 1;
        if open {
            // Only Segment and Cluster allow unknown size. They add no fixed
            // extent; their complete children must fit the enclosing extent.
            if !matches!(id, 0x18538067 | 0x1f43b675) {
                return Err(INVALID);
            }
            unknown = true;
            continue;
        }
        let end = stream
            .pos()
            .checked_add(size)
            .filter(|end| *end <= limit)
            .ok_or(INVALID)?;
        if matches!(id, 0x18538067 | 0x1f43b675 | 0xa0) {
            depth += 1;
            if depth == ends.len() {
                return Err(INVALID);
            }
            ends[depth] = end;
        } else {
            if matches!(id, 0xa1 | 0xa3) {
                block(&mut stream, end)?;
            }
            stream.seek(SeekFrom::Start(end))?;
        }
    }
    if !unknown {
        return Err(INVALID);
    }
    Ok(stream)
}

fn byte(stream: &mut MediaSourceStream<'_>, limit: u64) -> Result<u8, Error> {
    if stream.pos() >= limit {
        return Err(INVALID);
    }
    let mut value = [0];
    stream.read_exact(&mut value)?;
    Ok(value[0])
}

fn vint(stream: &mut MediaSourceStream<'_>, limit: u64, id: bool) -> Result<(u64, u32), Error> {
    let first = byte(stream, limit)?;
    let width = first.leading_zeros() + 1;
    if width > if id { 4 } else { 8 } {
        return Err(INVALID);
    }
    let mut value = u64::from(if id {
        first
    } else {
        first & ((0xff_u16 >> width) as u8)
    });
    for _ in 1..width {
        value = (value << 8) | u64::from(byte(stream, limit)?);
    }
    Ok((value, width))
}

fn block(stream: &mut MediaSourceStream<'_>, end: u64) -> Result<(), Error> {
    if vint(stream, end, false)?.0 == 0 {
        return Err(INVALID);
    }
    byte(stream, end)?;
    byte(stream, end)?;
    let lacing = (byte(stream, end)? >> 1) & 3;
    if lacing == 0 {
        return if stream.pos() < end {
            Ok(())
        } else {
            Err(INVALID)
        };
    }
    let count = u64::from(byte(stream, end)?) + 1;
    if count == 1 {
        return Err(INVALID);
    }
    let mut used = 0_u64;
    match lacing {
        1 => {
            for _ in 1..count {
                loop {
                    let value = byte(stream, end)?;
                    used = used.checked_add(u64::from(value)).ok_or(INVALID)?;
                    if value != 255 {
                        break;
                    }
                }
            }
        }
        2 => {
            let remaining = end - stream.pos();
            return if remaining > 0 && remaining.is_multiple_of(count) {
                Ok(())
            } else {
                Err(INVALID)
            };
        }
        3 => {
            let mut size = vint(stream, end, false)?.0;
            used = size;
            for _ in 2..count {
                let (encoded, width) = vint(stream, end, false)?;
                let bias = (1_i128 << (7 * width - 1)) - 1;
                size = u64::try_from(i128::from(size) + i128::from(encoded) - bias)
                    .map_err(|_| INVALID)?;
                used = used.checked_add(size).ok_or(INVALID)?;
            }
        }
        _ => unreachable!(),
    }
    if used >= end - stream.pos() {
        return Err(INVALID);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn check(payload: &[u8]) -> Result<MediaSourceStream<'static>, Error> {
        // Unknown-size Segment containing one complete SimpleBlock.
        let mut bytes = vec![
            0x18,
            0x53,
            0x80,
            0x67,
            0xff,
            0xa3,
            0x80 | payload.len() as u8,
        ];
        bytes.extend_from_slice(payload);
        let mut stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
        stream.seek(SeekFrom::End(0)).unwrap();
        complete_at_eof(stream, &mut || false)
    }

    #[test]
    fn complete_unlaced_and_all_three_lacing_modes_are_accepted() {
        for payload in [
            vec![0x81, 0, 0, 0, 55],
            vec![0x81, 0, 0, 2, 2, 1, 2, 10, 20, 21, 30, 31, 32],
            vec![0x81, 0, 0, 4, 2, 10, 11, 20, 21, 30, 31],
            vec![0x81, 0, 0, 6, 2, 0x81, 0xc0, 10, 20, 21, 30, 31, 32],
            vec![0x81, 0, 0, 6, 2, 0x83, 0xbd, 10, 11, 12, 20, 30, 31],
        ] {
            assert!(check(&payload).is_ok(), "{payload:?}");
        }
    }

    #[test]
    fn truncated_headers_and_invalid_laces_are_errors() {
        for payload in [
            vec![],
            vec![0x81],
            vec![0x81, 0, 0],
            vec![0x81, 0, 0, 0],
            vec![0x81, 0, 0, 2, 2, 255],
            vec![0x81, 0, 0, 4, 2, 10, 11, 20, 21, 30],
            vec![0x81, 0, 0, 6, 2, 0x81, 0x80, 10, 20],
            vec![0x81, 0, 0, 6, 2, 1, 0],
            vec![0x81, 0, 0, 4, 0, 10],
        ] {
            assert!(check(&payload).is_err(), "{payload:?}");
        }
    }
}
