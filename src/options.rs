use crate::Error;

/// Point boundaries are measured in audio frames, not individual channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Points(u32),
    FramesPerPoint(u32),
    PointsPerSecond(u32),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChannelMode {
    #[default]
    Mono,
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gain {
    Fixed(f64),
    Normalize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    pub resolution: Resolution,
    pub channels: ChannelMode,
    pub gain: Gain,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            resolution: Resolution::FramesPerPoint(256),
            channels: ChannelMode::Mono,
            gain: Gain::Fixed(1.0),
        }
    }
}

impl Options {
    /// Validate before opening the input or allocating an output buffer.
    pub fn validate(self) -> Result<(), Error> {
        match self.resolution {
            Resolution::Points(0) | Resolution::PointsPerSecond(0) => {
                return Err(Error::InvalidOption("point count/rate must be positive"));
            }
            Resolution::FramesPerPoint(0 | 1) => {
                return Err(Error::InvalidOption(
                    "frames per point must be at least two",
                ));
            }
            _ => {}
        }
        if let Gain::Fixed(gain) = self.gain
            && (!gain.is_finite() || gain < 0.0)
        {
            return Err(Error::InvalidOption("gain must be finite and nonnegative"));
        }
        Ok(())
    }
}
