# ============================================================================
# Fast directory tree with recursive sizes (Strict Vertically-Aligned GB)
#
# Requires:
#   gdu-go >= 5.36.1
#
# Target:
#   macOS / Apple Silicon / zsh / Homebrew gdu
#
# Usage:
#   tree
#   tree 2
#   tree 3
#   tree --sort size|name
#
# Default sort: size descending, dirs first, name-ascending ties.
#
# External dependency:
#   gdu-go
#
# Everything else uses zsh built-ins / zsh globbing.
# ============================================================================


# ----------------------------------------------------------------------------
# Configuration
# ----------------------------------------------------------------------------

typeset -g TREE_DIR_ICON='󰉋'
typeset -g TREE_FILE_ICON='󰈔'
typeset -g TREE_LINK_ICON='󰌷'

typeset -g TREE_COLOR_DIR=$'\033[1;34m'
typeset -g TREE_COLOR_BRANCH=$'\033[2m'
typeset -g TREE_COLOR_RESET=$'\033[0m'


# ----------------------------------------------------------------------------
# Locate gdu-go
#
# Do not depend on the zsh $commands cache.
# ----------------------------------------------------------------------------

_tree_find_gdu() {

    local candidate

    #
    # PATH lookup.
    #
    candidate="$(command -v gdu-go 2>/dev/null)"

    if [[ -n "$candidate" && -x "$candidate" ]]; then
        print -r -- "$candidate"
        return 0
    fi


    #
    # Homebrew Apple Silicon.
    #
    if [[ -x /opt/homebrew/bin/gdu-go ]]; then
        print -r -- /opt/homebrew/bin/gdu-go
        return 0
    fi


    #
    # Homebrew Intel / Rosetta.
    #
    if [[ -x /usr/local/bin/gdu-go ]]; then
        print -r -- /usr/local/bin/gdu-go
        return 0
    fi


    return 1
}


# ----------------------------------------------------------------------------
# Format byte count
#
# All displayed sizes use MiB (1 MiB = 1,048,576 bytes).
# Suppress sizes below 1 MiB for both files and directories.
# ----------------------------------------------------------------------------
_tree_fmt_size() {
    local b="$1"
    local type="${2:-dir}"

    [[ "$b" == <-> ]] || {
        print -r -- "$b"
        return
    }

    # Hide sizes below 1 MiB for both files and directories.
    # Fixed-width MB/MiB numeric column.
    _tree_fmt_mib _tfs_col "$b"
    print -r -- "$_tfs_col"
}

# ----------------------------------------------------------------------------
# Format the fixed-width MiB column using pure integer arithmetic.
#
# zsh has no unsigned int: typeset -i is signed 64-bit, the narrowest
# integer type available, and plenty for byte counts. Integer math skips
# zsh's slow float arithmetic path.
#
# Rounds half-to-even to match the previous printf '%8.0f' exactly.
#
# Usage: _tree_fmt_mib <dest_var> <bytes>
# (printf -v writes into the caller's variable via dynamic scope;
#  this build has no nameref support: no MULTIFUNCTION.)
# ----------------------------------------------------------------------------
_tree_fmt_mib() {
    local -i b="$2"

    # Hide sizes below 1 MiB.
    if (( b < 1048576 )); then
        printf -v "$1" '%8s' ''
        return
    fi

    local -i q=$(( b / 1048576 )) r=$(( b % 1048576 ))

    # Round half to even, matching libc printf '%.0f'.
    (( r > 524288 || ( r == 524288 && ( q & 1 ) ) )) && (( q += 1 ))

    # %8s pads but never truncates, unlike the (l.8.) flag.
    printf -v "$1" '%8s' "$q"
}

# ----------------------------------------------------------------------------
# Format an integer MiB value with comma thousands separators,
# right-aligned in the fixed 8-char column.
#
# Locale-independent: printf %'d only groups when LC_NUMERIC has a
# thousands-separator char (silently stops under C/POSIX locales).
#
# Writes to <dest_var> via printf -v so per-row callers avoid a $() fork.
# ----------------------------------------------------------------------------
_tree_fmt_comma() {
    local -i n="${2:-0}"
    local sign=''
    (( n < 0 )) && { sign='-'; n=-n; }

    local s="${n}"
    local out=''
    while (( ${#s} > 3 )); do
        out=",${s[-3,-1]}${out}"
        s="${s[1,-4]}"
    done

    local res="${sign}${s}${out}"
    printf -v "$1" '%8s' "$res"
}

# ----------------------------------------------------------------------------
# Format byte count (Human-readable / Adaptive units for footer)
# ----------------------------------------------------------------------------

_tree_fmt_human() {

    local b="$1"

    [[ "$b" == <-> ]] || {
        print -r -- "$b"
        return
    }

    if (( b < 1024 )); then
        printf '%d b' "$b"
    elif (( b < 1048576 )); then
        printf '%.1f Kb' $(( b / 1024.0 ))
    elif (( b < 1073741824 )); then
        printf '%.1f Mb' $(( b / 1048576.0 ))
    else
        printf '%.3f Gb' $(( b / 1073741824.0 ))
    fi
}


# ----------------------------------------------------------------------------
# Get file size
#
# zstat is part of zsh, so no external stat command is required.
#
# -L follows symlinks.
# ----------------------------------------------------------------------------

_tree_file_size() {

    local path="$1"

    if zstat -L +size "$path" 2>/dev/null; then

        [[ "$REPLY" == <-> ]] || return 1

        print -r -- "$REPLY"

        return 0
    fi


    return 1
}


# ----------------------------------------------------------------------------
# _tree_render
# ----------------------------------------------------------------------------

_tree_render() {

    local -i display_depth=${1:-0}
    local sort_mode="${2:-size}"

    # ------------------------------------------------------------------------
    # Find gdu
    # ------------------------------------------------------------------------

    local GDU

    GDU="$(_tree_find_gdu)"

    if [[ -z "$GDU" ]]; then

        printf '%s\n' \
            'tree: gdu-go not found.' \
            'Install with:' \
            '  brew install gdu' \
            >&2

        return 127

    fi


    # ------------------------------------------------------------------------
    # Direct files in current directory
    #
    # zsh glob:
    #   *(DN.)
    #
    # D = include dot files
    # N = nullglob
    # . = regular files
    # ------------------------------------------------------------------------

    local -a direct_files

    direct_files=( *(DN.) )


    local -i nfiles=${#direct_files}
    local -i files_size=0

    local file
    local file_size


    for file in "${direct_files[@]}"; do

        zstat -L -A file_size +size "$file" 2>/dev/null || continue
        [[ -n "$file_size" ]] || continue

        [[ "$file_size" == <-> ]] || continue

        (( files_size += file_size ))

    done


    # ------------------------------------------------------------------------
    # Data structures
    # ------------------------------------------------------------------------

    local -a paths
    local -a sizes
    local -a depths
    local -a types
    local -a formatted_sizes

    local -A row_by_path

    local -i nrows=0
    local -i maxw=8

    local -a parts
    local -a current_parts

    local -A has_later_sibling
    local -A seen_parent

    local line
    local bytes
    local rest
    local path
    local formatted

    local parent
    local ancestor
    local prefix
    local connector
    local icon
    local name

    local -i first=1
    local -i depth
    local -i i
    local -i j

    local total=0

    # For depth 0, we query gdu with depth 1 to get immediate subfolder sizes
    local -i gdu_depth=1
    if (( display_depth > 0 )); then
        gdu_depth=display_depth
    fi


    # ------------------------------------------------------------------------
    # Read recursive directory sizes from gdu
    # ------------------------------------------------------------------------

    while IFS= read -r line; do

        [[ -n "$line" ]] || continue


        #
        # Strip leading whitespace.
        #
        line="${line#"${line%%[![:space:]]*}"}"


        #
        # First field = bytes.
        #
        bytes="${line%%[[:space:]]*}"

        [[ "$bytes" == <-> ]] || continue


        #
        # Everything after byte count = path.
        #
        rest="${line#"$bytes"}"

        rest="${rest#"${rest%%[![:space:]]*}"}"

        [[ -n "$rest" ]] || continue


        #
        # First numeric record = total.
        #
        if (( first )); then

            first=0
            total="$bytes"

            continue

        fi


        path="$rest"


        #
        # Normalize ./foo
        #
        path="${path#./}"


        #
        # Normalize absolute $PWD/foo
        #
        if [[ "$path" == "$PWD/"* ]]; then
            path="${path#"$PWD/"}"
        fi


        #
        # Directory records only.
        #
        [[ -d "$path" ]] || continue


        #
        # Calculate depth.
        #
        parts=( ${(s:/:)path} )

        depth=${#parts}


        if (( display_depth == 0 )); then
            (( depth > 1 )) && continue
        else
            (( depth > display_depth )) && continue
        fi


        #
        # Store directory.
        #
        (( ++nrows ))

        paths[nrows]="$path"
        sizes[nrows]="$bytes"
        depths[nrows]="$depth"
        types[nrows]="dir"
        row_by_path[$path]=$nrows


        _tree_fmt_mib formatted "$bytes"

        formatted_sizes[nrows]="$formatted"


        if (( ${#formatted} > maxw )); then
            maxw=${#formatted}
        fi

    done < <(

        command "$GDU" \
            -cnp \
            --depth "$gdu_depth" \
            --no-prefix \
            .

    )


    # ------------------------------------------------------------------------
    # Add files and symlinks
    # ------------------------------------------------------------------------

    if (( display_depth == 0 )); then

        # Depth 0: only add direct files and direct symlinks in current dir
        local f
        local fsize

        for f in "${direct_files[@]}"; do

            fsize=""
            zstat -L -A fsize +size "$f" 2>/dev/null || continue

            [[ "$fsize" == <-> ]] || continue

            (( ++nrows ))

            paths[nrows]="$f"
            sizes[nrows]="$fsize"
            depths[nrows]=1
            types[nrows]="file"

            _tree_fmt_mib formatted "$fsize"

            formatted_sizes[nrows]="$formatted"

            if (( ${#formatted} > maxw )); then
                maxw=${#formatted}
            fi

        done

        local -a direct_links
        direct_links=( *(DN@) )
        local link

        for link in "${direct_links[@]}"; do

            fsize=""
            zstat -L -A fsize +size "$link" 2>/dev/null

            [[ -n "$fsize" && "$fsize" == <-> ]] || fsize=0

            (( ++nrows ))

            paths[nrows]="$link"
            sizes[nrows]="$fsize"
            depths[nrows]=1
            types[nrows]="link"

            _tree_fmt_mib formatted "$fsize"

            formatted_sizes[nrows]="$formatted"

            if (( ${#formatted} > maxw )); then
                maxw=${#formatted}
            fi

        done

    else

        # Depth >= 1: include files/symlinks up to display_depth + 1
        #
        # Glob per level so subtrees below the cutoff are never stat'd.

        local f
        local rel
        local fsize
        local prefix=""
        local pat
        local -i lvl

        for (( lvl = 1; lvl <= display_depth + 1; lvl++ )); do

            pat="${prefix}*(DN.)"

            for f in ${~pat}; do

                rel="${f#./}"

                fsize=""
                zstat -L -A fsize +size "$f" 2>/dev/null || continue

                [[ "$fsize" == <-> ]] || continue

                (( ++nrows ))

                paths[nrows]="$rel"
                sizes[nrows]="$fsize"
                depths[nrows]=$lvl
                types[nrows]="file"

                _tree_fmt_mib formatted "$fsize"

                formatted_sizes[nrows]="$formatted"

                if (( ${#formatted} > maxw )); then
                    maxw=${#formatted}
                fi

            done

            pat="${prefix}*(DN@)"

            for link in ${~pat}; do

                rel="${link#./}"

                fsize=""
                zstat -L -A fsize +size "$link" 2>/dev/null

                [[ -n "$fsize" && "$fsize" == <-> ]] || fsize=0

                (( ++nrows ))

                paths[nrows]="$rel"
                sizes[nrows]="$fsize"
                depths[nrows]=$lvl
                types[nrows]="link"

                if (( fsize < 1048576 )); then
                    formatted="${(l.8.)}"
                else
                    _tree_fmt_comma formatted "$(( fsize / 1048576 ))"
                fi

                formatted_sizes[nrows]="$formatted"

                if (( ${#formatted} > maxw )); then
                    maxw=${#formatted}
                fi

            done

            prefix+='*/'
        done

    fi


    # ------------------------------------------------------------------------
    # Nothing found
    # ------------------------------------------------------------------------

    if (( nrows == 0 )); then

        local -a direct_dirs
        direct_dirs=( *(DN/) )
        local -i ndirs=${#direct_dirs}

        local file_str dir_hdr total_hdr
        (( nfiles == 1 )) && file_str="1 file" || file_str="$nfiles files"
        (( ndirs == 1 )) && dir_hdr="1 dir" || dir_hdr="$ndirs dirs"
        _tree_fmt_comma total_hdr "$(( total / 1048576 ))"
        total_hdr+='Mb'

        printf '\n  %8s  %s\n' "$total_hdr" "$dir_hdr"
        printf '  %8s  %s\n\n' '--------' '------'

        printf '%s\n\n' 'No entries found.'
        printf '%s (%s)\n' "$file_str" "$(_tree_fmt_human "$files_size")"

        return 0

    fi


    # ------------------------------------------------------------------------
    # Sort keys using native zsh sorting
    #
    # name: per-component name, ascending.
    #
    # size: per-component rank + inverted size + name. Fixed-width
    # numeric fields make one ascending lexical sort yield, at every
    # level: dirs first, then files/links, size descending, name
    # ascending.
    # ------------------------------------------------------------------------

    local -a sort_keys
    local -a sorted_indices

    local idx
    local key
    local sep=$'\x01'
    local -i rank
    local -i inv
    local -i chain_idx
    local -i max64=9223372036854775807
    local padded
    local chain_path


    sort_keys=()


    for (( i = 1; i <= nrows; i++ )); do

        path="${paths[i]}"

        if [[ "$sort_mode" == size ]]; then

            parts=( ${(s:/:)path} )
            key=""
            chain_path=""

            for (( j = 1; j <= ${#parts}; j++ )); do

                chain_path="${chain_path:+$chain_path/}${parts[j]}"

                if (( j == ${#parts} )); then
                    chain_idx=$i
                else
                    chain_idx=${row_by_path[$chain_path]:-0}
                fi

                if (( chain_idx > 0 )) && [[ "${types[chain_idx]}" == dir ]]; then
                    rank=0
                else
                    rank=1
                fi

                if (( chain_idx > 0 )); then
                    inv=$(( max64 - sizes[chain_idx] ))
                else
                    inv=$max64
                fi

                printf -v padded '%019d' "$inv"

                key+="${rank}${padded}${parts[j]}${sep}"

            done

        else

            key="${path//\//"$sep"}"

            if [[ "${types[i]}" == dir ]]; then
                key+="${sep}"
            fi

        fi

        sort_keys+=(
            "${key}"$'\t'"${(l.8.0)i}"
        )

    done


    #
    # zsh native lexical sort.
    #
    sort_keys=( ${(o)sort_keys} )


    sorted_indices=()


    for key in "${sort_keys[@]}"; do

        idx="${key##*$'\t'}"

        #
        # Remove zero-padding.
        #
        idx="${idx##+(0)}"

        [[ -n "$idx" ]] || idx=0

        sorted_indices+=("$idx")

    done


    # ------------------------------------------------------------------------
    # Determine sibling relationships
    # ------------------------------------------------------------------------

    seen_parent=()
    has_later_sibling=()


    for (( i = ${#sorted_indices}; i >= 1; i-- )); do

        idx="${sorted_indices[i]}"

        path="${paths[idx]}"


        if [[ "$path" == */* ]]; then

            parent="${path:h}"


            if [[ ${seen_parent[$parent]} == 1 ]]; then

                has_later_sibling[$path]=1

            else

                has_later_sibling[$path]=0

            fi


            seen_parent[$parent]=1

        else

            has_later_sibling[$path]=0

        fi

    done


    # ------------------------------------------------------------------------
    # Header
    # ------------------------------------------------------------------------

    local -a direct_dirs
    direct_dirs=( *(DN/) )
    local -i ndirs=${#direct_dirs}

    local dir_hdr total_hdr
    if (( ndirs == 1 )); then
        dir_hdr="1 dir"
    else
        dir_hdr="$ndirs dirs"
    fi

    _tree_fmt_comma total_hdr "$(( total / 1048576 ))"
    total_hdr+='Mb'

    if (( ${#total_hdr} > maxw )); then
        maxw=${#total_hdr}
    fi

    local -i dirlen=${#dir_hdr}
    (( dirlen < 4 )) && dirlen=4
    local dir_underline="${(r:dirlen::-:)}"
    local total_underline="${(r:maxw::-:)}"

    printf '\n'

    printf '  %*s  %s\n' \
        "$maxw" \
        "$total_hdr" \
        "$dir_hdr"

    printf '  %*s  %s\n' \
        "$maxw" \
        "$total_underline" \
        "$dir_underline"


    # ------------------------------------------------------------------------
    # Render tree
    # ------------------------------------------------------------------------

    local current_type


    for idx in "${sorted_indices[@]}"; do

        path="${paths[idx]}"
        bytes="${sizes[idx]}"
        depth="${depths[idx]}"
        current_type="${types[idx]}"
        formatted="${formatted_sizes[idx]}"


        # --------------------------------------------------------------------
        # Top-level directory (depth 1)
        # --------------------------------------------------------------------

        if (( depth == 1 )) && [[ "$current_type" == dir ]]; then

            printf '  %*s  %s%s %s/%s\n' \
                "$maxw" \
                "$formatted" \
                "$TREE_COLOR_DIR" \
                "$TREE_DIR_ICON" \
                "$path" \
                "$TREE_COLOR_RESET"

            continue

        fi


        # --------------------------------------------------------------------
        # Top-level file / symlink (depth 1)
        # --------------------------------------------------------------------

        if (( depth == 1 )) && [[ "$current_type" != dir ]]; then

            if [[ "$current_type" == link ]]; then
                icon="$TREE_LINK_ICON"
            else
                icon="$TREE_FILE_ICON"
            fi


            printf '  %*s  %s %s\n' \
                "$maxw" \
                "$formatted" \
                "$icon" \
                "$path"

            continue

        fi


        # --------------------------------------------------------------------
        # Nested entries (depth > 1): show basename with tree guides
        # --------------------------------------------------------------------

        name="${path:t}"
        prefix=""

        current_parts=( ${(s:/:)path} )


        for (( j = 1; j < depth - 1; j++ )); do

            ancestor="${(j:/:)current_parts[1,j]}"


            if (( has_later_sibling[$ancestor] )); then

                prefix+="${TREE_COLOR_BRANCH}│   ${TREE_COLOR_RESET}"

            else

                prefix+='    '

            fi

        done


        # --------------------------------------------------------------------
        # Current connector
        # --------------------------------------------------------------------

        connector="${TREE_COLOR_BRANCH}└── ${TREE_COLOR_RESET}"

        if (( has_later_sibling[$path] )); then
            connector="${TREE_COLOR_BRANCH}├── ${TREE_COLOR_RESET}"
        fi


        # --------------------------------------------------------------------
        # Directory
        # --------------------------------------------------------------------

        if [[ "$current_type" == dir ]]; then

            printf '  %*s  %s%s%s%s %s/%s\n' \
                "$maxw" \
                "$formatted" \
                "$prefix" \
                "$connector" \
                "$TREE_COLOR_DIR" \
                "$TREE_DIR_ICON" \
                "$name" \
                "$TREE_COLOR_RESET"

            continue

        fi


        # --------------------------------------------------------------------
        # File / symlink
        # --------------------------------------------------------------------

        if [[ "$current_type" == link ]]; then
            icon="$TREE_LINK_ICON"
        else
            icon="$TREE_FILE_ICON"
        fi


        printf '  %*s  %s%s%s %s\n' \
            "$maxw" \
            "$formatted" \
            "$prefix" \
            "$connector" \
            "$icon" \
            "$name"

    done


    # ------------------------------------------------------------------------
    # Summary
    # ------------------------------------------------------------------------

    printf '\n'

    local file_str files_size_str

    if (( nfiles == 1 )); then
        file_str="1 file"
    else
        file_str="$nfiles files"
    fi

    files_size_str="$(_tree_fmt_human "$files_size")"

    printf '%s (%s)\n' \
        "$file_str" \
        "$files_size_str"

}


# ----------------------------------------------------------------------------
# tree
# ----------------------------------------------------------------------------

tree_legacy() {
    local target_dir="."
    local -i display_depth=0
    local sort_mode="size"
    local arg

    while (( $# )); do
        case "$1" in
            --sort)
                if (( $# < 2 )); then
                    printf 'tree: --sort requires size or name\n' >&2
                    return 1
                fi
                sort_mode="$2"
                shift 2
                ;;
            --sort=*)
                sort_mode="${1#--sort=}"
                shift
                ;;
            *)
                if [[ "$1" == <-> ]]; then
                    display_depth="$1"
                elif [[ -d "$1" ]]; then
                    target_dir="$1"
                else
                    printf 'tree: invalid argument: %s\n' "$1" >&2
                    printf 'Usage: tree [depth] [directory] [--sort size|name]\n' >&2
                    return 1
                fi
                shift
                ;;
        esac
    done

    case "$sort_mode" in
        size|name) ;;
        *)
            printf 'tree: invalid sort mode: %s (use size or name)\n' "$sort_mode" >&2
            return 1
            ;;
    esac

    if [[ "$target_dir" != "." ]]; then
        (
            builtin cd -q "$target_dir" || exit 1
            _tree_render "$display_depth" "$sort_mode"
        )
    else
        _tree_render "$display_depth" "$sort_mode"
    fi
}


# ----------------------------------------------------------------------------
# tree (Rust backend)
#
# Thin shim preserving the muscle memory: tree [depth] [directory] [--sort
# size|name]. All scanning/aggregation/sorting/rendering happens in one ztree
# process (--style tree). No zsh/stat, no gdu-go, no globbing needed here.
# The legacy pure-zsh implementation lives on as tree_legacy().
# ----------------------------------------------------------------------------

tree() {
    local target_dir="."
    local -i display_depth=0
    local sort_mode="size"

    while (( $# )); do
        case "$1" in
            --sort)
                if (( $# < 2 )); then
                    printf 'tree: --sort requires size or name\n' >&2
                    return 1
                fi
                sort_mode="$2"
                shift 2
                ;;
            --sort=*)
                sort_mode="${1#--sort=}"
                shift
                ;;
            *)
                if [[ "$1" == <-> ]]; then
                    display_depth="$1"
                elif [[ -d "$1" ]]; then
                    target_dir="$1"
                else
                    printf 'tree: invalid argument: %s\n' "$1" >&2
                    printf 'Usage: tree [depth] [directory] [--sort size|name]\n' >&2
                    return 1
                fi
                shift
                ;;
        esac
    done

    case "$sort_mode" in
        size|name) ;;
        *)
            printf 'tree: invalid sort mode: %s (use size or name)\n' "$sort_mode" >&2
            return 1
            ;;
    esac

    # ztree is the muscle-memory name (symlink/alias to this fork's dust);
    # the binary itself is `dust --style tree`.
    local ztree_bin
    ztree_bin="$(command -v ztree 2>/dev/null || command -v dust 2>/dev/null)"
    if [[ -z "$ztree_bin" ]]; then
        printf 'tree: ztree/dust not found (cargo install --path . --force)\n' >&2
        printf 'tree: falling back to tree_legacy\n' >&2
        tree_legacy "$display_depth" "$target_dir" --sort "$sort_mode"
        return
    fi

    "$ztree_bin" --style tree --sort "$sort_mode" --depth "$display_depth" -- "$target_dir"
}
