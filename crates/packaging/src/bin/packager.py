#!/usr/bin/env python3
"""Release packager for the Rust build (ticket 29).

One entry point per platform mode; the GitHub Actions release workflow
calls the mode matching its runner. Produces runnable artifacts next to
the freshly built `bilidown` release binary, bundling ffmpeg at
`bundle/ffmpeg(.exe)` so the runtime locator (crates/ffmpeg locate.rs)
finds it next to the executable.

Stdlib only. Layout mirrors the Python release story:

  windows  bilidown/            portable dir -> zip
                bilidown.exe
                bundle/ffmpeg.exe
                LICENSE
           bilidown_<ver>_windows_x64.exe        (Inno, CI-only)
  macos    bilidown.app/Contents/{MacOS,Resources}
                MacOS/bilidown
                Resources/{app.icns,bundle/ffmpeg,LICENSE}
           -> bilidown_<ver>_macos_<arch>.dmg    (create-dmg, CI-only)
  linux    bilidown.AppDir/usr/{bin,lib,share}
                usr/bin/bilidown
                usr/lib/bilidown/{bundle/ffmpeg,LICENSE}
                AppRun, .desktop, icon, metainfo
           -> bilidown_<ver>_linux_x86_64.AppImage (appimagetool, CI-only)

The Inno installer, dmg and AppImage steps need runner-only tools, so on
a bare machine the script stops at the staging directory and prints the
command CI runs; CI passes --isolate to build them.

ffmpeg source: FFMPEG_BIN (absolute path), else --ffmpeg-zip (an archive
whose first entry is the binary), else a small static-build release is
downloaded. The download pin matches the Python release story's build
(BTB builds, GPL, with libmp3lame for the m4a->mp3 path).
"""

from __future__ import annotations

import argparse
import os
import platform
import shutil
import stat
import subprocess
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[4]

APP_NAME = "bilidown"
APP_ID = "cn.stanzhai.bilidown"
INNO_APP_GUID = "B096F0C1-D105-4EF9-86E1-5E87DA884EA4"

# Static ffmpeg builds used by the Python releases (GPL + libmp3lame).
FFMPEG_BUILDS = {
    ("windows", "amd64"): (
        "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/"
        "ffmpeg-master-latest-win64-gpl.zip"
    ),
    ("linux", "amd64"): (
        "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/"
        "ffmpeg-master-latest-linux64-gpl.tar.xz"
    ),
    ("macos", "arm64"): (
        "https://github.com/eugeneware/ffmpeg-static/releases/latest/download/"
        "ffmpeg-darwin-arm64"
    ),
    ("macos", "x86_64"): (
        "https://github.com/eugeneware/ffmpeg-static/releases/latest/download/"
        "ffmpeg-darwin-x64"
    ),
}


def gnu_arch() -> str:
    m = platform.machine().lower()
    return {"amd64": "x86_64", "x86_64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}.get(m, m)


def ensure_executable(path: Path) -> None:
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def copy_binary(src: Path, dest: Path) -> None:
    """Copy file contents + plain 0755 mode (never source BSD flags)."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(src, dest)
    ensure_executable(dest)


def obtain_ffmpeg(mode: str, dest: Path, args: argparse.Namespace) -> Path:
    """Place a runnable ffmpeg at dest (binary file path)."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    name = "ffmpeg.exe" if mode == "windows" else "ffmpeg"

    if args.ffmpeg_bin:
        src = Path(args.ffmpeg_bin).resolve()
        if not src.is_file():
            raise SystemExit(f"--ffmpeg-bin not found: {src}")
        shutil.copyfile(src, dest)
    elif args.ffmpeg_zip:
        with zipfile.ZipFile(args.ffmpeg_zip) as zf:
            member = next(m for m in zf.namelist() if m.split("/")[-1] == name)
            with zf.open(member) as rf, open(dest, "wb") as wf:
                shutil.copyfileobj(rf, wf)
    else:
        if mode == "windows":
            key = ("windows", "amd64")
        elif mode == "macos":
            key = ("macos", "arm64" if platform.machine().lower() == "arm64" else "x86_64")
        else:
            key = ("linux", "amd64")
        url = FFMPEG_BUILDS[key]
        print(f"downloading ffmpeg: {url}", file=sys.stderr)
        with tempfile.TemporaryDirectory() as tmp:
            t = Path(tmp)
            if url.endswith((".zip", ".tar.xz", ".txz")):
                a = t / ("f.zip" if url.endswith(".zip") else "f.tar.xz")
                urllib.request.urlretrieve(url, a)  # noqa: S310 - pinned https url
                if a.name.endswith(".zip"):
                    with zipfile.ZipFile(a) as zf:
                        member = next(m for m in zf.namelist() if m.split("/")[-1] == name)
                        with zf.open(member) as rf, open(dest, "wb") as wf:
                            shutil.copyfileobj(rf, wf)
                else:
                    import tarfile

                    with tarfile.open(a) as tf:
                        member = next(m for m in tf.getnames() if m.split("/")[-1] == name)
                        with tf.extractfile(member) as rf, open(dest, "wb") as wf:
                            shutil.copyfileobj(rf, wf)
            else:
                urllib.request.urlretrieve(url, dest)  # noqa: S310
    ensure_executable(dest)
    return dest


def find_binary(mode: str, args: argparse.Namespace) -> Path:
    exe = "bilidown.exe" if mode == "windows" else "bilidown"
    if args.bin:
        p = Path(args.bin)
        if p.is_file():
            return p
        raise SystemExit(f"--bin not found: {p}")
    target_dir = Path("target") / "release"
    local = REPO / "crates" / target_dir / exe
    if local.is_file():
        return local
    raise SystemExit(
        f"release binary not found at {local}; run "
        "`cargo build --release -p app --manifest-path crates/Cargo.toml` or pass --bin"
    )


def copy_license(dest_dir: Path) -> None:
    shutil.copy2(REPO / "LICENSE", dest_dir / "LICENSE")


# ---------------------------------------------------------------- windows


def package_windows(args: argparse.Namespace) -> list[Path]:
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    root = out / "bilidown"
    if root.exists():
        shutil.rmtree(root)
    (root / "bundle").mkdir(parents=True)

    copy_binary(find_binary("windows", args), root / "bilidown.exe")
    obtain_ffmpeg("windows", root / "bundle" / "ffmpeg.exe", args)
    copy_license(root)

    zip_path = out / f"bilidown_{args.version}_windows_x64_portable.zip"
    if zip_path.exists():
        zip_path.unlink()
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for p in sorted(root.rglob("*")):
            zf.write(p, p.relative_to(root.parent))
    artifacts = [zip_path]

    if args.isolate:
        artifacts.append(build_inno(out, root, args))
    else:
        print("skipped installer: pass --isolate on a runner with Inno Setup (ISCC)")
    return artifacts


def build_inno(out: Path, root: Path, args: argparse.Namespace) -> Path:
    iscc = shutil.which("ISCC") or shutil.which("iscc")
    if not iscc:
        raise SystemExit("Inno Setup compiler (ISCC) not found on PATH")
    script = REPO / "crates" / "packaging" / "windows" / "setup.iss"
    work = out / "inno"
    work.mkdir(parents=True, exist_ok=True)
    # setup.iss resolves .\bilidown and .\LICENSE relative to itself
    staged = work / "setup.iss"
    shutil.copy2(script, staged)
    shutil.copytree(root, work / "bilidown", dirs_exist_ok=True)
    shutil.copy2(REPO / "LICENSE", work / "LICENSE")
    subprocess.run(
        [iscc, f"/DMyAppVersion={args.version}", f"/DMyAppVersionName={args.version}", str(staged.resolve())],
        check=True,
        cwd=work,
    )
    produced = next(work.glob("bilidown_*_windows_x64.exe"))
    moved = out / produced.name
    shutil.move(str(produced), moved)
    return moved


# ------------------------------------------------------------------ macos


def package_macos(args: argparse.Namespace) -> list[Path]:
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    app = out / "bilidown.app"
    if app.exists():
        shutil.rmtree(app)
    contents = app / "Contents"
    macos_dir, res = contents / "MacOS", contents / "Resources"
    (macos_dir / "bundle").mkdir(parents=True)
    res.mkdir(parents=True)

    copy_binary(find_binary("macos", args), macos_dir / "bilidown")
    obtain_ffmpeg("macos", macos_dir / "bundle" / "ffmpeg", args)
    shutil.copy2(REPO / "assets" / "app.icns", res / "app.icns")
    copy_license(res)

    plist = (REPO / "crates" / "packaging" / "macos" / "Info.plist").read_text(encoding="utf-8")
    (contents / "Info.plist").write_text(
        plist.replace("__VERSION__", args.version).replace("__VERSION_NAME__", args.version),
        encoding="utf-8",
    )

    if not args.isolate:
        print("skipped dmg: pass --isolate on a runner with create-dmg")
        return [app]
    dmg = out / f"bilidown_{args.version}_macos_{gnu_arch()}.dmg"
    if dmg.exists():
        dmg.unlink()
    subprocess.run(
        [
            "create-dmg",
            "--volname", f"{APP_NAME} Installer",
            "--window-pos", "400", "200",
            "--window-size", "660", "400",
            "--icon-size", "80",
            "--icon", "bilidown.app", "160", "160",
            "--hide-extension", "bilidown.app",
            "--app-drop-link", "500", "160",
            str(dmg),
            str(app),
        ],
        check=True,
    )
    return [dmg]


# ------------------------------------------------------------------ linux


DESKTOP_ENTRY = """[Desktop Entry]
Type=Application
Name=bilidown
Exec=bilidown
Icon=bilidown
Categories=Utility;
Terminal=false
Comment=A cross-platform video downloader for Bilibili
StartupWMClass=bilidown
"""

APPRUN = """#!/bin/bash
# AppImage 挂载点每次运行都不同，工作目录与 ffmpeg 定位都必须动态解析
HERE="$(dirname "$(readlink -f "${0}")")"
APPDIR="$HERE/usr/lib/bilidown"
cd "$APPDIR"
exec ./bilidown "$@"
"""


def package_linux(args: argparse.Namespace) -> list[Path]:
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    appdir = out / "bilidown.AppDir"
    if appdir.exists():
        shutil.rmtree(appdir)

    lib = appdir / "usr" / "lib" / "bilidown"
    share = appdir / "usr" / "share"
    (lib / "bundle").mkdir(parents=True)
    (share / "applications").mkdir(parents=True)
    (share / "icons" / "hicolor" / "scalable" / "apps").mkdir(parents=True)
    (share / "metainfo").mkdir(parents=True)

    copy_binary(find_binary("linux", args), lib / "bilidown")
    obtain_ffmpeg("linux", lib / "bundle" / "ffmpeg", args)
    copy_license(lib)

    (appdir / "bilidown.desktop").write_text(DESKTOP_ENTRY, encoding="utf-8")
    shutil.copy2(
        appdir / "bilidown.desktop", share / "applications" / "bilidown.desktop"
    )
    shutil.copy2(REPO / "assets" / "app.svg", appdir / "bilidown.svg")
    shutil.copy2(
        REPO / "assets" / "app.svg",
        share / "icons" / "hicolor" / "scalable" / "apps" / "bilidown.svg",
    )
    metainfo = (REPO / "assets" / "cn.stanzhai.bilidown.metainfo.xml").read_text(encoding="utf-8")
    import datetime
    import re

    metainfo = re.sub(
        r'<release version="[^"]*" date="[^"]*"\s*/>',
        f'<release version="{args.version}" date="{datetime.date.today().isoformat()}"/>',
        metainfo,
    )
    (share / "metainfo" / "cn.stanzhai.bilidown.metainfo.xml").write_text(
        metainfo, encoding="utf-8"
    )

    apprun = appdir / "AppRun"
    apprun.write_text(APPRUN, encoding="utf-8")
    ensure_executable(apprun)

    if not args.isolate:
        print("skipped AppImage: pass --isolate on a runner with appimagetool")
        return [appdir]
    return [build_appimage(out, appdir, args)]


def build_appimage(out: Path, appdir: Path, args: argparse.Namespace) -> Path:
    arch = gnu_arch()
    tool = shutil.which("appimagetool")
    if not tool:
        local = Path(f"appimagetool-{arch}.AppImage")
        if not local.is_file():
            url = (
                "https://github.com/AppImage/appimagetool/releases/download/"
                f"continuous/appimagetool-{arch}.AppImage"
            )
            print(f"downloading appimagetool: {url}", file=sys.stderr)
            urllib.request.urlretrieve(url, local)  # noqa: S310
            ensure_executable(local)
        tool = f"./{local}"
    runtime = Path(f"runtime-{arch}")
    if not runtime.is_file():
        url = (
            "https://github.com/AppImage/type2-runtime/releases/download/"
            f"continuous/runtime-{arch}"
        )
        print(f"downloading appimage runtime: {url}", file=sys.stderr)
        urllib.request.urlretrieve(url, runtime)  # noqa: S310
        ensure_executable(runtime)
    target = out / f"bilidown_{args.version}_linux_{arch}.AppImage"
    env = dict(os.environ, ARCH=arch, APPIMAGE_EXTRACT_AND_RUN="1")
    subprocess.run(
        [tool, "--no-appstream", "--runtime-file", str(runtime), str(appdir), str(target)],
        check=True,
        env=env,
    )
    return [target] if isinstance(target, list) else target


# -------------------------------------------------------------------- cli


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("mode", choices=["windows", "macos", "linux"])
    ap.add_argument("--version", required=True, help="e.g. 2.21.0")
    ap.add_argument("--out", default="dist", help="output directory (default dist)")
    ap.add_argument("--bin", help="path to the built bilidown executable")
    ap.add_argument("--ffmpeg-bin", help="path to a local ffmpeg executable")
    ap.add_argument("--ffmpeg-zip", help="zip archive containing ffmpeg")
    ap.add_argument(
        "--isolate",
        action="store_true",
        help="also build the installer/dmg/AppImage (needs ISCC/create-dmg/appimagetool)",
    )
    args = ap.parse_args()

    if args.mode == "windows" and platform.system() != "Windows" and not args.isolate:
        print("note: staging a windows layout on a non-windows host", file=sys.stderr)

    fn = {"windows": package_windows, "macos": package_macos, "linux": package_linux}[args.mode]
    for artifact in fn(args):
        print(artifact)
    return 0


if __name__ == "__main__":
    sys.exit(main())
