use anyhow::Result;
use wasm_jit::{
    parser,
    runtime::{Runtime, Value},
};

fn main() -> Result<()> {
    let bytes = include_bytes!("../tests/wasm/add_with_arg.wasm");
    let modules = parser::parse(bytes)?;
    println!("{:?}", modules);
    let mut runtime = Runtime::init(modules);
    let result = runtime.call_func_by_name("add", &[Value::I64(1000), Value::I64(2000)])?;
    println!("{:?}", result);

    Ok(())
}
