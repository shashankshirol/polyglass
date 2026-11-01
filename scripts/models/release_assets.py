#!/usr/bin/env python3
"""Stage the converted OPUS-MT models as GitHub release assets (M1-TR-7, D-9).

    python scripts/models/opus_mt.py --no-install
    python scripts/models/release_assets.py --tag models-opus-mt-2026-11-01

The models release workflow (`.github/workflows/models-release.yml`) runs both commands on a
clean runner when a `models-opus-mt-*` tag is pushed, then creates the pre-release from the
staged directory. Running them locally gives the same files, for a check before tagging.

The release is described by `models/manifest.toml`: every model whose file URLs point at
`https://github.com/shashankshirol/polyglass/releases/download/<tag>/` is part of it. Each URL
must name the asset `<model id>.<file name>` (a release has one flat list of assets, and both
directions have a `model.bin`), and each file is checked against the manifest's size and
SHA-256. `opus_mt.py` has already checked them against its own pins.

Output in `--dest` (default `target/opus-mt/release/<tag>`):
  assets/<id>.<name>        the model files
  assets/LICENSE-CC-BY-4.0.txt, assets/ATTRIBUTION.md, assets/SHA256SUMS
  notes.md                  the release notes (attribution, provenance, how to verify)

Only the Python standard library is used.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
MANIFEST = REPO / "models" / "manifest.toml"
RELEASES = "https://github.com/shashankshirol/polyglass/releases/download/"
TAG = re.compile(r"models-opus-mt-\d{4}-\d{2}-\d{2}(-\d+)?")
LICENSE_ASSET = "LICENSE-CC-BY-4.0.txt"

sys.path.insert(0, str(HERE))
import opus_mt  # noqa: E402  (the input zips, for provenance)


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def log(msg: str) -> None:
    print(f"[release_assets] {msg}", flush=True)


def commit() -> str:
    if sha := os.environ.get("GITHUB_SHA"):
        return sha
    out = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True)
    return out.stdout.strip() or "unknown"


def release_models(manifest: dict, tag: str) -> list[dict]:
    """The manifest entries hosted in this release, with every URL checked."""
    prefix = f"{RELEASES}{tag}/"
    models = []
    for m in manifest.get("model", []):
        urls = [f.get("url") for f in m["files"]]
        if not any(u and u.startswith(prefix) for u in urls):
            continue
        for f in m["files"]:
            want = f"{prefix}{m['id']}.{f['name']}"
            if f.get("url") != want:
                sys.exit(f"{m['id']}/{f['name']}: url is {f.get('url')!r}, expected {want!r}")
        if m["license"] != "CC-BY-4.0" or "LICENSE" not in {f["name"] for f in m["files"]}:
            sys.exit(f"{m['id']}: this script only knows CC-BY-4.0 models that ship a LICENSE file")
        models.append(m)
    if not models:
        sys.exit(f"no model in {MANIFEST.relative_to(REPO)} has URLs under {prefix}")
    return models


def stage(models: list[dict], src: Path, assets: Path) -> dict[str, tuple[str, int]]:
    """Copy and verify the model files; returns {asset name: (sha256, size)}."""
    shutil.rmtree(assets, ignore_errors=True)
    assets.mkdir(parents=True)
    staged = {}
    for m in models:
        for f in m["files"]:
            name = f"{m['id']}.{f['name']}"
            path = src / m["id"] / f["name"]
            if not path.is_file():
                sys.exit(f"missing {path}; run `python scripts/models/opus_mt.py --no-install` first")
            got = (sha256(path), path.stat().st_size)
            if got != (f["sha256"], f["size"]):
                sys.exit(f"{path}: got {got}, manifest has {(f['sha256'], f['size'])}")
            shutil.copyfile(path, assets / name)
            staged[name] = got
        log(f"{m['id']}: {len(m['files'])} files match the manifest")
    licenses = {staged[f"{m['id']}.LICENSE"][0] for m in models}
    if len(licenses) != 1:
        sys.exit("the models' LICENSE files differ; add each one to the release notes by hand")
    shutil.copyfile(assets / f"{models[0]['id']}.LICENSE", assets / LICENSE_ASSET)
    return staged


def model_lines(m: dict) -> list[str]:
    zip_spec = opus_mt.MODELS.get(m["id"], {})
    lines = [
        f"### `{m['id']}` ({m['source']} → {m['target']}, `{m['version']}`)",
        "",
        f"- License: {m['license']} (`{LICENSE_ASSET}`, also `{m['id']}.LICENSE`)",
        f"- Attribution: {m['attribution']}",
        f"- Source: {m['source_url']}",
    ]
    if zip_spec:
        lines.append(f"- Converted from: {zip_spec['url']} (SHA-256 `{zip_spec['sha256']}`)")
    return lines + [""]


def write_texts(models: list[dict], staged: dict[str, tuple[str, int]], tag: str, dest: Path) -> None:
    assets = dest / "assets"
    attribution = ["# Attribution", "", "The model files in this release are:", ""]
    for m in models:
        attribution += model_lines(m)
    (assets / "ATTRIBUTION.md").write_text("\n".join(attribution), encoding="utf-8", newline="\n")

    sums = {name: sha256(assets / name) for name in sorted(p.name for p in assets.iterdir())}
    (assets / "SHA256SUMS").write_text(
        "".join(f"{h}  {name}\n" for name, h in sums.items()), encoding="utf-8", newline="\n"
    )

    total = sum(size for _, size in staged.values())
    notes = [
        "**This is a models release, not an app release.** It hosts the translation models that "
        "Polyglass downloads when you click ⬇ in the lens toolbar; app releases are tagged "
        "separately.",
        "",
        f"Swedish ↔ English OPUS-MT models converted to CTranslate2 int8 "
        f"({len(models)} models, {total / 1e6:.1f} MB). The app downloads only the URLs in "
        "`models/manifest.toml` and checks every file against the SHA-256 listed there.",
        "",
        "## Attribution",
        "",
    ]
    for m in models:
        notes += model_lines(m)
    notes += [
        "## How these files were made",
        "",
        f"Built from commit {commit()} by `.github/workflows/models-release.yml`:",
        "",
        "```",
        "python scripts/models/opus_mt.py --no-install",
        f"python scripts/models/release_assets.py --tag {tag}",
        "```",
        "",
        "`opus_mt.py` downloads the official release zips (SHA-256 pinned), converts them with a "
        "hash-locked CTranslate2 4.8.4 and checks every output against pinned SHA-256s, so "
        "running it yourself gives byte-identical files. `SHA256SUMS` lists every asset.",
        "",
        "## Files",
        "",
        "| Asset | Bytes | SHA-256 |",
        "|---|---:|---|",
    ]
    notes += [f"| `{n}` | {s} | `{h}` |" for n, (h, s) in sorted(staged.items())]
    notes.append("")
    (dest / "notes.md").write_text("\n".join(notes), encoding="utf-8", newline="\n")


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--tag", required=True, help="release tag, models-opus-mt-YYYY-MM-DD[-N]")
    p.add_argument("--from", dest="src", type=Path, default=REPO / "target" / "opus-mt" / "out",
                   help="opus_mt.py's output directory")
    p.add_argument("--dest", type=Path, help="default: target/opus-mt/release/<tag>")
    args = p.parse_args()
    if not TAG.fullmatch(args.tag):
        p.error(f"tag {args.tag!r} doesn't look like models-opus-mt-YYYY-MM-DD[-N]")
    dest = (args.dest or REPO / "target" / "opus-mt" / "release" / args.tag).resolve()

    manifest = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    models = release_models(manifest, args.tag)
    staged = stage(models, args.src.resolve(), dest / "assets")
    write_texts(models, staged, args.tag, dest)
    log(f"staged {len(list((dest / 'assets').iterdir()))} assets and notes.md in {dest}")


if __name__ == "__main__":
    main()
