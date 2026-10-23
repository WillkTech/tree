#!/bin/zsh
# ============================================================================
# Deterministic corpus generator for the tree golden tests (plan.md §8.2).
#
# Usage: scripts/make_corpus.sh <dest>
#
# Corpora are committed as generators, not as files. Sizes are deterministic
# (fixed seed, multiples of 64 KiB in 0..2 MiB so the round-half-even
# boundary r == 524288 is exercised).
# ============================================================================
set -eu

dest="${1:?usage: make_corpus.sh <dest>}"
mkdir -p "$dest"

# Deterministic pseudo-random sizes: 0..2 MiB in 64 KiB steps.
RANDOM=42
sz() { print $(( ${1:-$((RANDOM))} * 65536 )); }
mk() { # mk <path> <size-index 0..32>
    local p="$1" k="${2:-$RANDOM}"
    if (( k == 0 )); then : > "$p"; else dd if=/dev/zero of="$p" bs=65536 count="$k" status=none; fi
}

# ---------------------------------------------------------------------------
# tiny: 10 dirs x 10 files
# ---------------------------------------------------------------------------
c="$dest/tiny"; mkdir -p "$c"
for d in {"01".."10"}; do
    mkdir -p "$c/d$d"
    for f in {1..10}; do mk "$c/d$d/f$f.bin" $(( (d*7 + f*13) % 33 )); done
    mk "$c/f_top_$d.bin" $(( (d*11) % 33 ))
done

# ---------------------------------------------------------------------------
# medium: 200 dirs x 50 files + 2 subdirs each (the §1 benchmark corpus)
# ---------------------------------------------------------------------------
c="$dest/medium"; mkdir -p "$c"
for d in {1..200}; do
    dd="$c/dir$(printf '%03d' "$d")"; mkdir -p "$dd"
    for f in {1..50}; do mk "$dd/f$(printf '%02d' "$f").bin" $(( (d*17 + f*29) % 33 )); done
    for s in 1 2; do
        sd="$dd/sub$s"; mkdir -p "$sd"
        for f in {1..10}; do mk "$sd/f$(printf '%02d' "$f").bin" $(( (d*23 + f*7 + s*3) % 33 )); done
    done
    mk "$c/topfile_$(printf '%03d' "$d").bin" $(( (d*31) % 33 ))
done

# ---------------------------------------------------------------------------
# large: 2,000 dirs x 50 files (~100k files)
# ---------------------------------------------------------------------------
c="$dest/large"; mkdir -p "$c"
for d in {1..2000}; do
    dd="$c/d$(printf '%04d' "$d")"; mkdir -p "$dd"
    for f in {1..50}; do mk "$dd/f$(printf '%02d' "$f").bin" $(( (d*13 + f*17) % 33 )); done
done

# ---------------------------------------------------------------------------
# deep: single-child chain + 10 files per level. 450 levels, not the plan's
# 500: macOS PATH_MAX (1024) clips the deepest filenames from a /tmp root.
# ---------------------------------------------------------------------------
c="$dest/deep"; mkdir -p "$c"
p="$c"
for l in {1..450}; do
    p="$p/a"; mkdir -p "$p"
    for f in {1..10}; do mk "$p/f$f.bin" $(( (l*7 + f*5) % 33 )); done
done

# ---------------------------------------------------------------------------
# symlinks: 200 symlinks to files, 50 to dirs, 20 dangling
# (+ one nested case: real dir > symlinked dir > files, to pin the
#  same-path sibling-hash quirk for nested symlinked dirs)
# ---------------------------------------------------------------------------
c="$dest/symlinks"; mkdir -p "$c/targets" "$c/dirs" "$c/realdir"
for i in {1..200}; do mk "$c/targets/t$(printf '%03d' "$i").bin" $(( (i*13) % 33 )); ln -s "targets/t$(printf '%03d' "$i").bin" "$c/link_to_file_$(printf '%03d' "$i")"; done
for i in {1..50}; do
    mkdir -p "$c/dirs/d$(printf '%02d' "$i")"
    mk "$c/dirs/d$(printf '%02d' "$i")/inner.bin" $(( (i*9) % 33 ))
    ln -s "dirs/d$(printf '%02d' "$i")" "$c/link_to_dir_$(printf '%02d' "$i")"
done
for i in {1..20}; do ln -s "nowhere_$i" "$c/dangling_$(printf '%02d' "$i")"; done
mkdir -p "$c/realdir"
ln -s ../dirs/d01 "$c/realdir/nested_link"
mk "$c/realdir/afile.bin" 20

# ---------------------------------------------------------------------------
# unicode: names with spaces, CJK, emoji, dots, leading dashes
# ---------------------------------------------------------------------------
c="$dest/unicode"; mkdir -p "$c"
mk "$c/plain.bin" 16
mk "$c/sp ace.bin" 24
mk "$c/a-b.bin" 8
mk "$c/a_b.bin" 32
mk "$c/a b.bin" 12
mk "$c/dot.file.bin" 28
mk "$c/-lead-dash.bin" 20
mk "$c/.hidden.bin" 14
mk "$c/CJK-文件.bin" 18
mk "$c/emoji-😀.bin" 26
mk "$c/Ümlaut.bin" 10
mkdir -p "$c/dir with space" "$c/dir.CJK-目录" "$c/-dir-dash" "$c/.hidden-dir"
mk "$c/dir with space/inner file.bin" 30
mk "$c/dir.CJK-目录/内部.bin" 22
mk "$c/-dir-dash/x.bin" 6
mk "$c/.hidden-dir/.y.bin" 31

# ---------------------------------------------------------------------------
# hardlinks: 100 files, each with 2 links (both links in the same dir so the
# aggregate is deterministic; rows show both links, full size each)
# ---------------------------------------------------------------------------
c="$dest/hardlinks"; mkdir -p "$c"
for i in {1..100}; do
    mk "$c/f$(printf '%03d' "$i").bin" $(( (i*11) % 33 ))
    ln "$c/f$(printf '%03d' "$i").bin" "$c/l$(printf '%03d' "$i").bin"
done

# ---------------------------------------------------------------------------
# perms: one chmod 000 subdir, one empty dir
# ---------------------------------------------------------------------------
c="$dest/perms"; mkdir -p "$c/locked" "$c/empty"
mk "$c/locked/hidden.bin" 16
mk "$c/visible.bin" 16
chmod 000 "$c/locked"

# ---------------------------------------------------------------------------
# empty: dir with no entries (tests the "No entries found." path)
# ---------------------------------------------------------------------------
mkdir -p "$dest/empty"

print -r -- "corpora written to $dest"
