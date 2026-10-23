#!/bin/zsh
# ============================================================================
# Golden-file harness for --style tree (plan.md §8.1).
#
#   scripts/golden.sh capture   # run legacy tree.zsh -> tests/golden/*.expected
#   scripts/golden.sh check     # run Rust binary, diff against goldens
#   scripts/golden.sh stable    # capture each golden 3x, flag instability
#
# Legacy capture runs under `zsh -f` with zmodload zsh/stat preloaded (the
# §1 autoload landmine) and LC_ALL=C pinned: the plan's sort spec (§4.4) is
# byte order, and zsh's (o) sort uses locale collation otherwise. The Rust
# binary always compares names in byte order.
#
# Corpora must already exist (scripts/make_corpus.sh). The large corpus is
# skipped by capture/stable/check unless INCLUDE_LARGE=1 (slow).
# ============================================================================
set -eu

here=${0:A:h}
root=${here:h}
TREE_ZSH=${TREE_ZSH:-$root/tree.zsh}
# Kept outside the repo at a short path: cargo's build-script fingerprint scan
# walks the whole project dir and dies on the chmod-000 perms corpus, and the
# deep corpus needs headroom under macOS PATH_MAX.
CORP=${CORP:-/tmp/treecorp}
GOLDEN=$root/tests/golden
BIN=${BIN:-$root/target/release/tree}

corpora=(tiny medium deep symlinks unicode hardlinks perms empty)
if [[ -d $CORP/large ]] && (( ${INCLUDE_LARGE:-0} )); then
    corpora+=(large)
fi
depths=(0 1 2 3)
sorts=(size name)

mode="${1:-check}"
[[ -d $GOLDEN ]] || mkdir -p "$GOLDEN"

run_legacy() { # <corpus-dir> <depth> <sort>
    LC_ALL=C zsh -f -c '
        zmodload zsh/stat || exit 90
        source "$1" || exit 91
        cd "$2" || exit 92
        tree "$3" --sort "$4"
    ' -- "$TREE_ZSH" "$1" "$2" "$3"
}

run_rust() { # <corpus-dir> <depth> <sort>
    # --f: the legacy script always listed files; the binary's default is
    # folders-only, so the golden contract runs with --f.
    "$BIN" --sort "$3" --depth "$2" --f --force-colors "$1"
}

case $mode in
capture)
    for c in "${corpora[@]}"; do
        [[ -d $CORP/$c ]] || { echo "missing corpus: $CORP/$c" >&2; exit 1; }
        for d in "${depths[@]}"; do
            for s in "${sorts[@]}"; do
                out="$GOLDEN/$c.$d.$s.expected"
                run_legacy "$CORP/$c" "$d" "$s" > "$out"
                echo "captured $out"
            done
        done
    done
    ;;
stable)
    tmp=$(mktemp -d)
    for c in "${corpora[@]}"; do
        [[ -d $CORP/$c ]] || continue
        for d in "${depths[@]}"; do
            for s in "${sorts[@]}"; do
                for i in 1 2 3; do
                    run_legacy "$CORP/$c" "$d" "$s" > "$tmp/run$i.out"
                done
                if cmp -s "$tmp/run1.out" "$tmp/run2.out" && cmp -s "$tmp/run2.out" "$tmp/run3.out"; then
                    echo "stable   $c.$d.$s"
                else
                    echo "UNSTABLE $c.$d.$s" >&2
                fi
            done
        done
    done
    rm -rf "$tmp"
    ;;
check)
    [[ -x $BIN ]] || { echo "binary not built: $BIN" >&2; exit 1; }
    tmp_actual=$(mktemp)
    fail=0
    for c in "${corpora[@]}"; do
        [[ -d $CORP/$c ]] || { echo "missing corpus: $CORP/$c" >&2; continue; }
        for d in "${depths[@]}"; do
            for s in "${sorts[@]}"; do
                exp="$GOLDEN/$c.$d.$s.expected"
                [[ -f $exp ]] || { echo "no golden: $exp" >&2; fail=1; continue; }
                run_rust "$CORP/$c" "$d" "$s" > "$tmp_actual"
                if ! cmp -s "$exp" "$tmp_actual"; then
                    echo "DIFF     $c.$d.$s"
                    diff "$exp" "$tmp_actual" | head -20
                    fail=1
                fi
            done
        done
    done
    rm -f "$tmp_actual"
    exit $fail
    ;;
*)
    echo "usage: $0 {capture|check|stable}" >&2
    exit 2
    ;;
esac
