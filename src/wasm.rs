use wasmparser::{Export, FuncType, Operator, ValType};

#[derive(Debug, Default)]
pub struct WasmModule<'a> {
    pub types: Vec<FuncType>,
    pub funcs: Vec<u32>,
    pub code: Vec<Func<'a>>,
    pub exports: Vec<Export<'a>>,
}

#[derive(Debug)]
pub struct Func<'a> {
    pub locals: Vec<(u32, ValType)>,
    pub body: Vec<Operator<'a>>,
}
