use libc::{c_int, c_void, size_t, PROT_EXEC, PROT_READ, PROT_WRITE};
use std::alloc::{alloc, dealloc, Layout, LayoutError};
use wasmparser::{FuncType, Operator};

use crate::wasm::Func;

extern "C" {
    fn mprotect(addr: *const c_void, len: size_t, prot: c_int) -> c_int;
}

pub struct Compiler {
    pub p_start: *mut u8,
    pub p_current: *mut u8,
    pub p_func_start: *mut u8,
}

const CODE_AREA_SIZE: usize = 1024;
const PAGE_SIZE: usize = 4096;

impl Compiler {
    pub unsafe fn new() -> Result<Compiler, LayoutError> {
        let layout = Layout::from_size_align(CODE_AREA_SIZE, PAGE_SIZE)?;
        let p_start = alloc(layout);
        let r = mprotect(
            p_start as *const c_void,
            CODE_AREA_SIZE,
            PROT_READ | PROT_WRITE | PROT_EXEC,
        );
        assert!(r == 0);
        Ok(Compiler {
            p_start,
            p_current: p_start,
            p_func_start: p_start,
        })
    }

    pub unsafe fn free(&self) -> Result<(), LayoutError> {
        let layout = Layout::from_size_align(CODE_AREA_SIZE, PAGE_SIZE)?;
        let r = mprotect(
            self.p_start as *const c_void,
            CODE_AREA_SIZE,
            PROT_READ | PROT_WRITE,
        );
        assert!(r == 0);
        dealloc(self.p_start, layout);
        Ok(())
    }

    unsafe fn push_code(&mut self, code: &[u8]) {
        for byte in code.iter() {
            *self.p_current = *byte;
            self.p_current = self.p_current.add(1);
        }
    }

    unsafe fn push_rax(&mut self) {
        self.push_code(&[0x50]); // push rax
    }

    unsafe fn pop_rax(&mut self) {
        self.push_code(&[0x58]); // pop rax
    }

    unsafe fn pop_rdi(&mut self) {
        self.push_code(&[0x5f]); // pop rdi
    }

    unsafe fn compile(&mut self, instrs: &[Operator<'_>]) {
        for instr in instrs {
            match instr {
                Operator::LocalGet { local_index } => {
                    let offset = 16 + local_index * 8;
                    if offset <= 128 {
                        // mov rax, [rbp - offset]
                        self.push_code(&[0x48, 0x8b, 0x45, (256 - offset) as u8])
                    } else {
                        // mov rax, [rbp - offset]
                        self.push_code(
                            &[
                                &[0x48, 0x8b, 0x85],
                                &(u32::MAX - offset + 1).to_le_bytes()[..],
                            ]
                            .concat(),
                        )
                    }
                    self.push_rax();
                }
                Operator::I32Const { value } => {
                    let value = *value as u32;
                    let value_bytes = value.to_le_bytes();
                    if value <= 127 {
                        self.push_code(&[0x6a, value as u8]) // push imm8
                    } else if value <= 0x7fffffff {
                        self.push_code(&[&[0x68], &value_bytes[..]].concat()) // push imm32
                    } else {
                        // 0x48: REX.W prefix
                        // 0xb8: mov rax, imm64
                        self.push_code(
                            &[&[0x48, 0xb8], &value_bytes[..], &[0x00, 0x00, 0x00, 0x00]].concat(),
                        );
                        self.push_rax();
                    }
                }
                Operator::I64Const { value } => {
                    let value = *value as u64;
                    let value_bytes = value.to_le_bytes();
                    if value <= 127 {
                        self.push_code(&[0x6a, value as u8]) // push imm8
                    } else if value <= 0x7fffffff {
                        self.push_code(&[&[0x68], &value_bytes[..]].concat()) // push imm32
                    } else {
                        // 0x48: REX.W prefix
                        // 0xb8: mov rax, imm64
                        self.push_code(&[&[0x48, 0xb8], &value_bytes[..]].concat());
                        self.push_rax();
                    }
                }
                Operator::I32Add | Operator::I64Add => {
                    self.pop_rdi();
                    self.pop_rax();
                    if instr == &Operator::I32Add {
                        // 0x01: add r/m32, r32
                        // 0xf8: rdi -> rax
                        self.push_code(&[0x01, 0xf8]);
                    } else {
                        // 0x48: REX.W prefix
                        // 0x01: add r/m64, r64
                        // 0xf8: rdi -> rax
                        self.push_code(&[0x48, 0x01, 0xf8]);
                    }
                    self.push_rax();
                }
                Operator::End => {}
                _ => unimplemented!("unimplemented instruction: {:?}", instr),
            }
        }
    }

    pub unsafe fn extract_func(&mut self) -> fn(*mut u64, *mut u64) -> () {
        let func = std::mem::transmute::<*mut u8, fn(*mut u64, *mut u64) -> ()>(self.p_func_start);
        self.p_func_start = self.p_current;
        func
    }

    pub unsafe fn compile_func(&mut self, func: &Func<'_>, func_type: &FuncType) {
        self.push_code(&[0x55]); // push rbp
        self.push_code(&[0x48, 0x89, 0xe5]); // mov rbp, rsp
        self.push_code(&[0x56]); // push rsi
        for i in 0..func_type.params().len() {
            if i == 0 {
                self.push_code(&[0x48, 0x8b, 0x07]); // mov rax, [rdi]
            } else if i < 128 / 8 {
                self.push_code(&[0x48, 0x8b, 0x47, i as u8 * 8]); // mov rax, [rdi + i * 8]
            } else {
                unimplemented!("more than 16 parameters are not supported yet");
            }
            self.push_rax();
        }
        if func.locals.len() > 0 {
            unimplemented!("local variables are not supported yet");
        }
        self.compile(&func.body);
        if func_type.results().len() > 1 {
            unimplemented!("multiple return values are not supported yet");
        }
        self.pop_rax();
        self.push_code(&[0x48, 0x8b, 0x4d, 0xf8]); // mov rcx, [rbp - 8]
        self.push_code(&[0x48, 0xc7, 0xc2, 0x00, 0x00, 0x00, 0x00]); // mov rdx, 0
        self.push_code(&[0x48, 0x89, 0x11]); // mov [rcx], rdx
        self.push_code(&[0x48, 0x89, 0x41, 0x08]); // mov [rcx + 8], rax
        self.push_code(&[0x48, 0x89, 0xec]); // mov rsp, rbp
        self.push_code(&[0x5d]); // pop rbp
        self.push_code(&[0xc3]); // ret
    }
}
