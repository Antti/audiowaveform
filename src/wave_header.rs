//! Bounded RIFF validation of declared chunk sizes and extensible channel masks.
//! Layout follows Microsoft's WAVEFORMATEX/WAVEFORMATEXTENSIBLE documentation.

use crate::Error;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
};

pub(crate) fn validate(file: &mut File, cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
    let length = file.metadata()?.len();
    if length < 12 {
        return Ok(());
    }
    let mut header = [0; 12];
    file.read_exact(&mut header)?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Ok(());
    }
    let end = u64::from(u32::from_le_bytes(
        header[4..8].try_into().expect("four bytes"),
    )) + 8;
    if end > length || end < 12 {
        return Err(Error::InvalidAudio("invalid RIFF length"));
    }
    let mut position = 12;
    while position < end {
        crate::error::poll(cancelled)?;
        if end - position < 8 {
            return Err(Error::InvalidAudio("truncated RIFF chunk header"));
        }
        file.seek(SeekFrom::Start(position))?;
        let mut chunk = [0; 8];
        file.read_exact(&mut chunk)?;
        let size = u64::from(u32::from_le_bytes(
            chunk[4..].try_into().expect("four bytes"),
        ));
        let data = position + 8;
        let next = data + size + size % 2;
        if next > end {
            return Err(Error::InvalidAudio("truncated RIFF chunk"));
        }
        if &chunk[..4] == b"fmt " {
            if size < 16 {
                return Err(Error::InvalidAudio("short WAV format descriptor"));
            }
            let mut format = [0; 40];
            let read = usize::try_from(size.min(40)).map_err(|_| Error::SizeOverflow)?;
            file.read_exact(&mut format[..read])?;
            let tag = u16::from_le_bytes([format[0], format[1]]);
            let channels = u16::from_le_bytes([format[2], format[3]]);
            if !(1..=18).contains(&channels) {
                return Err(Error::InvalidAudio(
                    "WAV requires 1 to 18 positioned channels",
                ));
            }
            if tag == 0xfffe {
                if read < 40 || u16::from_le_bytes([format[16], format[17]]) < 22 {
                    return Err(Error::InvalidAudio("short extensible WAV descriptor"));
                }
                let mask = u32::from_le_bytes(format[20..24].try_into().expect("four bytes"));
                if mask != 0 && mask.count_ones() != u32::from(channels) {
                    return Err(Error::InvalidAudio(
                        "WAV speaker mask disagrees with channel count",
                    ));
                }
            }
        }
        position = next;
    }
    Ok(())
}
