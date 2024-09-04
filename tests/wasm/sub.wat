(module
  (func (export "sub") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.sub
  )
  (func (export "i64sub") (param i64 i64) (result i64)
    local.get 0
    local.get 1
    i64.sub
  )
)
