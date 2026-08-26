(module
  ;; core kernel set 1 (cordis-rs): effect write records an inverse host-side.
  (import "host" "ctx_set" (func $ctx_set (param i32 i32 i32 i32)))

  (memory (export "memory") 1)

  ;; string pool
  (data (i32.const 0) "panel_width_percent")
  (data (i32.const 0x40) "22")
  (data (i32.const 0x80) "keybinds")
  (data (i32.const 0xC0) "{}")

  ;; Config is a write-only extension; mount populates config keys via ctx_set.
  (func (export "mount")
    (call $ctx_set (i32.const 0) (i32.const 19) (i32.const 0x40) (i32.const 2))
    (call $ctx_set (i32.const 0x80) (i32.const 8) (i32.const 0xC0) (i32.const 2))
  )

  ;; Required ABI exports; config never receives changes, so these are stubs.
  (func (export "on_change") (param i32 i32)
    (drop (local.get 0))
    (drop (local.get 1)))
  (func (export "scratch") (result i32 i32)
    (i32.const 0) (i32.const 0))
)
