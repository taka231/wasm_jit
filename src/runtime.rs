pub mod error;

use std::{
    alloc::{Layout, LayoutError},
    collections::HashMap,
};

use crate::{
    compiler::Compiler,
    wasm::{Func, WasmModule},
};
use anyhow::{bail, Context as _, Result};
use error::RuntimeError;
use wasmparser::{Export, ExternalKind, FuncType, ValType};

pub struct Runtime<'a> {
    types: Vec<FuncType>,
    funcs: Vec<u32>,
    code: Vec<Func<'a>>,
    exports: Exports<'a>,
    func_cache: HashMap<u32, fn(*mut u64, *mut u64) -> ()>,
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

type Exports<'a> = HashMap<&'a str, Export<'a>>;

impl<'a> Runtime<'a> {
    pub fn init(modules: WasmModule<'a>) -> Result<Runtime<'a>> {
        let exports = modules
            .exports
            .into_iter()
            .map(|export| (export.name, export))
            .collect();
        Ok(Runtime {
            types: modules.types,
            funcs: modules.funcs,
            code: modules.code,
            exports,
            func_cache: HashMap::new(),
            compiler: unsafe { Compiler::new()? },
        })
    }

    pub fn call_func_by_name(&mut self, name: &str, args: &[Value]) -> Result<Vec<Value>> {
        let Export { name, kind, index } = self
            .exports
            .get(name)
            .with_context(|| RuntimeError::ExportNotFound(name.into()))?;
        if *kind != ExternalKind::Func {
            bail!("Export kind is not a function: {}", name);
        }
        let type_index = self
            .funcs
            .get(*index as usize)
            .with_context(|| RuntimeError::FunctionNotFound((*name).into()))?;
        let func_type = self
            .types
            .get(*type_index as usize)
            .with_context(|| RuntimeError::FunctionTypeNotFound((*name).into()))?;
        let mut args = args.iter().map(|arg| arg.to_u64()).collect::<Vec<u64>>();
        unsafe {
            let code: fn(*mut u64, *mut u64) -> () = if let Some(code) = self.func_cache.get(index)
            {
                *code
            } else {
                let func = self
                    .code
                    .get(*index as usize)
                    .with_context(|| RuntimeError::FunctionNotFound((*name).into()))?;
                self.compiler.compile_func(&func, &func_type);
                let code = self.compiler.extract_func();
                self.func_cache.insert(*index, code);
                code
            };
            let args = args.as_mut_ptr();
            let result = std::alloc::alloc(Layout::from_size_align(16, 8)?) as *mut u64;
            code(args, result);
            if *result != 0 {
                let error = result.add(1) as *const anyhow::Error;
                if let Some(runtime_error) = (*error).downcast_ref::<RuntimeError>() {
                    bail!(runtime_error.clone());
                }
                if let Some(error) = (*error).downcast_ref::<LayoutError>() {
                    bail!(error.clone());
                }
                bail!("Something went wrong");
            }
            let value = *result.add(1);
            std::alloc::dealloc(result as *mut u8, Layout::from_size_align(16, 8)?);

            Ok(vec![Value::from_u64(value, &func_type.results()[0])])
        }
    }
}
