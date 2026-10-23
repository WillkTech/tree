// Tree style renderer: byte-for-byte port of ~/.config/zsh/functions/tree.zsh
// (plan.md §4). Consumes the Node tree produced by the rayon walk and emits
// the script's exact layout: flat rows, composite sort keys, reverse sibling
// pass, header/footer. All quirks the script has are preserved as-is (D3/D5).

use std::io::Write;

use rustc_hash::FxHashMap;

use crate::cli::SortMode;
use crate::node::{FileType, Node};

const MIB: u64 = 1_048_576;
const HALF: u64 = 524_288;
const GIB: u64 = 1_073_741_824;
// The script's max64 (i64::MAX) used for inverted sort sizes.
const MAX64: u64 = 9_223_372_036_854_775_807;
const SEP: u8 = 0x01;

// Nerd-font icons, uncolored (bytes verified against tree.zsh).
const DIR_ICON: &[u8] = "\u{F024B}".as_bytes();
const FILE_ICON: &[u8] = "\u{F0214}".as_bytes();
const LINK_ICON: &[u8] = "\u{F0337}".as_bytes();

const COLOR_DIR: &[u8] = b"\x1b[1;34m";
const COLOR_BRANCH: &[u8] = b"\x1b[2m";
const COLOR_RESET: &[u8] = b"\x1b[0m";

pub struct TreeOpts {
    pub display_depth: usize,
    pub sort: SortMode,
    pub colors: bool,
    // --f: also list files/symlinks (folders-only default). The old script's
    // behavior == show_files=true.
    pub show_files: bool,
    // -f: size column shows file counts instead of bytes.
    pub by_filecount: bool,
    // -L: symlinks were dereferenced by the walker; symlinked dirs become
    // normal traversed dirs (single row, real size).
    pub follow_links: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowType {
    Dir,
    File,
    Link,
}

struct Row {
    components: Vec<Vec<u8>>,
    rtype: RowType,
    // Bytes used for the sort key and the formatter (mixed semantics per D2).
    size: u64,
    formatted: Vec<u8>,
    key: Vec<u8>,
    idx: usize,
}

// ---------------------------------------------------------------------------
// Formatters (ports of _tree_fmt_mib / _tree_fmt_comma / _tree_fmt_human)
// ---------------------------------------------------------------------------

/// Fixed 8-char column, blank below 1 MiB, integer MiB, round half to even.
fn fmt_mib_half_even(bytes: u64) -> Vec<u8> {
    if bytes < MIB {
        return b"        ".to_vec();
    }
    let mut q = bytes / MIB;
    let r = bytes % MIB;
    if r > HALF || (r == HALF && (q & 1) == 1) {
        q += 1;
    }
    format!("{q:>8}").into_bytes()
}

/// Integer with comma thousands separators, right-aligned in 8 chars.
fn fmt_comma(n: u64) -> Vec<u8> {
    let s = n.to_string();
    let mut groups: Vec<&str> = Vec::new();
    let mut end = s.len();
    while end > 3 {
        groups.push(&s[end - 3..end]);
        end -= 3;
    }
    groups.push(&s[..end]);
    groups.reverse();
    format!("{:>8}", groups.join(",")).into_bytes()
}

/// Adaptive human units for the footer (b/Kb/Mb/Gb, {:.1}/{:.3}).
fn fmt_human(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} b")
    } else if bytes < MIB {
        format!("{:.1} Kb", bytes as f64 / 1024.0)
    } else if bytes < GIB {
        format!("{:.1} Mb", bytes as f64 / MIB as f64)
    } else {
        format!("{:.3} Gb", bytes as f64 / GIB as f64)
    }
}

// ---------------------------------------------------------------------------
// Row collection
// ---------------------------------------------------------------------------

/// The script's `[[ -d "$path" ]]` test: follows symlinks.
fn resolves_to_dir(node: &Node) -> bool {
    std::fs::metadata(&node.name).is_ok_and(|m| m.is_dir())
}

fn components(node: &Node, root: &Node) -> Vec<Vec<u8>> {
    // Byte-level strip: node names are root/child paths from the walker.
    let name = node.name.as_os_str().as_encoded_bytes();
    let prefix = root.name.as_os_str().as_encoded_bytes();
    let rel = if name.len() > prefix.len()
        && name[prefix.len()] == b'/'
        && &name[..prefix.len()] == prefix
    {
        &name[prefix.len() + 1..]
    } else {
        name
    };
    rel.split(|b| *b == b'/').map(|c| c.to_vec()).collect()
}

fn formatter(bytes: u64, rtype: RowType, opts: &TreeOpts) -> Vec<u8> {
    if opts.by_filecount {
        // -f: the column shows counts (comma-grouped), no blank rule.
        return fmt_comma(bytes);
    }
    match rtype {
        // The depth-dependent symlink formatter quirk (D5): at depth 0 links
        // use the MiB formatter; at depth >= 1, comma-grouped truncating MiB.
        RowType::Link if opts.display_depth > 0 => {
            if bytes < MIB {
                b"        ".to_vec()
            } else {
                fmt_comma(bytes / MIB)
            }
        }
        _ => fmt_mib_half_even(bytes),
    }
}

fn make_row(components: Vec<Vec<u8>>, rtype: RowType, size: u64, opts: &TreeOpts) -> Row {
    let formatted = formatter(size, rtype, opts);
    Row {
        components,
        rtype,
        size,
        formatted,
        key: Vec::new(),
        idx: 0,
    }
}

/// gdu pass: directory rows, descending through real dirs (and symlinked dirs
/// when -L dereferenced them). Without -L, a symlinked dir is a size-0 leaf
/// row (only with --f); with -L it is a normal traversed dir (single row).
fn collect_dir_rows(root: &Node, n_eff: usize, rows: &mut Vec<Row>, opts: &TreeOpts) {
    let mut stack: Vec<&Node> = vec![root];
    while let Some(node) = stack.pop() {
        if node.depth + 1 > n_eff {
            continue;
        }
        for child in &node.children {
            match child.filetype {
                FileType::Dir => {
                    rows.push(make_row(
                        components(child, root),
                        RowType::Dir,
                        child.size,
                        opts,
                    ));
                    stack.push(child);
                }
                FileType::Symlink if resolves_to_dir(child) => {
                    if opts.follow_links {
                        // -L: the walker descended and counted it; show it as
                        // the real dir it points at (single row).
                        rows.push(make_row(
                            components(child, root),
                            RowType::Dir,
                            child.size,
                            opts,
                        ));
                        stack.push(child);
                    } else if opts.show_files {
                        // The script's quirk: `[[ -d ]]` follows the link, so
                        // symlinked dirs get a size-0 dir row (the link row
                        // comes from the glob pass).
                        rows.push(make_row(components(child, root), RowType::Dir, 0, opts));
                    }
                }
                _ => {}
            }
        }
    }
}

/// glob pass (--f only): file + link rows, descending through real dirs AND
/// symlinked dirs (zsh globs traverse symlinked directories).
fn collect_glob_rows(root: &Node, l_eff: usize, rows: &mut Vec<Row>, opts: &TreeOpts) {
    let mut stack: Vec<&Node> = vec![root];
    while let Some(node) = stack.pop() {
        if node.depth + 1 > l_eff {
            continue;
        }
        for child in &node.children {
            let rtype = match child.filetype {
                FileType::File => Some(RowType::File),
                // With -L a symlinked dir is shown as a dir row only.
                FileType::Symlink if !(opts.follow_links && resolves_to_dir(child)) => {
                    Some(RowType::Link)
                }
                _ => None,
            };
            if let Some(rtype) = rtype {
                let comps = components(child, root);
                // Script quirk: `zstat -L -A` treats paths starting with '-'
                // as options, so the file loop's `|| continue` drops those
                // rows (link rows survive: their loop defaults the size).
                if rtype == RowType::File && comps[0].starts_with(b"-") {
                    if child.filetype == FileType::Dir || resolves_to_dir(child) {
                        stack.push(child);
                    }
                    continue;
                }
                let size = if opts.by_filecount {
                    child.size
                } else {
                    child.size_apparent
                };
                rows.push(make_row(comps, rtype, size, opts));
            }
            if child.filetype == FileType::Dir || resolves_to_dir(child) {
                stack.push(child);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sort keys (§4.4)
// ---------------------------------------------------------------------------

/// inv zero-padded to 19 digits (the script's printf '%019d'), no alloc.
fn itoa19(n: u64) -> [u8; 19] {
    let mut buf = [b'0'; 19];
    let mut v = n;
    let mut i = 19;
    while i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    buf
}

/// index zero-padded to 8 digits (the script's (l.8.0)), no alloc.
fn itoa8(n: usize) -> [u8; 8] {
    let mut buf = [b'0'; 8];
    let mut v = n;
    let mut i = 8;
    while i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    buf
}

/// Ancestor lookup table: directory rows only (the script's row_by_path,
/// keyed by the '/'-joined relative path).
type RowByPath = FxHashMap<Vec<u8>, usize>;

/// '/'-joined relative path (the script's hash keys); names cannot contain
/// '/'.
fn flat_path(components: &[Vec<u8>]) -> Vec<u8> {
    let mut p: Vec<u8> = Vec::new();
    for (j, c) in components.iter().enumerate() {
        if j > 0 {
            p.push(b'/');
        }
        p.extend_from_slice(c);
    }
    p
}

fn sort_key(row: &Row, rows: &[Row], row_by_path: &RowByPath, sort: SortMode) -> Vec<u8> {
    let n = row.components.len();
    let mut key: Vec<u8> = Vec::new();
    match sort {
        SortMode::Size => {
            for j in 0..n {
                // rank/size come from this row for the last component, from
                // the visible ancestor row otherwise (rank 1 / MAX64 if the
                // ancestor is not in the visible set).
                let (rank, size) = if j + 1 == n {
                    (
                        if row.rtype == RowType::Dir { 0 } else { 1 },
                        Some(row.size),
                    )
                } else {
                    match row_by_path.get(&flat_path(&row.components[..=j])) {
                        Some(&idx) => (
                            if rows[idx].rtype == RowType::Dir {
                                0
                            } else {
                                1
                            },
                            Some(rows[idx].size),
                        ),
                        None => (1, None),
                    }
                };
                let inv = size.map(|s| MAX64 - s).unwrap_or(MAX64);
                key.push(b'0' + rank);
                let digits = itoa19(inv);
                key.extend_from_slice(&digits);
                key.extend_from_slice(&row.components[j]);
                key.push(SEP);
            }
        }
        SortMode::Name => {
            for (j, comp) in row.components.iter().enumerate() {
                if j > 0 {
                    key.push(SEP);
                }
                key.extend_from_slice(comp);
            }
            if row.rtype == RowType::Dir {
                key.push(SEP);
            }
        }
    }
    key
}

// ---------------------------------------------------------------------------
// Renderer
// ---------------------------------------------------------------------------

pub fn render_tree(roots: &[Node], opts: &TreeOpts, out: &mut impl Write) -> std::io::Result<()> {
    for root in roots {
        render_one(root, opts, out)?;
    }
    Ok(())
}

/// Cheap node count for capacity reservation (one pointer walk).
fn estimate_rows(root: &Node, _n_eff: usize, _l_eff: usize) -> usize {
    let mut count = 1usize;
    let mut stack: Vec<&Node> = vec![root];
    while let Some(n) = stack.pop() {
        count += n.children.len();
        stack.extend(n.children.iter());
    }
    count
}

struct Counters {
    ndirs: usize,
    nfiles: usize,
    files_size: u64,
}

fn top_level_counters(root: &Node) -> Counters {
    // The script's fresh top-level globs: *(DN/) real dirs (zsh's / does not
    // match symlinks), *(DN.) regular files, sum of their apparent sizes.
    let mut c = Counters {
        ndirs: 0,
        nfiles: 0,
        files_size: 0,
    };
    for child in &root.children {
        match child.filetype {
            FileType::Dir => c.ndirs += 1,
            FileType::File => {
                c.nfiles += 1;
                // zstat-failure quirk: top-level names starting with '-' are
                // skipped by the script's summing loop, but still counted.
                let base = child
                    .name
                    .file_name()
                    .map(|n| n.as_encoded_bytes().to_vec())
                    .unwrap_or_default();
                if !base.starts_with(b"-") {
                    c.files_size += child.size_apparent;
                }
            }
            FileType::Symlink => {}
        }
    }
    c
}

fn render_one(root: &Node, opts: &TreeOpts, out: &mut impl Write) -> std::io::Result<()> {
    let display_depth = opts.display_depth;
    // D4: depth 0 scans with depth 1 and displays the top level only.
    let n_eff = if display_depth == 0 { 1 } else { display_depth };
    let l_eff = if display_depth == 0 {
        1
    } else {
        display_depth.saturating_add(1)
    };

    // gdu pass rows first, then glob pass rows (the script's insertion order).
    // The glob pass (--f) is skipped in folders-only mode.
    let mut rows: Vec<Row> = Vec::with_capacity(estimate_rows(root, n_eff, l_eff));
    collect_dir_rows(root, n_eff, &mut rows, opts);
    if opts.show_files {
        collect_glob_rows(root, l_eff, &mut rows, opts);
    }

    let counters = top_level_counters(root);
    let dir_hdr = if counters.ndirs == 1 {
        "1 dir".to_string()
    } else {
        format!("{} dirs", counters.ndirs)
    };
    let file_str = if counters.nfiles == 1 {
        "1 file".to_string()
    } else {
        format!("{} files", counters.nfiles)
    };
    let total_hdr = if opts.by_filecount {
        // -f: the header total is the total file count, no unit suffix.
        fmt_comma(root.size)
    } else {
        [fmt_comma(root.size / MIB), b"Mb".to_vec()].concat()
    };

    let mut maxw: usize = 8;
    for row in rows.iter() {
        if row.formatted.len() > maxw {
            maxw = row.formatted.len();
        }
    }
    if total_hdr.len() > maxw {
        maxw = total_hdr.len();
    }

    if rows.is_empty() {
        // Empty-result path (§4.6): fixed %8s column and fixed dash runs.
        writeln!(out)?;
        writeln!(
            out,
            "  {:>8}  {}",
            String::from_utf8_lossy(&total_hdr),
            dir_hdr
        )?;
        writeln!(out, "  --------  ------")?;
        writeln!(out)?;
        writeln!(out, "No entries found.")?;
        writeln!(out)?;
        writeln!(out, "{} ({})", file_str, fmt_human(counters.files_size))?;
        return Ok(());
    }

    // row_by_path: directory rows only, for ancestor sort-key components.
    let mut row_by_path: RowByPath = FxHashMap::default();
    for (idx, row) in rows.iter_mut().enumerate() {
        row.idx = idx;
        if row.rtype == RowType::Dir {
            row_by_path.insert(flat_path(&row.components), idx);
        }
    }
    // Keys reference rows by index, so build them all before sorting. The
    // script sorts "key\t<index zero-padded to 8>" as one string — the tab
    // suffix decides ties (e.g. name mode: a dir's trailing \x01 sorts before
    // a file's bare name because \x01 < \t). Reproduce the full string.
    for i in 0..rows.len() {
        let mut key = sort_key(&rows[i], &rows, &row_by_path, opts.sort);
        key.push(b'\t');
        key.extend_from_slice(&itoa8(rows[i].idx));
        rows[i].key = key;
    }

    // Native lexical sort, byte order (§4.4). Sort indices, not rows: the
    // rows carry the key buffers and we do not want to move them.
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_unstable_by(|&a, &b| rows[a].key.cmp(&rows[b].key));

    // Sibling pass (§4.5): reverse, keyed by the '/'-joined path string —
    // duplicate paths (nested symlinked dirs) share one entry, exactly like
    // the script.
    let mut has_later_sibling: FxHashMap<Vec<u8>, bool> = FxHashMap::default();
    let mut seen_parent: FxHashMap<Vec<u8>, bool> = FxHashMap::default();
    for &i in order.iter().rev() {
        let row = &rows[i];
        if row.components.len() > 1 {
            let parent = flat_path(&row.components[..row.components.len() - 1]);
            has_later_sibling.insert(
                flat_path(&row.components),
                seen_parent.get(&parent).copied().unwrap_or(false),
            );
            seen_parent.insert(parent, true);
        } else {
            has_later_sibling.insert(flat_path(&row.components), false);
        }
    }

    let cdir: &[u8] = if opts.colors { COLOR_DIR } else { b"" };
    let cbranch: &[u8] = if opts.colors { COLOR_BRANCH } else { b"" };
    let cres: &[u8] = if opts.colors { COLOR_RESET } else { b"" };

    // Header (§4.6).
    let dirlen = std::cmp::max(4, dir_hdr.len());
    writeln!(out)?;
    writeln!(
        out,
        "  {:>maxw$}  {}",
        String::from_utf8_lossy(&total_hdr),
        dir_hdr
    )?;
    writeln!(out, "  {:->maxw$}  {:->dirlen$}", "", "")?;

    // Rows (§4.5), in sorted order.
    for &i in order.iter() {
        let row = &rows[i];
        let depth = row.components.len();
        let name = &row.components[depth - 1];
        let flag = |path: &Vec<u8>| has_later_sibling.get(path).copied().unwrap_or(false);

        write!(
            out,
            "  {:>maxw$}  ",
            String::from_utf8_lossy(&row.formatted)
        )?;

        if depth > 1 {
            // Prefix stack + connector.
            for j in 1..depth - 1 {
                let ancestor = flat_path(&row.components[..j]);
                if flag(&ancestor) {
                    out.write_all(cbranch)?;
                    out.write_all("│   ".as_bytes())?;
                    out.write_all(cres)?;
                } else {
                    out.write_all(b"    ")?;
                }
            }
            out.write_all(cbranch)?;
            if flag(&flat_path(&row.components)) {
                out.write_all("├── ".as_bytes())?;
            } else {
                out.write_all("└── ".as_bytes())?;
            }
            out.write_all(cres)?;
        }

        match row.rtype {
            RowType::Dir => {
                out.write_all(cdir)?;
                out.write_all(DIR_ICON)?;
                out.write_all(b" ")?;
                out.write_all(name)?;
                out.write_all(b"/")?;
                out.write_all(cres)?;
            }
            _ => {
                out.write_all(icon(row.rtype))?;
                out.write_all(b" ")?;
                out.write_all(name)?;
            }
        }
        writeln!(out)?;
    }

    // Summary (§4.6).
    writeln!(out)?;
    writeln!(out, "{} ({})", file_str, fmt_human(counters.files_size))?;

    Ok(())
}

fn icon(rtype: RowType) -> &'static [u8] {
    match rtype {
        RowType::Dir => DIR_ICON,
        RowType::File => FILE_ICON,
        RowType::Link => LINK_ICON,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_opts(display_depth: usize, show_files: bool) -> TreeOpts {
        TreeOpts {
            display_depth,
            sort: SortMode::Size,
            colors: false,
            show_files,
            by_filecount: false,
            follow_links: false,
        }
    }

    fn node(name: &str, filetype: FileType, size: u64, size_apparent: u64, depth: usize) -> Node {
        Node {
            name: PathBuf::from(name),
            size,
            size_apparent,
            filetype,
            children: vec![],
            inode_device: None,
            depth,
        }
    }

    // --- formatters (§8.3) ---

    #[test]
    fn test_fmt_mib_blank_below_one_mib() {
        assert_eq!(fmt_mib_half_even(0), b"        ".to_vec());
        assert_eq!(fmt_mib_half_even(MIB - 1), b"        ".to_vec());
    }

    #[test]
    fn test_fmt_mib_round_half_to_even() {
        // r == 524288 with q odd -> round up; with q even -> stay.
        assert_eq!(
            fmt_mib_half_even(MIB + HALF),
            format!("{:>8}", 2).into_bytes()
        );
        assert_eq!(
            fmt_mib_half_even(2 * MIB + HALF),
            format!("{:>8}", 2).into_bytes()
        );
        assert_eq!(
            fmt_mib_half_even(3 * MIB + HALF),
            format!("{:>8}", 4).into_bytes()
        );
        // r above/below the threshold.
        assert_eq!(
            fmt_mib_half_even(2 * MIB + HALF + 1),
            format!("{:>8}", 3).into_bytes()
        );
        assert_eq!(
            fmt_mib_half_even(2 * MIB + HALF - 1),
            format!("{:>8}", 2).into_bytes()
        );
        assert_eq!(fmt_mib_half_even(5 * MIB), format!("{:>8}", 5).into_bytes());
    }

    #[test]
    fn test_fmt_comma() {
        assert_eq!(fmt_comma(0), b"       0".to_vec());
        assert_eq!(fmt_comma(999), b"     999".to_vec());
        assert_eq!(fmt_comma(1000), b"   1,000".to_vec());
        assert_eq!(fmt_comma(1_234_567), b"1,234,567".to_vec());
        assert_eq!(fmt_comma(12_345_678), b"12,345,678".to_vec());
    }

    #[test]
    fn test_fmt_human() {
        assert_eq!(fmt_human(0), "0 b");
        assert_eq!(fmt_human(1023), "1023 b");
        assert_eq!(fmt_human(1024), "1.0 Kb");
        assert_eq!(fmt_human(4352), "4.2 Kb"); // 4.25 ties go half-to-even
        assert_eq!(fmt_human(MIB), "1.0 Mb");
        assert_eq!(fmt_human(GIB), "1.000 Gb");
    }

    // --- comparator (§8.3) ---

    fn key_of(rows: &[Row], i: usize, map: &RowByPath, sort: SortMode) -> Vec<u8> {
        sort_key(&rows[i], rows, map, sort)
    }

    fn row(components: Vec<&[u8]>, rtype: RowType, size: u64) -> Row {
        Row {
            components: components.iter().map(|c| c.to_vec()).collect(),
            rtype,
            size,
            formatted: Vec::new(),
            key: Vec::new(),
            idx: 0,
        }
    }

    #[test]
    fn test_size_mode_dirs_first_size_desc_name_asc() {
        // Two dirs: big (2 MiB) sorts before small (1 MiB); a file (3 MiB)
        // sorts after both dirs despite being biggest (rank 1).
        let d_big = row(vec![b"b"], RowType::Dir, 2 * MIB);
        let d_small = row(vec![b"a"], RowType::Dir, MIB);
        let f = row(vec![b"z"], RowType::File, 3 * MIB);
        let rows = vec![d_big, d_small, f];
        let mut map = RowByPath::default();
        map.insert(b"b".to_vec(), 0);
        map.insert(b"a".to_vec(), 1);

        let k_big = key_of(&rows, 0, &map, SortMode::Size);
        let k_small = key_of(&rows, 1, &map, SortMode::Size);
        let k_file = key_of(&rows, 2, &map, SortMode::Size);
        assert!(k_big < k_small, "size desc");
        assert!(k_small < k_file, "dirs before files");

        // Name asc on ties: same-size dirs sort by name.
        let d1 = row(vec![b"x"], RowType::Dir, MIB);
        let d2 = row(vec![b"y"], RowType::Dir, MIB);
        let rows2 = vec![d1, d2];
        let mut map2 = RowByPath::default();
        map2.insert(b"x".to_vec(), 0);
        map2.insert(b"y".to_vec(), 1);
        assert!(
            key_of(&rows2, 0, &map2, SortMode::Size) < key_of(&rows2, 1, &map2, SortMode::Size)
        );
    }

    #[test]
    fn test_name_mode_dirs_before_files_on_identical_basename() {
        let d = row(vec![b"foo"], RowType::Dir, MIB);
        let f = row(vec![b"foo"], RowType::File, MIB);
        let rows = vec![d, f];
        let map = RowByPath::default();
        let k_dir = key_of(&rows, 0, &map, SortMode::Name);
        let k_file = key_of(&rows, 1, &map, SortMode::Name);
        assert_eq!(k_file, b"foo".to_vec());
        assert_eq!(k_dir, [b"foo".to_vec(), vec![SEP]].concat());
        // The script sorts the full "key\t<index>" string: the dir's trailing
        // \x01 (0x01) sorts before the file key's \t (0x09) -> dirs first.
        // Goldens (symlinks.*.name) pin this; the plan §4.4 guess was wrong.
        let full_dir = [k_dir.as_slice(), b"\t00000000"].concat();
        let full_file = [k_file.as_slice(), b"\t00000001"].concat();
        assert!(
            full_dir < full_file,
            "dirs sort before files on identical basenames"
        );
    }

    // --- per-type depth cutoff (§8.3) ---

    #[test]
    fn test_depth_cutoff_per_type() {
        // N = 1: dirs kept at depth 1, files kept at depth 2 (N+1), files at
        // depth 3 (N+2) dropped.
        let mut mid = node("/r/d", FileType::Dir, 0, 0, 1);
        let mut deep = node("/r/d/e", FileType::Dir, 0, 0, 2);
        let f2 = node("/r/d/f.bin", FileType::File, MIB, MIB, 2);
        let f3 = node("/r/d/e/g.bin", FileType::File, MIB, MIB, 3);
        deep.children.push(f3);
        mid.children.push(deep);
        mid.children.push(f2);
        let mut root = node("/r", FileType::Dir, 0, 0, 0);
        root.children.push(mid);

        let mut rows: Vec<Row> = Vec::new();
        let opts = test_opts(1, true);
        collect_dir_rows(&root, 1, &mut rows, &opts);
        collect_glob_rows(&root, 2, &mut rows, &opts);

        let paths: Vec<String> = rows
            .iter()
            .map(|r| {
                let mut p: Vec<u8> = Vec::new();
                for (j, c) in r.components.iter().enumerate() {
                    if j > 0 {
                        p.push(b'/');
                    }
                    p.extend_from_slice(c);
                }
                String::from_utf8_lossy(&p).to_string()
            })
            .collect();
        assert!(paths.contains(&"d".to_string()), "dir at N kept");
        assert!(paths.contains(&"d/f.bin".to_string()), "file at N+1 kept");
        assert!(!paths.contains(&"d/e".to_string()), "dir at N+1 dropped");
        assert!(
            !paths.contains(&"d/e/g.bin".to_string()),
            "file at N+2 dropped"
        );
    }

    // --- symlink policy (§8.3): needs a real filesystem ---

    #[test]
    fn test_symlink_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("realdir")).unwrap();
        std::fs::write(root.join("realdir/target.bin"), vec![0u8; 2 * MIB as usize]).unwrap();
        std::fs::File::create(root.join("plain.bin")).unwrap();
        std::fs::write(root.join("big.bin"), vec![0u8; 3 * MIB as usize]).unwrap();
        std::fs::create_dir(root.join("emptydir")).unwrap();
        std::os::unix::fs::symlink("big.bin", root.join("link_to_file")).unwrap();
        std::os::unix::fs::symlink("emptydir", root.join("link_to_dir")).unwrap();
        std::os::unix::fs::symlink("nowhere", root.join("dangling")).unwrap();

        // Build the tree the way the tree-mode walker would: symlink leaves
        // carry size_apparent = their own lstat size (the script's
        // `zstat -L -A` quirk does not actually follow links).
        let mut root_node = node(root.to_str().unwrap(), FileType::Dir, 0, 0, 0);
        let sz = |p: &str, ft: FileType| {
            let full = root.join(p);
            let apparent = std::fs::symlink_metadata(&full)
                .map(|m| m.len())
                .unwrap_or(0);
            node(full.to_str().unwrap(), ft, 0, apparent, 1)
        };
        root_node.children.push(sz("realdir", FileType::Dir));
        root_node.children.push(sz("plain.bin", FileType::File));
        root_node.children.push(sz("big.bin", FileType::File));
        root_node.children.push(sz("emptydir", FileType::Dir));
        root_node
            .children
            .push(sz("link_to_file", FileType::Symlink));
        root_node
            .children
            .push(sz("link_to_dir", FileType::Symlink));
        root_node.children.push(sz("dangling", FileType::Symlink));

        // display_depth 0: top level only; links use the MiB formatter (D5).
        let opts = test_opts(0, true);
        let mut out: Vec<u8> = Vec::new();
        render_one(&root_node, &opts, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();

        // maxw is 10 here (the header total "       0Mb" is 10 chars).
        // Link rows carry the symlink's own size (path length) -> all blank.
        assert!(
            text.contains("           3  \u{F0214} big.bin"),
            "big.bin shows 3: {text}"
        );
        assert!(
            text.contains("              \u{F0337} link_to_file"),
            "{text}"
        );
        // link_to_dir: appears twice — as a dir row (gdu size 0 -> blank) and
        // as a link row (own size 8 -> blank).
        assert!(
            text.contains("              \u{F024B} link_to_dir/"),
            "{text}"
        );
        assert!(
            text.contains("              \u{F0337} link_to_dir\n"),
            "{text}"
        );
        // Dangling link: own size -> blank column.
        assert!(text.contains("              \u{F0337} dangling"), "{text}");
        // realdir/emptydir dirs are rows; their contents are not (depth 0).
        assert!(text.contains("              \u{F024B} realdir/"), "{text}");
        assert!(!text.contains("realdir/target.bin"), "{text}");
        // Footer counts top-level regular files only: plain.bin (0) + big.bin.
        assert!(text.ends_with("2 files (3.0 Mb)\n"), "{text}");
    }

    #[test]
    fn test_folders_only_default() {
        // show_files=false (the new default): real dirs only. No file rows,
        // no link rows, no double rows for symlinked dirs. Header/footer
        // counts stay (they describe the base directory).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("realdir")).unwrap();
        std::fs::write(root.join("realdir/target.bin"), vec![0u8; 2 * MIB as usize]).unwrap();
        std::fs::write(root.join("big.bin"), vec![0u8; 3 * MIB as usize]).unwrap();
        std::os::unix::fs::symlink("realdir", root.join("link_to_dir")).unwrap();

        let mut root_node = node(root.to_str().unwrap(), FileType::Dir, 5 * MIB, 0, 0);
        let sz = |p: &str, ft: FileType, size: u64| {
            let full = root.join(p);
            node(full.to_str().unwrap(), ft, size, size, 1)
        };
        root_node
            .children
            .push(sz("realdir", FileType::Dir, 2 * MIB));
        root_node
            .children
            .push(sz("big.bin", FileType::File, 3 * MIB));
        root_node
            .children
            .push(sz("link_to_dir", FileType::Symlink, 0));
        // The walker descends symlinked dirs in tree mode; give it a child.
        let mut linkdir = sz("link_to_dir", FileType::Symlink, 0);
        linkdir
            .children
            .push(sz("link_to_dir/inner.bin", FileType::File, 0));
        root_node.children.pop();
        root_node.children.push(linkdir);

        let mut out: Vec<u8> = Vec::new();
        render_one(&root_node, &test_opts(0, false), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("\u{F024B} realdir/"), "{text}");
        assert!(!text.contains("big.bin\n"), "no file rows: {text}");
        assert!(!text.contains("\u{F0337}"), "no link rows: {text}");
        assert!(
            !text.contains("link_to_dir"),
            "no symlinked-dir rows: {text}"
        );
        assert!(!text.contains("inner.bin"), "no descent rows: {text}");
        assert!(text.contains("1 dir"), "header counts top-level real dirs");
        assert!(text.ends_with("1 file (3.0 Mb)\n"), "footer stays: {text}");
    }

    #[test]
    fn test_filecount_mode() {
        // -f: the size column shows counts (comma-grouped), header total is
        // the total count with no unit suffix.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut opts = test_opts(0, true);
        opts.by_filecount = true;

        let mut root_node = node(root.to_str().unwrap(), FileType::Dir, 1_234, 0, 0);
        let full = root.join("d");
        root_node
            .children
            .push(node(full.to_str().unwrap(), FileType::Dir, 1_000, 0, 1));
        let full = root.join("f.bin");
        root_node
            .children
            .push(node(full.to_str().unwrap(), FileType::File, 1, 655_360, 1));
        let mut out: Vec<u8> = Vec::new();
        render_one(&root_node, &opts, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        // maxw grows to the 10-char header total "   1,234" -> rows right-align to it.
        assert!(text.contains("     1,234  1 dir"), "{text}");
        assert!(text.contains("     1,000  \u{F024B} d/"), "{text}");
        assert!(text.contains("         1  \u{F0214} f.bin"), "{text}");
        // Footer stays byte-based (top-level file: 640 KiB).
        assert!(text.ends_with("1 file (640.0 Kb)\n"), "{text}");
    }

    #[test]
    fn test_follow_links_single_row() {
        // -L: the walker descended and counted the symlinked dir; the renderer
        // shows it as a single dir row with its real aggregate size.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join("realdir")).unwrap();
        std::fs::write(root.join("realdir/inner.bin"), vec![0u8; 2 * MIB as usize]).unwrap();
        std::os::unix::fs::symlink("realdir", root.join("linkdir")).unwrap();
        let mut opts = test_opts(0, true);
        opts.follow_links = true;

        let mut root_node = node(root.to_str().unwrap(), FileType::Dir, 2 * MIB, 0, 0);
        let mut linkdir = node(
            root.join("linkdir").to_str().unwrap(),
            FileType::Symlink,
            2 * MIB,
            2 * MIB,
            1,
        );
        linkdir.children.push(node(
            root.join("linkdir/inner.bin").to_str().unwrap(),
            FileType::File,
            2 * MIB,
            2 * MIB,
            2,
        ));
        root_node.children.push(linkdir);

        let mut out: Vec<u8> = Vec::new();
        render_one(&root_node, &opts, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        // maxw 10 (header "       2Mb"); row column right-aligns "       2".
        assert!(text.contains("           2  \u{F024B} linkdir/"), "{text}");
        // depth 0 -> files only at depth 1, so no inner.bin row; single row.
        assert_eq!(text.matches("linkdir").count(), 1, "{text}");
        assert!(!text.contains("inner.bin"), "{text}");
        assert!(!text.contains("\u{F0337}"), "no link row with -L: {text}");
    }

    #[test]
    fn test_maxw_growth_and_header() {
        // A 6-digit MiB value (999999) grows the column to 6+... all formatted
        // values are 8 chars, so maxw stays 8 for rows; the header total (10
        // chars) pushes maxw to 10 — every normal run has maxw >= 10.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("f.bin"), vec![0u8; 2 * MIB as usize]).unwrap();
        let mut root_node = node(root.to_str().unwrap(), FileType::Dir, 5 * MIB, 0, 0);
        let full = root.join("f.bin");
        root_node.children.push(node(
            full.to_str().unwrap(),
            FileType::File,
            2 * MIB,
            2 * MIB,
            1,
        ));
        let opts = test_opts(0, true);
        let mut out: Vec<u8> = Vec::new();
        render_one(&root_node, &opts, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        // blank, header, dashes, row, blank, footer
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[1], "         5Mb  0 dirs");
        assert_eq!(lines[2], "  ----------  ------");
        assert_eq!(lines[3], "           2  \u{F0214} f.bin");
        assert_eq!(lines[5], "1 file (2.0 Mb)");
    }

    #[test]
    fn test_empty_path_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let root_node = node(tmp.path().to_str().unwrap(), FileType::Dir, 0, 0, 0);
        let opts = test_opts(0, true);
        let mut out: Vec<u8> = Vec::new();
        render_one(&root_node, &opts, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "\n         0Mb  0 dirs\n  --------  ------\n\nNo entries found.\n\n0 files (0 b)\n"
        );
    }
}
