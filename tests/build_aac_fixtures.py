#!/usr/bin/env python3
"""Owned synthetic AAC timing fixtures. FFmpeg is only a development tool."""
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import tempfile
import wave

ROOT = Path(__file__).resolve().parent / "fixtures" / "aac"
ROOT.mkdir(parents=True, exist_ok=True)
cases, commands = [], []

with tempfile.TemporaryDirectory() as temporary:
    source = Path(temporary) / "source.wav"

    def encode(name, rate, frames, channels=1, signal="tone", extra=()):
        samples = []
        for i in range(frames):
            audible = signal != "silence" and (signal != "edges" or frames // 3 <= i < 2 * frames // 3)
            samples.extend(int(12000 * math.sin(2 * math.pi * hz * i / rate)) if audible else 0
                           for hz in (440, 880)[:channels])
        with wave.open(str(source), "wb") as out:
            out.setparams((channels, 2, rate, frames, "NONE", "not compressed"))
            out.writeframes(struct.pack("<" + "h" * len(samples), *samples))
        command = ["ffmpeg", "-v", "error", "-y", "-i", str(source), "-c:a", "aac",
                   "-movie_timescale", str(rate), *extra, str(ROOT / name)]
        subprocess.run(command, check=True)
        commands.append([part.replace(str(source), "$SOURCE").replace(str(ROOT), "$FIXTURES") for part in command])
        cases.append(dict(file=name, rate=rate, frames=frames, channels=channels, signal=signal))

    encode("short-44100.m4a", 44100, 2205)
    encode("short-48000.m4a", 48000, 2400)
    encode("short-32000.m4a", 32000, 1600)
    encode("short-88200.m4a", 88200, 4410)
    encode("short-96000.m4a", 96000, 4800)
    encode("odd.m4a", 44100, 2206)
    encode("one.m4a", 44100, 1)
    encode("silence.m4a", 44100, 2205, signal="silence")
    encode("silent-edges.m4a", 48000, 12000, channels=2, signal="edges")
    encode("coarse-movie-clock.m4a", 44100, 2206, extra=("-movie_timescale", "1000"))
    # FFmpeg rounds this edit down to 50 movie ticks = 2,205 playback frames.
    cases[-1]["playback_frames"] = 2205
    encode("fragmented.m4a", 44100, 2205, extra=("-movflags", "empty_moov+frag_keyframe"))
    cases[-1]["fragmented"] = True

    # Video precedes two AAC tracks with distinct ranges. The first audio track
    # has a short tone, the second is longer and has silent edges.
    command = ["ffmpeg", "-v", "error", "-y", "-f", "lavfi", "-i", "color=black:s=16x16:r=20:d=0.05",
               "-i", str(ROOT / "short-44100.m4a"), "-i", str(ROOT / "silent-edges.m4a"),
               "-map", "0:v", "-map", "1:a", "-map", "2:a", "-c:v", "mpeg4", "-c:a", "copy",
               "-movie_timescale", "44100", str(ROOT / "tracks.mp4")]
    subprocess.run(command, check=True)
    commands.append([part.replace(str(ROOT), "$FIXTURES") for part in command])

    command = ["ffmpeg", "-v", "error", "-y", "-itsoffset", "0.1", "-i",
               str(ROOT / "short-44100.m4a"), "-c:a", "copy", "-movie_timescale", "44100",
               str(ROOT / "offset.m4a")]
    subprocess.run(command, check=True)
    commands.append([part.replace(str(ROOT), "$FIXTURES") for part in command])
    cases.append(dict(file="offset.m4a", rate=44100, frames=2205, channels=1,
                      signal="tone", media_start=0, leading_frames=3386, playback_frames=6615))


def coarse_media_clock(data, end, with_edit):
    """Re-express known packet boundaries in millisecond media ticks.

    moov follows mdat in this fixture, so resizing tables preserves offsets.
    Round 0,1024,2048,3072 source frames to 0,23,46,70 ms. End=73 rounds the
    original short last packet; end=93 describes all 4096 decoded frames.
    """
    result, position = bytearray(), 0
    while position < len(data):
        size, kind = struct.unpack_from(">I4s", data, position)
        assert size >= 8
        payload = bytearray(data[position + 8:position + size])
        if kind in (b"moov", b"trak", b"mdia", b"minf", b"stbl", b"edts"):
            payload = coarse_media_clock(payload, end, with_edit)
        elif kind in (b"mdhd", b"mvhd"):
            payload[12:20] = struct.pack(">II", 1000, end if kind == b"mdhd" or not with_edit else 50)
        elif kind == b"tkhd":
            payload[20:24] = struct.pack(">I", 50 if with_edit else end)
        elif kind == b"elst":
            if with_edit:
                payload[8:16] = struct.pack(">Ii", 50, 23)
            else:
                kind = b"free"
        elif kind == b"stts":
            payload = bytes(4) + struct.pack(">IIIIIII", 3, 2, 23, 1, 24, 1, end - 70)
        result.extend(struct.pack(">I4s", len(payload) + 8, kind) + payload)
        position += size
    return result


for name, end, with_edit, media_start, playback_frames in [
    ("coarse-media-clock.m4a", 73, False, 0, 3220),
    ("rounded-media-end.m4a", 93, False, 0, 4096),
    ("coarse-media-edit.m4a", 73, True, 1015, 2205),
]:
    (ROOT / name).write_bytes(coarse_media_clock((ROOT / "short-44100.m4a").read_bytes(), end, with_edit))
    cases.append(dict(file=name, rate=44100, frames=2205, channels=1, signal="tone",
                      media_start=media_start, playback_frames=playback_frames))

def sample_timing(data, durations):
    """Keep synthetic AAC packets, replacing only their sample-table slots.

    As above, moov follows mdat, so resizing stts preserves chunk offsets.
    Short slots reproduce 712/1000-frame browser-recording overlaps; long
    slots reproduce timestamp gaps. No production recording is included.
    """
    result, position = bytearray(), 0
    end = sum(durations)
    while position < len(data):
        size, kind = struct.unpack_from(">I4s", data, position)
        assert size >= 8
        payload = bytearray(data[position + 8:position + size])
        if kind in (b"moov", b"trak", b"mdia", b"minf", b"stbl", b"edts"):
            payload = sample_timing(payload, durations)
        elif kind in (b"mdhd", b"mvhd"):
            payload[16:20] = struct.pack(">I", end if kind == b"mdhd" else end - 1024)
        elif kind == b"tkhd":
            payload[20:24] = struct.pack(">I", end - 1024)
        elif kind == b"elst":
            payload[8:16] = struct.pack(">Ii", end - 1024, 1024)
        elif kind == b"stts":
            payload = bytes(4) + struct.pack(">I", len(durations))
            payload += b"".join(struct.pack(">II", 1, duration) for duration in durations)
        result.extend(struct.pack(">I4s", len(payload) + 8, kind) + payload)
        position += size
    return result


for name, source, durations, channels in [
    ("overlap.m4a", "short-48000.m4a", [1024, 712, 1000, 352], 1),
    ("gap.m4a", "short-48000.m4a", [1024, 6144, 1024, 352], 1),
    ("mixed-stereo.m4a", "silent-edges.m4a",
     [1024, 712, 1024, 1024, 1024, 6144, 1000, 1024, 1024, 1024, 1024, 1024, 736], 2),
]:
    (ROOT / name).write_bytes(sample_timing((ROOT / source).read_bytes(), durations))
    cases.append(dict(file=name, rate=48000, frames=sum(durations) - 1024,
                      channels=channels, signal="timed-tone", packet_frames=durations))

manifest = dict(
    provenance="New arithmetic sine/silence signals encoded with FFmpeg; no legacy fixtures or waveform output used.",
    recipe="Signed 16-bit sine: int(12000*sin(2*pi*frequency*frame/rate)); mono 440 Hz, stereo 440/880 Hz. Edges silence first/last thirds.",
    ffmpeg=subprocess.check_output(["ffmpeg", "-version"], text=True).splitlines()[0],
    commands=commands, cases=cases,
    sha256={path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(ROOT.glob("*.m4a"))}
)
manifest["sha256"]["tracks.mp4"] = hashlib.sha256((ROOT / "tracks.mp4").read_bytes()).hexdigest()
(ROOT / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
