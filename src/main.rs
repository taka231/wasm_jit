use anyhow::Result;
use wasm_jit::parser;

fn main() -> Result<()> {
    let bytes = include_bytes!("../tests/wasm/add.wasm");
    let modules = parser::parse(bytes)?;
    println!("{:?}", modules);

    Ok(())
}
