(module
  (func (export "i64eq") (param i64 i64) (result i32)
    local.get 0
    local.get 1
    i64.eq
  )
  (func (export "i32eq") (param i32 i32) (result i32)
    local.get 0
    local.get 1
    i32.eq
  )
)
