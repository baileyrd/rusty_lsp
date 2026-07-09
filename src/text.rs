//! Position-encoding-aware conversions between LSP [`Position`]s and byte
//! offsets.
//!
//! The base LSP spec measures [`Position::character`] in **UTF-16 code units**,
//! while Rust strings are indexed in **UTF-8 bytes**. Any server that maps a
//! cursor to buffer contents must convert between the two, and getting it wrong
//! produces off-by-some bugs the moment a line contains a non-ASCII character.
//! [`position_to_offset`] and [`offset_to_position`] do that UTF-16 conversion
//! (the base-spec default), and the `_with` variants take a
//! [`PositionEncodingKind`] for servers that negotiated UTF-8 or UTF-32 via
//! [`crate::lsp::ServerCapabilities::position_encoding`] (LSP 3.17).
//!
//! Lines are delimited by `\n`; a `\r` from a `\r\n` sequence is treated as part
//! of the preceding line's content, matching [`str::split`]`('\n')`.

use crate::lsp::{Position, PositionEncodingKind};

/// Convert an LSP [`Position`] to a byte offset into `text`, assuming UTF-16
/// positions (the base-spec default). See [`position_to_offset_with`] for
/// other negotiated encodings.
///
/// A line beyond the end of `text`, or a column beyond the end of its line,
/// clamps to the nearest valid offset rather than panicking.
pub fn position_to_offset(text: &str, position: Position) -> usize {
    position_to_offset_with(text, position, PositionEncodingKind::Utf16)
}

/// Convert a byte offset into `text` to an LSP [`Position`], assuming UTF-16
/// positions (the base-spec default). See [`offset_to_position_with`] for
/// other negotiated encodings.
///
/// `offset` is clamped to `text.len()` and floored to a character boundary.
pub fn offset_to_position(text: &str, offset: usize) -> Position {
    offset_to_position_with(text, offset, PositionEncodingKind::Utf16)
}

/// Convert an LSP [`Position`] to a byte offset into `text`, measuring
/// [`Position::character`] in the given `encoding`.
///
/// A line beyond the end of `text`, or a column beyond the end of its line,
/// clamps to the nearest valid offset rather than panicking.
pub fn position_to_offset_with(
    text: &str,
    position: Position,
    encoding: PositionEncodingKind,
) -> usize {
    let mut line_start = 0usize;
    for (index, line) in text.split('\n').enumerate() {
        if index as u32 == position.line {
            return line_start + column_to_byte(line, position.character, encoding);
        }
        // +1 accounts for the '\n' consumed by `split`.
        line_start += line.len() + 1;
    }
    text.len()
}

/// Convert a byte offset into `text` to an LSP [`Position`], measuring
/// [`Position::character`] in the given `encoding`.
///
/// `offset` is clamped to `text.len()` and floored to a character boundary.
pub fn offset_to_position_with(
    text: &str,
    offset: usize,
    encoding: PositionEncodingKind,
) -> Position {
    let offset = floor_char_boundary(text, offset.min(text.len()));
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (index, ch) in text.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = index + 1;
        }
    }
    let character = byte_to_column(&text[line_start..], offset - line_start, encoding);
    Position { line, character }
}

/// Convert a column within a single `line` to a byte offset in that line,
/// measuring the column in the given `encoding`.
///
/// A column past the end of the line clamps to `line.len()`.
pub fn column_to_byte(line: &str, column: u32, encoding: PositionEncodingKind) -> usize {
    match encoding {
        PositionEncodingKind::Utf8 => utf8_column_to_byte(line, column),
        PositionEncodingKind::Utf16 => utf16_column_to_byte(line, column),
        PositionEncodingKind::Utf32 => utf32_column_to_byte(line, column),
    }
}

/// Convert a byte offset within a single `line` to its column, measured in
/// the given `encoding`.
///
/// `byte` is clamped to `line.len()` and floored to a character boundary.
pub fn byte_to_column(line: &str, byte: usize, encoding: PositionEncodingKind) -> u32 {
    match encoding {
        PositionEncodingKind::Utf8 => byte_to_utf8_column(line, byte),
        PositionEncodingKind::Utf16 => byte_to_utf16_column(line, byte),
        PositionEncodingKind::Utf32 => byte_to_utf32_column(line, byte),
    }
}

/// Convert a UTF-16 column within a single `line` to a byte offset in that line.
///
/// A column past the end of the line clamps to `line.len()`.
pub fn utf16_column_to_byte(line: &str, column: u32) -> usize {
    let mut col = 0u32;
    for (offset, ch) in line.char_indices() {
        if col >= column {
            return offset;
        }
        col += ch.len_utf16() as u32;
    }
    line.len()
}

/// Convert a byte offset within a single `line` to its UTF-16 column.
///
/// `byte` is clamped to `line.len()` and floored to a character boundary.
pub fn byte_to_utf16_column(line: &str, byte: usize) -> u32 {
    let end = floor_char_boundary(line, byte.min(line.len()));
    line[..end].chars().map(|c| c.len_utf16() as u32).sum()
}

/// Convert a UTF-8 column (a byte count) within a single `line` to a byte
/// offset in that line.
///
/// A column past the end of the line clamps to `line.len()`; a column that
/// lands mid-codepoint floors to the preceding character boundary.
pub fn utf8_column_to_byte(line: &str, column: u32) -> usize {
    floor_char_boundary(line, (column as usize).min(line.len()))
}

/// Convert a byte offset within a single `line` to its UTF-8 column (which,
/// by definition, is the byte offset itself, floored to a character
/// boundary and clamped to `line.len()`).
pub fn byte_to_utf8_column(line: &str, byte: usize) -> u32 {
    floor_char_boundary(line, byte.min(line.len())) as u32
}

/// Convert a UTF-32 column (a Unicode scalar value count) within a single
/// `line` to a byte offset in that line.
///
/// A column past the end of the line clamps to `line.len()`.
pub fn utf32_column_to_byte(line: &str, column: u32) -> usize {
    for (col, (offset, _ch)) in line.char_indices().enumerate() {
        if col as u32 >= column {
            return offset;
        }
    }
    line.len()
}

/// Convert a byte offset within a single `line` to its UTF-32 column (its
/// count of Unicode scalar values).
///
/// `byte` is clamped to `line.len()` and floored to a character boundary.
pub fn byte_to_utf32_column(line: &str, byte: usize) -> u32 {
    let end = floor_char_boundary(line, byte.min(line.len()));
    line[..end].chars().count() as u32
}

/// Largest character boundary `<= idx`. (Stand-in for the still-unstable
/// `str::floor_char_boundary`.)
fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_round_trips() {
        let text = "let x = 1;\nfn main() {}\n";
        // Start of second line.
        let pos = Position::new(1, 0);
        let offset = position_to_offset(text, pos);
        assert_eq!(&text[offset..offset + 2], "fn");
        assert_eq!(offset_to_position(text, offset), pos);
    }

    #[test]
    fn multibyte_columns_use_utf16_units() {
        // "é" is 2 UTF-8 bytes but 1 UTF-16 unit; "😀" is 4 bytes / 2 units.
        let line = "é😀x";
        assert_eq!(byte_to_utf16_column(line, 0), 0);
        assert_eq!(byte_to_utf16_column(line, 2), 1); // after "é"
        assert_eq!(byte_to_utf16_column(line, 6), 3); // after "é😀"
        // Inverse direction.
        assert_eq!(utf16_column_to_byte(line, 0), 0);
        assert_eq!(utf16_column_to_byte(line, 1), 2);
        assert_eq!(utf16_column_to_byte(line, 3), 6);
    }

    #[test]
    fn position_with_multibyte_prefix() {
        let text = "α = 1\nβ = 2"; // α, β are 2 bytes / 1 UTF-16 unit each
        // Column 4 on line 1 is the "2" after "β = ".
        let offset = position_to_offset(text, Position::new(1, 4));
        assert_eq!(&text[offset..], "2");
        assert_eq!(offset_to_position(text, offset), Position::new(1, 4));
    }

    #[test]
    fn out_of_range_clamps() {
        let text = "abc";
        assert_eq!(position_to_offset(text, Position::new(9, 9)), text.len());
        assert_eq!(utf16_column_to_byte("abc", 99), 3);
        assert_eq!(byte_to_utf16_column("abc", 99), 3);
    }

    #[test]
    fn utf8_columns_are_byte_offsets() {
        // "é" is 2 UTF-8 bytes; "😀" is 4 bytes. UTF-8 columns count bytes.
        let line = "é😀x";
        assert_eq!(byte_to_utf8_column(line, 0), 0);
        assert_eq!(byte_to_utf8_column(line, 2), 2); // after "é"
        assert_eq!(byte_to_utf8_column(line, 6), 6); // after "é😀"
        assert_eq!(utf8_column_to_byte(line, 0), 0);
        assert_eq!(utf8_column_to_byte(line, 2), 2);
        assert_eq!(utf8_column_to_byte(line, 6), 6);
        // Mid-codepoint columns floor to the preceding boundary.
        assert_eq!(utf8_column_to_byte(line, 1), 0);
    }

    #[test]
    fn utf32_columns_count_scalar_values() {
        // "é" and "😀" are each a single Unicode scalar value regardless of
        // their UTF-8/UTF-16 width.
        let line = "é😀x";
        assert_eq!(byte_to_utf32_column(line, 0), 0);
        assert_eq!(byte_to_utf32_column(line, 2), 1); // after "é"
        assert_eq!(byte_to_utf32_column(line, 6), 2); // after "é😀"
        assert_eq!(utf32_column_to_byte(line, 0), 0);
        assert_eq!(utf32_column_to_byte(line, 1), 2);
        assert_eq!(utf32_column_to_byte(line, 2), 6);
    }

    #[test]
    fn position_to_offset_with_honours_encoding() {
        let text = "😀x";
        // UTF-16: "😀" is 2 units, so column 2 lands right after it.
        let utf16_offset =
            position_to_offset_with(text, Position::new(0, 2), PositionEncodingKind::Utf16);
        assert_eq!(&text[utf16_offset..], "x");
        // UTF-32: "😀" is 1 scalar value, so column 1 lands right after it.
        let utf32_offset =
            position_to_offset_with(text, Position::new(0, 1), PositionEncodingKind::Utf32);
        assert_eq!(&text[utf32_offset..], "x");
        // UTF-8: "😀" is 4 bytes, so column 4 lands right after it.
        let utf8_offset =
            position_to_offset_with(text, Position::new(0, 4), PositionEncodingKind::Utf8);
        assert_eq!(&text[utf8_offset..], "x");

        assert_eq!(
            offset_to_position_with(text, utf32_offset, PositionEncodingKind::Utf32),
            Position::new(0, 1)
        );
    }

    #[test]
    fn floors_to_char_boundary() {
        let line = "é"; // bytes [0,1]; index 1 is mid-codepoint
        assert_eq!(byte_to_utf16_column(line, 1), 0);
    }
}
