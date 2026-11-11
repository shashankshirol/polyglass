"""Tests for release_assets.py. Run: python -m unittest discover -s scripts -p "test_*.py"."""

from __future__ import annotations

import hashlib
import re
import tempfile
import unittest
from pathlib import Path

import release_assets as ra


class Version(unittest.TestCase):
    def test_the_repository_files_agree_on_the_version(self) -> None:
        found = ra.versions()
        self.assertEqual(len(set(found.values())), 1, found)
        self.assertEqual(ra.version_of(f"v{found['Cargo.toml']}", found), found["Cargo.toml"])

    def test_a_tag_must_name_the_version_of_every_file(self) -> None:
        same = {"Cargo.toml": "0.2.0", "tauri.conf.json": "0.2.0", "package.json": "0.2.0"}
        self.assertEqual(ra.version_of("v0.2.0", same), "0.2.0")
        pre = dict.fromkeys(same, "0.2.0-beta.1")
        self.assertEqual(ra.version_of("v0.2.0-beta.1", pre), "0.2.0-beta.1")
        with self.assertRaises(SystemExit) as e:
            ra.version_of("v0.2.0", {**same, "package.json": "0.1.0"})
        self.assertIn("package.json has 0.1.0", str(e.exception))
        for bad in ["0.2.0", "v0.2", "models-opus-mt-2026-11-01", "v0.2.0 ", "v1.0.0; rm"]:
            with self.assertRaises(SystemExit, msg=bad):
                ra.version_of(bad, same)


class Installer(unittest.TestCase):
    NAME = "Polyglass_0.1.0_x64-setup.exe"

    def bundle(self, root: Path, size: int) -> Path:
        (root / "nsis").mkdir(parents=True, exist_ok=True)
        with (root / "nsis" / self.NAME).open("wb") as f:
            f.truncate(size)
        return root

    def test_size01_an_installer_over_15_mb_stops_the_release(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            ok = ra.installer(self.bundle(Path(tmp), 15_000_000), "0.1.0")
            self.assertEqual(ok.name, self.NAME)
            with self.assertRaises(SystemExit) as e:
                ra.installer(self.bundle(Path(tmp), 15_000_001), "0.1.0")
            self.assertIn("SIZE-01", str(e.exception))

    def test_a_missing_installer_or_another_version_is_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = self.bundle(Path(tmp), 10)
            with self.assertRaises(SystemExit):
                ra.installer(root, "0.2.0")
            (root / "nsis" / self.NAME).unlink()
            with self.assertRaises(SystemExit):
                ra.installer(root, "0.1.0")


class Winget(unittest.TestCase):
    SHA = "cd" * 32

    def test_the_three_manifest_files_name_the_release_assets(self) -> None:
        files = ra.winget_manifests("0.1.0", "v0.1.0", self.SHA)
        self.assertEqual(sorted(files), [
            "ShashankShirol.Polyglass.installer.yaml",
            "ShashankShirol.Polyglass.locale.en-US.yaml",
            "ShashankShirol.Polyglass.yaml",
        ])
        for text in files.values():
            self.assertIn("PackageIdentifier: ShashankShirol.Polyglass\nPackageVersion: 0.1.0\n", text)
            self.assertNotIn("\t", text)
        installer = files["ShashankShirol.Polyglass.installer.yaml"]
        base = "https://github.com/shashankshirol/polyglass/releases/download/v0.1.0/"
        self.assertIn(f"InstallerUrl: {base}Polyglass_0.1.0_x64-setup.exe\n"
                      f"  InstallerSha256: {'CD' * 32}\n", installer)
        # One installer, for the current user: v1 ships no MSI (D-24).
        self.assertEqual(installer.count("InstallerUrl:"), 1)
        self.assertIn("InstallerType: nullsoft\n  Scope: user\n", installer)
        self.assertNotIn(".msi", installer)
        self.assertEqual(re.findall(r"ManifestType: (\w+)", "".join(files.values())),
                         ["installer", "defaultLocale", "version"])

    def test_the_locale_file_links_the_privacy_policy_and_license(self) -> None:
        locale = ra.winget_manifests("0.1.0", "v0.1.0", self.SHA)[
            "ShashankShirol.Polyglass.locale.en-US.yaml"]
        self.assertIn("PrivacyUrl: https://github.com/shashankshirol/polyglass/blob/main/PRIVACY.md", locale)
        self.assertIn("License: Apache-2.0", locale)


class Sums(unittest.TestCase):
    def test_sha256sums_lists_every_file_in_sha256sum_format(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            b, a = Path(tmp) / "b.msi", Path(tmp) / "a.exe"
            b.write_bytes(b"msi")
            a.write_bytes(b"exe")
            text = ra.sums([b, a])
            self.assertEqual(text, f"{hashlib.sha256(b'exe').hexdigest()} *a.exe\n"
                                   f"{hashlib.sha256(b'msi').hexdigest()} *b.msi\n")

    def test_the_release_notes_name_the_installers_and_how_to_verify(self) -> None:
        text = ra.notes("0.1.0", "v0.1.0", "P-setup.exe")
        for want in ["Run `P-setup.exe`. It installs for the current user", "sha256sum --check SHA256SUMS",
                     "gh attestation verify P-setup.exe", "signed tag `v0.1.0`", "not code-signed yet"]:
            self.assertIn(want, text)
        signed = ra.notes("0.1.0", "v0.1.0", "P-setup.exe", signed=True)
        self.assertNotIn("not code-signed", signed)
        self.assertIn("certificate by SignPath Foundation", signed)


if __name__ == "__main__":
    unittest.main()
