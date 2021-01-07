use crate::buckets::*;
use crate::filedb::*;
use crate::util::*;
use codespan_reporting::files::Files;
use std::collections::{HashMap, HashSet};

pub const CLOSING_CHAR: u8 = !0;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TokenKind<'a> {
    Ident(u32),
    TypeIdent(u32),
    IntLiteral(i32),
    StringLiteral(&'a str),
    CharLiteral(i8),

    Include(u32),
    IncludeSys(u32),
    MacroDef(u32),
    FuncMacroDef(u32),
    MacroDefEnd,
    Pragma(&'a str),

    Void,
    Char,
    Int,
    Long,
    Float,
    Double,
    Unsigned,
    Static,
    Signed,
    Struct,
    Union,
    Enum,
    Sizeof,
    Typedef,
    Volatile,

    If,
    Else,
    Do,
    While,
    For,
    Break,
    Continue,
    Return,
    Goto,

    Dot,
    DotDotDot,
    Arrow,
    Bang,
    Question,
    Tilde,
    Star,
    Slash,
    Plus,
    Dash,
    Percent,
    PlusPlus,
    DashDash,

    Eq,
    EqEq,
    Neq,
    Leq,
    Lt,
    LtLt, // <<
    Geq,
    Gt,
    GtGt, // >>
    Amp,
    AmpAmp,
    Line,     // |
    LineLine, // ||
    Caret,
    AmpEq,
    LineEq,
    CaretEq,
    PlusEq,
    DashEq,
    SlashEq,
    StarEq,
    PercentEq,
    LtLtEq,
    GtGtEq,

    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,

    Semicolon,
    Colon,
    Comma,

    Unimplemented,
    Case,
    Const,
    Default,
    Extern,
    Switch,
    Short,
}

lazy_static! {
    pub static ref RESERVED_KEYWORDS: HashMap<&'static str, TokenKind<'static>> = {
        let mut set = HashMap::new();
        set.insert("auto", TokenKind::Unimplemented);
        set.insert("break", TokenKind::Break);
        set.insert("case", TokenKind::Case);
        set.insert("char", TokenKind::Char);
        set.insert("const", TokenKind::Const);
        set.insert("continue", TokenKind::Continue);
        set.insert("default", TokenKind::Default);
        set.insert("do", TokenKind::Do);
        set.insert("double", TokenKind::Double);
        set.insert("else", TokenKind::Else);
        set.insert("enum", TokenKind::Enum);
        set.insert("extern", TokenKind::Extern);
        set.insert("float", TokenKind::Float);
        set.insert("for", TokenKind::For);
        set.insert("goto", TokenKind::Goto);
        set.insert("if", TokenKind::If);
        set.insert("inline", TokenKind::Unimplemented);
        set.insert("int", TokenKind::Int);
        set.insert("long", TokenKind::Long);
        set.insert("register", TokenKind::Unimplemented);
        set.insert("restrict", TokenKind::Unimplemented);
        set.insert("return", TokenKind::Return);
        set.insert("short", TokenKind::Short);
        set.insert("signed", TokenKind::Signed);
        set.insert("sizeof", TokenKind::Sizeof);
        set.insert("static", TokenKind::Static);
        set.insert("struct", TokenKind::Struct);
        set.insert("switch", TokenKind::Switch);
        set.insert("typedef", TokenKind::Typedef);
        set.insert("union", TokenKind::Union);
        set.insert("unsigned", TokenKind::Unsigned);
        set.insert("void", TokenKind::Void);
        set.insert("volatile", TokenKind::Unimplemented);
        set.insert("while", TokenKind::While);
        set.insert("_Alignas", TokenKind::Unimplemented);
        set.insert("_Alignof", TokenKind::Unimplemented);
        set.insert("_Atomic", TokenKind::Unimplemented);
        set.insert("_Bool", TokenKind::Unimplemented);
        set.insert("_Complex", TokenKind::Unimplemented);
        set.insert("_Generic", TokenKind::Unimplemented);
        set.insert("_Imaginary", TokenKind::Unimplemented);
        set.insert("_Noreturn", TokenKind::Unimplemented);
        set.insert("_Static_assert", TokenKind::Unimplemented);
        set.insert("_Thread_local", TokenKind::Unimplemented);
        set.insert("_Float16", TokenKind::Unimplemented);
        set.insert("_Float16x", TokenKind::Unimplemented);
        set.insert("_Float32", TokenKind::Unimplemented);
        set.insert("_Float32x", TokenKind::Unimplemented);
        set.insert("_Float64", TokenKind::Unimplemented);
        set.insert("_Float64x", TokenKind::Unimplemented);
        set.insert("_Float128", TokenKind::Unimplemented);
        set.insert("_Float128x", TokenKind::Unimplemented);
        set.insert("_Decimal32", TokenKind::Unimplemented);
        set.insert("_Decimal32x", TokenKind::Unimplemented);
        set.insert("_Decimal64", TokenKind::Unimplemented);
        set.insert("_Decimal64x", TokenKind::Unimplemented);
        set.insert("_Decimal128", TokenKind::Unimplemented);
        set.insert("_Decimal128x", TokenKind::Unimplemented);

        set
    };
}

#[derive(Debug, Clone, Copy)]
pub struct Token<'a> {
    pub kind: TokenKind<'a>,
    pub loc: CodeLoc,
}

impl<'a> Token<'a> {
    pub fn new(kind: TokenKind<'a>, range: core::ops::Range<usize>, file: u32) -> Self {
        Self {
            kind,
            loc: l(range.start as u32, range.end as u32, file),
        }
    }
}

#[inline]
pub fn invalid_token(file: u32, begin: usize, end: usize) -> Error {
    return error!(
        "invalid token",
        l(begin as u32, end as u32, file),
        "token found here"
    );
}

pub type TokenDb<'a> = HashMap<u32, &'a [Token<'a>]>;

const WHITESPACE: [u8; 2] = [b' ', b'\t'];
const CRLF: [u8; 2] = [b'\r', b'\n'];

pub fn lex_file<'b>(
    buckets: &impl Allocator<'b>,
    token_db: &mut TokenDb<'b>,
    symbols: &mut FileDb,
    file: u32,
) -> Result<&'b [Token<'b>], Error> {
    if let Some(toks) = token_db.get(&file) {
        return Ok(toks);
    }

    let mut incomplete = HashSet::new();
    let tokens = Lexer::new(file).lex_file(buckets, &mut incomplete, token_db, symbols)?;
    token_db.insert(file, tokens);
    return Ok(tokens);
}

pub struct Lexer<'a> {
    file: u32,
    current: usize,
    output: Vec<Token<'a>>,
}

impl<'b> Lexer<'b> {
    pub fn new(file: u32) -> Self {
        Self {
            file,
            current: 0,
            output: Vec::new(),
        }
    }

    pub fn lex_file(
        mut self,
        buckets: &impl Allocator<'b>,
        incomplete: &mut HashSet<u32>,
        token_db: &mut TokenDb<'b>,
        symbols: &mut FileDb,
    ) -> Result<&'b [Token<'b>], Error> {
        let bytes = symbols.source(self.file).unwrap().as_bytes();

        self.lex_macro(buckets, incomplete, token_db, symbols, bytes)?;

        let mut done = self.lex_macro_or_token(buckets, incomplete, token_db, symbols, bytes)?;

        while !done {
            done = self.lex_macro_or_token(buckets, incomplete, token_db, symbols, bytes)?;
        }

        return Ok(buckets.add_array(self.output));
    }

    pub fn lex_macro_or_token(
        &mut self,
        buckets: &impl Allocator<'b>,
        incomplete: &mut HashSet<u32>,
        token_db: &mut TokenDb<'b>,
        symbols: &mut FileDb,
        data: &[u8],
    ) -> Result<bool, Error> {
        loop {
            while self.peek_eqs(data, &WHITESPACE) {
                self.current += 1;
            }

            if self.peek_eq_series(data, &[b'/', b'/']) {
                self.current += 2;
                while self.peek_neq(data, b'\n') && self.peek_neq_series(data, &CRLF) {
                    self.current += 1;
                }
            } else if self.peek_eq_series(data, &[b'/', b'*']) {
                self.current += 2;
                while self.peek_neq_series(data, &[b'*', b'/']) {
                    self.current += 1;
                }

                self.current += 2;
                continue;
            }

            if self.peek_eq(data, b'\n') {
                self.current += 1;
            } else if self.peek_eq_series(data, &CRLF) {
                self.current += 2;
            } else {
                break;
            }

            self.lex_macro(buckets, incomplete, token_db, symbols, data)?;
        }

        if self.current == data.len() {
            return Ok(true);
        }

        let tok = self.lex_token(buckets, symbols, data)?;
        self.output.push(tok);
        return Ok(false);
    }

    pub fn lex_macro(
        &mut self,
        buckets: &impl Allocator<'b>,
        incomplete: &mut HashSet<u32>,
        token_db: &mut TokenDb<'b>,
        symbols: &mut FileDb,
        data: &[u8],
    ) -> Result<(), Error> {
        if self.peek_eq(data, b'#') {
            self.current += 1;
        } else {
            return Ok(());
        }

        // macros!
        let begin = self.current;
        while self.peek_neqs(data, &WHITESPACE) {
            self.current += 1;
        }

        let directive = unsafe { std::str::from_utf8_unchecked(&data[begin..self.current]) };
        match directive {
            "pragma" => {
                self.current += 1;
                let begin = self.current;
                while self.peek_neq(data, b'\n') && self.peek_neq_series(data, &CRLF) {
                    self.current += 1;
                }

                let pragma = unsafe { std::str::from_utf8_unchecked(&data[begin..self.current]) };
                let pragma = buckets.add_str(pragma);

                self.output.push(Token::new(
                    TokenKind::Pragma(pragma),
                    begin..self.current,
                    self.file,
                ));
                return Ok(());
            }
            "define" => {
                while self.peek_eqs(data, &WHITESPACE) {
                    self.current += 1;
                }

                let ident_begin = self.current;
                if ident_begin == data.len() {
                    return Err(error!(
                        "unexpected end of file",
                        l(ident_begin as u32, begin as u32, self.file),
                        "EOF found here"
                    ));
                }

                while self.peek_check(data, is_ident_char) {
                    self.current += 1;
                }

                // Don't add the empty string
                if self.current - ident_begin == 0 {
                    return Err(error!(
                        "expected an identifer for macro declaration",
                        l(ident_begin as u32, ident_begin as u32 + 1, self.file),
                        "This should be an identifier"
                    ));
                }

                let id = symbols.translate_add(ident_begin..self.current, self.file);

                macro_rules! consume_whitespace_macro {
                    () => {
                        while self.peek_eqs(data, &WHITESPACE) {
                            self.current += 1;
                        }

                        if self.current == data.len() {
                            break;
                        }

                        if self.peek_eq_series(data, &[b'/', b'/']) {
                            self.current += 2;
                            while self.peek_neq(data, b'\n') && self.peek_neq_series(data, &CRLF) {
                                self.current += 1;
                            }
                        } else if self.peek_eq_series(data, &[b'/', b'*']) {
                            self.current += 2;
                            while self.peek_neq_series(data, &[b'*', b'/']) {
                                self.current += 1;
                            }

                            self.current += 2;
                            continue;
                        }

                        if self.peek_eq(data, b'\n') {
                            break;
                        } else if self.peek_eq_series(data, &CRLF) {
                            break;
                        } else if self.peek_eq_series(data, &[b'\\', b'\n']) {
                            self.current += 2;
                            continue;
                        } else if self.peek_eq_series(data, &[b'\\', b'\r', b'\n']) {
                            self.current += 3;
                            continue;
                        }
                    };
                }

                if !self.peek_eq(data, b'(') {
                    self.output.push(Token::new(
                        TokenKind::MacroDef(id),
                        begin..self.current,
                        self.file,
                    ));

                    loop {
                        consume_whitespace_macro!();
                        let tok = self.lex_token(buckets, symbols, data)?;
                        self.output.push(tok);
                    }

                    self.output.push(Token::new(
                        TokenKind::MacroDefEnd,
                        self.current..self.current,
                        self.file,
                    ));

                    return Ok(());
                }

                self.output.push(Token::new(
                    TokenKind::FuncMacroDef(id),
                    begin..self.current,
                    self.file,
                ));

                loop {
                    consume_whitespace_macro!();
                    let tok = self.lex_token(buckets, symbols, data)?;
                    self.output.push(tok);
                }

                self.output.push(Token::new(
                    TokenKind::MacroDefEnd,
                    self.current..self.current,
                    self.file,
                ));

                return Ok(());
            }
            "include" => {
                while self.peek_eqs(data, &WHITESPACE) {
                    self.current += 1;
                }

                if self.peek_eq(data, b'"') {
                    self.current += 1;
                    let name_begin = self.current;
                    while self.peek_neq(data, b'"') {
                        self.current += 1;
                    }

                    let name_end = self.current;
                    self.current += 1;
                    if !self.peek_eq(data, b'\n') && !self.peek_eq_series(data, &CRLF) {
                        return Err(expected_newline("include", begin, self.current, self.file));
                    }

                    let map_err = |err| {
                        error!(
                            "error finding file",
                            l(begin as u32, self.current as u32, self.file),
                            format!("got error '{}'", err)
                        )
                    };
                    let include_name =
                        unsafe { core::str::from_utf8_unchecked(&data[name_begin..name_end]) };
                    let include_id = symbols
                        .add_from_include(include_name, self.file)
                        .map_err(map_err)?;
                    self.output.push(Token::new(
                        TokenKind::Include(include_id),
                        begin..self.current,
                        self.file,
                    ));

                    if incomplete.contains(&include_id) {
                        return Err(error!(
                            "include cycle detected",
                            l(begin as u32, self.current as u32, self.file),
                            "found here"
                        ));
                    }

                    if token_db.contains_key(&include_id) {
                        return Ok(());
                    }

                    incomplete.insert(include_id);
                    let toks =
                        Lexer::new(include_id).lex_file(buckets, incomplete, token_db, symbols)?;
                    token_db.insert(include_id, toks);
                    incomplete.remove(&include_id);
                    return Ok(());
                } else if self.peek_eq(data, b'<') {
                    self.current += 1;
                    let name_begin = self.current;

                    while self.peek_neqs(data, &[b'>', b'\n']) && self.peek_neq_series(data, &CRLF)
                    {
                        self.current += 1;
                    }

                    let name_end = self.current;

                    if b'>' != self.expect(data)? {
                        return Err(error!(
                            "expected a '>'",
                            l((self.current - 1) as u32, self.current as u32, self.file),
                            "this should be a '>'"
                        ));
                    }

                    let sys_file =
                        unsafe { core::str::from_utf8_unchecked(&data[name_begin..name_end]) };

                    if !self.peek_eq(data, b'\n') && !self.peek_eq_series(data, &CRLF) {
                        return Err(expected_newline("include", begin, self.current, self.file));
                    }

                    let id_opt = symbols.file_names.get(sys_file).map(|i| Ok(*i));
                    let sys_lib = SYS_LIBS.get(sys_file).ok_or_else(|| {
                        return error!(
                            "library header file not found",
                            l(begin as u32, self.current as u32, self.file),
                            "include found here"
                        );
                    })?;
                    let sys_header = unsafe { core::str::from_utf8_unchecked(sys_lib.header) };
                    let sys_impl = unsafe { core::str::from_utf8_unchecked(sys_lib.lib) };
                    let id = id_opt.unwrap_or_else(|| {
                        let id = symbols.add(&sys_file, sys_header).unwrap();
                        let toks =
                            Lexer::new(id).lex_file(buckets, incomplete, token_db, symbols)?;
                        token_db.insert(id, toks);

                        let sys_lib = "libs/".to_string() + sys_file + ".c";
                        let lib_id = symbols.add(&sys_lib, sys_impl).unwrap();
                        let lib_toks = Lexer::new(lib_id).lex_file(
                            buckets,
                            &mut HashSet::new(),
                            token_db,
                            symbols,
                        )?;
                        token_db.insert(lib_id, lib_toks);

                        return Ok(id);
                    })?;

                    self.output.push(Token::new(
                        TokenKind::IncludeSys(id),
                        begin..self.current,
                        self.file,
                    ));

                    return Ok(());
                } else {
                    return Err(error!(
                        "expected a '<' or '\"' here",
                        l(begin as u32, self.current as u32, self.file),
                        "directive found here"
                    ));
                }
            }
            _ => {
                return Err(error!(
                    "invalid compiler directive",
                    l(begin as u32, self.current as u32, self.file),
                    "directive found here"
                ));
            }
        }
    }

    pub fn lex_token(
        &mut self,
        buckets: &impl Allocator<'b>,
        symbols: &mut FileDb,
        data: &[u8],
    ) -> Result<Token<'b>, Error> {
        let begin = self.current;
        self.current += 1;

        macro_rules! ret_tok {
            ($arg1:expr) => {{
                return Ok(Token::new($arg1, begin..self.current, self.file));
            }};
        }

        match data[begin] {
            x if (x >= b'A' && x <= b'Z') || (x >= b'a' && x <= b'z') || x == b'_' => {
                while self.peek_check(data, is_ident_char) {
                    self.current += 1;
                }

                let word = unsafe { std::str::from_utf8_unchecked(&data[begin..self.current]) };
                if let Some(kind) = RESERVED_KEYWORDS.get(word) {
                    ret_tok!(*kind);
                }

                let id = symbols.translate_add(begin..self.current, self.file);
                ret_tok!(TokenKind::Ident(id));
            }

            x if (x >= b'0' && x <= b'9') => {
                let mut int_value: i32 = (x - b'0') as i32;
                while self.peek_check(data, |b| b >= b'0' && b <= b'9') {
                    int_value *= 10;
                    int_value += (data[self.current] - b'0') as i32;
                    self.current += 1;
                }

                ret_tok!(TokenKind::IntLiteral(int_value));
            }

            b'\"' => {
                let mut cur = self.lex_character(b'\"', data)?;
                let mut chars = Vec::new();
                while cur != CLOSING_CHAR {
                    chars.push(cur);
                    cur = self.lex_character(b'\"', data)?;
                }

                let string = unsafe { std::str::from_utf8_unchecked(&chars) };
                let string = buckets.add_str(string);
                ret_tok!(TokenKind::StringLiteral(string));
            }

            b'\'' => {
                let byte = self.lex_character(b'\'', data)?;
                if byte == CLOSING_CHAR {
                    return Err(error!(
                        "empty character literal",
                        l(begin as u32, self.current as u32, self.file),
                        "found here"
                    ));
                }

                let closing = self.expect(data)?;
                if closing != b'\'' {
                    return Err(error!(
                        "expected closing single quote",
                        l(begin as u32, self.current as u32, self.file),
                        "this should be a closing single quote"
                    ));
                }

                ret_tok!(TokenKind::CharLiteral(byte as i8));
            }

            b'{' => ret_tok!(TokenKind::LBrace),
            b'}' => ret_tok!(TokenKind::RBrace),
            b'(' => ret_tok!(TokenKind::LParen),
            b')' => ret_tok!(TokenKind::RParen),
            b'[' => ret_tok!(TokenKind::LBracket),
            b']' => ret_tok!(TokenKind::RBracket),
            b'~' => ret_tok!(TokenKind::Tilde),
            b';' => ret_tok!(TokenKind::Semicolon),
            b':' => ret_tok!(TokenKind::Colon),
            b',' => ret_tok!(TokenKind::Comma),
            b'?' => ret_tok!(TokenKind::Question),

            b'.' => {
                if self.peek_eq(data, b'.') {
                    self.current += 1;
                    if self.peek_eq(data, b'.') {
                        self.current += 1;
                        ret_tok!(TokenKind::DotDotDot);
                    }

                    return Err(invalid_token(self.file, begin, self.current));
                }

                ret_tok!(TokenKind::Dot);
            }
            b'+' => {
                if self.peek_eq(data, b'+') {
                    self.current += 1;
                    ret_tok!(TokenKind::PlusPlus);
                } else if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::PlusEq);
                } else {
                    ret_tok!(TokenKind::Plus);
                }
            }
            b'-' => {
                if self.peek_eq(data, b'-') {
                    self.current += 1;
                    ret_tok!(TokenKind::DashDash);
                } else if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::DashEq);
                } else if self.peek_eq(data, b'>') {
                    self.current += 1;
                    ret_tok!(TokenKind::Arrow);
                } else {
                    ret_tok!(TokenKind::Dash);
                }
            }
            b'/' => {
                if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::SlashEq);
                } else {
                    ret_tok!(TokenKind::Slash);
                }
            }
            b'*' => {
                if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::StarEq);
                } else {
                    ret_tok!(TokenKind::Star);
                }
            }
            b'%' => {
                if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::PercentEq);
                } else {
                    ret_tok!(TokenKind::Percent);
                }
            }
            b'>' => {
                if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::Geq);
                } else if self.peek_eq(data, b'>') {
                    self.current += 1;
                    if self.peek_eq(data, b'=') {
                        self.current += 1;
                        ret_tok!(TokenKind::GtGtEq);
                    }
                    ret_tok!(TokenKind::GtGt);
                } else {
                    ret_tok!(TokenKind::Gt);
                }
            }
            b'<' => {
                if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::Leq);
                } else if self.peek_eq(data, b'<') {
                    self.current += 1;
                    if self.peek_eq(data, b'=') {
                        self.current += 1;
                        ret_tok!(TokenKind::LtLtEq);
                    }
                    ret_tok!(TokenKind::LtLt);
                } else {
                    ret_tok!(TokenKind::Lt);
                }
            }
            b'!' => {
                if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::Neq);
                } else {
                    ret_tok!(TokenKind::Bang);
                }
            }
            b'=' => {
                if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::EqEq);
                } else {
                    ret_tok!(TokenKind::Eq);
                }
            }
            b'|' => {
                if self.peek_eq(data, b'|') {
                    self.current += 1;
                    ret_tok!(TokenKind::LineLine);
                } else if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::LineEq);
                } else {
                    ret_tok!(TokenKind::Line);
                }
            }
            b'&' => {
                if self.peek_eq(data, b'&') {
                    self.current += 1;
                    ret_tok!(TokenKind::AmpAmp);
                } else if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::AmpEq);
                } else {
                    ret_tok!(TokenKind::Amp);
                }
            }
            b'^' => {
                if self.peek_eq(data, b'=') {
                    self.current += 1;
                    ret_tok!(TokenKind::CaretEq);
                } else {
                    ret_tok!(TokenKind::Caret);
                }
            }

            x => {
                return Err(invalid_token(self.file, begin, self.current));
            }
        }
    }

    #[inline]
    pub fn expect(&mut self, data: &[u8]) -> Result<u8, Error> {
        if self.current == data.len() {
            return Err(error!("unexpected end of file"));
        }

        let cur = self.current;
        self.current += 1;
        return Ok(data[cur]);
    }

    #[inline]
    pub fn peek_expect(&self, data: &[u8]) -> Result<u8, Error> {
        if self.current == data.len() {
            return Err(error!("unexpected end of file"));
        }

        return Ok(data[self.current]);
    }

    #[inline]
    pub fn peek_check(&self, data: &[u8], checker: impl Fn(u8) -> bool) -> bool {
        if self.current >= data.len() {
            return false;
        }

        return checker(data[self.current]);
    }

    #[inline]
    pub fn peek_eq(&self, data: &[u8], byte: u8) -> bool {
        if self.current >= data.len() {
            return false;
        }

        return data[self.current] == byte;
    }

    pub fn peek_neq_series(&self, data: &[u8], bytes: &[u8]) -> bool {
        let byte_len = bytes.len();
        if self.current + bytes.len() > data.len() {
            return false;
        }

        let eq_slice = &data[(self.current)..(self.current + byte_len)];
        return eq_slice != bytes;
    }

    pub fn peek_eq_series(&self, data: &[u8], bytes: &[u8]) -> bool {
        let byte_len = bytes.len();
        if self.current + bytes.len() > data.len() {
            return false;
        }

        let eq_slice = &data[(self.current)..(self.current + byte_len)];
        return eq_slice == bytes;
    }

    #[inline]
    pub fn peek_neq(&self, data: &[u8], byte: u8) -> bool {
        if self.current >= data.len() {
            return false;
        }

        return data[self.current] != byte;
    }

    #[inline]
    pub fn peek_neqs(&self, data: &[u8], bytes: &[u8]) -> bool {
        if self.current >= data.len() {
            return false;
        }

        for byte in bytes {
            if data[self.current] == *byte {
                return false;
            }
        }

        return true;
    }

    #[inline]
    pub fn peek_eqs(&self, data: &[u8], bytes: &[u8]) -> bool {
        if self.current >= data.len() {
            return false;
        }

        for byte in bytes {
            if data[self.current] == *byte {
                return true;
            }
        }

        return false;
    }

    pub fn lex_character(&mut self, surround: u8, data: &[u8]) -> Result<u8, Error> {
        loop {
            let cur_b = self.expect(data)?;
            let cur: char = cur_b.into();

            if !cur.is_ascii() {
                return Err(error!(
                    "character is not valid ascii",
                    l(self.current as u32 - 1, self.current as u32, self.file),
                    "invalid character literal here"
                ));
            }

            if cur_b == surround {
                return Ok(CLOSING_CHAR);
            }

            if cur_b == b'\n' || cur_b == b'\r' {
                if surround == b'\"' {
                    return Err(error!(
                        "invalid character found when parsing string literal",
                        l(self.current as u32 - 1, self.current as u32, self.file),
                        "invalid character here"
                    ));
                } else {
                    return Err(error!(
                        "invalid character found when parsing character literal",
                        l(self.current as u32 - 1, self.current as u32, self.file),
                        "invalid character here"
                    ));
                }
            }

            if cur_b != b'\\' {
                return Ok(cur_b);
            }

            match self.expect(data)? {
                b'n' => return Ok(b'\n'),
                b'\n' => continue,
                b'\'' => return Ok(b'\''),
                b'"' => return Ok(b'"'),
                b'0' => return Ok(b'\0'),
                _ => {
                    return Err(error!(
                        "invalid escape sequence",
                        l(self.current as u32 - 2, self.current as u32, self.file),
                        "invalid escape sequence here"
                    ))
                }
            }
        }
    }
}

pub fn is_ident_char(cur: u8) -> bool {
    (cur >= b'a' && cur <= b'z')
        || (cur >= b'A' && cur <= b'Z')
        || cur == b'_'
        || (cur >= b'0' && cur <= b'9')
}

#[inline]
pub fn expected_newline(
    directive_name: &'static str,
    begin: usize,
    current: usize,
    file: u32,
) -> Error {
    return error!(
        &format!("expected newline after {} directive", directive_name),
        l(begin as u32, current as u32, file),
        "directive here"
    );
}
