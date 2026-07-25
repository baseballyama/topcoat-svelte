// Minimal reproduction of the Boa 0.21.1 panic that blocks Svelte 5 SSR.
// A class CONSTRUCTOR that declares a block-scoped binding (let/const) and
// captures it in a closure created inside the constructor.
// Boa panics: "index out of bounds" at vm/opcode/define/mod.rs:82 (PutLexicalValue,
// code_block.bindings is empty). Node and QuickJS both return 1.
// This is exactly svelte SSRState's constructor: `let uid = 1; this.uid = () => `${p}s${uid++}``.
class S {
  constructor() {
    let u = 1;
    this.f = () => u++;
  }
}
globalThis.renderCounter = function () { return "" + new S().f(); };
