use anyhow::Result;
use wasm_jit::{
    parser,
    runtime::{Runtime, Value},
};

fn main() -> Result<()> {
    let bytes = include_bytes!("../tests/wasm/fib.wasm");
    let modules = parser::parse(bytes)?;
    let mut runtime = Runtime::init(modules);
    let result = runtime.call_func_by_name("fib", &[Value::I64(10)])?;
    println!("{:?}", result);
    let start = std::time::Instant::now();
    let result = runtime.call_func_by_name("fib", &[Value::I64(30)])?;
    println!("Elapsed: {:?}", start.elapsed());
    println!("{:?}", result);

    Ok(())
}
