use crate::{
    assembler::{
        ret, Add, Call, Cmp, Je, Jmp, Mov, Movzx, Pop, Push,
        Register32::{self, *},
        Register64::{self, *},
        Register8::*,
        Sete, Sub,
    },
    wasm::Func,
};
use anyhow::{bail, Result};
use libc::{c_int, c_void, size_t, PROT_EXEC, PROT_READ, PROT_WRITE};
use std::{
    alloc::{alloc, dealloc, Layout},
    collections::VecDeque,
};
use wasmparser::{BlockType, Operator};

use crate::runtime::{store::Store, Runtime};
use fxhash::FxHashMap;

extern "C" {
    fn mprotect(addr: *const c_void, len: size_t, prot: c_int) -> c_int;
}

pub struct Compiler {
    pub p_start: *mut u8,
    pub p_current: *mut u8,
    pub p_func_start: *mut u8,
    pub func_cache: FxHashMap<u32, *const ()>,
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
        else_vartual_stack: Option<VartualStack>,
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

#[derive(Debug, Clone)]
enum StackValue {
    Imm(i64),
    Reg(Register64),
}

#[derive(Debug, Clone)]
struct VartualStack {
    stack: VecDeque<StackValue>,
    unused_regs: VecDeque<Register64>,
}

impl VartualStack {
    fn new() -> VartualStack {
        VartualStack {
            stack: VecDeque::new(),
            unused_regs: VecDeque::from(vec![Rdi, Rsi, Rdx, Rcx, R8, R9, R10]),
        }
    }

    unsafe fn get_unused_reg(&mut self, compiler: &mut Compiler) -> Register64 {
        if let Some(reg) = self.unused_regs.pop_front() {
            return reg;
        }
        loop {
            let value = self.stack.pop_front().expect("stack is empty");
            match value {
                StackValue::Imm(n) => {
                    code! {compiler;
                        Rax.mov(n),
                        Compiler::push_data(Rax)
                    }
                }
                StackValue::Reg(reg) => {
                    code! {compiler;
                        Compiler::push_data(reg)
                    };
                    return reg;
                }
            }
        }
    }

    unsafe fn pop_value(&mut self, compiler: &mut Compiler) -> StackValue {
        if let Some(value) = self.stack.pop_back() {
            return value;
        }
        let reg = self.get_unused_reg(compiler);
        code! {compiler;
            Compiler::pop_data(reg)
        }
        StackValue::Reg(reg)
    }

    unsafe fn push_all(&mut self, compiler: &mut Compiler) {
        while let Some(value) = self.stack.pop_front() {
            match value {
                StackValue::Imm(n) => {
                    code! {compiler;
                        Rax.mov(n),
                        Compiler::push_data(Rax)
                    }
                }
                StackValue::Reg(reg) => {
                    code! {compiler;
                        Compiler::push_data(reg)
                    };
                    self.unused_regs.push_back(reg);
                }
            }
        }
    }
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
            func_cache: FxHashMap::default(),
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
        vartual_stack: &mut VartualStack,
        labels: &mut Vec<Label>,
    ) -> Result<()> {
        for instr in &func.body {
            match instr {
                Operator::Call { function_index } => {
                    let func_type = store.get_func_type_from_func_index(*function_index)?;
                    let args_num = func_type.params().len() as i32;
                    vartual_stack.push_all(self);

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
                    let reg = vartual_stack.get_unused_reg(self);
                    code! {self;
                        reg.mov(Rbp.with_offset(-offset))
                    };
                    vartual_stack.stack.push_back(StackValue::Reg(reg));
                    *stack_count += 1;
                }
                Operator::I32Const { value } => {
                    vartual_stack
                        .stack
                        .push_back(StackValue::Imm(*value as i64));
                    *stack_count += 1;
                }
                Operator::I64Const { value } => {
                    vartual_stack.stack.push_back(StackValue::Imm(*value));
                    *stack_count += 1;
                }
                Operator::I32Add | Operator::I64Add => {
                    let value2 = vartual_stack.pop_value(self);
                    let value1 = vartual_stack.pop_value(self);
                    match (value1, value2) {
                        (StackValue::Imm(n), StackValue::Imm(m)) => {
                            vartual_stack.stack.push_back(StackValue::Imm(n + m));
                        }
                        (StackValue::Reg(reg1), StackValue::Reg(reg2)) => {
                            if instr == &Operator::I32Add {
                                let reg1: Register32 = reg1.into();
                                let reg2: Register32 = reg2.into();
                                code! {self;
                                    reg1.add(reg2)
                                };
                            } else {
                                code! {self;
                                    reg1.add(reg2)
                                };
                            }
                            vartual_stack.stack.push_back(StackValue::Reg(reg1));
                            vartual_stack.unused_regs.push_back(reg2);
                        }
                        (StackValue::Reg(reg), StackValue::Imm(n))
                        | (StackValue::Imm(n), StackValue::Reg(reg)) => {
                            if instr == &Operator::I32Add {
                                let reg: Register32 = reg.into();
                                code! {self;
                                    Eax.mov(n as i32),
                                    reg.add(Eax)
                                };
                            } else {
                                code! {self;
                                    Rax.mov(n),
                                    reg.add(Rax)
                                };
                            }
                            vartual_stack.stack.push_back(StackValue::Reg(reg));
                        }
                    }
                    *stack_count -= 1;
                }
                Operator::I32Sub | Operator::I64Sub => {
                    let value2 = vartual_stack.pop_value(self);
                    let value1 = vartual_stack.pop_value(self);
                    match (value1, value2) {
                        (StackValue::Imm(n), StackValue::Imm(m)) => {
                            vartual_stack.stack.push_back(StackValue::Imm(n - m));
                        }
                        (StackValue::Reg(reg1), StackValue::Reg(reg2)) => {
                            if instr == &Operator::I32Sub {
                                let reg1: Register32 = reg1.into();
                                let reg2: Register32 = reg2.into();
                                code! {self;
                                    reg1.sub(reg2)
                                };
                            } else {
                                code! {self;
                                    reg1.sub(reg2)
                                };
                            }
                            vartual_stack.stack.push_back(StackValue::Reg(reg1));
                            vartual_stack.unused_regs.push_back(reg2);
                        }
                        (value1 @ StackValue::Reg(reg), StackValue::Imm(n))
                        | (value1 @ StackValue::Imm(n), StackValue::Reg(reg)) => {
                            if matches!(value1, StackValue::Reg(_)) {
                                if instr == &Operator::I32Sub {
                                    let reg: Register32 = reg.into();
                                    code! {self;
                                        Eax.mov(n as i32),
                                        reg.sub(Eax)
                                    };
                                } else {
                                    code! {self;
                                        Rax.mov(n),
                                        reg.sub(Rax)
                                    };
                                }
                            } else if instr == &Operator::I32Sub {
                                let reg: Register32 = reg.into();
                                code! {self;
                                    Eax.mov(n as i32),
                                    Eax.sub(reg),
                                    reg.mov(Eax)
                                };
                            } else {
                                code! {self;
                                    Rax.mov(n),
                                    Rax.sub(reg),
                                    reg.mov(Rax)
                                };
                            }
                            vartual_stack.stack.push_back(StackValue::Reg(reg));
                        }
                    }
                    *stack_count -= 1;
                }
                Operator::I32Eq | Operator::I64Eq => {
                    let value2 = vartual_stack.pop_value(self);
                    let value1 = vartual_stack.pop_value(self);
                    match (value1, value2) {
                        (StackValue::Imm(n), StackValue::Imm(m)) => {
                            vartual_stack.stack.push_back(StackValue::Imm(if n == m {
                                1
                            } else {
                                0
                            }));
                        }
                        (StackValue::Reg(reg1), StackValue::Reg(reg2)) => {
                            if instr == &Operator::I32Add {
                                let reg1: Register32 = reg1.into();
                                let reg2: Register32 = reg2.into();
                                code! {self;
                                    reg1.cmp(reg2),
                                    Al.sete(),
                                    Eax.movzx(Al),
                                    reg1.mov(Eax)
                                };
                            } else {
                                code! {self;
                                    reg1.cmp(reg2),
                                    Al.sete(),
                                    Eax.movzx(Al),
                                    reg1.mov(Rax)
                                };
                            }
                            vartual_stack.stack.push_back(StackValue::Reg(reg1));
                            vartual_stack.unused_regs.push_back(reg2);
                        }
                        (StackValue::Reg(reg), StackValue::Imm(n))
                        | (StackValue::Imm(n), StackValue::Reg(reg)) => {
                            if instr == &Operator::I32Add {
                                let reg: Register32 = reg.into();
                                code! {self;
                                    Eax.mov(n as i32),
                                    reg.cmp(Eax),
                                    Al.sete(),
                                    Eax.movzx(Al),
                                    reg.mov(Eax)
                                };
                            } else {
                                code! {self;
                                    Rax.mov(n),
                                    reg.cmp(Rax),
                                    Al.sete(),
                                    Eax.movzx(Al),
                                    reg.mov(Rax)
                                };
                            }
                            vartual_stack.stack.push_back(StackValue::Reg(reg));
                        }
                    }
                    *stack_count -= 1;
                }
                Operator::If { blockty } => {
                    let value = vartual_stack.pop_value(self);
                    match value {
                        StackValue::Imm(n) => {
                            code! {self;
                                Eax.mov(n as i32),
                                Eax.cmp(0),
                                0_i32.je()
                            };
                        }
                        StackValue::Reg(reg) => {
                            let reg32: Register32 = reg.into();
                            code! {self;
                                reg32.cmp(0),
                                0_i32.je()
                            };
                            vartual_stack.unused_regs.push_back(reg);
                        }
                    }
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
                        else_vartual_stack: Some(vartual_stack.clone()),
                    });
                }
                Operator::Else => {
                    let label = labels.last_mut().unwrap();
                    let Label::End {
                        address_reserved,
                        start_offset,
                        else_vartual_stack,
                        ..
                    } = label
                    else {
                        unreachable!()
                    };
                    vartual_stack.push_all(self);
                    *vartual_stack = else_vartual_stack.take().unwrap();
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
                            ..
                        } => {
                            vartual_stack.push_all(self);
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
                            for _ in 0..result_len.min(7) {
                                let reg = vartual_stack.get_unused_reg(self);
                                code! {self;
                                    Compiler::pop_data(reg)
                                };
                                vartual_stack.stack.push_front(StackValue::Reg(reg));
                            }
                            if result_len > 7 {
                                code! {self;
                                    R11.add(-relation + 7 * 8)
                                };
                                for _ in (7..result_len).rev() {
                                    code! {self;
                                        Rax.mov(R11.with_offset(relation - result_len as i32 * 8)),
                                        Self::push_data(Rax)
                                    }
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
        let mut vartual_stack = VartualStack::new();
        self.compile(
            func,
            func_index,
            store,
            &mut stack_count,
            &mut vartual_stack,
            &mut labels,
        )?;
        vartual_stack.push_all(self);
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
