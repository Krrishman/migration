#!/usr/bin/env python3
"""Build the browser-preview mock data from a real fixture run.

    cd src-tauri && MA_DUMP_DIR=/tmp/ma-dump cargo run --no-default-features \
        --example fixture_demo -- ../fixtures/source-pc /tmp/ma-usb /tmp/ma-target
    python3 scripts/build_mock_data.py /tmp/ma-dump /tmp/ma-usb /tmp/ma-target

Paths are rewritten to Windows form and byte sizes are scaled up so the demo
looks like a realistic PC. The UI labels this data as simulated.
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCALE = 24_000  # fixture files are tiny; scale sizes for a realistic preview
SIZE_KEYS = {"estimated_size", "size_bytes", "captured_bytes", "total_bytes", "bytes_done", "bytes_total", "bytes", "profile_size", "estimated_bundle_bytes"}


def main(dump: Path, usb: str, target: str):
    src = str(ROOT / "fixtures" / "source-pc")
    out = ROOT / "src" / "mocks" / "data"
    out.mkdir(parents=True, exist_ok=True)

    def fix_str(v: str) -> str:
        for pre, rep in ((src, "C:"), (usb, "E:"), (target, "C:")):
            if pre in v:
                i = v.find(pre)
                v = v[:i] + rep + v[i + len(pre):].replace("/", "\\")
        return v

    def fix(v, key=None):
        if isinstance(v, dict):
            return {k: fix(x, k) for k, x in v.items()}
        if isinstance(v, list):
            return [fix(x) for x in v]
        if isinstance(v, str):
            return fix_str(v)
        if isinstance(v, int) and not isinstance(v, bool) and key in SIZE_KEYS:
            return v * SCALE
        if isinstance(v, float) and key == "bytes_per_second":
            return v * SCALE
        return v

    for name in ["scan", "capture-summary", "overview", "plan", "restore-summary"]:
        data = fix(json.loads((dump / f"{name}.json").read_text()))
        (out / f"{name}.json").write_text(json.dumps(data, indent=1) + "\n")
    print(f"Mock data written to {out}")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        print(__doc__)
        sys.exit(2)
    main(Path(sys.argv[1]), sys.argv[2].rstrip("/"), sys.argv[3].rstrip("/"))
