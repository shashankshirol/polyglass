#!/usr/bin/env bash
# QUAL-02 gate: scripts/bench-gate.sh <baseline pg-bench> <new pg-bench>
#
# Runs both binaries on the same fixtures, one after the other on this machine, and fails when
# the new one's total p50 is more than 20% slower (`pg-bench --baseline`, crates/pg-bench/src/
# gate.rs). Two setups: the pipeline's own overhead with mock OCR and translation, and capture →
# change detection → Windows OCR → grouping on the English dialog (GitHub's Windows runners have
# the en-US OCR language, not sv-SE). On-device translation isn't part of the gate: it needs the
# models (≈ 160 MB) and is measured by the Measure workflow.
#
# A setup that fails is measured again, up to three times, and fails the gate only if every
# attempt does: another process on a shared runner can slow one run down.
set -u
base=${1:?usage: bench-gate.sh <baseline pg-bench> <new pg-bench>}
head=${2:?usage: bench-gate.sh <baseline pg-bench> <new pg-bench>}
out=$(mktemp -d)
failed=0

gate() { # name, --min-regression-ms for the new binary, then the setup's pg-bench arguments
  local name=$1 floor=$2 attempt
  shift 2
  for attempt in 1 2 3; do
    echo "== $name (attempt $attempt)"
    if ! "$base" "$@" --json > "$out/$name.json"; then
      echo "the baseline pg-bench can't run this setup; skipped"
      return 0
    fi
    if "$head" "$@" --json --baseline "$out/$name.json" --min-regression-ms "$floor" > /dev/null; then
      return 0
    fi
  done
  failed=1
}

gate mock 0.2 --fixture tests/fixtures/ui-dialog-sv --runs 1000
gate windows-ocr 1 --fixture tests/fixtures/ui-dialog-en --target sv --ocr windows --runs 200
rm -rf "$out"
exit $failed
