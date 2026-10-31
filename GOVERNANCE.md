# Governance

Polyglass uses a lightweight maintainer model.

- **Maintainers** review and merge PRs, cut releases, and decide on ADRs. The initial maintainer
  is the project owner; new maintainers are invited after sustained contributions.
- **Decisions** are made by lazy consensus on the PR or issue. If maintainers disagree, the
  project owner decides.
- **Architecture decisions** are recorded as ADRs in `docs/adr/`.
- **Releases** are built only in CI from signed tags (OSS-02).
- **Model manifest changes** (`models/manifest.toml`) need review from a maintainer who has
  checked the URL, SHA-256 and license.
