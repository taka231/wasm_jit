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
