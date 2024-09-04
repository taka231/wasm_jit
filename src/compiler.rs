use crate::assembler::{
    ret, Add, Addressing, Call, Cmp, Je, Mov, Movzx, Pop, Push, Register32::*, Register64::*,
    Register8::*, Sete, Sub,
};
use anyhow::Result;
use libc::{c_int, c_void, size_t, PROT_EXEC, PROT_READ, PROT_WRITE};
use std::alloc::{alloc, dealloc, Layout, LayoutError};
use wasmparser::{BlockType, Operator};

use crate::runtime::{store::Store, Runtime};

extern "C" {
    fn mprotect(addr: *const c_void, len: size_t, prot: c_int) -> c_int;
}

pub struct Compiler {
    pub p_start: *mut u8,
    pub p_current: *mut u8,
    pub p_func_start: *mut u8,
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
pub type JITFunc = fn(args: *mut u64, result: *mut u64, runtime: &mut Runtime) -> ();

fn alloc_u64_array(size: usize) -> *mut u64 {
    let layout = Layout::from_size_align(size * std::mem::size_of::<u64>(), 8).unwrap();
    unsafe { alloc(layout) as *mut u64 }
}

fn dealloc_u64_array(ptr: *mut u64, size: usize) {
    let layout = Layout::from_size_align(size * std::mem::size_of::<u64>(), 8).unwrap();
    unsafe { dealloc(ptr as *mut u8, layout) }
}

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

    fn local_offset(local_index: u32) -> u32 {
        24 + local_index * 8
    }

    unsafe fn compile(
        &mut self,
        instrs: &[Operator<'_>],
        store: &Store<'_>,
        stack_count: &mut usize,
        labels: &mut Vec<Label>,
    ) -> Result<()> {
        for instr in instrs {
            match instr {
                Operator::Call { function_index } => {
                    let func_type = store.get_func_type_from_func_index(*function_index)?;
                    let args_num = func_type.params().len() as i32;

                    code! {self;
                        Edi.mov(args_num),
                        R10.mov(alloc_u64_array as usize as i64),
                        R10.call()
                    };
                    for i in (0..args_num).rev() {
                        let offset = 8 * i;
                        code! {self;
                            Rdi.pop(),
                            Rax.with_offset(offset).mov(Rdi)
                        };
                    }
                    *stack_count -= args_num as usize;
                    code! {self;
                        Rdi.mov(Rbp.with_offset(-16)),
                        Esi.mov(*function_index as i32),
                        Rdx.mov(Rbp.with_offset(-8)),
                        Ecx.mov(args_num),
                        R8.mov(Rax),
                        R10.mov(Runtime::call_func_internal as usize as i64),
                        R10.call(),
                        Rax.with_offset(8).push()
                    };
                    *stack_count += 1;
                }
                Operator::LocalGet { local_index } => {
                    let offset = Compiler::local_offset(*local_index) as i32;
                    code! {self;
                        Rax.mov(Rbp.with_offset(-offset)),
                        Rax.push()
                    };
                    *stack_count += 1;
                }
                Operator::I32Const { value } => self.push_code(&value.push()),
                Operator::I64Const { value } => {
                    if i32::MIN as i64 <= *value && *value <= i32::MAX as i64 {
                        let value = *value as i32;
                        self.push_code(&value.push());
                    } else {
                        code! {self;
                            Rax.mov(*value),
                            Rax.push()
                        };
                    }
                    *stack_count += 1;
                }
                Operator::I32Add | Operator::I64Add => {
                    code! {self;
                        Rdi.pop(),
                        Rax.pop(),
                        if instr == &Operator::I32Add {
                            Eax.add(Edi)
                        } else {
                            Rax.add(Rdi)
                        },
                        Rax.push()
                    };
                    *stack_count -= 1;
                }
                Operator::I32Sub | Operator::I64Sub => {
                    code! {self;
                        Rdi.pop(),
                        Rax.pop(),
                        if instr == &Operator::I32Sub {
                            Eax.sub(Edi)
                        } else {
                            Rax.sub(Rdi)
                        },
                        Rax.push()
                    };
                    *stack_count -= 1;
                }
                Operator::I32Eq | Operator::I64Eq => {
                    code! {self;
                        Rdi.pop(),
                        Rax.pop(),
                        if instr == &Operator::I32Eq {
                            Eax.cmp(Edi)
                        } else {
                            Rax.cmp(Rdi)
                        },
                        Al.sete(),
                        Eax.movzx(Al),
                        Rax.push()
                    };
                    *stack_count -= 1;
                }
                Operator::If { blockty } => {
                    code! {self;
                        Rax.pop(),
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
                        address_reserved, ..
                    } = label
                    else {
                        unreachable!()
                    };
                    let if_start = address_reserved[0];
                    address_reserved.remove(0);
                    let relative_offset = self.p_current as usize - if_start as usize;
                    Compiler::write_i32(if_start.sub(4), relative_offset as i32);
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
                                Rsp.add(relation)
                            };
                            for i in (0..result_len).rev() {
                                code! {self;
                                    Rsp.with_offset(-relation + i as i32 * 8).push()
                                }
                            }
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

    pub(crate) unsafe fn extract_func(&mut self) -> JITFunc {
        let func_pointer = self.p_func_start as *const ();
        let func = std::mem::transmute::<*const (), JITFunc>(func_pointer);
        self.p_func_start = self.p_current;
        func
    }

    pub(crate) unsafe fn compile_func(&mut self, func_index: u32, store: &Store<'_>) -> Result<()> {
        let func = store.get_code(func_index)?;
        let func_type = store.get_func_type_from_func_index(func_index)?;
        let mut stack_count = 0;
        let mut labels = vec![Label::FuncEnd(Vec::new())];
        code! {self;
            Rbp.push(),
            Rbp.mov(Rsp),
            Rsi.push(),
            Rdx.push()
        };
        stack_count += 2;
        for i in 0..func_type.params().len() {
            code! {self;
                Rax.mov(Rdi.with_offset(i as i32 * 8)),
                Rax.push()
            };
        }
        stack_count += func_type.params().len();
        if !func.locals.is_empty() {
            unimplemented!("local variables are not supported yet");
        }
        self.compile(&func.body, store, &mut stack_count, &mut labels)?;
        if func_type.results().len() > 1 {
            unimplemented!("multiple return values are not supported yet");
        }
        code! {self;
            Rax.pop(),
            Rcx.mov(Rbp.with_offset(-8)),
            Rdx.mov(0),
            Rcx.to_mem().mov(Rdx),
            Rcx.with_offset(8).mov(Rax),
            Rsp.mov(Rbp),
            Rbp.pop(),
            ret()
        }
        Ok(())
    }
}
