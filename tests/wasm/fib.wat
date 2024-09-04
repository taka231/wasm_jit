(module
  (func $fib (param $n i64) (result i64)
    (if (result i64) (i64.eq (local.get $n) (i64.const 0))
      (then (i64.const 0))
      (else
        (if (result i64) (i64.eq (local.get $n) (i64.const 1))
          (then (i64.const 1))
          (else
            (i64.add
              (call $fib (i64.sub (local.get $n) (i64.const 1)))
              (call $fib (i64.sub (local.get $n) (i64.const 2)))
            )
          )
        )
      )
    )
  )
  (export "fib" (func $fib))
)
