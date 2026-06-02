//! UTF-16 aware conversions between LSP [`Position`]s and byte offsets.
//!
//! The base LSP spec measures [`Position::character`] in **UTF-16 code units**,
//! while Rust strings are indexed in **UTF-8 bytes**. Any server that maps a
//! cursor to buffer contents must convert between the two, and getting it wrong
//! produces off-by-some bugs the moment a line contains a non-ASCII character.
//! These helpers do that conversion once, correctly, so language logic can work
//! in byte offsets.
//!
//! Lines are delimited by `\n`; a `\r` from a `\r\n` sequence is treated as part
//! of the preceding line's content, matching [`str::split`]`('\n')`.

use crate::lsp::Position;

/// Convert an LSP [`Position`] to a byte offset into `text`.
///
/// A line beyond the end of `text`, or a column beyond the end of its line,
/// clamps to the nearest valid offset rather than panicking.
pub fn position_to_offset(text: &str, position: Position) -> usize {
    let mut line_start = 0usize;
    for (index, line) in text.split('\n').enumerate() {
        if index as u32 == position.line {
            return line_start + utf16_column_to_byte(line, position.character);
        }
        // +1 accounts for the '\n' consumed by `split`.
        line_start += line.len() + 1;
    }
    text.len()
}

/// Convert a byte offset into `text` to an LSP [`Position`].
///
/// `offset` is clamped to `text.len()` and floored to a character boundary.
pub fn offset_to_position(text: &str, offset: usize) -> Position {
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
    let character = byte_to_utf16_column(&text[line_start..], offset - line_start);
    Position { line, character }
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
    fn floors_to_char_boundary() {
        let line = "é"; // bytes [0,1]; index 1 is mid-codepoint
        assert_eq!(byte_to_utf16_column(line, 1), 0);
    }
}
