# Releasing Polyglass

Releases are built by GitHub Actions from a signed version tag and nowhere else (SPEC OSS-02).
`.github/workflows/release.yml` does the work; this page says what the maintainer does around it
and how code signing is set up.

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

4. The workflow checks the tag's signature and the version, runs `cargo deny` and `npm audit`,
   builds the MSI and NSIS installers on `windows-latest`, signs them if signing is set up
   (below), and stages a **draft** release with the installers, `THIRD_PARTY_NOTICES.md`, two
   CycloneDX SBOMs, the winget manifest files and `SHA256SUMS`. GitHub stores build-provenance
   and SBOM attestations for the installers.
5. If signing is on, approve the signing request in SignPath when it arrives (the build job
   waits up to four hours for it).
6. Read the draft release, then publish it by hand.

A tag with a hyphen (`v0.2.0-beta.1`) becomes a pre-release. A release is never replaced: fix
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
   give the project an artifact configuration for what the workflow uploads: one ZIP holding
   `msi/Polyglass_<version>_x64_en-US.msi` and `nsis/Polyglass_<version>_x64-setup.exe`. A
   starting point (check it against SignPath's artifact configuration reference):

   ```xml
   <artifact-configuration xmlns="http://signpath.io/artifact-configuration/v1">
     <zip-file>
       <directory path="msi">
         <msi-file path="Polyglass_*_x64_en-US.msi">
           <authenticode-sign />
         </msi-file>
       </directory>
       <directory path="nsis">
         <pe-file path="Polyglass_*_x64-setup.exe">
           <authenticode-sign />
         </pe-file>
       </directory>
     </zip-file>
   </artifact-configuration>
   ```

   This signs the two installers. `polyglass.exe` inside them stays unsigned, which is enough
   for SmartScreen and for the installer's publisher name; signing it too means either listing
   it inside the `msi-file` element or signing while Tauri bundles (`bundle.windows.signCommand`).
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

In the build job, after `tauri build`: the unsigned installers are uploaded as the artifact
`unsigned-installers`, a signing request for it is submitted, the job waits for the approval and
downloads the signed files, checks each one's Authenticode signature, and puts them in place of
the unsigned ones. Hashes, SBOM subjects, the winget manifests and the attestations are all made
from the signed files. The steps have not run yet: expect to adjust them on the first signed
release.

Signed updates (SEC-04) are a separate thing: Polyglass has no updater yet.

## winget

`release_assets.py` writes the three manifest files for each release (`winget-*.yaml` among the
release assets). Nothing is submitted automatically. To publish to winget, copy them without
the `winget-` prefix to `manifests/s/ShashankShirol/Polyglass/<version>/` in a fork of
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs), run `winget validate
--manifest <folder>`, and open a pull request there. The installer URLs only work once the
release is published.
