use crate::filedb::FileDb;
use crate::runtime::*;
use crate::util::*;
use crate::{compile, emit_err};
use std::collections::HashMap;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum InMessage {
    Run(HashMap<String, String>),
    Ecall(EcallResult),
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "type", content = "payload")]
pub enum OutMessage {
    Startup,
    Compiled,
    FileIds(HashMap<u32, String>),
    CompileError {
        rendered: String,
        errors: Vec<Error>,
    },
    InvalidInput(String),
    JumpTo(CodeLoc),
    Debug(String),
    Stdout(String),
    Stderr(String),
    Stdlog(String),
    Ecall(EcallExt),
}

#[rustfmt::skip] // rustfmt deletes the keyword async
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = JSON)]
    #[wasm_bindgen(catch)]
    pub fn stringify(val: JsValue) -> Result<JsValue, JsValue>;

    pub type RunEnv;

    #[wasm_bindgen(method)]
    pub async fn wait(this: &RunEnv, timeout: u32);
    #[wasm_bindgen(method)]
    pub fn send(this: &RunEnv, message: JsValue);
    #[wasm_bindgen(method)]
    pub fn recv(this: &RunEnv) -> JsValue;
}

#[wasm_bindgen]
pub async fn run(env: RunEnv) -> Result<(), JsValue> {
    use InMessage as In;
    use OutMessage as Out;

    let for_send = env.clone();
    let send = move |mes: Out| {
        let for_send: RunEnv = for_send.clone().unchecked_into();
        for_send.send(JsValue::from_serde(&mes).unwrap());
    };

    let global_send = send.clone();
    register_output(move |s| global_send(Out::Debug(s)));

    let recv = || -> Result<Option<In>, JsValue> {
        let js_value = env.recv();
        if js_value.is_undefined() || js_value.is_null() {
            return Ok(None);
        }

        let out = match js_value.into_serde::<In>() {
            Ok(o) => o,
            Err(e) => {
                send(Out::InvalidInput(stringify(js_value)?.as_string().unwrap()));
                return Ok(None);
            }
        };

        return Ok(Some(out));
    };

    let mut files = FileDb::new();
    let mut kernel: Option<Kernel> = None;

    send(Out::Startup);

    loop {
        debug!("running another iteration of loop...");

        while let Some(input) = recv()? {
            match input {
                In::Run(sources) => {
                    files = FileDb::new();
                    kernel = None;
                    for (name, contents) in sources {
                        files.add(&name, &contents).unwrap();
                    }

                    let program = match compile(&mut files) {
                        Ok(p) => p,
                        Err(errors) => {
                            let mut writer = StringWriter::new();
                            emit_err(&errors, &files, &mut writer);
                            let rendered = writer.to_string();
                            send(Out::CompileError { rendered, errors });
                            continue;
                        }
                    };

                    send(Out::Compiled);
                    kernel = Some(Kernel::new(&program));
                }
                In::Ecall(res) => {
                    let kernel = match &mut kernel {
                        Some(k) => k,
                        None => panic!("idk man"),
                    };

                    match kernel.resolve_result(res) {
                        Ok(()) => {}
                        Err(err) => {
                            let e_str = print_error(&err, &kernel.memory, &files);
                            send(Out::Stderr(e_str));
                            env.wait(0).await;
                            continue;
                        }
                    }
                }
            }
        }

        if let Some(kern) = &mut kernel {
            let result = kern.run_op_count(5000);

            for TS(tag, s) in &kern.events() {
                match tag {
                    WriteEvent::StdoutWrite => send(Out::Stdout(s.to_string())),
                    WriteEvent::StderrWrite => send(Out::Stderr(s.to_string())),
                    WriteEvent::StdlogWrite => send(Out::Stdlog(s.to_string())),
                }
            }

            let ecall_req = match result {
                Ok(RuntimeStatus::Running) => {
                    env.wait(1).await;
                    continue;
                }
                Ok(RuntimeStatus::Blocked(req)) => req,
                Ok(RuntimeStatus::Exited(code)) => {
                    kernel = None;
                    env.wait(0).await;
                    continue;
                }
                Err(e) => {
                    let e_str = print_error(&e, &kern.memory, &files);
                    send(Out::Stderr(e_str));
                    kernel = None;
                    env.wait(0).await;
                    continue;
                }
            };

            send(Out::Ecall(ecall_req));
        }

        env.wait(0).await;
    }
}