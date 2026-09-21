#!/usr/bin/env python3
"""Compare short/long PCM input memory in separate release-build processes."""
import json
from pathlib import Path
import platform
import re
import struct
import subprocess
import wave

root = Path(__file__).resolve().parents[1]
output = root / "measurements"
output.mkdir(exist_ok=True)
binary = root / "target/release/examples/measure"
subprocess.run(["cargo", "build", "--offline", "--release", "--all-features", "--example", "measure"], cwd=root, check=True)
block = struct.pack("<4096h", *([-8000, 8000] * 2048))
rows = []
for seconds in (30, 1800):
    path = output / f"{seconds}s.wav"
    frames = seconds * 48000
    with wave.open(str(path), "wb") as stream:
        stream.setparams((1, 2, 48000, frames, "NONE", "not compressed"))
        remaining = frames
        while remaining:
            take = min(remaining, 4096)
            stream.writeframesraw(block[:take * 2])
            remaining -= take
    for gain in ("fixed", "normalize"):
        flag = "-l" if platform.system() == "Darwin" else "-v"
        command = ["/usr/bin/time", flag, str(binary), str(path), gain]
        result = subprocess.run(command, capture_output=True, text=True, check=True)
        pattern = (r"(\d+)\s+maximum resident set size" if platform.system() == "Darwin"
                   else r"Maximum resident set size \(kbytes\):\s+(\d+)")
        rss = int(re.search(pattern, result.stderr).group(1))
        if platform.system() != "Darwin":
            rss *= 1024
        scratch = int(re.search(r"scratch_capacity_bytes: (\d+)", result.stdout).group(1))
        peaks = int(re.search(r"peak_capacity_bytes: (\d+)", result.stdout).group(1))
        row = {"seconds": seconds, "frames": frames, "gain": gain, "rss_bytes": rss,
               "scratch_capacity_bytes": scratch, "peak_capacity_bytes": peaks,
               "result": result.stdout.strip(), "time_output": result.stderr.strip()}
        rows.append(row)
        print(f"{seconds}s {gain}: RSS={rss} scratch={scratch} peaks={peaks}", flush=True)
for gain in ("fixed", "normalize"):
    short, long = [row for row in rows if row["gain"] == gain]
    assert short["scratch_capacity_bytes"] == long["scratch_capacity_bytes"]
    assert short["peak_capacity_bytes"] == long["peak_capacity_bytes"]
report = {"platform": platform.platform(), "points": 110, "rows": rows}
(output / "memory.json").write_text(json.dumps(report, indent=2) + "\n")
print("PASS: application-owned capacities remain constant for a 60x longer recording")
