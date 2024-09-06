pub mod error;
pub mod store;

use std::{alloc::Layout, collections::HashMap};

use crate::{
    compiler::{Compiler, JITFunc},
    wasm::WasmModule,
};
use anyhow::{bail, Error, Result};
use error::RuntimeError;
use store::Store;
use wasmparser::{Export, ExternalKind, ValType};

pub struct Runtime<'a> {
    store: Store<'a>,
    compiler: Compiler,
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

impl<'a> Runtime<'a> {
    pub fn init(modules: WasmModule<'a>) -> Runtime<'a> {
        let store = Store::new(modules);
        Runtime {
            store,
            compiler: unsafe { Compiler::new() },
        }
    }

    pub fn call_func_by_name(&mut self, name: &str, args: &[Value]) -> Result<Vec<Value>> {
        let Export { name, kind, index } = self.store.get_export(name)?;
        let index = *index;
        if *kind != ExternalKind::Func {
            bail!("Export kind is not a function: {}", name);
        }
        let mut args = args.iter().map(|arg| arg.to_u64()).collect::<Vec<u64>>();
        unsafe {
            let result = self.call_func_by_index(index, &mut args)?;
            let func_type = self.store.get_func_type_from_func_index(index)?;

            Ok(vec![Value::from_u64(result[0], &func_type.results()[0])])
        }
    }

    unsafe fn call_func_by_index(&mut self, index: u32, args: &mut [u64]) -> Result<Vec<u64>> {
        let code: JITFunc = if let Some(code) = self.compiler.func_cache.get(&index) {
            std::mem::transmute::<*const (), JITFunc>(*code)
        } else {
            self.compiler.compile_func(index, &self.store)?;
            let code = self.compiler.extract_func(index);
            code
        };
        let args = args.as_mut_ptr();
        let result = std::alloc::alloc(Layout::from_size_align(16, 8)?) as *mut u64;
        code(args, result, self);
        if *result != 0 {
            let error = result.add(1) as *const anyhow::Error;
            if let Some(runtime_error) = (*error).downcast_ref::<RuntimeError>() {
                bail!(runtime_error.clone());
            }
            bail!("Something went wrong");
        }
        let value = *result.add(1);
        std::alloc::dealloc(result as *mut u8, Layout::from_size_align(16, 8)?);

        Ok(vec![value])
    }

    pub(crate) unsafe fn call_func_internal(
        &mut self,
        index: u32,
        result_ptr: *mut u64,
        args_num: u32,
        args: *mut u64,
    ) -> *mut u64 {
        let args_slice = std::slice::from_raw_parts_mut(args, args_num as usize);
        let result = self.call_func_by_index(index, args_slice);
        std::alloc::dealloc(
            args as *mut u8,
            Layout::from_size_align(8 * args_num as usize, 8).unwrap(),
        );
        match result {
            Ok(result) => {
                *result_ptr = 0;
                *(result_ptr.add(1)) = result[0];
            }
            Err(err) => {
                *result_ptr = 1;
                assert!(std::mem::size_of::<Error>() <= 8);
                let error_ptr = result_ptr.add(1) as *mut Error;
                *error_ptr = err;
            }
        }
        result_ptr
    }
}
