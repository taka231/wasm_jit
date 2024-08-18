use anyhow::Result;
use wasmparser::{Parser, Payload::*};

use crate::wasm::WasmModule;

pub fn parse(buf: &[u8]) -> Result<WasmModule<'_>> {
    let parser = Parser::new(0);
    let mut module = WasmModule::default();

    for payload in parser.parse_all(buf) {
        match payload? {
            TypeSection(types) => {
                for ty in types.into_iter_err_on_gc_types() {
                    module.types.push(ty?);
                }
            }
            FunctionSection(funcs) => {
                for func in funcs {
                    module.funcs.push(func?);
                }
            }
            CodeSectionEntry(body) => {
                let body = body.get_operators_reader()?;
                let mut instrs = Vec::new();
                for instr in body {
                    instrs.push(instr?);
                }
                module.code.push(instrs);
            }
            ExportSection(exports) => {
                for export in exports {
                    module.exports.push(export?);
                }
            }
            _ => {}
        }
    }
    Ok(module)
}
