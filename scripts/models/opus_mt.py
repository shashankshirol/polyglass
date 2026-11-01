#!/usr/bin/env python3
"""Convert Helsinki-NLP's OPUS-MT models to CTranslate2 int8 and install them (ADR-0005).

One command, from the repository root:

    python scripts/models/opus_mt.py

1. Creates a venv in the work directory with the hash-locked converter from
   `scripts/models/requirements.txt` (ctranslate2, numpy, pyyaml).
2. Downloads each official OPUS-MT release zip and checks its SHA-256 (pinned below).
3. Extracts only the files the converter needs and converts the Marian `.npz` to CTranslate2 int8.
4. Checks every output file against the SHA-256 pinned below, so the result is byte-for-byte
   what `models/manifest.toml` lists.
5. Installs the models into the desktop app's model store with
   `cargo run -p pg-models --bin import-models`, which verifies them against the manifest again
   (MDL-04).

The app normally downloads the same files from this repository's GitHub release (owner decision
D-9; `scripts/models/release_assets.py` stages that release from this script's output). Use this
script to install offline, to check the release, or to build a new one. Options: `--work DIR` (default `target/opus-mt`), `--store DIR` (default: the
app's store), `--no-install`, `--clean` (delete downloads and the venv afterwards), `--update-pins`
(print the output hashes instead of checking them, for a new release or converter version), and
model ids to limit the run (default: all below). Needs Python >= 3.12 and, for the install step,
Rust/Cargo. Only the Python standard library is used outside the venv.

Licenses: the models are CC-BY-4.0 (the LICENSE in each zip; Hugging Face's cards say Apache-2.0,
the stricter one is used). The LICENSE file is installed next to each model and the manifest
carries the attribution.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import urllib.request
import venv
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
REQUIREMENTS = Path(__file__).resolve().parent / "requirements.txt"

# Newest official OPUS-MT releases for each direction (Tatoeba-Challenge, released-models.txt,
# checked 2026-11-01). Both are transformer-align with 32k SentencePiece vocabularies; the newer
# transformer-big releases of other pairs would not fit SIZE-05.
MODELS = {
    "opus-mt-sv-en": {
        "url": "https://object.pouta.csc.fi/Tatoeba-MT-models/swe-eng/opus%2Bbt-2021-04-30.zip",
        "size": 286052961,
        "sha256": "2c19bd94e20abc47fc8c58eefab1dcd5de7bd887c6f3eebe0aa15c809769cba1",
        # Converted files (SHA-256, bytes); `--update-pins` prints these.
        "outputs": {
            "LICENSE": ("945fbcb37deca05bccd7c3ca0e261679f1f5373515504fdbad68f6f3cf6f49b9", 18652),
            "config.json": ("95dabd564e6abdb470e95f9106766647d8dc59e1bc2a1517d292c0cee5ca5812", 231),
            "model.bin": ("1971f05964f526a13c76df3f1178e522a7c42818243a1b8e5272877952ae65cc", 81875117),
            "shared_vocabulary.json": ("27f6e96e3f8266da803bf91a5b120f375b6cd00676582450c8e55c233e0c6705", 1327665),
            "source.spm": ("fb63614d9ec099e6fcbf1efd42cac97464c9017bc49a44181c986e9d4b18c6da", 811815),
            "target.spm": ("63d083fc4aafe025d3d7bc382cce13e3df54acb23e001a37f48535dbffc44700", 799820),
        },
    },
    "opus-mt-en-sv": {
        "url": "https://object.pouta.csc.fi/Tatoeba-MT-models/eng-swe/opus%2Bbt-2021-04-14.zip",
        "size": 286201633,
        "sha256": "1446a6b6d6bcdc6e862eb0cedd733f781ed9d1ee88fbb0bdb2f324549f16b0f9",
        "outputs": {
            "LICENSE": ("945fbcb37deca05bccd7c3ca0e261679f1f5373515504fdbad68f6f3cf6f49b9", 18652),
            "config.json": ("95dabd564e6abdb470e95f9106766647d8dc59e1bc2a1517d292c0cee5ca5812", 231),
            "model.bin": ("284be38e4e010626bdef041ddbd73a27a00dc9be872df1d05078f44a6859e68c", 81875117),
            "shared_vocabulary.json": ("27f6e96e3f8266da803bf91a5b120f375b6cd00676582450c8e55c233e0c6705", 1327665),
            "source.spm": ("63d083fc4aafe025d3d7bc382cce13e3df54acb23e001a37f48535dbffc44700", 799820),
            "target.spm": ("fb63614d9ec099e6fcbf1efd42cac97464c9017bc49a44181c986e9d4b18c6da", 811815),
        },
    },
}

# What the converter writes, plus what is copied from the zip.
CONVERTED = ["model.bin", "config.json", "shared_vocabulary.json"]
COPIED = ["source.spm", "target.spm", "LICENSE"]


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def log(msg: str) -> None:
    print(f"[opus_mt] {msg}", flush=True)


def ensure_venv(work: Path) -> Path:
    """The converter's venv, (re)installed when requirements.txt changes."""
    root = work / "venv"
    python = root / ("Scripts/python.exe" if os.name == "nt" else "bin/python")
    stamp = root / "requirements.sha256"
    want = sha256(REQUIREMENTS)
    if python.exists() and stamp.is_file() and stamp.read_text().strip() == want:
        return python
    log(f"creating venv in {root}")
    shutil.rmtree(root, ignore_errors=True)
    venv.EnvBuilder(with_pip=True, clear=True).create(root)
    subprocess.run(
        [str(python), "-I", "-m", "pip", "install", "--quiet", "--disable-pip-version-check",
         "--require-hashes", "--only-binary", ":all:", "--no-deps", "-r", str(REQUIREMENTS)],
        check=True,
    )
    stamp.write_text(want)
    return python


def download(model_id: str, spec: dict, downloads: Path) -> Path:
    zip_path = downloads / f"{model_id}.zip"
    if zip_path.is_file() and zip_path.stat().st_size == spec["size"] and sha256(zip_path) == spec["sha256"]:
        log(f"{model_id}: using {zip_path}")
        return zip_path
    downloads.mkdir(parents=True, exist_ok=True)
    part = zip_path.with_suffix(".zip.part")
    log(f"{model_id}: downloading {spec['url']} ({spec['size'] / 1e6:.0f} MB)")
    req = urllib.request.Request(spec["url"], headers={"User-Agent": "polyglass-opus-mt/1"})
    with urllib.request.urlopen(req, timeout=60) as resp, part.open("wb") as out:
        shutil.copyfileobj(resp, out, 1 << 20)
    got = sha256(part)
    if part.stat().st_size != spec["size"] or got != spec["sha256"]:
        part.unlink()
        sys.exit(f"{model_id}: download does not match the pinned SHA-256 (got {got})")
    part.replace(zip_path)
    return zip_path


def extract(zip_path: Path, dest: Path) -> None:
    """Extract decoder.yml, the files it names, the SentencePiece models and the LICENSE."""
    shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True)
    with zipfile.ZipFile(zip_path) as z:
        names = set(z.namelist())
        decoder = z.read("decoder.yml").decode("utf-8")
        wanted = {"decoder.yml", *COPIED}
        # decoder.yml lists the model and vocabulary files (`  - <name>` lines).
        wanted |= {line.strip()[2:].strip() for line in decoder.splitlines() if line.strip().startswith("- ")}
        for name in sorted(wanted):
            if name not in names or "/" in name or "\\" in name or name.startswith("."):
                sys.exit(f"{zip_path.name}: unexpected or missing member {name!r}")
            (dest / name).write_bytes(z.read(name))


def convert(python: Path, model_id: str, src: Path, out: Path, work: Path) -> None:
    shutil.rmtree(out, ignore_errors=True)
    log(f"{model_id}: converting to CTranslate2 int8")
    subprocess.run(
        [str(python), "-I", "-c", "from ctranslate2.converters.opus_mt import main; main()",
         "--model_dir", str(src), "--output_dir", str(out), "--quantization", "int8"],
        check=True,
        cwd=work,
    )
    for name in COPIED:
        shutil.copyfile(src / name, out / name)


def check_outputs(model_id: str, spec: dict, out: Path, update: bool) -> None:
    files = sorted(CONVERTED + COPIED)
    actual = {name: (sha256(out / name), (out / name).stat().st_size) for name in files}
    if update:
        print(f'    "{model_id}" outputs:')
        for name, (h, size) in actual.items():
            print(f'            "{name}": ("{h}", {size}),')
        return
    pinned = spec["outputs"]
    bad = [n for n in files if pinned.get(n) != actual[n]]
    if bad:
        for n in bad:
            log(f"{model_id}/{n}: got {actual[n]}, pinned {pinned.get(n)}")
        sys.exit(f"{model_id}: converted files differ from the pinned SHA-256 (see above)")
    total = sum(size for _, size in actual.values())
    log(f"{model_id}: {len(files)} files match the pinned SHA-256 ({total / 1e6:.1f} MB)")


def install(out_root: Path, ids: list[str], store: str | None) -> None:
    cmd = ["cargo", "run", "--quiet", "-p", "pg-models", "--bin", "import-models", "--",
           "--from", str(out_root), *ids]
    if store:
        cmd += ["--store", store]
    log("installing: " + " ".join(cmd))
    subprocess.run(cmd, check=True, cwd=REPO)


def main() -> None:
    if sys.version_info < (3, 12):
        sys.exit("Python >= 3.12 is required (numpy 2.5.4)")
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("ids", nargs="*", metavar="MODEL_ID",
                   help=f"models to convert (default: all of {', '.join(MODELS)})")
    p.add_argument("--work", type=Path, default=REPO / "target" / "opus-mt")
    p.add_argument("--store", help="model store to install into (default: the app's)")
    p.add_argument("--no-install", action="store_true")
    p.add_argument("--clean", action="store_true", help="delete downloads and the venv afterwards")
    p.add_argument("--update-pins", action="store_true",
                   help="print output hashes instead of checking them (implies --no-install)")
    args = p.parse_args()
    ids = args.ids or list(MODELS)
    unknown = [i for i in ids if i not in MODELS]
    if unknown:
        p.error(f"unknown model id(s) {unknown}; known: {list(MODELS)}")
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)

    python = ensure_venv(work)
    for model_id in ids:
        spec = MODELS[model_id]
        zip_path = download(model_id, spec, work / "downloads")
        src = work / "extract" / model_id
        extract(zip_path, src)
        out = work / "out" / model_id
        convert(python, model_id, src, out, work)
        shutil.rmtree(src)
        check_outputs(model_id, spec, out, args.update_pins)

    if not (args.no_install or args.update_pins):
        install(work / "out", ids, args.store)
    if args.clean:
        shutil.rmtree(work / "downloads", ignore_errors=True)
        shutil.rmtree(work / "venv", ignore_errors=True)
    log(f"done; converted models are in {work / 'out'}")


if __name__ == "__main__":
    main()
