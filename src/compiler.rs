use crate::{
    assembler::{
        ret, Add, Call, Cmp, Je, Jmp, Mov, Movzx, Pop, Push,
        Register32::*,
        Register64::{self, *},
        Register8::*,
        Sete, Sub,
    },
    wasm::Func,
};
use anyhow::Result;
use libc::{c_int, c_void, size_t, PROT_EXEC, PROT_READ, PROT_WRITE};
use std::{
    alloc::{alloc, dealloc, Layout},
    collections::HashMap,
};
use wasmparser::{BlockType, Operator};

use crate::runtime::{store::Store, Runtime};

extern "C" {
    fn mprotect(addr: *const c_void, len: size_t, prot: c_int) -> c_int;
}

pub struct Compiler {
    pub p_start: *mut u8,
    pub p_current: *mut u8,
    pub p_func_start: *mut u8,
    pub func_cache: HashMap<u32, *const ()>,
}

enum Label {
    FuncEnd(Vec<*mut u8>),
    LoopStart {
        start: *mut u8,
        start_offset: usize,
        block_type: BlockType,
    },
    End {
        address_reserved: Vec<*mut u8>,
        start_offset: usize,
        block_type: BlockType,
    },
}

const CODE_AREA_SIZE: usize = 1024;
const PAGE_SIZE: usize = 4096;
pub type JITFunc = fn(runtime: &mut Runtime, sp: *mut u64) -> u64;

macro_rules! code {
    {$self:expr; $($code:expr),+} => {
        for code in &[$($code),+] {
            $self.push_code(&code);
        }
    };
}

impl Compiler {
    pub(crate) unsafe fn new() -> Compiler {
        let layout = Layout::from_size_align(CODE_AREA_SIZE, PAGE_SIZE).unwrap();
        let p_start = alloc(layout);
        let r = mprotect(
            p_start as *const c_void,
            CODE_AREA_SIZE,
            PROT_READ | PROT_WRITE | PROT_EXEC,
        );
        assert!(r == 0);
        Compiler {
            p_start,
            p_current: p_start,
            p_func_start: p_start,
            func_cache: HashMap::new(),
        }
    }

    pub(crate) unsafe fn free(&self) {
        let layout = Layout::from_size_align(CODE_AREA_SIZE, PAGE_SIZE).unwrap();
        let r = mprotect(
            self.p_start as *const c_void,
            CODE_AREA_SIZE,
            PROT_READ | PROT_WRITE,
        );
        assert!(r == 0);
        dealloc(self.p_start, layout);
    }

    unsafe fn push_code(&mut self, code: &[u8]) {
        for byte in code.iter() {
            *self.p_current = *byte;
            self.p_current = self.p_current.add(1);
        }
    }

    unsafe fn write_i32(pointer: *mut u8, value: i32) {
        let bytes = value.to_le_bytes();
        for (i, byte) in bytes.iter().enumerate() {
            *pointer.add(i) = *byte;
        }
    }

    unsafe fn push_data(data: Register64) -> Vec<u8> {
        let mut code = Vec::new();
        code.extend_from_slice(&R11.to_mem().mov(data));
        code.extend_from_slice(&R11.add(8));
        code
    }

    unsafe fn pop_data(data: Register64) -> Vec<u8> {
        let mut code = Vec::new();
        code.extend_from_slice(&R11.add(-8));
        code.extend_from_slice(&data.mov(R11.to_mem()));
        code
    }

    fn local_offset(local_index: u32) -> u32 {
        8 * (Self::LOCAL_BASE_COUNT + 1) + local_index * 8
    }

    const LOCAL_BASE_COUNT: u32 = 1;

    unsafe fn compile(
        &mut self,
        func: &Func<'_>,
        func_index: u32,
        store: &Store<'_>,
        stack_count: &mut usize,
        labels: &mut Vec<Label>,
    ) -> Result<()> {
        for instr in &func.body {
            match instr {
                Operator::Call { function_index } => {
                    let func_type = store.get_func_type_from_func_index(*function_index)?;
                    let args_num = func_type.params().len() as i32;

                    code! {self;
                        Rdi.mov(Rbp.with_offset(-8)),
                        Rsi.mov(R11),
                        Edx.mov(*function_index as i32),
                        if *function_index == func_index {
                            R10.mov(self.p_func_start as usize as i64)
                        } else {
                            R10.mov(Runtime::call_func_internal as usize as i64)
                        },
                        R11.push(),
                        R10.call()
                    }

                    *stack_count -= args_num as usize;

                    code! {self;
                        R11.pop(),
                        R11.add(8 * (func_type.results().len() as i32 - args_num))
                    }
                    *stack_count += func_type.results().len();
                }
                Operator::LocalGet { local_index } => {
                    let offset = Compiler::local_offset(*local_index) as i32;
                    code! {self;
                        Rax.mov(Rbp.with_offset(-offset)),
                        Self::push_data(Rax)
                    };
                    *stack_count += 1;
                }
                Operator::I32Const { value } => {
                    code! {self;
                        Eax.mov(*value),
                        Self::push_data(Rax)
                    };
                    *stack_count += 1;
                }
                Operator::I64Const { value } => {
                    code! {self;
                        Rax.mov(*value),
                        Self::push_data(Rax)
                    };
                    *stack_count += 1;
                }
                Operator::I32Add | Operator::I64Add => {
                    code! {self;
                        Self::pop_data(Rdi),
                        Self::pop_data(Rax),
                        if instr == &Operator::I32Add {
                            Eax.add(Edi)
                        } else {
                            Rax.add(Rdi)
                        },
                        Self::push_data(Rax)
                    };
                    *stack_count -= 1;
                }
                Operator::I32Sub | Operator::I64Sub => {
                    code! {self;
                        Self::pop_data(Rdi),
                        Self::pop_data(Rax),
                        if instr == &Operator::I32Sub {
                            Eax.sub(Edi)
                        } else {
                            Rax.sub(Rdi)
                        },
                        Self::push_data(Rax)
                    };
                    *stack_count -= 1;
                }
                Operator::I32Eq | Operator::I64Eq => {
                    code! {self;
                        Self::pop_data(Rdi),
                        Self::pop_data(Rax),
                        if instr == &Operator::I32Eq {
                            Eax.cmp(Edi)
                        } else {
                            Rax.cmp(Rdi)
                        },
                        Al.sete(),
                        Eax.movzx(Al),
                        Self::push_data(Rax)
                    };
                    *stack_count -= 1;
                }
                Operator::If { blockty } => {
                    code! {self;
                        Self::pop_data(Rax),
                        Rdi.mov(0),
                        Rax.cmp(Rdi),
                        0_i32.je()
                    };
                    let params_len = match blockty {
                        BlockType::FuncType(n) => {
                            let func_type = store.get_func_type(*n)?;
                            func_type.params().len()
                        }
                        _ => 0,
                    };
                    *stack_count -= 1;
                    labels.push(Label::End {
                        address_reserved: vec![self.p_current],
                        start_offset: *stack_count - params_len,
                        block_type: *blockty,
                    });
                }
                Operator::Else => {
                    let label = labels.last_mut().unwrap();
                    let Label::End {
                        address_reserved,
                        start_offset,
                        ..
                    } = label
                    else {
                        unreachable!()
                    };
                    code! {self;
                        0_i32.jmp()
                    };
                    address_reserved.push(self.p_current);
                    let if_start = address_reserved[0];
                    address_reserved.remove(0);
                    let relative_offset = self.p_current as usize - if_start as usize;
                    Compiler::write_i32(if_start.sub(4), relative_offset as i32);
                    *stack_count = *start_offset;
                }
                Operator::End => {
                    let label = labels.pop().unwrap();
                    match label {
                        Label::End {
                            address_reserved,
                            start_offset,
                            block_type,
                        } => {
                            for address in address_reserved {
                                let relative_offset = self.p_current as usize - address as usize;
                                Compiler::write_i32(address.sub(4), relative_offset as i32);
                            }
                            let result_len = match block_type {
                                BlockType::FuncType(n) => {
                                    let func_type = store.get_func_type(n)?;
                                    func_type.results().len()
                                }
                                BlockType::Type(_) => 1,
                                BlockType::Empty => 0,
                            };
                            if result_len == *stack_count - start_offset {
                                continue;
                            }
                            let relation = (*stack_count - start_offset) as i32 * 8;
                            code! {self;
                                R11.add(-relation)
                            };
                            for _ in (0..result_len).rev() {
                                code! {self;
                                    Rax.mov(R11.with_offset(relation - result_len as i32 * 8)),
                                    Self::push_data(Rax)
                                }
                            }
                            *stack_count = start_offset + result_len;
                        }
                        Label::FuncEnd(address_reserved) => {
                            for address in address_reserved {
                                let relative_offset = self.p_current as usize - address as usize;
                                Compiler::write_i32(address.sub(4), relative_offset as i32);
                            }
                        }
                        Label::LoopStart {
                            start,
                            start_offset,
                            block_type,
                        } => unimplemented!(),
                    }
                }
                _ => unimplemented!("unimplemented instruction: {:?}", instr),
            }
        }
        Ok(())
    }

    pub(crate) unsafe fn extract_func(&mut self, index: u32) -> JITFunc {
        let func_pointer = self.p_func_start as *const ();
        self.func_cache.insert(index, func_pointer);
        let func = std::mem::transmute::<*const (), JITFunc>(func_pointer);
        self.p_func_start = self.p_current;
        func
    }

    pub(crate) unsafe fn compile_func(&mut self, func_index: u32, store: &Store<'_>) -> Result<()> {
        let func = store.get_code(func_index)?;
        let func_type = store.get_func_type_from_func_index(func_index)?;
        code! {self;
            Rbp.push(),
            Rbp.mov(Rsp),
            Rdi.push(),
            // R11 is used as a data stack pointer
            R11.mov(Rsi)
        };
        for i in (0..func_type.params().len()).rev() {
            code! {self;
                Rax.mov(R11.with_offset(-((i+1) as i32 * 8))),
                Rax.push()
            };
        }
        code! {self;
            R11.add(-8 * func_type.params().len() as i32)
        };

        // 16byte align
        if func_type.params().len() % 2 == 1 {
            code! {self;
                Rsp.add(-8)
            };
        }

        if !func.locals.is_empty() {
            unimplemented!("local variables are not supported yet");
        }
        let mut stack_count = 0;
        let mut labels = vec![Label::FuncEnd(Vec::new())];
        self.compile(func, func_index, store, &mut stack_count, &mut labels)?;
        let result_len = func_type.results().len();
        if result_len != 0 && result_len != stack_count {
            code! {self;
                R11.add(-8 * stack_count as i32)
            };
            for _ in 0..result_len {
                code! {self;
                    Rax.mov(R11.with_offset(8 * (stack_count - result_len) as i32)),
                    Self::push_data(Rax)
                };
            }
        }
        code! {self;
            Rax.mov(0),
            Rsp.mov(Rbp),
            Rbp.pop(),
            ret()
        }
        Ok(())
    }
}
