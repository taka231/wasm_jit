use wasmparser::{Export, FuncType, Operator};

#[derive(Debug, Default)]
pub struct WasmModule<'a> {
    pub types: Vec<FuncType>,
    pub funcs: Vec<u32>,
    pub code: Vec<Vec<Operator<'a>>>,
    pub exports: Vec<Export<'a>>,
}
