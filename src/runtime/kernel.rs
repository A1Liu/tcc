use super::error::*;
use super::fs::*;
use super::interpreter::*;
use super::memory::*;
use super::types::*;
use crate::util::*;
use core::mem;

#[derive(Debug, Clone, Copy)]
pub enum IRtStat {
    // internal runtime status
    Running,
    Blocked,
    Exited(i32),
}

#[derive(Debug, Clone, Copy)]
pub enum KernStat {
    Running,
    Errored(u32),
}

pub struct Process {
    pub memory: Memory,
    pub status: IRtStat,
}

impl Process {
    pub fn new(binary: &BinaryData) -> Self {
        Self {
            memory: Memory::new(binary),
            status: IRtStat::Running,
        }
    }
}

pub struct Kernel {
    pub files: FileSystem,
    pub in_begin: usize,
    pub input: String,
    pub output: TaggedMultiArray<WriteEvent, u8>,
    // pub pipes: TaggedMultiVec<usize, u8>,
    pub processes: TaggedMultiVec<Process, FdKind>,

    pub term_proc: u32,
    pub current_proc: u32,
    pub current_proc_op_count: u32,
    pub active_count: u32,
}

const PROC_MAX_OP_COUNT: u32 = 5000;

impl Kernel {
    pub fn new(files: Vec<(String, u32, Vec<u8>)>) -> Self {
        Self {
            files: FileSystem::new(files),
            in_begin: 0,
            input: String::new(),
            output: TaggedMultiArray::new(),

            processes: TaggedMultiVec::new(),

            term_proc: !0,
            current_proc: !0,
            current_proc_op_count: 0,
            active_count: 0,
        }
    }

    pub fn loc(&self) -> CodeLoc {
        if self.current_proc == !0 {
            return NO_FILE;
        }

        let tag = self.processes.get(self.current_proc as usize).unwrap().tag;
        return tag.memory.loc;
    }

    pub fn cur_mem(&self) -> Option<&Memory> {
        if self.current_proc == !0 {
            return None;
        }

        let tag = &self.processes.get(self.current_proc as usize).unwrap().tag;
        return Some(&tag.memory);
    }

    pub fn load_program(&mut self, binary: &BinaryData) -> u32 {
        if self.term_proc != !0 {
            let mut prev = self.processes.get_mut(self.term_proc as usize).unwrap();
            prev.tag_mut().status = IRtStat::Exited(1);
        }

        self.term_proc = self.processes.len() as u32;
        if self.current_proc == !0 {
            self.current_proc = self.term_proc;
        }

        let (i, o, proc) = (FdKind::TermIn, FdKind::TermOut, Process::new(binary));

        self.in_begin = 0;
        self.input.clear();
        mem::drop(mem::replace(&mut self.output, TaggedMultiArray::new()));
        self.processes.push(proc, vec![i, o, o, o]);
        self.active_count += 1;
        return self.term_proc;
    }

    // pub fn run_debug(&mut self, binary: &BinaryData, files: &FileDb) -> Result<i32, IError> {
    //     loop {
    //         let mut proc = self.processes.get_mut(proc_id as usize).unwrap();

    //         let memory = &mut proc.tag_mut().memory;
    //         if memory.loc != NO_FILE {
    //             println!("{}", files.loc_to_string(memory.loc));
    //         }

    //         println!("{:?}", memory.expr_stack);
    //         let (count, res) = run_op_count(memory, !0);
    //         self.current_proc_op_count += count;
    //         println!("{:?}\n", memory.expr_stack);

    //         if let Some(ecall) = res? {
    //             if let RuntimeStatus::Exited(e) = self.ecall(self.current_proc, ecall)? {
    //                 return Ok(e);
    //             }
    //         }
    //     }
    // }

    pub fn run(&mut self, binary: &BinaryData) -> Result<i32, IError> {
        let proc_id = self.load_program(binary);

        loop {
            let proc = self.processes.get_mut(proc_id as usize).unwrap();
            if let IRtStat::Exited(c) = proc.tag().status {
                return Ok(c);
            }

            self.run_op_count(!0)?;
        }
    }

    pub fn run_op_count(&mut self, mut count: u32) -> Result<RuntimeStatus, IError> {
        while count > 0 && self.active_count != 0 {
            let mut proc = match self.processes.get_mut(self.current_proc as usize) {
                Some(p) => p,
                None => {
                    return Err(ierror!(
                        "NoProcesses",
                        "tried to run kernel with no processes (this is a bug in TCI)"
                    ))
                }
            };

            if let IRtStat::Exited(_) = proc.tag().status {
                self.current_proc_op_count = 0;
                self.current_proc += 1;
                if self.current_proc as usize == self.processes.len() {
                    self.current_proc = 0;
                }
                continue;
            }

            let ops_allowed = core::cmp::min(count, PROC_MAX_OP_COUNT - self.current_proc_op_count);
            let (ran_count, res) = run_op_count(&mut proc.tag_mut().memory, ops_allowed);
            self.current_proc_op_count += ran_count;
            count -= ran_count;

            match res {
                Err(e) => {
                    proc.tag_mut().status = IRtStat::Exited(1);
                    self.active_count -= 1;
                    return Err(e);
                }
                Ok(Some(ecall)) => {
                    let val = self.ecall(self.current_proc, ecall);

                    match &val {
                        Ok(RuntimeStatus::Exited(e)) => self.active_count -= 1,
                        Err(e) => {
                            self.active_count -= 1;
                            let mut proc =
                                self.processes.get_mut(self.current_proc as usize).unwrap();
                            proc.tag_mut().status = IRtStat::Exited(1);
                        }
                        Ok(x) => {}
                    }

                    return val;
                }
                Ok(None) => {}
            }

            self.current_proc_op_count = 0;
            self.current_proc += 1;
            if self.current_proc as usize == self.processes.len() {
                self.current_proc = 0;
            }
        }

        if count == 0 {
            return Ok(RuntimeStatus::Running);
        } else {
            return Ok(RuntimeStatus::Exited(0));
        }
    }

    pub fn resolve_result(&mut self, res: EcallResult) -> Result<(), IError> {
        return Ok(());
    }

    #[inline]
    pub fn ecall(&mut self, proc: u32, req: EcallExt) -> Result<RuntimeStatus, IError> {
        let mut proc = self.processes.get_mut(proc as usize).unwrap();

        match req {
            EcallExt::Exit(exit) => {
                proc.tag_mut().status = IRtStat::Exited(exit);
                return Ok(RuntimeStatus::Exited(exit));
            }

            EcallExt::OpenFd { name, open_mode } => {
                let id = match open_mode {
                    OpenMode::Read => self.files.open(&name),
                    OpenMode::Create => self.files.open_create(&name),
                    OpenMode::CreateClear => self.files.open_create_clear(&name),
                };

                let err = match id {
                    Err(e) => e,
                    Ok(idx) => {
                        let len = proc.len() as u64;
                        proc.tag_mut().memory.push(len);
                        proc.push(FdKind::FileSys(idx));
                        return Ok(RuntimeStatus::Blocked(EcallExt::OpenFd { name, open_mode }));
                    }
                };

                proc.tag_mut().memory.push(err.to_u64());
                return Ok(RuntimeStatus::Running);
            }

            EcallExt::ReadFd {
                len,
                buf,
                begin,
                fd,
            } => {
                let to_ret = match proc.get(fd as usize) {
                    None => EcallError::DoesntExist.to_u64(),
                    Some(FdKind::TermIn) => {
                        let end = core::cmp::min(self.input.len(), len as usize);
                        let bytes = &self.input.as_bytes()[(self.in_begin as usize)..end];
                        proc.tag_mut().memory.write_bytes(buf, bytes)?;

                        let begin = self.in_begin as usize;
                        self.in_begin = if end == self.input.len() {
                            self.input.clear();
                            0
                        } else {
                            end
                        };

                        (end - begin) as u64
                    }
                    Some(FdKind::TermOut) => EcallError::ReadTermOut.to_u64(),
                    Some(FdKind::TermErr) => EcallError::ReadTermErr.to_u64(),
                    Some(FdKind::TermLog) => EcallError::ReadTermLog.to_u64(),
                    Some(FdKind::FileSys(fd)) => {
                        match self.files.read_file_range(*fd, begin, len) {
                            Ok(file_buffer) => {
                                proc.tag_mut().memory.write_bytes(buf, file_buffer)?;
                                file_buffer.len() as u64
                            }
                            Err(e) => e.to_u64(),
                        }
                    }
                    _ => unimplemented!(),
                };

                proc.tag_mut().memory.push(to_ret);
                return Ok(RuntimeStatus::Running);
            }
            EcallExt::WriteFd { buf, begin, fd } => {
                match proc.get(fd as usize).map(|a| *a) {
                    None => {
                        proc.tag_mut().memory.push(EcallError::DoesntExist.to_u64());
                        return Ok(RuntimeStatus::Running);
                    }
                    Some(FdKind::TermIn) => {
                        proc.tag_mut().memory.push(EcallError::WriteTermIn.to_u64());
                        return Ok(RuntimeStatus::Running);
                    }
                    Some(FdKind::TermOut) => {
                        self.output.push_from(WriteEvent::StdoutWrite, &buf);
                        proc.tag_mut().memory.push(0u64);
                    }
                    Some(FdKind::TermErr) => {
                        self.output.push_from(WriteEvent::StderrWrite, &buf);
                        proc.tag_mut().memory.push(0u64);
                    }
                    Some(FdKind::TermLog) => {
                        self.output.push_from(WriteEvent::StdlogWrite, &buf);
                        proc.tag_mut().memory.push(0u64);
                    }
                    Some(FdKind::FileSys(fd)) => {
                        self.files
                            .write_to_file_range(fd, begin, &buf)
                            .map(|len| proc.tag_mut().memory.push(len as u64))
                            .unwrap_or_else(|err| proc.tag_mut().memory.push(err.to_u64()));
                        return Ok(RuntimeStatus::Blocked(EcallExt::WriteFd { buf, begin, fd }));
                    }
                    _ => unimplemented!(),
                }

                return Ok(RuntimeStatus::Running);
            }
            EcallExt::AppendFd { buf, fd } => {
                match proc.get(fd as usize).map(|a| *a) {
                    None => {
                        proc.tag_mut().memory.push(EcallError::DoesntExist.to_u64());
                        return Ok(RuntimeStatus::Running);
                    }
                    Some(FdKind::TermIn) => {
                        proc.tag_mut().memory.push(EcallError::WriteTermIn.to_u64());
                        return Ok(RuntimeStatus::Running);
                    }
                    Some(FdKind::TermOut) => {
                        self.output.push_from(WriteEvent::StdoutWrite, &buf);
                        proc.tag_mut().memory.push(0u64);
                    }
                    Some(FdKind::TermErr) => {
                        self.output.push_from(WriteEvent::StderrWrite, &buf);
                        proc.tag_mut().memory.push(0u64);
                    }
                    Some(FdKind::TermLog) => {
                        self.output.push_from(WriteEvent::StdlogWrite, &buf);
                        proc.tag_mut().memory.push(0u64);
                    }
                    Some(FdKind::FileSys(fd)) => {
                        self.files
                            .append_to_file(fd, &buf)
                            .map(|len| proc.tag_mut().memory.push(len as u64))
                            .unwrap_or_else(|err| proc.tag_mut().memory.push(err.to_u64()));
                        return Ok(RuntimeStatus::Blocked(EcallExt::AppendFd { buf, fd }));
                    }
                    _ => unimplemented!(),
                }

                return Ok(RuntimeStatus::Running);
            }

            _ => unimplemented!(),
        }
    }

    pub fn events(&mut self) -> TaggedMultiArray<WriteEvent, u8> {
        return mem::replace(&mut self.output, TaggedMultiArray::new());
    }

    pub fn term_out(&mut self) -> String {
        let mut out = StringWriter::new();
        for TE(tag, s) in &self.output {
            write_utf8_lossy(&mut out, s).unwrap();
        }

        mem::drop(mem::replace(&mut self.output, TaggedMultiArray::new()));

        return out.into_string();
    }
}
