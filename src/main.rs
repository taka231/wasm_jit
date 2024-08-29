use anyhow::Result;
use wasm_jit::{
    parser,
    runtime::{Runtime, Value},
};

fn main() -> Result<()> {
    let bytes = include_bytes!("../tests/wasm/call.wasm");
    let modules = parser::parse(bytes)?;
    println!("{:?}", modules);
    let mut runtime = Runtime::init(modules)?;
    let result = runtime.call_func_by_name("_start", &[])?;
    println!("{:?}", result);

    Ok(())
}
