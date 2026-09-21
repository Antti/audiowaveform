use std::{collections::TryReserveError, fmt, io};

/// Failures from validation, decoding, cancellation, and checked allocation.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    InvalidOption(&'static str),
    InvalidAudio(&'static str),
    InputChanged,
    Cancelled,
    InternalPanic,
    SizeOverflow,
    Allocation(TryReserveError),
    Io(io::Error),
    Decode(symphonia::core::errors::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOption(message) => write!(f, "invalid option: {message}"),
            Self::InvalidAudio(message) => write!(f, "invalid audio: {message}"),
            Self::InputChanged => f.write_str("audio changed during generation"),
            Self::Cancelled => f.write_str("generation cancelled"),
            Self::InternalPanic => f.write_str("audio processing panicked"),
            Self::SizeOverflow => f.write_str("waveform dimensions exceed addressable memory"),
            Self::Allocation(error) => write!(f, "cannot allocate waveform memory: {error}"),
            Self::Io(error) => write!(f, "audio I/O: {error}"),
            Self::Decode(error) => write!(f, "audio decoding: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Allocation(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Decode(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<symphonia::core::errors::Error> for Error {
    fn from(error: symphonia::core::errors::Error) -> Self {
        Self::Decode(error)
    }
}

impl From<TryReserveError> for Error {
    fn from(error: TryReserveError) -> Self {
        Self::Allocation(error)
    }
}

pub(crate) fn elements(points: u64, channels: usize) -> Result<usize, Error> {
    usize::try_from(points)
        .ok()
        .and_then(|p| p.checked_mul(channels))
        .and_then(|p| p.checked_mul(2))
        .ok_or(Error::SizeOverflow)
}

pub(crate) fn poll(cancelled: &mut impl FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
