use anyhow::Result;
use wasm_jit::{
    parser,
    runtime::{Runtime, Value},
};

#[test]
fn test_add() -> Result<()> {
    let bytes = include_bytes!("../tests/wasm/add.wasm");
    let modules = parser::parse(bytes)?;
    let mut runtime = Runtime::init(modules);
    let result = runtime.call_func_by_name("_start", &[])?;
    assert_eq!(result, vec![Value::I64(30)]);

    Ok(())
}

#[test]
fn test_add_with_args() -> Result<()> {
    let bytes = include_bytes!("../tests/wasm/add_with_arg.wasm");
    let modules = parser::parse(bytes)?;
    let mut runtime = Runtime::init(modules);
    let result = runtime.call_func_by_name("add", &[Value::I64(10), Value::I64(20)])?;
    assert_eq!(result, vec![Value::I64(30)]);
    let result = runtime.call_func_by_name("add", &[Value::I64(1000), Value::I64(2000)])?;
    assert_eq!(result, vec![Value::I64(3000)]);
    let result = runtime.call_func_by_name("add32", &[Value::I32(10), Value::I32(20)])?;
    assert_eq!(result, vec![Value::I32(30)]);
    let result = runtime.call_func_by_name("add32", &[Value::I32(1000), Value::I32(2000)])?;
    assert_eq!(result, vec![Value::I32(3000)]);

    Ok(())
}

#[test]
fn test_call() -> Result<()> {
    let bytes = include_bytes!("../tests/wasm/call.wasm");
    let modules = parser::parse(bytes)?;
    let mut runtime = Runtime::init(modules);
    let result = runtime.call_func_by_name("_start", &[])?;
    assert_eq!(result, vec![Value::I64(300)]);

    Ok(())
}
