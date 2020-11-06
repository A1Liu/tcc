use crate::lexer::*;
use crate::util::*;
use std::collections::HashMap;

pub fn preprocess_file<'a>(tokens: &'a [Token<'a>]) -> Result<Vec<Token<'a>>, Error> {
    let mut toks = tokens.iter();
    let mut macros: HashMap<u32, Macro<'a>> = HashMap::new();
    let mut output = Vec::new();

    let expect = || error!("expected token");

    while let Some(mut tok) = toks.next() {
        let id = match tok.kind {
            TokenKind::Ident(id) | TokenKind::TypeIdent(id) => id,
            TokenKind::MacroDef(id) => {
                let macro_begin = tok.loc;
                let mut macro_toks = Vec::new();
                tok = toks.next().ok_or_else(expect)?;
                while tok.kind != TokenKind::MacroDefEnd {
                    macro_toks.push(*tok);
                    tok = toks.next().ok_or_else(expect)?;
                }

                macros.insert(
                    id,
                    Macro {
                        kind: MacroKind::Value(macro_toks),
                        loc: l_from(macro_begin, tok.loc),
                    },
                );

                continue;
            }
            TokenKind::FuncMacroDef(id) => {
                let macro_begin = tok.loc;
                let mut macro_toks = Vec::new();
                tok = toks.next().ok_or_else(expect)?;
                while tok.kind != TokenKind::MacroDefEnd {
                    macro_toks.push(*tok);
                    tok = toks.next().ok_or_else(expect)?;
                }

                let macro_def = parse_func_macro(l_from(macro_begin, tok.loc), &macro_toks)?;
                macros.insert(id, macro_def);

                continue;
            }
            _ => {
                output.push(*tok);
                continue;
            }
        };

        let macro_def = match macros.get(&id) {
            Some(def) => def,
            None => {
                output.push(*tok);
                continue;
            }
        };

        let (macro_params, macro_toks) = match &macro_def.kind {
            MacroKind::Marker => {
                return Err(error!(
                    "used marker macro in code",
                    macro_def.loc, "macro defined here", tok.loc, "used here"
                ))
            }
            MacroKind::Value(toks) => {
                output.extend_from_slice(toks);
                continue;
            }
            MacroKind::Func { params, toks } => (params, toks),
        };

        let rparen_tok = toks.next().ok_or_else(expect)?;
        match rparen_tok.kind {
            TokenKind::LParen => {}
            _ => {
                return Err(error!(
                    "expected a left paren '(' because of function macro invokation",
                    tok.loc, "macro used here", macro_def.loc, "macro defined here"
                ));
            }
        }

        let mut actual_params = Vec::new();
        let mut paren_count = 0;
        let mut current_tok = toks.next().ok_or_else(expect)?;

        if current_tok.kind != TokenKind::RParen {
            loop {
                let mut current_param = Vec::new();
                while paren_count != 0
                    || (current_tok.kind != TokenKind::RParen
                        && current_tok.kind != TokenKind::Comma)
                {
                    current_param.push(*current_tok);
                    match current_tok.kind {
                        TokenKind::LParen => paren_count += 1,
                        TokenKind::RParen => paren_count -= 1,
                        _ => {}
                    }

                    current_tok = toks.next().ok_or_else(expect)?;
                }

                actual_params.push(current_param);
                if current_tok.kind == TokenKind::RParen {
                    break;
                }
            }
        }

        if macro_params.len() != actual_params.len() {
            return Err(error!(
                "provided wrong number of arguments to macro",
                macro_def.loc,
                format!(
                    "macro defined here (takes in {} arguments)",
                    macro_params.len()
                ),
                l_from(tok.loc, current_tok.loc),
                format!(
                    "macro used here (passed in {} arguments)",
                    actual_params.len()
                )
            ));
        }

        let mut params_hash = HashMap::new();
        for (idx, param) in actual_params.into_iter().enumerate() {
            params_hash.insert(macro_params[idx].0, param);
        }

        let mut expanded = expand_macro(macro_toks, params_hash);
        output.append(&mut expanded);
    }

    return Ok(output);
}

#[derive(Debug, Clone)]
pub enum MacroKind<'a> {
    Func {
        params: Vec<(u32, CodeLoc)>,
        toks: Vec<Token<'a>>,
    },
    Value(Vec<Token<'a>>),
    Marker,
}

#[derive(Debug, Clone)]
pub struct Macro<'a> {
    kind: MacroKind<'a>,
    loc: CodeLoc,
}

pub fn parse_func_macro<'a>(loc: CodeLoc, macro_def: &[Token<'a>]) -> Result<Macro<'a>, Error> {
    let mut current = 0;
    let mut params = Vec::new();

    let expect = || error!("expected token");

    let mut tok = macro_def.get(current).ok_or_else(expect)?;
    debug_assert!(tok.kind == TokenKind::LParen);

    current += 1;
    tok = macro_def.get(current).ok_or_else(expect)?;
    loop {
        let id = match tok.kind {
            TokenKind::Ident(id) => id,
            TokenKind::TypeIdent(id) => id,
            _ => {
                return Err(error!(
                    "expected a function macro parameter",
                    tok.loc, "this should be an identifier"
                ))
            }
        };

        params.push((id, tok.loc));

        current += 1;
        tok = macro_def.get(current).ok_or_else(expect)?;
        match tok.kind {
            TokenKind::Comma => {}
            TokenKind::RParen => break,
            _ => {
                return Err(error!(
                    "expected a ')' to end macro parameters or a comma",
                    tok.loc, "this should be ')' or ','"
                ))
            }
        }

        current += 1;
        tok = macro_def.get(current).ok_or_else(expect)?;
    }

    current += 1;
    let mut toks = Vec::new();
    toks.extend_from_slice(&macro_def[current..]);

    return Ok(Macro {
        kind: MacroKind::Func { params, toks },
        loc,
    });
}

pub fn expand_macro<'a>(
    macro_def: &[Token<'a>],
    params: HashMap<u32, Vec<Token<'a>>>,
) -> Vec<Token<'a>> {
    let mut from = Vec::new();

    for tok in macro_def {
        match &tok.kind {
            TokenKind::TypeIdent(id) | TokenKind::Ident(id) => {
                if let Some(expand) = params.get(id) {
                    from.extend_from_slice(expand);
                } else {
                    from.push(*tok);
                }
            }
            x => from.push(*tok),
        }
    }

    return from;
}
