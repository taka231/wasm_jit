(module
  (func (export "add") (param i64) (param i64) (result i64)
    local.get 0
    local.get 1
    i64.add
  )
  (func (export "add32") (param i32) (param i32) (result i32)
    local.get 0
    local.get 1
    i32.add
  )
)
