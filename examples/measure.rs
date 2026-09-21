//! Development measurement helper; not a compatibility CLI or waveform exporter.
use waveform_core::{Gain, Options, Resolution, generate};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("provide an audio path")?;
    let waveform = generate(
        path,
        Options {
            resolution: Resolution::Points(110),
            gain: if std::env::args().nth(2).as_deref() == Some("normalize") {
                Gain::Normalize
            } else {
                Gain::Fixed(1.0)
            },
            ..Options::default()
        },
    )?;
    println!(
        "frames={} channels={} points={} duration={} stats={:?}",
        waveform.source_frames(),
        waveform.channels(),
        waveform.len(),
        waveform.duration(),
        waveform.statistics()
    );
    Ok(())
}
