use anyhow::Result;
use wasmparser::Parser;

fn main() -> Result<()> {
    let bytes = include_bytes!("../tests/wasm/add.wasm");
    let parser = Parser::new(0);

    for payload in parser.parse_all(bytes) {
        let payload = payload?;
        println!("{:?}", payload);
    }
    Ok(())
}
