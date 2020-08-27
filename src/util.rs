use codespan_reporting::diagnostic::{Diagnostic, Label};
use codespan_reporting::term::termcolor::{ColorSpec, WriteColor};
use core::{fmt, ops};
use std::io;

macro_rules! error {
    ($arg1:expr) => {
        Error::new($arg1, vec![])
    };

    ($msg:expr, $range1:expr, $file1:expr, $msg1:expr) => {
        Error::new(
            $msg,
            vec![util::ErrorSection {
                location: CodeLoc {
                    range: $range1,
                    file: $file1,
                },
                message: $msg1.to_string(),
            }],
        )
    };

    ($msg:expr, $loc1:expr, $msg1:expr) => {
        Error::new(
            $msg,
            vec![util::ErrorSection {
                location: $loc1,
                message: $msg1.to_string(),
            }],
        )
    };

    ($msg:expr, $range1:expr, $file1:expr, $msg1:expr, $range2:expr, $file2:expr, $msg2:expr) => {
        Error::new(
            $msg,
            vec![
                util::ErrorSection {
                    location: CodeLoc {
                        range: $range1,
                        file: $file1,
                    },
                    message: $msg1.to_string(),
                },
                util::ErrorSection {
                    location: CodeLoc {
                        range: $range2,
                        file: $file2,
                    },
                    message: $msg2.to_string(),
                },
            ],
        )
    };

    ($msg:expr, $loc1:expr, $msg1:expr, $loc2:expr, $msg2:expr) => {
        Error::new(
            $msg,
            vec![
                util::ErrorSection {
                    location: $loc1,
                    message: $msg1.to_string(),
                },
                util::ErrorSection {
                    location: $loc1,
                    message: $msg2.to_string(),
                },
            ],
        )
    };
}

#[derive(Debug)]
pub struct ErrorSection {
    pub location: CodeLoc,
    pub message: String,
}

#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub sections: Vec<ErrorSection>,
}

impl Into<Label<u32>> for &ErrorSection {
    fn into(self) -> Label<u32> {
        Label::primary(self.location.file, self.location.range).with_message(&self.message)
    }
}

impl Error {
    pub fn new(message: &str, sections: Vec<ErrorSection>) -> Error {
        Self {
            message: message.to_string(),
            sections,
        }
    }

    pub fn diagnostic(&self) -> Diagnostic<u32> {
        Diagnostic::error()
            .with_message(&self.message)
            .with_labels(self.sections.iter().map(|x| x.into()).collect())
    }
}

#[derive(Clone, PartialEq, Copy)]
pub struct Range {
    pub start: u32,
    pub end: u32,
}

impl fmt::Debug for Range {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "{}..{}", self.start, self.end)
    }
}

impl fmt::Display for Range {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(fmt, "{}..{}", self.start, self.end)
    }
}

pub fn r(start: u32, end: u32) -> Range {
    assert!(start <= end);

    Range { start, end }
}

pub fn r_from(range1: Range, range2: Range) -> Range {
    assert!(range1.start <= range2.end);

    Range {
        start: range1.start,
        end: range2.end,
    }
}

impl Range {
    pub fn cloc(self, file: u32) -> CodeLoc {
        CodeLoc { range: self, file }
    }
}

impl Into<ops::Range<usize>> for Range {
    fn into(self) -> ops::Range<usize> {
        (self.start as usize)..(self.end as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeLoc {
    pub range: Range,
    pub file: u32,
}

pub fn align_usize(size: usize, align: usize) -> usize {
    if size == 0 {
        return 0;
    }

    ((size - 1) / align * align) + align
}

pub fn align_u32(size: u32, align: u32) -> u32 {
    if size == 0 {
        return 0;
    }

    ((size - 1) / align * align) + align
}

// https://stackoverflow.com/questions/28127165/how-to-convert-struct-to-u8
pub unsafe fn any_as_u8_slice_mut<T: Sized + Copy>(p: &mut T) -> &mut [u8] {
    std::slice::from_raw_parts_mut(p as *mut T as *mut u8, std::mem::size_of::<T>())
}

pub fn any_as_u8_slice<T: Sized + Copy>(p: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts(p as *const T as *const u8, std::mem::size_of::<T>()) }
}

pub fn u32_to_u16_tup(value: u32) -> (u16, u16) {
    ((value >> 16) as u16, value as u16)
}

pub fn fold_binary<I, Iter: Iterator<Item = I>>(
    mut iter: Iter,
    mut reducer: impl FnMut(I, I) -> I,
) -> Option<I> {
    let first = iter.next()?;
    let second = match iter.next() {
        Some(s) => s,
        None => return Some(first),
    };

    let mut source = Vec::new();
    source.push(reducer(first, second));

    loop {
        let first = match iter.next() {
            Some(f) => f,
            None => break,
        };

        let val = match iter.next() {
            Some(e) => reducer(first, e),
            None => first,
        };

        source.push(val);
    }

    let mut target = Vec::new();
    loop {
        let mut iter = source.into_iter();

        let first = iter.next().unwrap();
        let second = match iter.next() {
            Some(s) => s,
            None => return Some(first),
        };

        target.push(reducer(first, second));

        loop {
            let first = match iter.next() {
                Some(f) => f,
                None => break,
            };

            let val = match iter.next() {
                Some(e) => reducer(first, e),
                None => first,
            };

            target.push(val);
        }

        source = target;
        target = Vec::new();
    }
}

pub struct StringWriter {
    buf: Vec<u8>,
}

impl StringWriter {
    pub fn new() -> StringWriter {
        StringWriter {
            buf: Vec::with_capacity(8 * 1024),
        }
    }

    pub fn into_string(self) -> String {
        return unsafe { String::from_utf8_unchecked(self.buf) };
    }

    pub fn to_string(&self) -> String {
        return unsafe { String::from_utf8_unchecked(self.buf.clone()) };
    }
}

impl io::Write for StringWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let map_err = |err| io::Error::new(io::ErrorKind::InvalidInput, err);
        core::str::from_utf8(buf).map_err(map_err)?;
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl WriteColor for StringWriter {
    fn supports_color(&self) -> bool {
        false
    }

    fn set_color(&mut self, color: &ColorSpec) -> io::Result<()> {
        return Ok(());
    }

    fn reset(&mut self) -> io::Result<()> {
        return Ok(());
    }
}

pub struct RecordingWriter<W>
where
    W: io::Write,
{
    pub string: StringWriter,
    pub writer: W,
}

impl<W> RecordingWriter<W>
where
    W: io::Write,
{
    pub fn new(writer: W) -> Self {
        Self {
            string: StringWriter::new(),
            writer,
        }
    }
}

impl<W> io::Write for RecordingWriter<W>
where
    W: io::Write,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.string.write(buf).expect("should not fail");
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl<W> WriteColor for RecordingWriter<W>
where
    W: WriteColor,
{
    fn supports_color(&self) -> bool {
        false
    }

    fn set_color(&mut self, color: &ColorSpec) -> io::Result<()> {
        return Ok(());
    }

    fn reset(&mut self) -> io::Result<()> {
        return Ok(());
    }
}

pub struct Void {
    unused: (),
}

impl Void {
    pub fn new() -> Self {
        return Self { unused: () };
    }
}

impl io::Write for Void {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// https://tools.ietf.org/html/rfc3629
static UTF8_CHAR_WIDTH: [u8; 256] = [
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x1F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x3F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x5F
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
    1, // 0x7F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, // 0x9F
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, // 0xBF
    0, 0, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
    2, // 0xDF
    3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, // 0xEF
    4, 4, 4, 4, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, // 0xFF
];

pub struct Utf8Lossy {
    bytes: [u8],
}

impl Utf8Lossy {
    pub fn from_str(s: &str) -> &Utf8Lossy {
        Utf8Lossy::from_bytes(s.as_bytes())
    }

    pub fn from_bytes(bytes: &[u8]) -> &Utf8Lossy {
        // SAFETY: Both use the same memory layout, and UTF-8 correctness isn't required.
        unsafe { core::mem::transmute(bytes) }
    }

    pub fn chunks(&self) -> Utf8LossyChunksIter<'_> {
        Utf8LossyChunksIter {
            source: &self.bytes,
        }
    }
}

pub struct Utf8LossyChunksIter<'a> {
    source: &'a [u8],
}

pub struct Utf8LossyChunk<'a> {
    /// Sequence of valid chars.
    /// Can be empty between broken UTF-8 chars.
    pub valid: &'a str,
    /// Single broken char, empty if none.
    /// Empty iff iterator item is last.
    pub broken: &'a [u8],
}

impl<'a> Utf8LossyChunksIter<'a> {
    fn next(&mut self) -> Utf8LossyChunk<'a> {
        if self.source.is_empty() {
            return Utf8LossyChunk {
                valid: "",
                broken: self.source,
            };
        }

        const TAG_CONT_U8: u8 = 128;
        fn safe_get(xs: &[u8], i: usize) -> u8 {
            *xs.get(i).unwrap_or(&0)
        }

        let mut i = 0;
        while i < self.source.len() {
            let i_ = i;

            // SAFETY: `i` starts at `0`, is less than `self.source.len()`, and
            // only increases, so `0 <= i < self.source.len()`.
            let byte = unsafe { *self.source.get_unchecked(i) };
            i += 1;

            if byte < 128 {
            } else {
                let w = UTF8_CHAR_WIDTH[byte as usize];

                macro_rules! error {
                    () => {{
                        // SAFETY: We have checked up to `i` that source is valid UTF-8.
                        unsafe {
                            let r = Utf8LossyChunk {
                                valid: core::str::from_utf8_unchecked(&self.source[0..i_]),
                                broken: &self.source[i_..i],
                            };
                            self.source = &self.source[i..];
                            return r;
                        }
                    }};
                }

                match w {
                    2 => {
                        if safe_get(self.source, i) & 192 != TAG_CONT_U8 {
                            error!();
                        }
                        i += 1;
                    }
                    3 => {
                        match (byte, safe_get(self.source, i)) {
                            (0xE0, 0xA0..=0xBF) => (),
                            (0xE1..=0xEC, 0x80..=0xBF) => (),
                            (0xED, 0x80..=0x9F) => (),
                            (0xEE..=0xEF, 0x80..=0xBF) => (),
                            _ => {
                                error!();
                            }
                        }
                        i += 1;
                        if safe_get(self.source, i) & 192 != TAG_CONT_U8 {
                            error!();
                        }
                        i += 1;
                    }
                    4 => {
                        match (byte, safe_get(self.source, i)) {
                            (0xF0, 0x90..=0xBF) => (),
                            (0xF1..=0xF3, 0x80..=0xBF) => (),
                            (0xF4, 0x80..=0x8F) => (),
                            _ => {
                                error!();
                            }
                        }
                        i += 1;
                        if safe_get(self.source, i) & 192 != TAG_CONT_U8 {
                            error!();
                        }
                        i += 1;
                        if safe_get(self.source, i) & 192 != TAG_CONT_U8 {
                            error!();
                        }
                        i += 1;
                    }
                    _ => {
                        error!();
                    }
                }
            }
        }

        let r = Utf8LossyChunk {
            // SAFETY: We have checked that the entire source is valid UTF-8.
            valid: unsafe { core::str::from_utf8_unchecked(self.source) },
            broken: &[],
        };
        self.source = &[];
        r
    }
}

pub fn string_append_utf8_lossy(string: &mut String, bytes: &[u8]) {
    string.reserve(bytes.len());
    let mut iter = Utf8Lossy::from_bytes(bytes).chunks();

    const REPLACEMENT: &str = "\u{FFFD}";

    loop {
        let Utf8LossyChunk { valid, broken } = iter.next();
        string.push_str(valid);
        if !broken.is_empty() {
            string.push_str(REPLACEMENT);
        } else {
            return;
        }
    }
}

pub fn write_utf8_lossy(mut write: impl io::Write, bytes: &[u8]) -> io::Result<usize> {
    let mut iter = Utf8Lossy::from_bytes(bytes).chunks();

    const REPLACEMENT: &str = "\u{FFFD}";

    let mut total = 0;
    loop {
        let Utf8LossyChunk { valid, broken } = iter.next();
        write.write(valid.as_bytes())?;
        total += valid.len();
        if !broken.is_empty() {
            write.write(REPLACEMENT.as_bytes())?;
            total += REPLACEMENT.len();
        } else {
            return Ok(total);
        }
    }
}
