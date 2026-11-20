#!/usr/bin/env bash
# Release smoke test (docs/design/DECISIONS.md section 6), run on every
# target's own runner against the binary that ships in its archive:
#
#   1. `qr --version` prints the expected version;
#   2. a PNG and an SVG are generated from the same text;
#   3. the PNG is decoded with `qr decode` and must equal the input;
#   4. the SVG must be an SVG document.
#
# Usage: scripts/smoke-test.sh <path-to-qr> [expected-version]
# Runs under bash on Linux, macOS and Windows (Git Bash).
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <path-to-qr> [expected-version]" >&2
  exit 2
fi
qr=$1
expected_version=${2:-}

fail() {
  echo "smoke test FAILED: $*" >&2
  exit 1
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Non-ASCII, punctuation and a URL, so encoding modes and UTF-8 handling on
# each platform are exercised, not only the happy ASCII path.
input='https://example.com/qr?smoke=1&os=all ✓ Grüße'

version_line=$("$qr" --version) || fail "qr --version exited $?"
version_line=${version_line%$'\r'}
echo "version: $version_line"
[[ $version_line == "qr "* ]] || fail "unexpected --version output: $version_line"
if [[ -n $expected_version && $version_line != "qr $expected_version" ]]; then
  fail "expected 'qr $expected_version', got '$version_line'"
fi

"$qr" -o "$work/smoke.png" -- "$input" || fail "PNG generation exited $?"
"$qr" -o "$work/smoke.svg" -- "$input" || fail "SVG generation exited $?"
test -s "$work/smoke.png" || fail "smoke.png is empty"
test -s "$work/smoke.svg" || fail "smoke.svg is empty"

# PNG signature: \x89 P N G.
[[ $(head -c 4 "$work/smoke.png" | od -An -tx1 | tr -d ' \n') == 89504e47 ]] ||
  fail "smoke.png is not a PNG"
grep -q '<svg' "$work/smoke.svg" || fail "smoke.svg is not an SVG"

# --raw prints the payload unescaped plus a newline, which $(...) strips.
decoded=$("$qr" decode --raw "$work/smoke.png") || fail "decode exited $?"
[[ $decoded == "$input" ]] || fail "decoded '$decoded', expected '$input'"

echo "smoke test passed: --version, PNG + SVG generated, PNG decodes to the input"
