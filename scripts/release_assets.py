#!/usr/bin/env python3
"""Stage an app release: installer, SBOMs, winget manifests, SHA256SUMS (OSS-02, M5-2, M5-4).

    python scripts/release_assets.py check-version --tag v0.1.0
    python scripts/release_assets.py stage --tag v0.1.0

The release workflow (`.github/workflows/release.yml`) runs both when a maintainer starts it for a `v*` tag,
after `npm run tauri build` and `python scripts/notices.py --out <dest>/THIRD_PARTY_NOTICES.md`.
Running them locally gives the same layout, for a check before tagging; nothing is uploaded or
submitted from here.

`check-version`: the tag must be `v<version>` and the version must be the one in Cargo.toml
(`[workspace.package]`), `apps/desktop/src-tauri/tauri.conf.json` and `apps/desktop/package.json`.

`stage` writes into `--dest` (default `target/release-assets/<tag>`), one flat folder:
  Polyglass_<version>_x64-setup.exe
                              the NSIS installer from `--bundle` (default `target/release/bundle`);
                              it must be within SIZE-01 (15 MB). v1 ships no MSI (D-24).
  polyglass-rust.cdx.json     CycloneDX SBOM of the Rust crates in polyglass.exe (`cargo cyclonedx`)
  polyglass-ui.cdx.json       CycloneDX SBOM of the npm packages bundled into the UI
                              (`@cyclonedx/cyclonedx-npm`, runtime dependencies only)
  winget-<Id>.installer.yaml, winget-<Id>.locale.en-US.yaml, winget-<Id>.yaml
                              the winget manifest for this version (`winget_manifests`); for a
                              submission they go to manifests/s/ShashankShirol/Polyglass/<version>/
                              of microsoft/winget-pkgs without the `winget-` prefix
  THIRD_PARTY_NOTICES.md      kept if it is already there (the workflow writes it first)
  SHA256SUMS                  over every file above
  RELEASE_NOTES.md            the release text (not in SHA256SUMS; it isn't an asset)

The C++ code compiled into the app (CTranslate2, SentencePiece and their third-party code) is
in neither SBOM, because no package manager knows it; THIRD_PARTY_NOTICES.md lists it.

`--no-sbom` leaves the SBOMs out (they need `cargo cyclonedx` and network access for `npx`).
Only the Python standard library is used.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
APP = REPO / "apps" / "desktop"
REPO_URL = "https://github.com/shashankshirol/polyglass"
TAG = re.compile(r"v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)")

# SIZE-01: installer, Windows, no models.
SIZE_LIMIT = 15_000_000

WINGET_ID = "ShashankShirol.Polyglass"
WINGET_SCHEMA = "1.10.0"
# Pinned: `npx` downloads it for the run.
CYCLONEDX_NPM = "@cyclonedx/cyclonedx-npm@7.0.0"
CYCLONEDX_SPEC = "1.5"


def log(msg: str) -> None:
    print(f"[release_assets] {msg}", flush=True)


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def versions(repo: Path = REPO) -> dict[str, str]:
    """The app's version in each file that states it."""
    cargo = tomllib.loads((repo / "Cargo.toml").read_text(encoding="utf-8"))
    app = repo / "apps" / "desktop"
    tauri = json.loads((app / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
    npm = json.loads((app / "package.json").read_text(encoding="utf-8"))
    return {
        "Cargo.toml": cargo["workspace"]["package"]["version"],
        "tauri.conf.json": tauri["version"],
        "package.json": npm["version"],
    }


def version_of(tag: str, found: dict[str, str]) -> str:
    """The version `tag` names; an error unless every file in `found` has it."""
    m = TAG.fullmatch(tag)
    if not m:
        raise SystemExit(f"{tag!r} is not a release tag (v<major>.<minor>.<patch>[-<pre>])")
    version = m.group(1)
    wrong = {name: v for name, v in found.items() if v != version}
    if wrong:
        raise SystemExit(f"tag {tag} names {version}, but " + ", ".join(
            f"{name} has {v}" for name, v in wrong.items()))
    return version


def installer(bundle: Path, version: str) -> Path:
    """The NSIS installer of `version` in a Tauri bundle folder."""
    path = bundle / "nsis" / f"Polyglass_{version}_x64-setup.exe"
    if not path.is_file():
        raise SystemExit(f"{path} is missing: run `npm run tauri build` in apps/desktop first")
    size = path.stat().st_size
    if size > SIZE_LIMIT:
        raise SystemExit(f"{path.name} is {size:,} bytes, over SIZE-01's {SIZE_LIMIT:,}")
    return path


def winget_manifests(version: str, tag: str, sha: str) -> dict[str, str]:
    """The three winget manifest files (name → text) for the installer with the SHA-256 `sha`.
    It installs for the current user without elevation."""
    base = f"{REPO_URL}/releases/download/{tag}"
    head = f"PackageIdentifier: {WINGET_ID}\nPackageVersion: {version}\n"
    schema = "# yaml-language-server: $schema=https://aka.ms/winget-manifest.{}.{}.schema.json\n\n"
    installer = schema.format("installer", WINGET_SCHEMA) + head + f"""\
MinimumOSVersion: 10.0.19041.0
InstallModes:
- interactive
- silent
UpgradeBehavior: install
Installers:
- Architecture: x64
  InstallerType: nullsoft
  Scope: user
  InstallerUrl: {base}/Polyglass_{version}_x64-setup.exe
  InstallerSha256: {sha.upper()}
ManifestType: installer
ManifestVersion: {WINGET_SCHEMA}
"""
    locale = schema.format("defaultLocale", WINGET_SCHEMA) + head + f"""\
PackageLocale: en-US
Publisher: Polyglass contributors
PublisherUrl: {REPO_URL}
PublisherSupportUrl: {REPO_URL}/issues
PrivacyUrl: {REPO_URL}/blob/main/PRIVACY.md
PackageName: Polyglass
PackageUrl: {REPO_URL}
License: Apache-2.0
LicenseUrl: {REPO_URL}/blob/main/LICENSE
ShortDescription: A movable lens that translates the text under it on your computer.
Description: |-
  Polyglass is an always-on-top lens for the Windows desktop. It reads the text under it with
  Windows OCR and translates it on the device (Swedish and English), so nothing on the screen
  is sent anywhere. The translation models are downloaded separately on first run.
Tags:
- ocr
- offline
- screen-translation
- swedish
- translation
ReleaseNotesUrl: {REPO_URL}/releases/tag/{tag}
ManifestType: defaultLocale
ManifestVersion: {WINGET_SCHEMA}
"""
    root = schema.format("version", WINGET_SCHEMA) + head + f"""\
DefaultLocale: en-US
ManifestType: version
ManifestVersion: {WINGET_SCHEMA}
"""
    return {
        f"{WINGET_ID}.installer.yaml": installer,
        f"{WINGET_ID}.locale.en-US.yaml": locale,
        f"{WINGET_ID}.yaml": root,
    }


def sums(files: list[Path]) -> str:
    """`sha256sum`-format lines (binary mode) for `files`, sorted by name."""
    return "".join(f"{sha256(p)} *{p.name}\n" for p in sorted(files, key=lambda p: p.name))


def notes(version: str, tag: str, name: str, signed: bool = False) -> str:
    signing = (
        "The installer is code-signed (free code signing provided by SignPath.io, certificate "
        "by SignPath Foundation)."
        if signed else
        "The installer is not code-signed yet, so Windows SmartScreen may warn about an unknown "
        "publisher."
    )
    return f"""\
Polyglass {version} for Windows 10 (2004 or later) and Windows 11, x64.

## Install

Run `{name}`. It installs for the current user and needs no administrator rights.

{signing}

The translation models are not included: Polyglass offers to download them on first run
(about 160 MB for Swedish ↔ English).

## Verify

    sha256sum --check SHA256SUMS
    gh attestation verify {name} --repo shashankshirol/polyglass

`SHA256SUMS` covers every file of this release. The installer was built by this repository's
release workflow from the signed tag `{tag}`; GitHub stores its build-provenance and SBOM
attestations. `polyglass-rust.cdx.json` and `polyglass-ui.cdx.json` are CycloneDX SBOMs of the
Rust crates and npm packages in the app; `THIRD_PARTY_NOTICES.md` has all license texts,
including the C++ code compiled in (CTranslate2, SentencePiece).

[Privacy]({REPO_URL}/blob/{tag}/PRIVACY.md): text is read and translated on your computer.
"""


def run(cmd: list[str], cwd: Path) -> None:
    log("$ " + " ".join(cmd))
    # `npx` is a .cmd file on Windows: found through the shell only.
    subprocess.run(cmd, cwd=cwd, check=True, shell=sys.platform == "win32" and cmd[0] == "npx")


def sboms(dest: Path) -> list[Path]:
    """Write both CycloneDX SBOMs into `dest`."""
    rust = dest / "polyglass-rust.cdx.json"
    manifest = APP / "src-tauri" / "Cargo.toml"
    # cargo-cyclonedx writes next to the crate's Cargo.toml.
    written = manifest.parent / "polyglass-rust.cdx.json"
    run(["cargo", "cyclonedx", "--manifest-path", str(manifest), "--format", "json",
         "--spec-version", CYCLONEDX_SPEC, "--target", "x86_64-pc-windows-msvc",
         "--override-filename", "polyglass-rust.cdx"], REPO)
    shutil.move(written, rust)
    # It also writes one for every other workspace member; only the app's is a release asset.
    for other in (REPO / "crates").glob("*/polyglass-rust.cdx.json"):
        other.unlink()
    ui = dest / "polyglass-ui.cdx.json"
    run(["npx", "--yes", CYCLONEDX_NPM, "--omit", "dev", "--spec-version", CYCLONEDX_SPEC,
         "--output-reproducible", "--output-format", "JSON", "--output-file", str(ui)], APP)
    for path in (rust, ui):
        bom = json.loads(path.read_text(encoding="utf-8"))
        if bom.get("bomFormat") != "CycloneDX" or not bom.get("components"):
            raise SystemExit(f"{path.name} is not a CycloneDX SBOM with components")
        log(f"{path.name}: {len(bom['components'])} components")
    return [rust, ui]


def stage(tag: str, bundle: Path, dest: Path, with_sbom: bool, signed: bool = False) -> None:
    version = version_of(tag, versions())
    found = installer(bundle, version)
    dest.mkdir(parents=True, exist_ok=True)
    keep = dest / "THIRD_PARTY_NOTICES.md"
    for old in dest.iterdir():
        if old != keep:
            raise SystemExit(f"{dest} already holds {old.name}; stage into an empty folder")
    assets = [keep] if keep.is_file() else []
    shutil.copy2(found, dest / found.name)
    assets.append(dest / found.name)
    log(f"{found.name}: {found.stat().st_size:,} bytes (SIZE-01 limit {SIZE_LIMIT:,})")
    if with_sbom:
        assets += sboms(dest)
    for name, text in winget_manifests(version, tag, sha256(found)).items():
        path = dest / f"winget-{name}"
        path.write_text(text, encoding="utf-8", newline="\n")
        assets.append(path)
    (dest / "SHA256SUMS").write_text(sums(assets), encoding="utf-8", newline="\n")
    (dest / "RELEASE_NOTES.md").write_text(
        notes(version, tag, found.name, signed), encoding="utf-8", newline="\n")
    log(f"staged {len(assets)} assets + SHA256SUMS in {dest}")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check-version", help="the tag names the version in every file")
    check.add_argument("--tag", required=True)
    st = sub.add_parser("stage", help="collect the release assets")
    st.add_argument("--tag", required=True)
    st.add_argument("--bundle", type=Path, default=REPO / "target" / "release" / "bundle")
    st.add_argument("--dest", type=Path)
    st.add_argument("--no-sbom", action="store_true", help="leave the SBOMs out (local check)")
    st.add_argument("--signed", action="store_true",
                    help="the installer is code-signed (the release notes say so)")
    args = ap.parse_args()
    if args.command == "check-version":
        log(f"{args.tag}: version {version_of(args.tag, versions())} in every file")
    else:
        dest = args.dest or REPO / "target" / "release-assets" / args.tag
        stage(args.tag, args.bundle.resolve(), dest.resolve(), not args.no_sbom, args.signed)


if __name__ == "__main__":
    main()
