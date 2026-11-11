# Releasing Polyglass

Releases are built by GitHub Actions from a signed version tag and nowhere else (SPEC OSS-02).
`.github/workflows/release.yml` does the work, and only when a maintainer starts it by hand:
pushing a tag starts nothing. This page says what the maintainer does around it and how code
signing is set up.

## Making a release

1. Set the version in `Cargo.toml` (`[workspace.package]`), `apps/desktop/src-tauri/tauri.conf.json`
   and `apps/desktop/package.json`, and commit. `python scripts/release_assets.py check-version
   --tag v<version>` checks that the three agree.
2. Run the manual Windows CI job on that commit (Actions → CI → Run workflow) and, after a
   WebView2 runtime update, `apps/desktop/scripts/lens-net.ps1` (PRIV-01).
3. Create and push a **signed** tag (GitHub must know the key: Settings → SSH and GPG keys):

   ```
   git tag -s v0.1.0 -m "Polyglass 0.1.0"
   git push origin main v0.1.0
   ```

4. Start the build: GitHub → Actions → Release → Run workflow, and type the tag (`v0.1.0`) in
   the "tag" box. From a terminal: `gh workflow run release.yml -f tag=v0.1.0`.
5. The workflow checks out that tag, checks its signature and the version, runs `cargo deny` and `npm audit`,
   builds the installer on `windows-latest`, signs it if signing is set up
   (below), and stages a **draft** release with the installer, `THIRD_PARTY_NOTICES.md`, two
   CycloneDX SBOMs, the winget manifest files and `SHA256SUMS`. GitHub stores build-provenance
   and SBOM attestations for the installer.
6. If signing is on, approve the signing request in SignPath when it arrives (the build job
   waits up to four hours for it).
7. Read the draft release, then publish it by hand.

The workflow file that runs is the one on the branch it is started from (normally `main`); the
code it builds is the tag's. A tag with a hyphen (`v0.2.0-beta.1`) becomes a pre-release. A release is never replaced: fix
forward with a new version.

To rehearse the staging locally (nothing is uploaded):

```
cd apps/desktop && npm run tauri build && cd ../..
python scripts/release_assets.py stage --tag v0.1.0
```

It needs `cargo install cargo-cyclonedx --locked` for the Rust SBOM and network access for
`npx` (the npm SBOM); `--no-sbom` leaves both out.

## Code signing with SignPath Foundation

The owner chose [SignPath Foundation](https://signpath.org/), which signs open-source projects
for free with its own certificate (decision D-4, 2026-11-10). Until the three secrets below
exist, the workflow skips its "Sign" steps and releases are unsigned.

### What the maintainer sets up, once

1. **Make the repository public.** SignPath verifies that each signed file was built from this
   repository by this workflow.
2. **Meet the Foundation's conditions** (<https://signpath.org/terms>): an OSI license with no
   proprietary parts (Apache-2.0 here), a released and maintained project, multi-factor
   authentication on GitHub and SignPath for everyone on the team, and a "Code signing policy"
   on the project's home page. For Polyglass that is a README section like this, added when the
   application is accepted:

   ```markdown
   ## Code signing policy

   Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
   [SignPath Foundation](https://signpath.org/).

   - Committers and reviewers: [the repository's maintainers](https://github.com/shashankshirol/polyglass/graphs/contributors)
   - Approvers: [the repository owner](https://github.com/shashankshirol)
   - Privacy: Polyglass sends nothing anywhere unless you ask it to download a translation
     model. See [PRIVACY.md](PRIVACY.md).
   ```

3. **Apply** at <https://signpath.org/apply> with the repository URL. SignPath creates an
   organization and a project for Polyglass.
4. **In SignPath:** link the predefined "GitHub.com" trusted build system to the project, and
   give the project an artifact configuration for what the workflow uploads: a ZIP holding
   `Polyglass_<version>_x64-setup.exe`. A starting point (check it against SignPath's artifact
   configuration reference):

   ```xml
   <artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
     <zip-file>
       <pe-file path="Polyglass_*_x64-setup.exe">
         <authenticode-sign />
       </pe-file>
     </zip-file>
   </artifact-configuration>
   ```

   This signs the installer. `polyglass.exe` inside it stays unsigned, which is enough for
   SmartScreen and for the installer's publisher name; signing it too means signing while Tauri
   bundles (`bundle.windows.signCommand`).
5. **Create a CI user** in SignPath with submitter rights for the project's release signing
   policy, and an API token for it.
6. **Add three repository secrets** (GitHub → Settings → Secrets and variables → Actions):

   | Secret | Value |
   |---|---|
   | `SIGNPATH_ORGANIZATION_ID` | the organization ID SignPath shows |
   | `SIGNPATH_PROJECT_SLUG` | the project's slug |
   | `SIGNPATH_API_TOKEN` | the CI user's API token |

   If the signing policy isn't called `release-signing`, also set the repository *variable*
   `SIGNPATH_SIGNING_POLICY_SLUG`.

The token is entered in GitHub's settings by the maintainer and nowhere else: it is never
committed, pasted into an issue or given to a tool.

### What the workflow then does

In the build job, after `tauri build`: the unsigned installer is uploaded as the artifact
`unsigned-installer`, a signing request for it is submitted, the job waits for the approval and
downloads the signed file, checks its Authenticode signature, and puts it in place of the
unsigned one. Hashes, SBOM subjects, the winget manifest and the attestations are all made from
the signed file. The steps have not run yet: expect to adjust them on the first signed
release.

## Updater (planned for v2)

v1 has no updater and never checks for updates (owner decision D-21): new versions come from
the releases page or `winget upgrade`. Nothing for it is compiled into the app, on purpose: a
dormant updater would be network code in an app whose promise is that it makes no connections.
What is prepared is the plan, so that v2 only has to follow it (SPEC SEC-04: opt-in, signed,
unsigned updates rejected):

1. **Key pair.** `npm run tauri signer generate` makes the updater's own key pair (minisign;
   this is not the code-signing certificate). The public key goes into `tauri.conf.json`
   (`plugins.updater.pubkey`); the private key and its password become the repository secrets
   `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, entered by the
   maintainer.
2. **App.** Add `tauri-plugin-updater` (a new dependency: check it with `cargo deny` and add it
   to the notices), register it in `main.rs`, and give no window its permissions: the check
   runs from Rust only, like the hotkeys. Add `[updates] check = false` to `settings.toml`
   (`settings.rs`) and a switch in Settings; with it off, no request is made. The endpoint is
   `https://github.com/shashankshirol/polyglass/releases/latest/download/latest.json`.
3. **Build.** Set `bundle.createUpdaterArtifacts: true`; `tauri build` then writes a `.sig` next
   to the installer when the two secrets are in the build job's environment. If SignPath
   signing is on, the `.sig` must be made **after** the signed installer comes back
   (`npm run tauri signer sign <file>`), or it won't match the published file.
4. **Release.** `scripts/release_assets.py stage` adds the `.sig` files and writes `latest.json`
   (version, notes, date, and per platform the installer URL and signature); both go into
   `SHA256SUMS`.
5. **Docs and tests.** PRIVACY.md gets the update check back (what is requested, from GitHub,
   only when switched on); a test that an update with a wrong signature is refused; a
   `lens-net.ps1` run with the switch off.

## One installer

v1 ships only the NSIS installer (`Polyglass_<version>_x64-setup.exe`): it installs for the
current user and needs no administrator rights (owner decision D-24). There is no MSI. If an
all-users or centrally deployed install is ever asked for, add `"msi"` to `bundle.targets` in
`tauri.conf.json` and put it back into `scripts/release_assets.py`, the workflow's signing and
attestation steps and the winget manifest (`git log -S"x64_en-US.msi"` finds the removal).

## winget

`release_assets.py` writes the three manifest files for each release (`winget-*.yaml` among the
release assets). Nothing is submitted automatically. To publish to winget, copy them without
the `winget-` prefix to `manifests/s/ShashankShirol/Polyglass/<version>/` in a fork of
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs), run `winget validate
--manifest <folder>`, and open a pull request there. The installer URLs only work once the
release is published.
