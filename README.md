# qr-cli

**`qr` makes and reads QR codes from the command line, offline.** It writes
terminal output, PNG, JPEG, WebP, SVG, PDF and EPS files, and it builds ready-made
WiFi, contact, email, SMS, phone, location and calendar codes.

```text
$ qr 'https://example.com'
    █▀▀▀▀▀█ ▀▄█ █▄█ ▄ █▀▀▀▀▀█
    █ ███ █ ▀█▀ ▄█▄ ▀ █ ███ █
    █ ▀▀▀ █ ▄█ █▄ █   █ ▀▀▀ █
    ▀▀▀▀▀▀▀ ▀▄▀ █ █▄█ ▀▀▀▀▀▀▀
    █▄▀▄▄▄▀▀▄▄████▀▄▀▄▄█ ▄▀▄█
    ▀▄ ▀▄█▀▀▄███▄▄ ▀▄▀ ▄▀█▀ ▀
     ▄▄▀█▀▀▀█▄█ █▀ ██ ██   ▄█
    ▀▀▀ ▀▀▀▄▀▄ ▄█▀█▀██▀▄▄█▀ ▀
    ▀▀  ▀ ▀▀▄▀   ██ █▀▀▀█  ▀▄
    █▀▀▀▀▀█ ▀ ▀▀ ▄ ▄█ ▀ █  ▄█
    █ ███ █  █ ▀█▀ ▀▀█▀▀█ ▄█▀
    █ ▀▀▀ █ ▀█ ▄█▀████▄██▀ ▀▀
    ▀▀▀▀▀▀▀ ▀▀▀  ▀▀ ▀ ▀  ▀  ▀

$ qr 'https://example.com/menu' --shape dots --fg navy -o menu.png
$ qr decode menu.png
https://example.com/menu
```

- **Offline.** The binary makes no network connections. It reads only the input
  you give it and writes only to the path you name.
- **Many formats.** Terminal, text and ASCII output, plus PNG, JPEG, WebP, SVG,
  PDF and EPS files.
- **Safe by default.** Output files are written atomically, batch never
  overwrites without `--force`, and decoded data is escaped before it reaches
  your terminal.
- **Scriptable.** `--json` output, documented exit codes, NDJSON batch reports,
  and shell completions and a man page in every release.

> **Status:** pre-1.0 (version `0.1.0`). Until 1.0.0, minor versions may change
> the command line. See [CHANGELOG.md](CHANGELOG.md) for what changed.

## Contents

- [Install](#install)
- [Quick start](#quick-start)
- [Output formats](#output-formats)
- [Styling and size](#styling-and-size)
- [Payloads](#payloads)
- [Decode](#decode)
- [Batch](#batch)
- [JSON output and exit codes](#json-output-and-exit-codes)
- [Shell completions and man page](#shell-completions-and-man-page)
- [Security notes](#security-notes)
- [Performance notes](#performance-notes)
- [Troubleshooting and FAQ](#troubleshooting-and-faq)
- [Not in 0.1.0](#not-in-010)
- [Contributing](#contributing)
- [Licence](#licence)

## Install

Each [GitHub Release](https://github.com/adamh-nourish/qr-cli/releases) has
archives for Linux (x86_64 and aarch64, static musl), macOS (Apple silicon and
Intel) and Windows (x64 and Arm64). Every archive holds the `qr` binary, shell
completions (`completions/`) and the man page (`man/qr.1`).

### macOS and Linux (shell installer)

```sh norun
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/adamh-nourish/qr-cli/releases/latest/download/qr-cli-installer.sh | sh
```

### Windows (PowerShell installer)

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/adamh-nourish/qr-cli/releases/latest/download/qr-cli-installer.ps1 | iex"
```

Both installers check the archive's SHA-256 checksum and put `qr` in
`~/.cargo/bin` (on Windows, `%USERPROFILE%\.cargo\bin`). Two environment
variables change that:

| Variable | Effect |
|---|---|
| `QR_CLI_INSTALL_DIR` | Install under this prefix instead. The binary goes in its `bin/` folder. |
| `QR_CLI_NO_MODIFY_PATH=1` | Do not edit your shell profile to add the folder to `PATH`. |

### Manual download and checksum check

Use this when you want to inspect an archive before running anything. Replace
the target with yours, for example `aarch64-apple-darwin`,
`x86_64-unknown-linux-musl` or `x86_64-pc-windows-msvc`. Each archive has a
`.sha256` file next to it.

macOS and Linux:

```sh norun
TARGET=aarch64-apple-darwin
BASE=https://github.com/adamh-nourish/qr-cli/releases/latest/download
curl -LO "$BASE/qr-cli-$TARGET.tar.xz"
curl -LO "$BASE/qr-cli-$TARGET.tar.xz.sha256"
shasum -a 256 -c "qr-cli-$TARGET.tar.xz.sha256"   # on Linux: sha256sum -c
tar -xJf "qr-cli-$TARGET.tar.xz"
./qr-cli-$TARGET/qr --version
```

Windows (PowerShell), for the `.zip` archive:

```powershell
$t = "x86_64-pc-windows-msvc"
$base = "https://github.com/adamh-nourish/qr-cli/releases/latest/download"
Invoke-WebRequest "$base/qr-cli-$t.zip" -OutFile "qr-cli-$t.zip"
Invoke-WebRequest "$base/qr-cli-$t.zip.sha256" -OutFile "qr-cli-$t.zip.sha256"
# The first field of the .sha256 file must equal this hash.
Get-FileHash "qr-cli-$t.zip" -Algorithm SHA256
Expand-Archive "qr-cli-$t.zip" -DestinationPath .
.\qr.exe --version
```

Move the `qr` binary (`qr.exe` on Windows) somewhere on your `PATH`.

### From source (Cargo)

With Rust 1.85 or later:

```sh norun
cargo install --locked --git https://github.com/adamh-nourish/qr-cli qr-cli
```

The package is named `qr-cli` and installs a binary named `qr`. `--locked`
uses the `Cargo.lock` committed in the repository.

### Check the install

```sh
qr --version
```

### Gatekeeper, SmartScreen and `cargo audit`

The binaries are not code-signed yet. A browser download may trigger a
Gatekeeper warning on macOS or a SmartScreen warning on Windows. The shell and
PowerShell installers and `cargo install` are not affected. Release binaries are
built with [`cargo auditable`](https://github.com/rust-secure-code/cargo-auditable),
so `cargo audit bin qr` can check them against known vulnerabilities.

### The name `qr` is shared

The Python `qrcode` package also installs a command called `qr`. If both are on
your `PATH`, whichever comes first wins. To see which one you will run:

```sh norun
which -a qr
```

Call the one you want by its full path (for example `~/.cargo/bin/qr`) when you
need to be sure.

## Quick start

Show a code in the terminal. This is the default when stdout is a terminal:

```sh
qr 'https://example.com'
```

Write a PNG file. The format comes from the extension:

```sh
qr 'https://example.com' -o code.png
```

Use a payload helper. `qr url` adds `https://` when there is no scheme:

```sh
qr url example.com/menu -o menu.svg
```

Read a code back from an image:

```sh
qr decode code.png
```

Encode standard input. One trailing newline is dropped:

```sh
echo 'Hello, world' | qr -o hello.png
```

### Where the data comes from

The data is the `TEXT` argument, standard input, or a file named with `-i`
(`--input`):

- `TEXT` is a quoted argument, such as `qr 'hello world'`.
- Standard input is used when there is no `TEXT` and stdin is not a terminal.
  `qr - -o out.png` makes the `-` explicit.
- `-i FILE` reads the file's bytes. `-i -` reads standard input.
- `--keep-newline` keeps a trailing newline from standard input or a file,
  where by default one is dropped.

```sh
qr -i urls.txt -o list.png
qr -i urls.txt --keep-newline -o list-kept.png
```

Text that starts with a dash, or that is also a command name such as `wifi`,
needs `--` before it. Options must come before the `--`:

```sh
qr -o dash.png -- '-start'
qr -- wifi
```

## Output formats

Pick the format with `-f` (`--format`). Without it, `qr` uses the extension of
the `-o` (`--output`) path. Without either, it writes the terminal format when
stdout is a terminal, and text otherwise.

| `-f` | Extension | What it is | Use it for |
|---|---|---|---|
| `terminal` | (none) | Unicode half blocks, black on white with ANSI colour | Showing a code in a terminal |
| `text` | `.txt` | Unicode half blocks, no escape sequences | Pasting into documents, or a terminal without colour |
| `ascii` | (none) | `##` and spaces, pure ASCII | Logs, mail and terminals without Unicode |
| `png` | `.png` | Lossless image (1-bit, or RGBA when transparent) | Screens, slides and most general use |
| `svg` | `.svg` | Scalable vector image | Web pages and anything that must scale |
| `jpeg` | `.jpg`, `.jpeg` | Lossy image, quality 92, at least 4 pixels per module | Only when a JPEG is required. PNG is safer for codes |
| `webp` | `.webp` | Lossless WebP image | Smaller lossless files for the web |
| `pdf` | `.pdf` | One-page vector PDF | Print |
| `eps` | `.eps` | Encapsulated PostScript, vector | Print workflows that need EPS |

Some examples:

```sh
qr 'hello' -f ascii
qr 'hello' -o hello.txt
qr 'hello' -o hello.jpg
qr 'hello' -o hello.webp
qr 'hello' -f pdf -o hello.pdf
qr 'hello' -o hello.eps
qr 'hello' -f svg -o - > stdout.svg
```

`-o -` writes to standard output. Without `-f`, the format there is text. Binary
formats need `-f`, and `qr` refuses to write binary output to a terminal.

### Data URIs

`--data-uri` prints the image as a `data:` URI instead of raw bytes. The default
format is SVG; `-f` chooses another. With `-o`, the file holds the URI text, so
give it a `.txt` name.

```sh
qr 'https://example.com' --data-uri
qr 'https://example.com' --data-uri -f png -o uri.txt
```

## Styling and size

### Colours

`--fg` colours the dark modules and `--bg` the light modules and the quiet zone.
A colour is `#RGB`, `#RRGGBB`, `#RRGGBBAA` or a CSS colour name such as `navy`.
The defaults are `black` and `white`.

```sh
qr 'https://example.com' --fg navy --bg '#fffbe6' -o colour.png
qr 'https://example.com' --fg '#1a2b3c80' -o translucent.png
```

- `--transparent` leaves the background unpainted (image formats only).
- `--invert` swaps dark and light modules (or `--fg` and `--bg` for images).
- `qr` warns when the contrast is below 3:1, and when the modules are lighter
  than the background.

```sh
qr 'https://example.com' --transparent -o transparent.png
qr 'https://example.com' --invert -o inverted.png
```

### Module shapes

`--shape` draws the dark modules as `square` (the default), `rounded` or `dots`.
The three finder patterns (the large corner squares) stay square, so scanners
still find the code.

```sh
qr 'https://example.com' --shape rounded -o rounded.png
qr 'https://example.com' --shape dots --fg navy -o dots.png
qr 'https://example.com' --shape dots --fg navy -o dots.svg
```

Shapes apply to `png`, `jpeg`, `webp`, `svg`, `pdf` and `eps`. Text output does
not support them: `qr` warns and ignores `--shape`.

### Logo

`--logo FILE` draws a PNG, JPEG or WebP image over the centre of the code. It
works with `png`, `jpeg`, `webp` and `svg` output.

```sh
qr 'https://example.com' --logo logo.png -o with-logo.png
qr 'https://example.com' --logo logo.png --logo-scale 0.25 -o big-logo.svg
```

- The logo file is at most 10 MiB and 8192 × 8192 pixels.
- `--logo-scale` is the logo's width as a fraction of the code's width, from
  `0.05` to `0.3` (default `0.2`).
- A logo forces error correction level H, which lets the code survive the
  covered modules. A lower `--ec` is replaced, with a warning.
- A logo on text, PDF or EPS output is a usage error (exit 2). SVG logo files
  are not supported.
- The result is read back automatically (see [Verification](#verification)).

### Size, scale and resolution

| Option | Meaning | Default |
|---|---|---|
| `--scale N` | Pixels per module (PNG, JPEG, WebP, SVG), or points (PDF, EPS). 1 to 16384. | `10` |
| `--size PX` | Width and height in pixels for PNG, JPEG and WebP: the largest whole module size that fits. Vector formats stay within this size. 1 to 16384. | none |
| `--dpi N` | Pixel density recorded in a PNG or JPEG file. 1 to 65535. | none |
| `--margin N` | Quiet zone in modules. Below 4 may not scan, and `qr` warns. | `4` |

```sh
qr 'https://example.com' --scale 8 -o scale8.png
qr 'https://example.com' --size 512 -o size512.png
qr 'https://example.com' --size 1200 --dpi 300 -o print.png
qr 'https://example.com' -f pdf --scale 4 -o scale4.pdf
```

`--size` sets the overall image size. `qr` picks the largest module size that
keeps the whole code, quiet zone included, inside that size.

### Error correction, version, mode and mask

| Option | Meaning | Default |
|---|---|---|
| `--ec L\|M\|Q\|H` | Error correction. Roughly, L recovers about 7% damage, M about 15%, Q about 25% and H about 30%. Higher levels hold less data. | `M` (`H` with `--logo`) |
| `--qr-version N` | Smallest symbol version to use, 1 to 40. Larger data still grows it. | smallest that fits |
| `--mode MODE` | `auto`, `numeric`, `alnum` or `byte`. | `auto` |
| `--mask N` | Force mask pattern 0 to 7. | chosen automatically |

```sh
qr 'https://example.com' --ec H -o ec-h.png
qr 1234567890 --mode numeric -o numeric.png
qr 'https://example.com' --qr-version 5 --mask 3 -o v5.png
```

Byte-mode capacity at the largest version (40) is:

| `--ec` | Bytes at version 40 |
|---|---|
| L | 2,953 |
| M | 2,331 |
| Q | 1,663 |
| H | 1,273 |

When the data does not fit, `qr` reports its length, the maximum at the chosen
level, and a hint such as using a lower level. Input from the command line or
standard input is read up to 7,089 bytes; a longer input is refused with exit
code 3.

### Verification

`--verify` renders the code, reads it back with the decoder that `qr decode`
uses, and fails with exit code 6 unless the code holds the input. Text output is
checked as a plain grid of modules.

```sh
qr 'https://example.com' --verify -o checked.png
```

Verification runs automatically for images with a `--logo`, a non-default
`--shape`, `--fg` or `--bg`, `--transparent`, or `--invert`. Use `--no-verify` to
skip it. That is faster, and it suits large batches of styled codes whose
output you have already checked once.

### Colour in the terminal

`--color auto|always|never` controls ANSI colour. In `auto` mode (the default),
`qr` colours output only when writing to a terminal. The `NO_COLOR` environment
variable turns colour off, and `CLICOLOR_FORCE` turns it on.

```sh
qr 'https://example.com' --color never
NO_COLOR=1 qr 'https://example.com'
```

### Output files and overwrites

- `-o PATH` replaces an existing file atomically, so a failed write leaves the
  old file intact. The file keeps its permissions. A symlink is replaced, not
  followed.
- `--no-clobber` fails (exit 4) instead of replacing an existing file.

```sh
qr 'https://example.com' -o code.png
qr 'https://example.com' -o code.png --no-clobber || echo "exit code $?"
```

### Messages

- `-q`/`--quiet` prints errors only, with no warnings.
- `-v`/`--verbose` also prints what was encoded: the data length, version,
  error-correction level, mode and mask.

```sh
qr 'hello' -v -o verbose.png
```

## Payloads

Each payload subcommand builds the text that phones and scanners understand,
then renders it with the same options as the default command (every
[styling and size](#styling-and-size) option applies). Add `--print-payload` to
print that text instead of a code.

```sh
qr url example.com/menu --print-payload
```

### `qr url`

Encodes a web link. `https://` is added when there is no scheme. Other schemes,
such as `spotify:track:...`, are kept as given. `javascript:`, `vbscript:` and
`data:` links are refused.

```sh
qr url example.com/menu -o menu-url.png
qr url http://localhost:8080 -o dev.png
qr url 'https://example.com/?q=a%20b' --print-payload
```

`localhost:8080` has no scheme, so `qr` reads the colon before a digit as a port
and adds `https://`. Write `http://localhost:8080` for a local plain-HTTP link.

### `qr wifi`

Encodes WiFi credentials that phones can join with one tap.

| Option | Meaning |
|---|---|
| `--ssid NAME` | The network name. Required, at most 32 bytes. |
| `--password-stdin` | Read the password from standard input. One trailing newline is dropped. |
| `--password PASSWORD` | The password on the command line. It is visible in shell history and the process list, so prefer `--password-stdin`. |
| `--auth wpa\|wep\|nopass` | `wpa` (WPA, WPA2 and WPA3, the default), `wep`, or `nopass` for an open network. |
| `--hidden` | The network does not broadcast its name. |

```sh
printf 'correct horse 42' | qr wifi --ssid Home --password-stdin -o wifi.png
qr wifi --ssid Home --password-stdin --print-payload < pw.txt
qr wifi --ssid 'Cafe Guest' --auth nopass -o cafe.png
qr wifi --ssid Lab --auth wep --password-stdin --hidden < key.txt -o lab.png
```

`--password-stdin` keeps the password out of shell history. A WPA password has 8
to 63 characters, or 64 hexadecimal digits. A WEP key has 5 or 13 characters, or
10 or 26 hexadecimal digits. `--print-payload` prints the password, because the
code holds it.

### `qr vcard`

Encodes a contact card (vCard 3.0). The name is the only argument. The other
fields can be given as options, and `--phone`, `--email`, `--url` and `--address`
can be repeated. The options are `--note`, `--org` and `--title`.

```sh
qr vcard 'Ada Lovelace' --phone '+44 20 7946 0000' --email ada@example.com -o ada.png
qr vcard 'Ada Lovelace' --org 'Analytical Engines' --title Engineer \
  --url example.com -o ada-work.svg
```

### `qr mecard`

Encodes a compact contact card (MeCard). It has fewer fields than a vCard, so
the code is smaller. Options: `--phone`, `--email`, `--url` and `--address`
(all repeatable), and `--note`.

```sh
qr mecard 'Ada Lovelace' --phone '+44 20 7946 0000'
qr mecard Ada --email ada@example.com --url example.com -o ada-mecard.svg
```

### `qr email`

Encodes an email draft as a `mailto:` link. The recipient is the argument.
Options: `--subject`, `--body`, and `--cc` and `--bcc` (repeatable).

```sh
qr email ada@example.com
qr email ada@example.com --subject 'Hello' --body 'See you at 3' -o mail.png
qr email ada@example.com --cc bob@example.com -o mail-cc.png
```

### `qr sms`

Encodes a text message draft. The message is optional: it can be the second
argument or `--body`. Phone numbers are at most 24 bytes; messages are at most
1,000 bytes.

```sh
qr sms '+44 7700 900123' -o sms-number.png
qr sms '+44 7700 900123' 'On my way' -o sms.png
qr sms '+44 7700 900123' --body 'On my way' -o sms-body.png
```

### `qr tel`

Encodes a phone number to call (a `tel:` link).

```sh
qr tel '+44 20 7946 0000'
qr tel 911 -o call.png
```

### `qr geo`

Encodes a location (a `geo:` link). Give the latitude first, then the longitude,
in decimal degrees. Latitude runs from `-90` to `90` and longitude from `-180`
to `180`.

```sh
qr geo 51.5007 -0.1246
qr geo -33.8568 151.2153 -o opera-house.png
```

### `qr event`

Encodes a calendar event (iCalendar). `--start` is required. It accepts:

| Form | Meaning |
|---|---|
| `2026-11-20` | An all-day event |
| `2026-11-20T15:30` | Local time, wherever the phone is |
| `2026-11-20T15:30:00Z`, `2026-11-20T15:30:00+01:00` | A fixed time, converted to UTC |

`--end` takes the same forms. An all-day end date is exclusive, so
`--start 2026-11-20 --end 2026-11-22` covers two days. `--location` gives the
place, and `--description` a longer note.

```sh
qr event 'Team day' --start 2026-11-20 -o team-day.png
qr event 'Offsite' --start 2026-11-20 --end 2026-11-22 -o offsite.png
qr event Launch --start 2026-11-20T15:30 --end 2026-11-20T17:00 -o launch.png
qr event Call --start 2026-11-20T15:30:00Z --location 'Room 4' -o call-event.png
qr event Review --start 2026-11-21T10:00 --description 'Bring the draft' --print-payload
```

### Checking a payload

Every payload subcommand accepts `--print-payload`. Piping its output into `qr`
encodes exactly the same data:

```sh
qr url example.com/menu --print-payload | qr -o piped.png
```

## Decode

`qr decode` reads every QR code in PNG, JPEG or WebP images and prints the data
of each code, top to bottom and then left to right.

```sh
qr decode code.png
qr decode code.png dots.png
qr decode - < code.png
```

- The format comes from the file's contents, not its name.
- Each image is read up to 10 MiB and 8192 × 8192 pixels.
- Output escapes control and bidirectional characters (for example `\u{1b}`)
  and bytes that are not UTF-8 (for example `\x{ff}`). A code therefore cannot
  send escape sequences to your terminal.
- With several images, each line starts with the image's path.
- `--raw` prints each code's data unescaped, one code per line, with no path
  prefix. It is refused when stdout is a terminal.
- `--json` prints one object with the codes of every image. See
  [JSON output and exit codes](#json-output-and-exit-codes).

```sh
qr decode code.png --raw > data.txt
qr decode code.png dots.png --json
```

`qr decode` reads PNG, JPEG and WebP only. To read back SVG or PDF output,
render a PNG of the same code.

## Batch

`qr batch` writes one code per CSV row, or per line of a text file, into a
directory. Rows are rendered in parallel and reported in row order.

```sh
qr batch urls.csv --out-dir codes
qr batch urls.txt --lines --out-dir lines --name 'url-{n:04}' -f svg
qr batch rows.csv --out-dir named --name '{name}-{n:04}'
```

### Input

- **CSV (the default).** If one cell of the first row is `data`, that row is a
  header. The `data` column is encoded, and a `filename` column names the
  files. Every other column whose name uses only letters, digits and `_` is a
  template field for `--name`. Without a header, the first column is the data
  and the second is the file name. Use `--no-header` when the first row is data
  even though a cell says `data`.
- **`--lines`.** Every non-blank line is one code.
- A UTF-8 byte-order mark is ignored.
- Limits: 50 MiB of input, 100,000 rows, and 1,000 cells per row. An input with
  no rows is a usage error (exit 2).
- `-` reads the input from standard input.

### File names

`--name` is a template for the file name, without the extension. The default is
`{filename}` when the CSV has a `filename` column, and the row number otherwise.

| Field | Value |
|---|---|
| `{n}` | The row number. `{n:04}` pads it to four digits. |
| `{hash}` | 16 hex digits of the data (FNV-1a 64). |
| `{data}` | The data itself. Avoid it for secrets. |
| `{filename}` | The `filename` column. |
| `{column}` | Any other header column, by name. |

`qr` removes path separators, `:`, control characters, and leading `.` or `-`
from the field values. It refuses Windows device names such as `CON` and `NUL`.
Names are cut at 200 bytes and must be unique, ignoring case and Unicode
normalisation. Each file gets the format's extension and stays directly inside
`--out-dir`.

### Options

| Option | Meaning |
|---|---|
| `--out-dir DIR` | Where the files go. Created if missing. Required. |
| `-f FORMAT` | Output format, default `png`. `text` and `ascii` write `.txt`. The terminal format is not available. |
| `--jobs N` | Worker threads, 1 to 256. Default: the number of CPUs. |
| `--fail-fast` | Stop at the first failed row. Rows already started finish. |
| `--force` | Replace files that already exist in `--out-dir`. |
| `--no-verify` | Skip the automatic read-back of styled codes. |
| `--json` | One JSON object per row, then a summary line (NDJSON). |

Every style and size option of the default command applies to each file.

### Safety rules

- Existing files are never replaced without `--force`. A row whose file exists
  fails, and the other rows still run.
- A failed row is reported as `row N (line L): ...`, and the other rows still
  run unless `--fail-fast` is set. The exit status is the status of the first
  failed row.
- Each file is written atomically. Files are not flushed to disk one by one, so
  after a power loss some recently written files may be missing. Rerun with
  `--force`, or into a new directory, to fill the gaps.
- Names are sanitised before any file is written. If two rows would produce the
  same name after normalisation, the later row fails.
- Do not put secrets such as WiFi passwords or URL tokens in `{data}` or a
  `{column}` field. File names are visible to anyone who can list the folder.

### Batch reports

`--json` prints one NDJSON line per row, then a summary line. A success line
looks like this:

```sh
qr batch rows.csv --out-dir ndjson-out --json > report.ndjson
```

```json
{"row":1,"line":2,"output":"ndjson-out/1.png","format":"png","version":2,"ec":"M","mode":"byte","mask":1,"modules":25,"bytes":21,"written":428}
{"summary":{"rows":2,"written":2,"failed":0,"skipped":0}}
```

A failed row has the same `row` and `line` keys, with an `error` object that
holds the same `code`, `message` and `hint` keys as the errors below.

## JSON output and exit codes

### `--json` on the default command and the payloads

`--json` prints one JSON object on standard output describing the code it
wrote. It needs a destination, so use `-o` or `--data-uri`: the JSON and the
text output cannot share standard output.

```sh
qr 'hello' -o hello-json.png --json
qr 'hello' --data-uri --json
```

The object has these keys:

| Key | Meaning |
|---|---|
| `output` | The file name, or `null` when there is no `-o` (with `--data-uri`). |
| `format` | The output format, such as `png`. |
| `version`, `ec`, `mode`, `mask`, `modules` | The symbol's version, error-correction level, data mode, mask and side length in modules. |
| `bytes` | The length of the encoded data. |
| `written` | The size of the output in bytes (the file, or the data URI). |
| `data_uri` | Present only with `--data-uri`. |

### Errors

With `--json`, errors go to standard error as one object:

```json
{"error":{"code":"output_exists","message":"output file 'code.svg' already exists","hint":"remove it, choose another path or drop --no-clobber"}}
```

Each error has a `code`, a `message` and one `hint`. The codes you are most
likely to see are `usage`, `data_too_long`, `output_exists`, `read_failed`,
`write_failed`, `invalid_logo`, `verification_failed`, `no_code_found`,
`too_many_finders` and `batch_incomplete`.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | OK |
| 1 | Internal error |
| 2 | Usage error: a bad option, value or argument |
| 3 | Data too long for the chosen level |
| 4 | I/O error: a file could not be read or written, or it exists and `--no-clobber` is set |
| 5 | No code found (`qr decode`) |
| 6 | Verification failed: the rendered code does not decode to the input |

`qr decode` exits with 0 when every image held a code. It exits with 4 when an
image could not be read or is not an acceptable image (4 wins over 5), and with
5 when an image held no readable code. `qr batch` exits with the status of its
first failed row.

```sh
qr 'hello' -o code.png --no-clobber || echo "exit code $?"
```

## Shell completions and man page

`qr completions SHELL` prints a completion script for `bash`, `zsh`, `fish`,
`powershell` or `elvish`. `qr man` prints the man page in roff format.

```sh
qr completions bash > qr.bash
qr completions zsh > _qr
qr man > qr.1
```

The release archives already contain these files, in `completions/` and
`man/`. Install them where your shell and `man` look. For example:

```sh norun
# zsh: any folder on your fpath; this one is a common choice
qr completions zsh > ~/.zfunc/_qr
# bash, with the bash-completion package
qr completions bash > ~/.local/share/bash-completion/completions/qr
# man page, system-wide
sudo install -m 644 qr.1 /usr/local/share/man/man1/qr.1
```

Open a new shell after installing completions, and run `man qr` to read the
man page.

## Security notes

- **No network access.** `qr` makes no network connections. It never fetches
  an image or a logo by URL.
- **Input limits.** Text input is read up to 7,089 bytes, and a longer input is
  refused. Capacity is checked exactly against the encoder: the largest
  byte-mode payload is 2,953 bytes at level L. `--scale`, `--size`, `--dpi`,
  `--logo-scale` and `--jobs` are range-checked.
- **Image limits.** Logos and decoded images are at most 10 MiB and 8192 × 8192
  pixels. The header is checked before decoding, and decoders may allocate at
  most 128 MiB. `qr decode` refuses an image with more than 64 finder-like
  patterns, so a crafted image cannot make the search run for minutes.
- **Render limits.** Output is at most 16,384 pixels per side.
- **Terminal safety.** Decoded data and echoed input are escaped before they
  reach your terminal. `--raw` output is refused on a terminal.
- **Output files.** Written atomically to a temporary file in the same folder,
  then renamed over the target. A symlink at the target is replaced rather than
  followed.
- **Batch names.** Path separators and other unsafe characters are removed from
  names. Every file stays inside `--out-dir`, and existing files are not
  replaced without `--force`.
- **Secrets on the command line.** A password given with `--password` appears in
  shell history and in the process list. Use `--password-stdin` or a file.
- **Printed payloads.** `--print-payload` prints the exact content of a code,
  including a WiFi password.
- **Dependencies.** `cargo deny` checks licences, RustSec advisories and sources
  on every pull request. Network and OpenSSL crates are banned.

To report a vulnerability, follow [SECURITY.md](SECURITY.md). Please do not open
a public issue.

## Performance notes

- **One-shot commands** start no threads. Only `qr batch` starts a worker pool.
- **Batch throughput.** On the development Mac, 10,000 short URLs written as
  256-pixel PNG files with `--jobs 4` took about 2.3 seconds (about 4,400 codes
  per second), with a peak resident size of about 9 MB. The design target is
  2,500 codes per second on a four-core runner. Throughput depends on your
  hardware, the payload length and the output size.
- **Verification costs memory and time.** In the design measurements, reading
  back a version 40 PNG raised its peak memory from about 3.4 MiB to about
  10 MiB. For large runs of styled codes, `--no-verify` skips the read-back.
- **WebP is the most memory-hungry format on large codes.** The encoder alone
  needed about 80 MiB at `--size 4000` in the design measurements. Prefer PNG
  for big images.
- **Vector output** (`svg`, `pdf`, `eps`) does not rasterise the code, so it is
  the lightest choice for a single large code.
- **Size.** `--size` sets the overall image size directly, and `--scale` sets a
  fixed module size. Memory for PNG and WebP grows with the pixel count.

## Troubleshooting and FAQ

**`data too long`.** The text does not fit at the chosen level and version.
Shorten it, lower `--ec` (for example from `H` to `M`), or see the capacity table
in [Error correction, version, mode and mask](#error-correction-version-mode-and-mask).

**`no QR code could be read back from the rendered png` (exit 6).** The colours,
or a logo, make the code unreadable. Use a darker `--fg`, a lighter `--bg` or a
smaller `--logo-scale`. `--no-verify` writes the file anyway, but a code that
fails the read-back may not scan.

**The terminal code looks wrong or is hard to read.** Use `-f ascii` if your
terminal does not draw the Unicode block characters well, or `-f text` for the
Unicode version without colour. Keep `--margin` at 4 or more: the quiet zone is
part of the scan.

**`cannot infer the output format`.** The extension is not one of `.png`,
`.svg`, `.jpg`, `.jpeg`, `.webp`, `.pdf`, `.eps` or `.txt`. Pass `-f FORMAT`.

**`output file … already exists`.** `--no-clobber` is set, or a file of that name
already exists in a batch directory. Remove the old file, choose another name,
or use `--force` with `qr batch`.

**`qr decode` cannot read my SVG or PDF.** Decoding handles PNG, JPEG and WebP
only. Render a PNG of the code and decode that.

**`--raw` is refused.** Standard output is a terminal, where the data could send
escape sequences. Redirect the output to a file or a pipe.

**`--logo` is refused.** Logos work with PNG, JPEG, WebP and SVG output. SVG logo
files are not supported, and the logo file itself must be PNG, JPEG or WebP.

**Both the Python `qrcode` tool and `qr-cli` are installed.** Both provide a
`qr` command. See [The name `qr` is shared](#the-name-qr-is-shared).

## Not in 0.1.0

These are deliberately left out of this release. The design record
([`docs/design/DECISIONS.md`](docs/design/DECISIONS.md)) lists them as deferred
or cut:

- Configuration files and `QR_*` environment variables. The exceptions are
  `NO_COLOR` and `CLICOLOR_FORCE`.
- Clipboard output, Sixel and Kitty images, and logos on PDF or EPS output.
- SVG logos, colour presets, frames, captions, gradients and custom eyes.
- Structured append, Micro QR and rMQR, EPC/SEPA and crypto payloads, and
  `--optimize`.
- Fetching inputs or logos by URL, server, GUI or webcam modes, dynamic or
  short-link codes, and an npm wrapper.
- Homebrew, Scoop, winget and crates.io publishing, and code-signed binaries.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the workspace layout, the local
quality gates and the pull request rules. The design decisions are in
[`docs/design/DECISIONS.md`](docs/design/DECISIONS.md); read the section that
covers your change before you start.

## Licence

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.

JPEG output uses the `jpeg-encoder` crate, which is based in part on the work of
the Independent JPEG Group.
