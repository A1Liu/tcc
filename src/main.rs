#![allow(dead_code)]
#![allow(unused_variables)]

#[macro_use]
extern crate lazy_static;

#[macro_use]
mod util;

mod assembler;
mod ast;
mod buckets;
mod filedb;
mod interpreter;
mod lexer;
mod parser;
mod runtime;
mod type_checker;

#[cfg(test)]
mod test;

use crate::filedb::{FileDb, FileDbRef};
use codespan_reporting::files::Files;
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream, WriteColor};
use core::mem;
use runtime::{DefaultIO, RuntimeIO};
use std::env;
use std::fs::read_to_string;
use util::{r, CodeLoc, Error, Range};

#[derive(Clone, Copy)]
pub struct Environment<'a> {
    pub buckets: buckets::BucketListRef<'a>,
    pub files: FileDbRef<'a>,
}

fn run<'a>(env: Environment<'a>, runtime_io: impl RuntimeIO) -> Result<i32, Error> {
    let mut symbols = lexer::Symbols::new();
    let files = env.files.iter();
    let files = files.map(|id| (id, env.files.source(id).unwrap()));

    let mut token_lists = Vec::new();
    files
        .map(|(id, source)| lexer::lex_file(env.buckets, &mut symbols, id, source))
        .try_fold((), |_, t| -> Result<(), Error> { Ok(token_lists.push(t?)) })?;

    let mut end = env.buckets;
    while let Some(n) = end.next() {
        end = n;
    }
    end = end.force_next();

    let iter = token_lists.into_iter().enumerate();
    let mut iter = iter.map(|(file, tokens)| {
        let ast = parser::parse_tokens(end, file as u32, &tokens)?;
        let typed_ast = type_checker::check_file(end, file as u32, &ast)?;
        Ok(typed_ast)
    });

    let mut assembler = assembler::Assembler::new();
    iter.try_fold((), |prev, tenv| -> Result<(), Error> {
        assembler.add_file(tenv?)?;
        Ok(())
    })?;

    let program = assembler.assemble(&env, &symbols)?;
    mem::drop(env);
    mem::drop(symbols);

    let mut runtime = interpreter::Runtime::new(runtime_io);
    Ok(runtime.run_program(program))
}

fn run_on_file(
    runtime_io: impl RuntimeIO,
    filename: &str,
    writer: &mut impl WriteColor,
) -> Result<i32, Error> {
    let buckets = buckets::BucketList::new();
    let mut files = FileDb::new();
    let input = read_to_string(&filename).unwrap();
    let file_id = files.add(buckets, filename, &input);
    let files = FileDbRef::new(buckets, &files);

    let env = Environment { buckets, files };

    match run(env, runtime_io) {
        Ok(x) => return Ok(x),
        Err(err) => {
            let config = codespan_reporting::term::Config::default();
            codespan_reporting::term::emit(writer, &config, &env.files, &err.diagnostic())
                .expect("why did this fail?");
            return Err(err);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let buckets = buckets::BucketList::new();
    let mut files = FileDb::new();

    let writer = StandardStream::stderr(ColorChoice::Always);
    let runtime_io = DefaultIO::new();

    for arg in args.iter().skip(1) {
        let filename = buckets.add_str(&arg);
        let input = buckets.add_str(&read_to_string(&filename).unwrap());
        files.add(buckets, filename, input);
    }

    let files = FileDbRef::new(buckets, &files);
    let env = Environment { buckets, files };

    match run(env, runtime_io) {
        Err(err) => {
            let config = codespan_reporting::term::Config::default();

            codespan_reporting::term::emit(
                &mut writer.lock(),
                &config,
                &env.files,
                &err.diagnostic(),
            )
            .expect("why did this fail?");
        }
        Ok(ret_code) => {
            eprintln!("TCI: return code was {}", ret_code);
            std::process::exit(ret_code as i32);
        }
    }
}
