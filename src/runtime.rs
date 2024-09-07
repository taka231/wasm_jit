pub mod error;
pub mod store;

use std::{
    alloc::Layout,
    collections::HashMap,
    ffi::{c_int, c_void},
};

use crate::{
    compiler::{Compiler, JITFunc},
    wasm::WasmModule,
};
use anyhow::{bail, Error, Result};
use error::RuntimeError;
use libc::size_t;
use store::Store;
use wasmparser::{Export, ExternalKind, ValType};

pub struct Runtime<'a> {
    store: Store<'a>,
    compiler: Compiler,
    stack_base: *mut u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl Value {
    fn to_u64(&self) -> u64 {
        match self {
            Value::I32(v) => *v as u64,
            Value::I64(v) => *v as u64,
            Value::F32(v) => f32::to_bits(*v) as u64,
            Value::F64(v) => f64::to_bits(*v),
        }
    }

    fn from_u64(bytes: u64, value_type: &ValType) -> Value {
        match value_type {
            ValType::I32 => Value::I32(bytes as i32),
            ValType::I64 => Value::I64(bytes as i64),
            ValType::F32 => Value::F32(f32::from_bits(bytes as u32)),
            ValType::F64 => Value::F64(f64::from_bits(bytes)),
            _ => unimplemented!("Unsupported value type: {:?}", value_type),
        }
    }
}

const PAGE_SIZE: usize = 4096;
const STACK_SIZE: usize = PAGE_SIZE * 1;

extern "C" {
    fn mprotect(addr: *const c_void, len: size_t, prot: c_int) -> c_int;
}

impl<'a> Runtime<'a> {
    pub fn init(modules: WasmModule<'a>) -> Runtime<'a> {
        let store = Store::new(modules);
        let stack_layout = Layout::from_size_align(STACK_SIZE + PAGE_SIZE, 8).unwrap();
        let sp = unsafe { std::alloc::alloc(stack_layout) as *mut u64 };
        unsafe {
            mprotect(
                sp.add(STACK_SIZE) as *const c_void,
                PAGE_SIZE,
                libc::PROT_NONE,
            );
        }
        Runtime {
            store,
            compiler: unsafe { Compiler::new() },
            stack_base: sp,
        }
    }

    pub fn call_func_by_name(&mut self, name: &str, args: &[Value]) -> Result<Vec<Value>> {
        let Export { name, kind, index } = self.store.get_export(name)?;
        let index = *index;
        if *kind != ExternalKind::Func {
            bail!("Export kind is not a function: {}", name);
        }
        for (i, arg) in args.iter().enumerate() {
            unsafe {
                *self.stack_base.add(i) = arg.to_u64();
            }
        }
        unsafe {
            self.call_func_by_index(self.stack_base.add(args.len()), index)?;
            let func_type = self.store.get_func_type_from_func_index(index)?;
            let mut result = Vec::new();
            for i in 0..func_type.results().len() {
                result.push(Value::from_u64(
                    *self.stack_base.add(i),
                    &func_type.results()[i],
                ));
            }

            Ok(result)
        }
    }

    unsafe fn call_func_by_index(&mut self, sp: *mut u64, index: u32) -> Result<()> {
        let code: JITFunc = if let Some(code) = self.compiler.func_cache.get(&index) {
            std::mem::transmute::<*const (), JITFunc>(*code)
        } else {
            self.compiler.compile_func(index, &self.store)?;
            let code = self.compiler.extract_func(index);
            code
        };
        let result = code(self, sp);
        if result != 0 {
            let error = std::mem::transmute::<u64, Error>(result);
            return Err(error);
        }
        Ok(())
    }

    pub(crate) unsafe fn call_func_internal(&mut self, sp: *mut u64, index: u32) -> u64 {
        let result = self.call_func_by_index(sp, index);
        match result {
            Ok(_) => 0,
            Err(err) => std::mem::transmute::<Error, u64>(err),
        }
    }
}
