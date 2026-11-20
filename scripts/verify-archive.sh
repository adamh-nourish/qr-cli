#!/usr/bin/env bash
# Checks one release archive the way a user receives it, then smoke tests the
# binary inside it (scripts/smoke-test.sh):
#
#   - its SHA-256 matches the published `.sha256` file;
#   - it holds the binary, both licences, README.md, CHANGELOG.md, the
#     completions for every shell and the man page;
#   - on Linux, the binary is statically linked.
#
# Usage: scripts/verify-archive.sh <distrib-dir> <target-triple> [expected-version]
# Runs under bash on Linux, macOS and Windows (Git Bash).
set -euo pipefail

if [[ $# -lt 2 || $# -gt 3 ]]; then
  echo "usage: $0 <distrib-dir> <target-triple> [expected-version]" >&2
  exit 2
fi
distrib=$1
target=$2
expected_version=${3:-}
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

fail() {
  echo "archive check FAILED: $*" >&2
  exit 1
}

case $target in
  *-windows-*) ext=zip bin=qr.exe ;;
  *) ext=tar.xz bin=qr ;;
esac
name="qr-cli-${target}"
archive="${name}.${ext}"
[[ -f "$distrib/$archive" ]] || fail "missing $distrib/$archive"
[[ -f "$distrib/$archive.sha256" ]] || fail "missing $distrib/$archive.sha256"

# The .sha256 file is "<hash> *<file>"; compare the hash only, so a trailing
# blank line or the binary-mode marker cannot matter.
want=$(awk 'NF { print $1; exit }' "$distrib/$archive.sha256")
if command -v sha256sum > /dev/null; then
  got=$(sha256sum "$distrib/$archive" | awk '{ print $1 }')
else
  got=$(shasum -a 256 "$distrib/$archive" | awk '{ print $1 }')
fi
[[ -n $want && $got == "$want" ]] || fail "$archive: SHA-256 $got, expected $want"
echo "checksum OK: $archive $got"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
if [[ $ext == zip ]]; then
  if command -v unzip > /dev/null; then
    unzip -q "$distrib/$archive" -d "$work"
  else
    # PowerShell needs Windows paths, not Git Bash's /tmp/... ones.
    powershell.exe -NoProfile -NonInteractive -Command \
      "Expand-Archive -LiteralPath '$(cygpath -w "$distrib/$archive")' -DestinationPath '$(cygpath -w "$work")'"
  fi
else
  tar -xJf "$distrib/$archive" -C "$work"
fi

# dist puts tar.xz contents under a <name>/ directory and zip contents at the
# top level; accept either.
root=$work
[[ -d "$work/$name" ]] && root="$work/$name"

for f in "$bin" LICENSE-MIT LICENSE-APACHE README.md CHANGELOG.md \
  completions/qr.bash completions/_qr completions/qr.fish \
  completions/_qr.ps1 completions/qr.elv man/qr.1; do
  [[ -s "$root/$f" ]] || fail "$archive does not contain $f"
done
echo "contents OK: $(cd "$root" && find . -type f | LC_ALL=C sort | tr '\n' ' ')"

if [[ $target == *-linux-* ]] && command -v file > /dev/null; then
  info=$(file -b "$root/$bin")
  [[ $info =~ static(-pie)?\ linked ]] || fail "$bin is not statically linked: $info"
  echo "linkage OK: $info"
fi

"$here/smoke-test.sh" "$root/$bin" "$expected_version"
