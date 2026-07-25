# Draft issue for boa-dev/boa

Title: **Panic (index out of bounds in PutLexicalValue) when a class constructor's closure captures a block-scoped binding**

---

## Describe the bug

Boa 0.21.1 panics with a Rust `index out of bounds` (not a JS exception) when
evaluating a class whose **constructor** declares a `let`/`const` binding that
is captured by a closure created inside that constructor.

```
panicked at boa_engine-0.21.1/src/vm/opcode/define/mod.rs:82
```

(`PutLexicalValue` executes with an empty `code_block.bindings`.)

## To reproduce

```js
class S {
  constructor() {
    let u = 1;
    this.f = () => u++;
  }
}
new S().f(); // Boa 0.21.1: panic — Node and QuickJS: returns 1
```

```rust
use boa_engine::{Context, Source};

fn main() {
    let mut context = Context::default();
    let src = r#"
        class S { constructor() { let u = 1; this.f = () => u++; } }
        new S().f();
    "#;
    let result = context.eval(Source::from_bytes(src));
    println!("{result:?}"); // never reached: panics inside eval
}
```

## Narrowing

- Constructor + `let`/`const` captured by a closure created in the constructor → panic
- Same shape in an ordinary function or a non-constructor class method → works
- Constructor with `var` instead of `let` → works
- Constructor with `let` that is **not** captured → works

## Impact

This pattern appears verbatim in Svelte 5's server runtime
(`svelte/internal/server`, class `SSRState`):

```js
constructor(id_prefix) {
  let uid = 1;
  this.uid = () => `${id_prefix}s${uid++}`;
}
```

which is on the mandatory path of every server render, so Boa currently cannot
be used to server-side-render Svelte 5 components. The reproduction is
unaffected by transpilation target (es2020–esnext).

## Environment

- boa_engine 0.21.1 (crates.io), default features
- rustc 1.96.0, macOS (darwin 25.3.0, arm64)
