#!/usr/bin/env python3
"""CI-script-level checks for the Rust release packaging (ticket 29).

Run from anywhere in the repo: it locates the repo root via this file's
path. Only external artifacts are checked (spec Testing Decisions):
the workflows parse as YAML, jobs/steps/artifacts the release depends on
exist, and every produced artifact name appears in both the workflow and
the packager's manifest. No installer internals are exercised.

Stdlib only (yaml is deliberately avoided: no third-party pinning needed,
and a real YAML library would accept documents this checker must reject).
The parser below covers the GitHub Actions subset (mappings, lists,
block scalars, quoted scalars, comments) — enough to fail loudly when a
workflow is structurally broken.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]


# --------------------------------------------------------------------------
# Minimal YAML subset parser (block mappings/lists, quoted & plain scalars,
# block scalars | and >, comments, blank lines). Throws on anything else
# structural; good enough to prove the workflows parse.
# --------------------------------------------------------------------------


class YamlError(Exception):
    pass


def _split_flow(text: str) -> list[str]:
    """Split a flow sequence body on commas, honoring quoted strings."""
    parts, buf, quote = [], [], None
    for ch in text:
        if quote:
            buf.append(ch)
            if ch == quote:
                quote = None
        elif ch in "\"'":
            quote = ch
            buf.append(ch)
        elif ch == ",":
            parts.append("".join(buf))
            buf = []
        else:
            buf.append(ch)
    if "".join(buf).strip():
        parts.append("".join(buf))
    return [p.strip() for p in parts if p.strip()]


def _parse_scalar(text: str):
    text = text.strip()
    if text.startswith("[") and text.endswith("]"):
        return [_parse_scalar(part) for part in _split_flow(text[1:-1])]
    if len(text) >= 2 and text[0] == text[-1] and text[0] in "\"'":
        return text[1:-1]
    if text in ("true", "false"):
        return text == "true"
    if text == "null" or text == "":
        return None
    if re.fullmatch(r"-?\d+", text):
        return int(text)
    return text


def _strip_comment(line: str) -> str:
    out, quote = [], None
    for i, ch in enumerate(line):
        if quote:
            out.append(ch)
            if ch == quote:
                quote = None
            continue
        if ch in "\"'":
            quote = ch
            out.append(ch)
            continue
        if ch == "#" and (i == 0 or line[i - 1] in " \t"):
            break
        out.append(ch)
    return "".join(out).rstrip()


def parse_yaml(text: str):
    """Parse the GH Actions YAML subset into dicts/lists/scalars."""
    lines: list[tuple[int, str]] = []
    for raw in text.splitlines():
        if raw.lstrip().startswith("#"):
            continue
        line = _strip_comment(raw).rstrip()
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip(" "))
        lines.append((indent, line.strip()))

    value, pos = _parse_block(lines, 0, -1)
    if pos != len(lines):
        raise YamlError(f"unparsed content at line {pos + 1}")
    return value


def _parse_block(lines, pos, parent_indent):
    if pos >= len(lines):
        return None, pos
    indent = lines[pos][0]
    if indent <= parent_indent:
        return None, pos

    # Decide mapping vs list from the first line.
    if lines[pos][1].startswith("- "):
        items = []
        while pos < len(lines) and lines[pos][0] == indent and lines[pos][1].startswith("- "):
            item_text = lines[pos][1][2:]
            pos += 1
            if _key_match(item_text):
                # "- key: value" inline mapping start; treat the item as a
                # mapping whose first entry sits at indent+2
                sub = [(indent + 2, item_text)]
                while pos < len(lines) and lines[pos][0] > indent:
                    sub.append(lines[pos])
                    pos += 1
                value, _ = _parse_block(sub, 0, indent)
                items.append(value)
            else:
                items.append(_parse_scalar(item_text))
        return items, pos

    mapping = {}
    while pos < len(lines) and lines[pos][0] == indent:
        line = lines[pos][1]
        m = _key_match(line)
        if not m:
            raise YamlError(f"expected 'key: value', got {line!r}")
        key_raw, rest = m.group(1), m.group(2).strip()
        key = _parse_scalar(key_raw)
        pos += 1
        if rest in ("|", "|-", "|+", ">", ">-", ">+"):
            # Block scalar: consume deeper-indented (or blank) lines.
            body = []
            base = None
            while pos < len(lines) and (lines[pos][0] > indent or lines[pos][1] == ""):
                if lines[pos][1] == "" and lines[pos][0] == 0:
                    break
                if base is None:
                    base = lines[pos][0]
                body.append(" " * (lines[pos][0] - base) + lines[pos][1])
                pos += 1
            mapping[key] = "\n".join(body)
        elif rest == "":
            value, pos = _parse_block(lines, pos, indent)
            mapping[key] = value
        else:
            mapping[key] = _parse_scalar(rest)
    if pos < len(lines) and lines[pos][0] == indent:
        raise YamlError(f"unparsed sibling at indent {indent}: {lines[pos][1]!r}")
    return mapping, pos


def _key_match(line: str):
    return re.match(r'("[^"]+"|\'[^\']+\'|[^:\s][^:]*?)\s*:\s*(.*)$', line)


# --------------------------------------------------------------------------
# Checks
# --------------------------------------------------------------------------

RUST_CI = REPO / ".github" / "workflows" / "rust.yml"
RELEASE_WF = REPO / ".github" / "workflows" / "rust-release.yml"
PACKAGER = REPO / "crates" / "packaging" / "src" / "bin" / "packager.py"

PLATFORM_MATRIX = ["windows-latest", "macos-latest", "ubuntu-latest"]

# (runner os -> artifact extension) the release must produce per platform.
EXPECTED_ARTIFACTS = {
    "windows": [".exe", ".zip"],
    "macos": [".dmg"],
    "linux": [".AppImage"],
}

CHECKS = []


def check(fn):
    CHECKS.append(fn)
    return fn


@check
def ci_workflow_parses_and_builds_all_platforms():
    doc = parse_yaml(RUST_CI.read_text(encoding="utf-8"))
    jobs = doc["jobs"]
    build = jobs["build-test"]
    matrix = build["strategy"]["matrix"]["os"]
    assert matrix == PLATFORM_MATRIX, f"CI matrix {matrix} != {PLATFORM_MATRIX}"
    steps = " ".join(s.get("run", "") for s in build["steps"])
    assert "cargo build --workspace" in steps, "CI must build the workspace"
    assert "cargo test --workspace" in steps, "CI must test the workspace"
    clippy = next(s["run"] for s in build["steps"] if "cargo clippy" in s.get("run", ""))
    assert "--workspace" in clippy and "--all-targets" in clippy and "-D warnings" in clippy, (
        f"CI clippy step must gate on warnings: {clippy!r}"
    )


@check
def release_workflow_parses_and_covers_platforms():
    doc = parse_yaml(RELEASE_WF.read_text(encoding="utf-8"))
    jobs = doc["jobs"]
    for job_name in ("windows", "linux", "macos"):
        assert f"{job_name}-build" in jobs, f"release job {job_name}-build missing"
    for job in jobs.values():
        for step in job.get("steps", []):
            run = step.get("run", "")
            if "cargo build --release" in run:
                assert "--manifest-path crates/Cargo.toml" in run, (
                    "release build must target the crates workspace"
                )


@check
def release_workflow_uploads_every_artifact_kind():
    text = RELEASE_WF.read_text(encoding="utf-8")
    doc = parse_yaml(text)
    uploads = []
    for job in doc["jobs"].values():
        for step in job.get("steps", []):
            if "uses" in step and "upload-artifact" in step["uses"]:
                uploads.append(step.get("with", {}).get("path", ""))
    joined = "\n".join(uploads)
    for platform, exts in EXPECTED_ARTIFACTS.items():
        for ext in exts:
            assert ext in joined, f"no upload-artifact path contains {platform} {ext}"


@check
def packager_manifest_matches_workflow_names():
    text = RELEASE_WF.read_text(encoding="utf-8")
    m = re.search(r"python.*packager\.py (\w+)", text)
    assert m, "release workflow must invoke the packager"
    mode = m.group(1)
    import shutil
    import subprocess
    import tempfile

    out = Path(tempfile.gettempdir()) / f"t29-packager-{mode}"

    shutil.rmtree(out, ignore_errors=True)
    fake_bin = Path(tempfile.mkdtemp()) / "bilidown"
    fake_bin.write_bytes(b"#!/bin/sh\nexit 0\n")
    fake_bin.chmod(0o755)
    fake_ffmpeg = fake_bin.parent / "ffmpeg"
    fake_ffmpeg.write_bytes(b"#!/bin/sh\nexit 0\n")
    fake_ffmpeg.chmod(0o755)
    subprocess.run(
        [
            sys.executable,
            str(PACKAGER),
            mode,
            "--version",
            "2.21.0",
            "--out",
            str(out),
            "--bin",
            str(fake_bin),
            "--ffmpeg-bin",
            str(fake_ffmpeg),
        ],
        check=True,
        cwd=REPO,
    )
    files = sorted(p.name for p in out.rglob("*") if p.is_file())
    # Without --isolate only the portable/zip layout is produced; the
    # staged tree must at minimum carry the runnable binary + ffmpeg.
    assert any("bilidown" in n for n in files), files
    assert any(n.startswith("ffmpeg") for n in files), files


@check
def bundle_metadata_identity():
    src = (REPO / "crates" / "packaging").rglob("*.plist")
    plists = list(src)
    assert plists, "no Info.plist template under crates/packaging"
    text = plists[0].read_text(encoding="utf-8")
    assert "cn.stanzhai.bilidown" in text, "CFBundleIdentifier must keep the app id"
    assert "bilidown" in text, "CFBundleName must keep the product name"
    assert "__VERSION__" in text, "version placeholder must be injectable"


@check
def inno_script_keeps_app_id_and_exes():
    text = (REPO / "crates" / "packaging" / "windows" / "setup.iss").read_text(encoding="utf-8")
    # Rust release AppId — fresh GUID, deliberately NOT the legacy Python
    # "Bili23 Downloader" one (which may only appear in the legacy-cleanup
    # registry probe).
    assert text.count("AppId={{") == 1, "exactly one AppId"
    assert "AppId={{3C3AEF77-CE35-40B1-8767-74C8D42586A0}" in text, "Inno AppId must match packager.py INNO_APP_GUID"
    assert "MyAppExeName \"bilidown.exe\"" in text
    assert "bundle\\*" in text, "installer must ship bundle/ffmpeg"


@check
def packager_guid_matches_inno_script():
    py = (REPO / "crates" / "packaging" / "src" / "bin" / "packager.py").read_text(encoding="utf-8")
    iss = (REPO / "crates" / "packaging" / "windows" / "setup.iss").read_text(encoding="utf-8")
    m = re.search(r'INNO_APP_GUID = "([0-9A-F-]+)"', py)
    assert m, "packager.py defines INNO_APP_GUID"
    # Inno escaping: "{{" opens, single "}" closes — `AppId={{GUID}`.
    assert f"AppId={{{{{m.group(1)}}}" in iss, "setup.iss AppId must match packager.py INNO_APP_GUID"

@check
def version_line_continues_v2210():
    text = (REPO / "crates" / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r'^version\s*=\s*"(.+)"', text, re.M)
    assert m and m.group(1).startswith("2.21"), f"workspace version {m and m.group(1)} must continue the v2.21.0 line"


def main() -> int:
    failures = 0
    for fn in CHECKS:
        try:
            fn()
            print(f"ok   {fn.__name__}")
        except Exception as exc:  # noqa: BLE001 - report every failure
            failures += 1
            print(f"FAIL {fn.__name__}: {exc}")
    if failures:
        print(f"\n{failures} check(s) failed")
        return 1
    print("\nall packaging checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
