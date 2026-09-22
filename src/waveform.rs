/// Application-owned capacity measurements, excluding decoder/container memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
    pub decode_passes: u8,
    pub scratch_capacity_bytes: usize,
    /// Maximum simultaneous reducer buffers, including the final peak buffer.
    pub peak_capacity_bytes: usize,
}

/// Owned, interleaved signed min/max pairs in point/channel/min-max order.
#[derive(Debug)]
pub struct Waveform {
    pub(crate) data: Vec<i16>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: usize,
    pub(crate) source_frames: u64,
    pub(crate) frames_per_point: u64,
    pub(crate) statistics: Statistics,
}

impl Waveform {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn channels(&self) -> usize {
        self.channels
    }
    /// Playback frames after trimming, including supported MP4 leading silence.
    pub fn source_frames(&self) -> u64 {
        self.source_frames
    }
    pub fn len(&self) -> usize {
        self.data.len() / 2 / self.channels
    }
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
    pub fn duration(&self) -> f64 {
        self.source_frames as f64 / f64::from(self.sample_rate)
    }
    /// Nominal resolution. For exact-count timing use duration / len instead.
    pub fn frames_per_point(&self) -> u64 {
        self.frames_per_point
    }
    /// Heap bytes retained by the waveform, excluding the inline struct.
    pub fn allocated_bytes(&self) -> usize {
        self.data.capacity() * std::mem::size_of::<i16>()
    }
    pub fn data16(&self) -> &[i16] {
        &self.data
    }
    /// Converts lazily with signed truncation; allocates nothing.
    pub fn data8(&self) -> impl ExactSizeIterator<Item = i8> + DoubleEndedIterator + '_ {
        self.data.iter().map(|&value| (value / 256) as i8)
    }
    pub fn point(&self, index: usize, channel: usize) -> Option<[i16; 2]> {
        if index >= self.len() || channel >= self.channels {
            return None;
        }
        let offset = 2 * (index * self.channels + channel);
        Some([self.data[offset], self.data[offset + 1]])
    }
    pub fn statistics(&self) -> Statistics {
        self.statistics
    }
}
