//! Minimal read-only CFF (Compact Font Format) structure access for
//! CIDFontType0 subsetting — INDEX traversal, Top DICT operator scanning,
//! charset parsing, and the charset bridge for subsetted programs.
//! CIDFontType0 子集化所需的最小只读 CFF 结构访问 — INDEX 遍历、Top DICT
//! 操作符扫描、charset 解析，以及子集字体的 charset 桥接。
//!
//! The typst subsetter rewrites a CFF charset as an identity mapping
//! (new CID = new GID) and does not preserve the original CIDs, but a
//! CIDFontType0 in a PDF has no `/CIDToGIDMap` — the font's own charset IS
//! the CID→glyph mapping, and the content stream's CIDs must keep working
//! untouched. The bridge therefore appends a fresh format-0 charset that
//! assigns every kept glyph its original CID, and repoints the Top DICT's
//! charset offset (the subsetter always writes it as a fixed 5-byte integer
//! operand, so repointing never shifts anything else in the font).
//!
//! Everything here is bounds-checked and returns `None` on the first
//! surprise — callers treat that as "leave this font alone" (fail-closed,
//! matching the rest of the subsetting pass). The parser only ever reads;
//! the single mutation is the byte-for-byte operand patch of the bridge.
//!
//! typst subsetter 重写 CFF charset 时使用恒等映射（新 CID = 新 GID）且不保留
//! 原 CID，而 PDF 的 CIDFontType0 没有 `/CIDToGIDMap` —— 字体自身的 charset
//! 就是 CID→字形映射，内容流的 CID 必须原样可用。桥接方案：在子集字体尾部
//! 追加一张 format-0 charset，把每个保留字形的原 CID 还给它，并把 Top DICT
//! 的 charset 偏移改指过去（subsetter 固定写 5 字节整型操作数，改偏移不会
//! 引起字体内部任何位移）。

/// A parsed CFF font — just enough structure for subsetting decisions.
pub(crate) struct CffFont<'a> {
    data: &'a [u8],
    /// Parsed Top DICT facts.
    charset: Option<DictOperand>,
    charstrings: Option<DictOperand>,
    cid_keyed: bool,
}

/// One integer DICT operand: its value and its absolute byte span.
#[derive(Clone)]
struct DictOperand {
    value: i64,
    start: usize,
    end: usize,
}

/// Offsets of interest in a Top DICT.
const OP_CHARSET: u8 = 15;
const OP_CHAR_STRINGS: u8 = 17;
/// First byte of the two-byte ROS operator (12 30).
const OP_ESC: u8 = 12;
const OP_ROS: u8 = 30;

impl<'a> CffFont<'a> {
    /// Parse the CFF header, skip the fixed INDEX sequence (Name, Top DICT,
    /// String, Global Subr), and scan the Top DICT. `None` on any structural
    /// surprise.
    pub(crate) fn parse(data: &'a [u8]) -> Option<Self> {
        // Header: major, minor, hdrSize, offSize.
        let header_size = usize::from(*data.get(2)?);
        if header_size < 4 || header_size > data.len() {
            return None;
        }

        let name_index = skip_index(data, header_size)?;
        let top_index = name_index;
        let top_dict_start = top_index;
        let top_dict_end = skip_index(data, top_index)?;
        if top_dict_end > data.len() {
            return None;
        }
        let string_index = skip_index(data, top_dict_end)?;
        let _global_subr = skip_index(data, string_index)?;

        // The Top DICT INDEX holds exactly one object in a well-formed CFF.
        let top_dict = index_object(data, top_dict_start, 0)?;

        let mut scanner = DictScanner::new(data, top_dict);
        let mut charset = None;
        let mut charstrings = None;
        let mut cid_keyed = false;
        while let Some(operator) = scanner.next_operator() {
            match operator {
                Operator::One(OP_CHARSET) => charset = scanner.last_operand(),
                Operator::One(OP_CHAR_STRINGS) => charstrings = scanner.last_operand(),
                Operator::Two(OP_ESC, OP_ROS) => cid_keyed = true,
                _ => {}
            }
        }

        Some(Self {
            data,
            charset,
            charstrings,
            cid_keyed,
        })
    }

    pub(crate) fn is_cid_keyed(&self) -> bool {
        self.cid_keyed
    }

    /// Glyph count from the CharStrings INDEX (the authoritative nGlyphs).
    pub(crate) fn charstrings_count(&self) -> Option<usize> {
        let operand = self.charstrings.as_ref()?;
        let offset = operand.positive_offset(self.data.len())?;
        index_count(self.data, offset)
    }

    /// Full charset as `cids_by_gid` (length = glyph count; entry 0 is the
    /// implicit `.notdef` zero). `None` for predefined charsets (offset 0 —
    /// only legal on SID-keyed fonts) or any parse surprise, including range
    /// tables that do not cover exactly `glyphs - 1` entries.
    pub(crate) fn cids_by_gid(&self) -> Option<Vec<u16>> {
        if !self.cid_keyed {
            return None;
        }
        let operand = self.charset.as_ref()?;
        let charset_at = operand.positive_offset(self.data.len())?;
        let glyphs = self.charstrings_count()?;

        let mut cids = vec![0u16; glyphs];
        let mut gid = 1usize;
        let mut cursor = charset_at;
        match *self.data.get(cursor)? {
            0 => {
                while gid < glyphs {
                    cids[gid] = read_be16(self.data, cursor + 1 + (gid - 1) * 2)?;
                    gid += 1;
                }
            }
            1 => {
                cursor += 1;
                while gid < glyphs {
                    let first = read_be16(self.data, cursor)? as u32;
                    let n_left = u32::from(*self.data.get(cursor + 2)?);
                    for step in 0..=n_left {
                        if gid >= glyphs {
                            return None; // range overruns the glyph count
                        }
                        cids[gid] = u16::try_from(first + step).ok()?;
                        gid += 1;
                    }
                    cursor += 3;
                }
            }
            2 => {
                cursor += 1;
                while gid < glyphs {
                    let first = read_be16(self.data, cursor)? as u32;
                    let n_left = u32::from(read_be16(self.data, cursor + 2)?);
                    for step in 0..=n_left {
                        if gid >= glyphs {
                            return None;
                        }
                        cids[gid] = u16::try_from(first + step).ok()?;
                        gid += 1;
                    }
                    cursor += 4;
                }
            }
            _ => return None,
        }
        if gid != glyphs {
            return None; // range table undershot the glyph count
        }
        Some(cids)
    }

    /// Absolute byte span of the Top DICT's charset operand (the subsetter
    /// bridge patches these bytes in place).
    pub(crate) fn charset_operand_span(&self) -> Option<std::ops::Range<usize>> {
        let operand = self.charset.as_ref()?;
        Some(operand.start..operand.end)
    }
}

impl DictOperand {
    /// The operand as a byte offset into the font, valid only when positive
    /// and inside the data.
    fn positive_offset(&self, limit: usize) -> Option<usize> {
        let value = usize::try_from(self.value).ok()?;
        (value > 0 && value < limit).then_some(value)
    }
}

// ---------------------------------------------------------------------------
// DICT scanning
// ---------------------------------------------------------------------------

enum Operator {
    One(u8),
    Two(u8, u8),
}

/// Sequential DICT scanner: yields operators and remembers the span of the
/// integer operand that immediately preceded the last operator.
struct DictScanner<'a> {
    data: &'a [u8],
    /// Absolute position of the next unread byte.
    cursor: usize,
    end: usize,
    last_operand: Option<DictOperand>,
}

impl<'a> DictScanner<'a> {
    fn new(data: &'a [u8], range: std::ops::Range<usize>) -> Self {
        Self {
            data,
            cursor: range.start,
            end: range.end,
            last_operand: None,
        }
    }

    /// The integer operand preceding the operator just returned by
    /// `next_operator`.
    fn last_operand(&self) -> Option<DictOperand> {
        self.last_operand.clone()
    }

    fn next_operator(&mut self) -> Option<Operator> {
        self.last_operand = None;
        loop {
            let byte = *self.data.get(self.cursor)?;
            match byte {
                // One-byte integer operands.
                32..=246 => {
                    self.record_operand(self.cursor, self.cursor + 1, i64::from(byte) - 139);
                }
                // Two-byte positive / negative integers.
                247..=250 => {
                    let next = i64::from(*self.data.get(self.cursor + 1)?);
                    let value = (i64::from(byte) - 247) * 256 + next + 108;
                    self.record_operand(self.cursor, self.cursor + 2, value);
                }
                251..=254 => {
                    let next = i64::from(*self.data.get(self.cursor + 1)?);
                    let value = -(i64::from(byte) - 251) * 256 - next - 108;
                    self.record_operand(self.cursor, self.cursor + 2, value);
                }
                // Fixed-size integers.
                28 => {
                    let value = read_be16(self.data, self.cursor + 1)? as i16;
                    self.record_operand(self.cursor, self.cursor + 3, i64::from(value));
                }
                29 => {
                    let value = i32::from_be_bytes(
                        self.data
                            .get(self.cursor + 1..self.cursor + 5)?
                            .try_into()
                            .ok()?,
                    );
                    self.record_operand(self.cursor, self.cursor + 5, i64::from(value));
                }
                // Real numbers: nibble pairs until an 0xF terminator nibble.
                30 => {
                    let mut scan = self.cursor + 1;
                    loop {
                        let pair = *self.data.get(scan)?;
                        scan += 1;
                        if pair & 0x0F == 0x0F {
                            break;
                        }
                        if scan >= self.end {
                            return None;
                        }
                    }
                    self.cursor = scan;
                }
                // Operators.
                0..=21 => {
                    let operator = if byte == OP_ESC {
                        let escaped = *self.data.get(self.cursor + 1)?;
                        self.cursor += 2;
                        Operator::Two(byte, escaped)
                    } else {
                        self.cursor += 1;
                        Operator::One(byte)
                    };
                    return Some(operator);
                }
                _ => return None,
            }
        }
    }

    fn record_operand(&mut self, start: usize, end: usize, value: i64) {
        self.cursor = end;
        self.last_operand = Some(DictOperand { value, start, end });
    }
}

// ---------------------------------------------------------------------------
// INDEX primitives
// ---------------------------------------------------------------------------

/// Read the object count of the INDEX at `pos` without materializing it.
fn index_count(data: &[u8], pos: usize) -> Option<usize> {
    Some(usize::from(read_be16(data, pos)?))
}

/// Skip the INDEX at `pos`, returning the start position of whatever
/// follows it (offsets array plus object data).
fn skip_index(data: &[u8], pos: usize) -> Option<usize> {
    let count = index_count(data, pos)?;
    if count == 0 {
        return Some(pos + 2);
    }
    let off_size = usize::from(*data.get(pos + 2)?);
    if off_size == 0 || off_size > 4 {
        return None;
    }
    let read_offset = |which: usize| -> Option<usize> {
        let base = pos + 3 + which * off_size;
        let mut value = 0usize;
        for byte in data.get(base..base + off_size)? {
            value = (value << 8) | usize::from(*byte);
        }
        Some(value)
    };
    // Offsets are 1-based; the last one minus one is the object data length.
    let data_len = read_offset(count)?.checked_sub(1)?;
    Some(pos + 3 + (count + 1) * off_size + data_len)
}

/// Byte range of object `index` inside the INDEX at `pos`.
fn index_object(data: &[u8], pos: usize, index: usize) -> Option<std::ops::Range<usize>> {
    let count = index_count(data, pos)?;
    if index >= count {
        return None;
    }
    let off_size = usize::from(*data.get(pos + 2)?);
    if off_size == 0 || off_size > 4 {
        return None;
    }
    let read_offset = |which: usize| -> Option<usize> {
        let base = pos + 3 + which * off_size;
        let mut value = 0usize;
        for byte in data.get(base..base + off_size)? {
            value = (value << 8) | usize::from(*byte);
        }
        Some(value)
    };
    // Offsets are 1-based from the byte preceding the object data.
    let anchor = pos + 3 + (count + 1) * off_size - 1;
    let start = anchor + read_offset(index)?.checked_sub(1)?;
    let end = anchor + read_offset(index + 1)?.checked_sub(1)?;
    if start <= end && end <= data.len() {
        Some(start..end)
    } else {
        None
    }
}

fn read_be16(data: &[u8], pos: usize) -> Option<u16> {
    let bytes = data.get(pos..pos + 2)?;
    Some(u16::from_be_bytes(bytes.try_into().ok()?))
}

// ---------------------------------------------------------------------------
// OpenType wrapper plumbing
// ---------------------------------------------------------------------------

/// Extract the raw `CFF ` table from an OpenType (OTTO) wrapper. The
/// subsetter only consumes OpenType data, while PDFs embed bare CFF as
/// FontFile3/CIDFontType0C — this bridges the way in, and [`wrap_bare_cff`]
/// the way out.
pub(crate) fn extract_cff_table(data: &[u8]) -> Option<&[u8]> {
    let magic = u32::from_be_bytes(data.get(0..4)?.try_into().ok()?);
    if magic != 0x4F54_544F {
        // 0x00010000 (TrueType-flavored) has no CFF table; collections are
        // not embedded per-face in PDFs.
        return None;
    }
    let count = usize::from(read_be16(data, 4)?);
    for index in 0..count {
        let record = 12 + index * 16;
        if data.get(record..record + 4)? == b"CFF " {
            let offset =
                u32::from_be_bytes(data.get(record + 8..record + 12)?.try_into().ok()?) as usize;
            let length =
                u32::from_be_bytes(data.get(record + 12..record + 16)?.try_into().ok()?) as usize;
            return data.get(offset..offset.checked_add(length)?);
        }
    }
    None
}

/// Wrap a bare CFF font program in a minimal single-table OTTO container so
/// the subsetter accepts it.
pub(crate) fn wrap_bare_cff(cff: &[u8]) -> Vec<u8> {
    let mut wrapper = Vec::with_capacity(28 + cff.len() + 4);
    wrapper.extend_from_slice(b"OTTO");
    // numTables = 1; searchRange/entrySelector/rangeShift for one table.
    wrapper.extend_from_slice(&1u16.to_be_bytes());
    wrapper.extend_from_slice(&16u16.to_be_bytes());
    wrapper.extend_from_slice(&0u16.to_be_bytes());
    wrapper.extend_from_slice(&0u16.to_be_bytes());
    wrapper.extend_from_slice(b"CFF ");
    wrapper.extend_from_slice(&0u32.to_be_bytes()); // checksum (unused by subsetter)
    wrapper.extend_from_slice(&28u32.to_be_bytes()); // offset: 12 + one record
    wrapper.extend_from_slice(&(cff.len() as u32).to_be_bytes());
    wrapper.extend_from_slice(cff);
    while wrapper.len() % 4 != 0 {
        wrapper.push(0);
    }
    wrapper
}

// ---------------------------------------------------------------------------
// The subset bridge
// ---------------------------------------------------------------------------

/// Append a format-0 charset mapping each new glyph id (index into
/// `cids_by_new_gid`) to its original CID, then repoint the Top DICT's
/// charset operand at it. Returns `false` (leaving the bytes untouched is
/// then the caller's job) when the subset does not have the shape the
/// subsetter produces — the patched font must never be half-written.
pub(crate) fn bridge_subset_charset(cff: &mut Vec<u8>, cids_by_new_gid: &[u16]) -> bool {
    let Some(font) = CffFont::parse(cff) else {
        return false;
    };
    if !font.is_cid_keyed() {
        return false;
    }
    let Some(span) = font.charset_operand_span() else {
        return false;
    };
    // The subsetter always writes the charset operand as a 29-prefixed
    // 5-byte integer; anything else means we are not looking at its output.
    if cff.get(span.start) != Some(&29) || span.len() != 5 {
        return false;
    }
    if cids_by_new_gid.len() > u16::MAX as usize + 1 {
        return false;
    }

    let new_offset = cff.len() as u32;
    let mut table = Vec::with_capacity(1 + cids_by_new_gid.len() * 2);
    table.push(0u8); // format 0: one CID per glyph, glyphs 1..n
    for cid in cids_by_new_gid.iter().skip(1) {
        table.extend_from_slice(&cid.to_be_bytes());
    }
    cff.extend_from_slice(&table);

    cff[span.start..span.end].copy_from_slice(&[
        29,
        (new_offset >> 24) as u8,
        (new_offset >> 16) as u8,
        (new_offset >> 8) as u8,
        new_offset as u8,
    ]);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed CID-keyed test program (see scripts/make-cff-fixture.sh).
    const TEST_CFF: &[u8] = include_bytes!("../../assets/test-font-cid.cff");

    #[test]
    fn parses_the_committed_cid_font() {
        let font = CffFont::parse(TEST_CFF).expect("fixture must parse");
        assert!(font.is_cid_keyed(), "Source Han subset is CID-keyed");
        let glyphs = font.charstrings_count().expect("CharStrings INDEX");
        let cids = font.cids_by_gid().expect("charset must parse");
        assert_eq!(cids.len(), glyphs);
        assert_eq!(glyphs, 40, "fixture regeneration changed the glyph set");
        // The fixture charset is deliberately non-identity (gid 1 -> CID 17).
        assert_eq!(cids[1], 17);
        assert_ne!(cids[1], 1, "fixture must exercise the mapping");
        // The CIDs the integration test draws.
        let contains = |cid: u16| cids.contains(&cid);
        assert!(contains(49) && contains(37) && contains(39));
        assert!(contains(11760) && contains(31694) && contains(23326) && contains(38497));
    }

    #[test]
    fn garbage_programs_fail_closed() {
        assert!(CffFont::parse(&[]).is_none());
        assert!(CffFont::parse(b"not a font").is_none());
        assert!(CffFont::parse(&[1, 0, 4, 2]).is_none());
        // Truncated mid-charset: the fixture's INDEX headers and Top DICT
        // live in the first ~96 bytes and the 40-entry format-0 charset
        // ends around byte 177, so 120 cuts it in half — the deep reads must
        // refuse (the fail-closed surface the subsetting pass relies on).
        let truncated = &TEST_CFF[..120];
        assert!(CffFont::parse(truncated)
            .and_then(|font| font.cids_by_gid())
            .is_none());
    }

    #[test]
    fn opentype_wrapper_round_trips_the_table() {
        let wrapped = wrap_bare_cff(TEST_CFF);
        let extracted = extract_cff_table(&wrapped).expect("table must come back");
        assert_eq!(extracted, TEST_CFF);
        // The committed OTF asset carries the same program.
        let otf = include_bytes!("../../assets/test-font-cid.otf");
        assert_eq!(extract_cff_table(otf), Some(TEST_CFF));
    }

    #[test]
    fn bridge_repoints_charset_and_keeps_original_cids() {
        // Subset-shaped font: take the real font and give it a subsetter-style
        // Top DICT by running the actual subsetter over the wrapped program.
        let wrapped = wrap_bare_cff(TEST_CFF);
        let mut remapper = subsetter::GlyphRemapper::new();
        // Keep .notdef (0, implicit) plus the glyphs behind CIDs 49 and 11760.
        let original = CffFont::parse(TEST_CFF).unwrap();
        let cids = original.cids_by_gid().unwrap();
        let gid_of = |cid: u16| cids.iter().position(|&c| c == cid).unwrap() as u16;
        let keep = [gid_of(49), gid_of(11760)];
        for gid in keep {
            remapper.remap(gid);
        }
        let subset = subsetter::subset(&wrapped, 0, &remapper).expect("subset must work");
        let mut cff = extract_cff_table(&subset)
            .expect("subset stays CFF-flavored")
            .to_vec();

        // New gid order: .notdef, then the kept glyphs in remap order.
        let cids_by_new_gid: Vec<u16> = remapper
            .remapped_gids()
            .map(|old_gid| cids[usize::from(old_gid)])
            .collect();
        assert_eq!(cids_by_new_gid[0], 0, ".notdef leads");

        assert!(bridge_subset_charset(&mut cff, &cids_by_new_gid));
        let bridged = CffFont::parse(&cff).expect("bridged font must parse");
        let bridged_cids = bridged.cids_by_gid().expect("bridged charset parses");
        assert_eq!(bridged_cids.len(), remapper.num_gids() as usize);
        // The kept CIDs are exactly the ones reachable now.
        let mut reachable: Vec<u16> = bridged_cids[1..].to_vec();
        reachable.sort_unstable();
        let mut expected = vec![49u16, 11760];
        expected.sort_unstable();
        assert_eq!(reachable, expected, "original CIDs must come back");
    }

    #[test]
    fn bridge_refuses_foreign_shapes() {
        // The committed font's charset operand is not a 5-byte integer — the
        // bridge must refuse to touch it.
        let mut bytes = TEST_CFF.to_vec();
        assert!(!bridge_subset_charset(&mut bytes, &[0, 49]));
    }
}
