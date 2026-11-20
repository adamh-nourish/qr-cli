#!/usr/bin/env bash
# Generates the shell completions and the man page that go into every release
# archive (dist-workspace.toml `include`), from the `qr` binary itself so they
# always match its argument parser.
#
# Usage: scripts/dist-extras.sh <path-to-qr> [out-dir]   (default: dist-extras)
#
# Layout (file names are the ones each shell looks for):
#   <out-dir>/completions/qr.bash   bash
#   <out-dir>/completions/_qr       zsh
#   <out-dir>/completions/qr.fish   fish
#   <out-dir>/completions/_qr.ps1   PowerShell
#   <out-dir>/completions/qr.elv    elvish
#   <out-dir>/man/qr.1              man page (roff)
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <path-to-qr> [out-dir]" >&2
  exit 2
fi
qr=$1
out=${2:-dist-extras}

if [[ ! -x $qr ]]; then
  echo "error: '$qr' is not an executable file" >&2
  exit 2
fi

rm -rf "$out/completions" "$out/man"
mkdir -p "$out/completions" "$out/man"

completion_file() {
  case $1 in
    bash) echo qr.bash ;;
    zsh) echo _qr ;;
    fish) echo qr.fish ;;
    powershell) echo _qr.ps1 ;;
    elvish) echo qr.elv ;;
  esac
}

for shell in bash zsh fish powershell elvish; do
  file="$out/completions/$(completion_file "$shell")"
  "$qr" completions "$shell" > "$file"
  # An empty script would ship silently; fail the build instead.
  test -s "$file"
done

"$qr" man > "$out/man/qr.1"
grep -q '^\.TH ' "$out/man/qr.1"

echo "wrote completions and man page to $out:"
find "$out" -type f | LC_ALL=C sort
