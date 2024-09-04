(module
  (func (export "_start") (result i64)
    i64.const 100
    i64.const 200
    call $add
    i64.const 300
    i64.const 400
    call $sub
    call $add
  )
  (func $add (param i64 i64) (result i64)
    local.get 0
    local.get 1
    i64.add
  )
  (func $sub (param i64 i64) (result i64)
    local.get 0
    local.get 1
    i64.sub
  )
)
