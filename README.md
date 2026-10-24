[![Build Status](https://github.com/willktech/tree/actions/workflows/CICD.yml/badge.svg?branch=master)](https://github.com/willktech/tree/actions)

# tree

A fast, folders-first directory tree with sizes.

`tree` is a Rust fork of [dust](https://github.com/bootandy/dust). It keeps
dust's parallel scanner and reproduces the output of a legacy `tree.zsh`
script (gdu + zsh globbing + zstat) in a single process — byte-for-byte, and
roughly 30× faster. The binary is named `tree` and speaks the script's
argument shape directly; no shell wrapper is involved.

## Highlights

- **Folders only by default**, with recursive sizes: `tree 1` shows the first
  level of folders, `tree 2` levels 1–2, and so on.
- **`--f` adds files**: `tree 1 --f` lists level-1 folders plus files at the
  base level and one level deeper (the legacy script's behavior).
- **Byte-identical** to the legacy script on all golden corpora (72 tests,
  both sort modes, depths 0–3).
- **One binary**: the upstream dust percent-bar output is still available via
  `--style dust`, with every dust flag intact.
- Same scan engine as dust: rayon parallelism, hardlink deduplication,
  filesystem limits, ignore/filter rules, mtime filters.

## Install

From a checkout of this repository:

```bash
cargo install --path . --force --root ~/.local
```

That installs `~/.local/bin/tree`; make sure `~/.local/bin` is on your
`PATH`. Or build and copy manually:

```bash
cargo build --release
install -m 755 target/release/tree ~/.local/bin/tree
```

> If you previously used a zsh function named `tree`, remove it (or its
> `source` line): a shell function shadows the binary on `PATH`.

## Usage

```
tree [DEPTH] [PATH] [--sort size|name] [--f] [-f] [-L] [--style dust]
```

`DEPTH` is the first all-digit positional argument; the first non-digit
positional is the path (default `.`). This is the legacy script's arg shape,
so `tree 2 ~/src` works as expected. The default depth is `0` — the top level.
`-d N` / `--depth N` is equivalent and works in both styles.

| Command | Output |
|---|---|
| `tree` | top-level folders only |
| `tree 1` | first level of folders |
| `tree 2` | folders levels 1–2 |
| `tree 1 --f` | folders ≤ 1, plus files ≤ 2 (base level and inside level-1 folders) |
| `tree -f` | folders with file counts instead of bytes |
| `tree -L` | dereference symlinks: traverse and count them |
| `tree --sort name` | per-component name order |
| `tree --style dust` | upstream dust percent-bar output |

Example — `tree 1 --f`:

```

        10  2 dirs (Mb)
  --------  -----------
         6  󰉋 src/
         4  ├── 󰈔 main.rs
         2  └── 󰈔 lib.rs
         1  󰉋 docs/
         1  └── 󰈔 README.md
         2  󰈔 notes.txt
         1  󰈔 Cargo.toml

2 files (3.0 Mb)
```

The header shows the total size and the number of top-level folders; the
footer shows the number of top-level files and their combined size. The size
column is right-aligned and comma-grouped, so the least-significant digit of
every row (and of the header total) sits in the same column: `222,570`,
`222,164`, `213,372`, `5,821`, `18`. Folder sizes are disk usage (allocated
blocks); file sizes are apparent size. Values below 1 MiB render blank in the
size column (legacy behavior).

### Depth and files

Folders are shown up to the display depth. With `--f`, files and symlinks are
shown one level deeper — the legacy script's rule: directories ≤ N, files and
symlinks ≤ N+1. Thus `tree 1 --f` shows level-1 folders and the files at the
base level *and* inside those folders.

### Folders, files, and links

- `--f` (alias `--files`) lists files and symlinks. Without it, only real
  folders are listed.
- `-f` / `--filecount` (dust's flag) switches the size column to the number of
  files each folder contains; the header total becomes the total count.
- `-L` / `--dereference-links` traverses symlinked folders and counts their
  contents toward totals. Without `-L`, symlinked folders are like Windows
  shortcuts: they contribute 0 to totals and are only listed with `--f`.
- `-s` / `--apparent-size` uses file lengths instead of allocated blocks for
  folder totals.

### Sorting

- `--sort size` (default): folders first, then size descending, name ascending
  on ties.
- `--sort name`: per-component name ascending. Folders sort before files on
  identical basenames.
- Names compare in byte order (the legacy script's `LC_ALL=C` behavior).

### Other tree-mode details

- Files whose relative path starts with `-` are dropped (a `zstat` quirk in the
  legacy script) and excluded from the footer sum.
- With `--f`, a symlinked folder at display depth renders twice: as a folder
  row and as a link row, matching the script.
- Colors follow `NO_COLOR`/TTY detection. `-C`/`--force-colors` forces them and
  `-c`/`--no-colors` disables them; the legacy script always colored.
- Walker options apply in tree mode too: `-x`, `-i`, `-X`, `-I`, `-e`, `-v`,
  `-M`, `-A`, `-y`, `-T`, `--files0-from`, `--files-from`.
- Display-only dust flags (`-n`, `-r`, `-p`, `-b`, `-B`, `-R`, `-D`, `-F`,
  `-t`, `-z`, `-o`, `-j`, `-m`, `-w`, `--collapse`, `--skip-total`) are
  ignored in tree mode. Use `--style dust` for those.

## The dust style

`tree --style dust ...` keeps the upstream dust output unchanged: percent
bars, terminal-height limiting, JSON, and every upstream flag. Run
`tree --help` for the full list.

```
tree --style dust
tree --style dust <dir>
tree --style dust -d 2
tree --style dust -n 30
tree --style dust -D          # directories only
tree --style dust -F          # files only
tree --style dust -b          # no percent bars
tree --style dust -j | jq     # JSON
```

<p>
  <img src="media/demo.gif" alt="dust demo — percent-bar output" width="600">
</p>

## Config file

The config file is unchanged from dust and is still read from
`$XDG_CONFIG_HOME/dust/config.toml` (falling back to
`~/.config/dust/config.toml`) or `~/.dust.toml`:

```
$ cat ~/.config/dust/config.toml
reverse=true
limit-filesystem=true
```

Keys use the long flag name in kebab-case. See
[config/config.toml](config/config.toml) for a fuller sample.

Config keys apply where the equivalent flag applies: walker/color/depth keys
affect tree mode, display-only keys affect `--style dust`. `--style`, `--sort`,
`--f`, and the positional depth are command-line only.

## Performance

Measured on an Apple M5 Max (18 cores), macOS/APFS, end-to-end (scan +
aggregate + sort + render):

- Legacy `tree.zsh`: **~1.65 s** for a 10k-file corpus.
- `tree`: **~60 ms** for the 14.2k-file medium corpus, **~430 ms** for the
  ~100k-file large corpus.

The scan dominates the medium corpus; on the ~100k-file corpus the render pass
is the larger share (~110 ms scan, ~300 ms render).

## Golden tests

The 72 goldens in [tests/golden](tests/golden) are the legacy script's output
(9 corpora × 4 depths × 2 sorts). They are generated on macOS/APFS only
(filesystem allocation differs elsewhere).

```bash
./scripts/make_corpus.sh /tmp/treecorp   # deterministic corpora
./scripts/golden.sh check                # diff the binary against goldens
./scripts/golden.sh capture              # regenerate from tree.zsh
./scripts/golden.sh stable               # check golden determinism
```

The harness runs the binary with `--f` — the legacy script always listed
files, so `--f` is the golden contract. The large (100k-file) corpus is
skipped by default (64 goldens); `INCLUDE_LARGE=1` includes it (all 72,
slow). [tree.zsh](tree.zsh) is kept in the repo as the reference
implementation the goldens are captured from.

## Development

```bash
cargo test                     # unit + integration tests
cargo bench                    # scan+render benchmarks (needs corpora)
cargo fmt --all -- --check
cargo clippy --locked -- -D warnings
```

## Differences from the legacy script

- Colors are TTY/`NO_COLOR`-gated instead of unconditional.
- Permission-denied folders are reported on stderr (the script skipped them
  silently); stdout is unchanged.
- Hardlink deduplication is deterministic (inode order).
- The autoload `zmodload zsh/stat` landmine is gone — there is no zsh left.

## Upstream and license

This is a fork of [bootandy/dust](https://github.com/bootandy/dust); the scan
engine, dust-style renderer, and most flags come from upstream. The `tree`
style and the folders-only default are this fork's additions. Licensed under
Apache-2.0; see [LICENSE](LICENSE).
