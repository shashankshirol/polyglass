#!/usr/bin/env python3
"""Write THIRD_PARTY_NOTICES.md for the Polyglass app (OSS-01).

    python scripts/notices.py [--out THIRD_PARTY_NOTICES.md]

Run it for every release (M5-2). It needs `cargo about` (`cargo install cargo-about --locked
--features cli`) and the app's npm packages (`npm ci` in apps/desktop).

1. `cargo about generate` lists the Rust crates compiled into polyglass.exe with their license
   texts (`about.toml`: the licenses deny.toml allows, Windows target, no build or dev
   dependencies; `about.hbs`: the layout). It runs offline on Cargo.lock (`--frozen`) and stops
   on a crate whose license it can't read (`--fail`).
2. cargo-about can't see the C++ code that `ct2rs` (CTranslate2 and its third-party code) and
   `sentencepiece-sys` (SentencePiece and its third-party code) compile into the app, nor the
   npm package bundled into the UI. Their license files are appended from the crates' sources
   (found through `cargo metadata`) and from node_modules. The list is kept here: check it when
   those crates or packages are upgraded. A file that is missing stops the script.

Translation models aren't bundled; their licenses are in models/manifest.toml.
Only the Python standard library is used.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
APP_MANIFEST = REPO / "apps" / "desktop" / "src-tauri" / "Cargo.toml"

# (crate whose source holds it, name, SPDX license, license file relative to the crate).
BUNDLED = [
    ("ct2rs", "CTranslate2 (OpenNMT)", "MIT", "CTranslate2/LICENSE"),
    ("ct2rs", "ruy (Google), in CTranslate2", "Apache-2.0", "CTranslate2/third_party/ruy/LICENSE"),
    ("ct2rs", "cpu_features (Google), in CTranslate2", "Apache-2.0",
     "CTranslate2/third_party/cpu_features/LICENSE"),
    ("ct2rs", "spdlog (Gabi Melman), in CTranslate2", "MIT", "CTranslate2/third_party/spdlog/LICENSE"),
    ("sentencepiece-sys", "SentencePiece (Google)", "Apache-2.0", "source/LICENSE"),
    ("sentencepiece-sys", "Abseil (Google), in SentencePiece", "Apache-2.0",
     "source/third_party/absl/LICENSE"),
    ("sentencepiece-sys", "Protocol Buffers lite (Google), in SentencePiece", "BSD-3-Clause",
     "source/third_party/protobuf-lite/LICENSE"),
    ("sentencepiece-sys", "darts-clone (Susumu Yata), in SentencePiece", "BSD-3-Clause",
     "source/third_party/darts_clone/LICENSE"),
    ("sentencepiece-sys", "esaxx (Daisuke Okanohara), in SentencePiece", "MIT",
     "source/third_party/esaxx/LICENSE"),
]
# Header-only parts of CTranslate2 whose license is stated in the file itself.
HEADER_NOTES = [
    "BS::thread_pool (Barak Shoshany), MIT, https://github.com/bshoshany/thread-pool, in "
    "CTranslate2 (`third_party/BS_thread_pool.hpp`).",
    "avx_mathfun / neon_mathfun (Julien Pommier; AVX version modified in Marian), zlib and MIT, "
    "in CTranslate2 (`third_party/*_mathfun.h`).",
]
# (package, name, SPDX license, license files relative to node_modules/<package>).
NPM = [
    ("@tauri-apps/api", "@tauri-apps/api (Tauri Programme within The Commons Conservancy)",
     "Apache-2.0 OR MIT", ["LICENSE-APACHE-2.0", "LICENSE-MIT"]),
]


def crate_dirs() -> dict[str, Path]:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--frozen", "--manifest-path", str(APP_MANIFEST)],
        check=True, capture_output=True, text=True, encoding="utf-8", cwd=REPO,
    ).stdout
    return {p["name"]: Path(p["manifest_path"]).parent for p in json.loads(out)["packages"]}


def section(name: str, spdx: str, texts: list[str]) -> str:
    body = "\n\n".join(t.strip() for t in texts)
    return f"### {name}\n\nLicense: {spdx}\n\n```\n{body}\n```\n"


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--out", type=Path, default=REPO / "THIRD_PARTY_NOTICES.md")
    args = p.parse_args()

    rust = subprocess.run(
        ["cargo", "about", "generate", "--frozen", "--fail", "--manifest-path", str(APP_MANIFEST),
         str(REPO / "about.hbs")],
        check=True, capture_output=True, text=True, encoding="utf-8", cwd=REPO,
    ).stdout

    dirs = crate_dirs()
    parts = [rust.rstrip(), "", "## C++ libraries built into the app", "",
             "Compiled from source by the `ct2rs` and `sentencepiece-sys` crates.", ""]
    for crate, name, spdx, rel in BUNDLED:
        f = dirs[crate] / rel
        if not f.is_file():
            sys.exit(f"missing license file for {name}: {f}")
        parts.append(section(name, spdx, [f.read_text(encoding="utf-8", errors="replace")]))
    parts += ["Also in CTranslate2, with their license in the file itself:", ""]
    parts += [f"- {n}" for n in HEADER_NOTES]
    parts += ["", "## JavaScript bundled into the user interface", ""]
    modules = REPO / "apps" / "desktop" / "node_modules"
    for package, name, spdx, files in NPM:
        texts = []
        for rel in files:
            f = modules / package / rel
            if not f.is_file():
                sys.exit(f"missing license file for {package}: {f} (run `npm ci` in apps/desktop)")
            texts.append(f.read_text(encoding="utf-8", errors="replace"))
        parts.append(section(name, spdx, texts))

    args.out.write_text("\n".join(parts).rstrip() + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {args.out} ({args.out.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
