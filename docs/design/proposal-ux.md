# UX proposal: `qr` CLI interface

## Identity
- **Binary:** `qr` (crate/repo `qr-cli`). It is short and easy to guess. **Known conflict:** Python's `qrcode` package also installs a `qr` script. The README should mention this, and packages can ship a `qrgen` alias if a distro asks for one.
- **Principle:** the bare command does the obvious thing, flags change the output, and subcommands are only for structured payloads and tooling.

## Command structure
Generation is the root command. Subcommands build structured payloads or provide tooling.
```
qr [OPTIONS] [TEXT]                 # generate (default)
qr url|wifi|vcard|mecard|email|sms|tel|geo|event [ARGS] [OPTIONS]
qr decode <IMAGE>...                # read/verify
qr batch <FILE>                     # many codes
qr completions <SHELL> | qr man | qr config {path,show,init}
```
**Ambiguity rule:** if the first positional matches a subcommand name, it runs that subcommand. To encode the literal word, use `qr -- wifi` or `qr text wifi`. Document this in `--help`.

## Input (exactly one source; using more than one is a usage error)
1. Positional argument: `qr "hello"`
2. stdin, when there is no positional and stdin is not a TTY: `echo hello | qr`. `-` means stdin explicitly.
3. File: `qr -i payload.txt` (raw bytes, so binary-safe)
4. With no input and a TTY stdin, print short usage and exit 2. Never hang waiting for input.

Trailing newlines from stdin are stripped by default. `--keep-newline` keeps them. Input is capped at 7,089 bytes (the QR v40 maximum) before encoding, so the error comes early.

## Zero-config happy path
```
qr "https://example.com"            # Unicode half-block QR in terminal
qr "https://example.com" -o code.png   # format inferred from extension
qr "hi" -o code.svg --fg '#1a73e8'
```

## Output resolution (in this order)
1. `--format/-f` wins.
2. Otherwise infer from the `-o` extension (`.png .svg .jpg/.jpeg .webp .pdf .eps .txt`). An unknown extension is a usage error with a hint.
3. No `-o` and stdout is a TTY: terminal render.
4. No `-o` and stdout is not a TTY: UTF-8 text render (so `qr x | pbcopy` works). Binary output needs `-f png -o -`, and binary is refused when stdout is a TTY.

Writes are atomic (temp file, then rename). Existing files are overwritten unless `--no-clobber` is set.

## Formats
| v1 | Later |
|---|---|
| `terminal` (ANSI half-blocks), `text` (Unicode), `ascii` (`##`/spaces), `png`, `svg`, `jpeg`, `webp` (lossless), `pdf` (vector, hand-emitted), `eps`, `--data-uri` (wraps png/svg as `data:`) | clipboard (`--copy`, adds deps and needs a platform security review), Sixel/Kitty inline images, `html`, Micro QR, rMQR |

## Styling flags (v1)
- `--ec L|M|Q|H` (default M; raised to H automatically when `--logo` is used, with a notice)
- `--margin N` (quiet zone in modules, default 4; values below 4 warn)
- `--scale N` (pixels per module, default 10) **or** `--size PX` (target width; they conflict)
- `--dpi N` (PNG/JPEG/PDF metadata)
- `--fg`, `--bg`: `#RGB`, `#RRGGBB[AA]` or CSS names; `--transparent` sets bg alpha to 0 (PNG/WebP/SVG only)
- `--shape square|rounded|dots` (modules; raster and vector)
- `--logo PATH`, `--logo-scale 0.2` (maximum 0.3) — PNG, SVG and PDF
- `--qr-version 1..40` (minimum version), `--mask 0..7`, `--mode auto|numeric|alnum|byte`
- `--invert` (for dark terminals and themes)
- **Safety rails:** warn when fg/bg contrast is below 3:1 or when fg is lighter than bg. When `--logo`, `--shape` or a custom colour is used, run `--verify` automatically: decode the rendered raster with `rqrr`, exit 6 if it fails, and allow `--no-verify` to skip.

Later: gradients, custom finder/eye shapes, frames with caption text, style presets (`--preset`).

## Payload helpers
v1 (each validates and escapes per spec):
```
qr url example.com                         # adds https:// and validates
qr wifi --ssid Home --password 's3cr;t' --auth wpa [--hidden]
qr wifi --ssid Home --password-stdin       # keeps secrets out of shell history
qr vcard --name "Ada Lovelace" --phone +441234 --email ada@x.io --org ACME
qr mecard --name "Ada" --phone +441234
qr email ada@x.io --subject Hi --body "..."
qr sms +441234567 --body "Hello"
qr tel +441234567
qr geo 51.5074 -0.1278
qr event --title Launch --start 2026-12-01T09:00Z --end 2026-12-01T10:00Z --location HQ
```
Every helper accepts `--print-payload`, which shows the encoded string and skips rendering. Useful for debugging.

Later: `bitcoin:`/`ethereum:` URIs (crypto is deferred because of low demand and the cost of validating many address formats), EPC/SEPA GiroCode, Swiss QR-bill, MeCard-to-vCard conversion.

## Batch (v1)
```
qr batch links.csv --out-dir out/ -f svg              # CSV columns: data[,filename]
qr batch urls.txt --lines --out-dir out/ --name '{n:04}.png'
```
- Template variables: `{n}`, `{hash}`, and CSV columns (`{filename}`, `{data}`).
- Filenames are sanitised: `..`, absolute paths and separators are rejected, and everything stays inside `--out-dir`.
- Runs in parallel with `--jobs N` (default: number of CPUs).
- `--fail-fast` or continue-on-error (the default), with a summary printed to stderr.

## Decode (v1)
```
qr decode photo.jpg            # prints payload(s)
qr decode a.png --json
```
Image dimensions and decode time are capped to resist decompression bombs. If no QR code is found, exit 5.

## Config and environment
- Config file: `$XDG_CONFIG_HOME/qr/config.toml`, `~/Library/Application Support/qr/` on macOS, `%APPDATA%\qr\` on Windows.
- `--config PATH` and `--no-config` override it. The config can only hold style defaults (`fg`, `bg`, `ec`, `margin`, `scale`, `shape`, `format`), never input data.
- Environment variables: `QR_FG`, `QR_BG`, `QR_EC`, `QR_FORMAT`, `QR_CONFIG`.
- Precedence: flags > env > config > built-in defaults. `qr config show` prints the effective values and where each came from.

## Machine output
`--json` prints a result object to stdout:
`{"output":"code.png","format":"png","version":4,"ec":"M","modules":33,"bytes":18,"verified":true}`.
Errors go to stderr as `{"error":{"code":"data_too_long","message":"...","hint":"..."}}`. Batch mode prints one JSON line per item (NDJSON).

## Exit codes
0 ok · 1 runtime/internal · 2 usage (clap) · 3 data too long for QR · 4 I/O (read/write/permission) · 5 decode: no code found · 6 verify failed. These are documented in the man page and README.

## Error style
Lowercase message, the specific values involved, and one actionable hint:
```
error: data too long: 3,214 bytes exceeds 2,953 (max at --ec L)
  hint: shorten the URL or lower --ec
error: cannot infer format from 'out.gif'
  hint: use -f png|svg|jpeg|webp|pdf|eps|txt
```
No stack traces. `-v/--verbose` adds detail and `-q/--quiet` suppresses warnings.

## Colour and TTY
- `--color auto|always|never` applies to diagnostics. NO_COLOR and CLICOLOR_FORCE are honoured.
- The terminal QR render forces black-on-white with ANSI background codes so it scans on dark themes. Under NO_COLOR or `--color never` it falls back to plain half-blocks (use `--invert` if the result looks reversed).
- Width check: if the code is wider than the terminal, warn and suggest `-o file.png`.

## Tooling
- `qr completions bash|zsh|fish|powershell|elvish` (clap_complete)
- `qr man > qr.1`, also pre-generated in release archives
- `--help` shows 3–5 examples per command; `-h` is short help and `--help` is long.
- `--version` includes the commit hash.

## Release scope
- **v1:** everything marked v1 above.
- **v1.x:** clipboard, Sixel/Kitty, presets, frames and captions, crypto/EPC payloads, `--split` (structured append).
- **v2:** Micro QR/rMQR, gradients, custom eyes, `qr serve`? (Unlikely: a server falls outside the CLI's scope and widens the security surface.)
