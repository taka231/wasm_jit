use std::collections::HashMap;

use crate::wasm::{Func, WasmModule};
use anyhow::{Context as _, Result};
use wasmparser::{Export, FuncType};

use super::error::RuntimeError;
type Exports<'a> = HashMap<&'a str, Export<'a>>;

#[derive(Debug)]
pub struct Store<'a> {
    pub types: Vec<FuncType>,
    pub funcs: Vec<u32>,
    pub code: Vec<Func<'a>>,
    pub exports: Exports<'a>,
}

impl<'a> Store<'a> {
    pub fn new(modules: WasmModule<'a>) -> Store<'a> {
        Store {
            types: modules.types,
            funcs: modules.funcs,
            code: modules.code,
            exports: modules
                .exports
                .into_iter()
                .map(|export| (export.name, export))
                .collect(),
        }
    }

    pub fn get_export(&self, name: &str) -> Result<&Export<'a>> {
        self.exports
            .get(name)
            .with_context(|| RuntimeError::ExportNotFound(name.into()))
    }

    pub fn get_func_type_from_func_index(&self, index: u32) -> Result<&FuncType> {
        let func_index = self
            .funcs
            .get(index as usize)
            .with_context(|| RuntimeError::FunctionNotFound(index.to_string()))?;
        let func_type = self
            .types
            .get(*func_index as usize)
            .with_context(|| RuntimeError::FunctionTypeNotFound(index.to_string()))?;
        Ok(func_type)
    }

    pub fn get_code(&self, index: u32) -> Result<&Func<'a>> {
        self.code
            .get(index as usize)
            .with_context(|| RuntimeError::FunctionNotFound(index.to_string()))
    }
}
