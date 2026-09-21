//! Incremental audio peak generation without rendering or waveform file formats.
//!
//! ```no_run
//! use waveform_core::{generate, Options, Resolution};
//! let waveform = generate("recording.wav", Options {
//!     resolution: Resolution::Points(110),
//!     ..Options::default()
//! })?;
//! let peaks: Vec<i8> = waveform.data8().collect();
//! assert_eq!(peaks.len(), 220);
//! # Ok::<(), waveform_core::Error>(())
//! ```

#![forbid(unsafe_code)]

mod decode;
mod error;
#[cfg(feature = "mkv")]
mod matroska;
mod options;
mod pcm;
mod reduce;
#[cfg(feature = "wav")]
mod wave_header;
mod waveform;

pub use decode::{generate, generate_with_cancel};
pub use error::Error;
pub use options::{ChannelMode, Gain, Options, Resolution};
pub use pcm::{PcmFormat, PcmStream, generate_pcm, generate_pcm_with_cancel};
pub use waveform::{Statistics, Waveform};
