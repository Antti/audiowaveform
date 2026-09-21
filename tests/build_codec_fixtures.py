#!/usr/bin/env python3
"""Rebuild owned synthetic codec fixtures; FFmpeg is only a development tool."""
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import wave

ROOT = Path(__file__).resolve().parent / "fixtures" / "codecs"
ROOT.mkdir(parents=True, exist_ok=True)
rate, frames = 48000, 12000
samples = [[int(12000 * math.sin(2 * math.pi * hz * i / rate)) for hz in (440, 880)]
           for i in range(frames)]
source = ROOT / "source.wav"
with wave.open(str(source), "wb") as out:
    out.setparams((2, 2, rate, frames, "NONE", "not compressed"))
    out.writeframes(b"".join(struct.pack("<hh", *frame) for frame in samples))
cases = []
commands = []
def ffmpeg(arguments):
    command = ["ffmpeg", "-v", "error", "-y"] + arguments
    subprocess.run(command, check=True)
    commands.append([part.replace(str(ROOT), "$FIXTURES") for part in command])

for feature, name, arguments, lossless in [
    ("wav", "pcm.wav", ["-c:a", "pcm_s16le"], True),
    ("wav", "ima.wav", ["-c:a", "adpcm_ima_wav"], False),
    ("wav", "ms.wav", ["-c:a", "adpcm_ms"], False),
    ("flac", "audio.flac", ["-c:a", "flac"], True),
    ("ogg", "flac.ogg", ["-c:a", "flac", "-f", "ogg"], True),
    ("ogg", "vorbis.ogg", ["-c:a", "vorbis", "-strict", "experimental"], False),
    ("mp2", "audio.mp2", ["-c:a", "mp2", "-b:a", "128k"], False),
    ("mp3", "audio.mp3", ["-c:a", "libmp3lame", "-b:a", "128k"], False),
    ("aac", "audio.aac", ["-c:a", "aac", "-b:a", "128k", "-f", "adts"], False),
    ("m4a", "aac.m4a", ["-c:a", "aac", "-b:a", "128k"], False),
    ("m4a", "alac.m4a", ["-c:a", "alac"], True),
    ("aiff", "audio.aiff", ["-c:a", "pcm_s16be"], True),
    ("caf", "pcm.caf", ["-c:a", "pcm_s16le"], True),
    ("caf", "alac.caf", ["-c:a", "alac"], True),
    ("mkv", "live.webm", ["-c:a", "vorbis", "-strict", "experimental", "-live", "1"], False),
    ("mkv", "pcm.mkv", ["-c:a", "pcm_s16le"], True),
]:
    ffmpeg(["-i", str(source)] + arguments + [str(ROOT / name)])
    cases.append({"feature": feature, "file": name, "lossless": lossless})

# MPEG-1 Layer I silence: sync/version/layer/protection, 128 kb/s, 48 kHz,
# mono. All 32 subband bit-allocation entries are zero; there are no scale
# factors or sample payloads. Frame length = 12 * bitrate / rate * 4 = 128.
header = (0x7ff << 21) | (3 << 19) | (3 << 17) | (1 << 16) | (4 << 12) | (1 << 10) | (3 << 6)
(ROOT / "silence.mp1").write_bytes((struct.pack(">I", header) + bytes(124)) * 32)
cases.append({"feature": "mp1", "file": "silence.mp1", "lossless": False, "silent": True})

# A video track, a non-default tone, then a default silent audio track.
ffmpeg(["-f", "lavfi", "-i", "color=black:s=16x16:r=4:d=0.25",
        "-i", str(source), "-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo:d=0.25",
        "-map", "0:v", "-map", "1:a", "-map", "2:a", "-c:v", "mpeg4", "-c:a", "alac",
        "-disposition:a:0", "0", "-disposition:a:1", "default", str(ROOT / "default-track.mp4")])
ffmpeg(["-i", str(source), "-i", str(source), "-map", "0:a", "-map", "1:a",
        "-c:a:0", "libopus", "-c:a:1", "alac", "-disposition:a:0", "default",
        "-disposition:a:1", "0", str(ROOT / "unsupported-default.mp4")])


ffmpeg(["-f", "lavfi", "-i", "color=black:s=16x16:r=4:d=0.25",
        "-i", str(source), "-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo:d=0.25",
        "-map", "0:v", "-map", "1:a", "-map", "2:a", "-c:v", "ffv1", "-c:a", "pcm_s16le",
        "-disposition:a:0", "0", "-disposition:a:1", "default", "-write_crc32", "0", str(ROOT / "default-track.mkv")])
ffmpeg(["-i", str(source), "-i", str(source), "-map", "0:a", "-map", "1:a",
        "-c:a:0", "libopus", "-c:a:1", "pcm_s16le", "-disposition:a:0", "default",
        "-disposition:a:1", "0", str(ROOT / "unsupported-default.mkv")])

# Make FlagDefault=1 explicit on the last track. FFmpeg otherwise omits this
# schema-default value; Symphonia 0.6.1 only reports explicitly present flags.
# Replace that track's FlagLacing=0 element (same three-byte size). No blocks
# use lacing, so its default value is harmless. CRC emission is disabled above.
track_path = ROOT / "default-track.mkv"
track_bytes = track_path.read_bytes()
marker = bytes.fromhex("9c8100")
assert track_bytes.count(marker) == 3
position = track_bytes.rindex(marker)
assert position < 1024
track_path.write_bytes(track_bytes[:position] + bytes.fromhex("888101") + track_bytes[position + 3:])

# Pure arithmetic expectations, never the output of a waveform implementation.
expected = {}
for mode in ("mono", "split"):
    signal = [[sum(frame) / 2] for frame in samples] if mode == "mono" else samples
    peaks = []
    for i in range(110):
        bucket = signal[i * frames // 110:(i + 1) * frames // 110]
        for channel in range(len(signal[0])):
            values = [frame[channel] for frame in bucket]
            peaks += [int(min(values)), int(max(values))]
    expected[mode] = peaks
manifest = {"sample_rate": rate, "source_frames": frames, "cases": cases, "expected16": expected,
            "generator": "two sines, 440 and 880 Hz; 12000 amplitude; signed 16-bit PCM; 12000 frames",
            "ffmpeg": subprocess.check_output(["ffmpeg", "-version"], text=True).splitlines()[0],
            "commands": commands,
            "sha256": {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
                       for p in sorted(ROOT.iterdir()) if p.suffix != ".json"}}
(ROOT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(f"Built {len(cases)} codec cases plus track-selection fixtures from a synthetic signal")
