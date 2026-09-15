//! Encoding gate for the WASI p3 async ABI.
//!
//! This is the one test that pins the *encoder* contract rather than any Dovetail
//! program: it builds a core module by hand and runs it, so a wit-component or
//! wasmtime upgrade that renames a builtin, changes a mangling, or drops a
//! feature fails here — where the cause is one line of WAT — instead of showing
//! up as every p3 program in the suite failing to instantiate.
//!
//! Proves that the production component-encoding path (`embed_component_metadata`
//! then `ComponentEncoder`) can express the full async ABI Dovetail's codegen will
//! emit: an async-lowered import, the waitable-set builtins, `task.return`, and
//! both export lift shapes. The core module is the spike's E1 and E5 guests
//! rewritten against wit-component's "Legacy" name manglings:
//!
//! ```text
//! async-lowered import:   module `<pkg>/<iface>`, field `[async-lower]<fn>`
//! waitable builtins:      module `$root`, fields `[waitable-set-new]`,
//!                         `[waitable-join]`, `[waitable-set-wait]`,
//!                         `[waitable-set-drop]`, `[subtask-drop]`
//! callback-lifted export: `[async-lift]<name>` + `[callback][async-lift]<name>`
//! stackful export:        `[async-lift-stackful]<name>`, core func returning
//!                         nothing
//! task.return, world:     module `[export]$root`, field `[task-return]<name>`
//! task.return, interface: module `[export]<pkg>/<iface>`, field
//!                         `[task-return]<name>`, export name
//!                         `[async-lift-stackful]<pkg>/<iface>#<name>`
//! ```
//!
//! The stackful lift is the one Dovetail's own exports use
//! (`emit_export_section`), and it is here because the two lifts fail
//! differently: a callback lift returns a code to the host at every suspension
//! point, while a stackful lift blocks inside `waitable-set.wait` in the middle
//! of its own control flow — which the host permits only with
//! `wasm_component_model_async_stackful` (see `p3::configure_p3_engine`), and
//! whose value comes back through `task.return` rather than a core return.
//! Nothing else in the suite would say which of those an upgrade broke.
//!
//! Both SCOPES of it are pinned, because they mangle differently and Dovetail
//! emits one of each: the test wrappers are world-scoped (`[task-return]test-n0`
//! from `[export]$root`), while `run` is interface-scoped — export
//! `[async-lift-stackful]wasi:cli/run@<ver>#run`, its `task.return` imported
//! from `[export]wasi:cli/run@<ver>` rather than from `[export]$root`. A rename
//! on the interface-scoped side would take out every Dovetail program built for
//! the CLI world while leaving a world-scoped fixture perfectly green.
//!
//! Semantics asserted:
//! - `run` (callback, world-scoped): two async-lowered host calls (100ms tag=1,
//!   30ms tag=2) interleave in one waitable-set on a single task; the 30ms op
//!   completes first, so the guest returns 2*100 + 42 = 242.
//! - `run-stackful` (world-scoped): one 20ms call, awaited by blocking mid-body
//!   on `waitable-set.wait`, handed back with `task.return` as 7 + 1000 = 1007.
//! - `spike:gate/runner#go` (interface-scoped): the same stackful shape reached
//!   through an exported interface — one 10ms call, returned as 5 + 2000 = 2005.

use std::time::Duration;
use wasmtime::component::{Component, Linker, Val};
use wasmtime::{Config, Engine, Store};
use wit_component::StringEncoding;
use wit_parser::Resolve;

const WIT: &str = r#"
package spike:gate;

interface host {
  slow: async func(ms: u32, tag: u32) -> u32;
}

interface runner {
  go: async func() -> u32;
}

world gate {
  import host;
  export run: async func() -> u32;
  export run-stackful: async func() -> u32;
  export runner;
}
"#;

const CORE_WAT: &str = r#"
(module
  (import "spike:gate/host" "[async-lower]slow" (func $slow (param i32 i32 i32) (result i32)))
  (import "$root" "[waitable-set-new]" (func $ws_new (result i32)))
  (import "$root" "[waitable-join]" (func $join (param i32 i32)))
  (import "$root" "[waitable-set-wait]" (func $ws_wait (param i32 i32) (result i32)))
  (import "$root" "[waitable-set-drop]" (func $ws_drop (param i32)))
  (import "$root" "[subtask-drop]" (func $subtask_drop (param i32)))
  (import "[export]$root" "[task-return]run" (func $task_return (param i32)))
  (import "[export]$root" "[task-return]run-stackful" (func $task_return_stackful (param i32)))
  ;; Interface-scoped: the task.return of an exported INTERFACE comes from a
  ;; module named for that interface, not from `[export]$root`. This is the shape
  ;; Dovetail's `run` export uses.
  (import "[export]spike:gate/runner" "[task-return]go" (func $task_return_go (param i32)))

  (memory (export "memory") 1)
  (global $bump (mut i32) (i32.const 0x100))
  (func (export "cabi_realloc") (param i32 i32 i32 i32) (result i32)
    (local $ret i32)
    (local.set $ret (global.get $bump))
    (global.set $bump (i32.add (global.get $bump) (local.get 3)))
    (local.get $ret))

  (global $s1 (mut i32) (i32.const 0))
  (global $s2 (mut i32) (i32.const 0))
  (global $ws (mut i32) (i32.const 0))
  (global $count (mut i32) (i32.const 0))
  (global $first (mut i32) (i32.const 0))

  ;; retptrs: 0x20 = slow(100,1) result, 0x30 = slow(30,2) result

  (func $start (param $ms i32) (param $tag i32) (param $retp i32) (result i32)
    (local $ret i32)
    (local.set $ret (call $slow (local.get $ms) (local.get $tag) (local.get $retp)))
    ;; expect state STARTED (1)
    (if (i32.ne (i32.and (local.get $ret) (i32.const 0xf)) (i32.const 1)) (then unreachable))
    (i32.shr_u (local.get $ret) (i32.const 4)))

  (func (export "[async-lift]run") (result i32)
    (global.set $s1 (call $start (i32.const 100) (i32.const 1) (i32.const 0x20)))
    (global.set $s2 (call $start (i32.const 30) (i32.const 2) (i32.const 0x30)))
    (global.set $ws (call $ws_new))
    (call $join (global.get $s1) (global.get $ws))
    (call $join (global.get $s2) (global.get $ws))
    ;; CALLBACK_CODE_WAIT | ws << 4
    (i32.or (i32.const 2) (i32.shl (global.get $ws) (i32.const 4))))

  (func (export "[callback][async-lift]run") (param $event i32) (param $index i32) (param $payload i32) (result i32)
    (if (i32.ne (local.get $event) (i32.const 1)) (then unreachable))   ;; EVENT_SUBTASK
    (if (i32.ne (local.get $payload) (i32.const 2)) (then unreachable)) ;; STATE_RETURNED
    (global.set $count (i32.add (global.get $count) (i32.const 1)))
    (if (i32.eq (global.get $count) (i32.const 1))
      (then
        (if (i32.eq (local.get $index) (global.get $s1))
          (then (global.set $first (i32.load (i32.const 0x20))))
          (else (global.set $first (i32.load (i32.const 0x30)))))))
    (call $subtask_drop (local.get $index))
    (if (result i32) (i32.eq (global.get $count) (i32.const 2))
      (then
        (call $task_return (i32.add (i32.mul (global.get $first) (i32.const 100)) (i32.const 42)))
        (i32.const 0)) ;; CALLBACK_CODE_EXIT
      (else (i32.or (i32.const 2) (i32.shl (global.get $ws) (i32.const 4))))))

  ;; The shape Dovetail's own exports use: no callback, no core result. It blocks
  ;; on waitable-set.wait halfway through its own control flow — the thing a
  ;; callback lift cannot express — and delivers the value with task.return.
  ;; retptrs: 0x40 = slow(20,7) result, 0x50 = wait's (index, payload) pair.
  (func (export "[async-lift-stackful]run-stackful")
    (local $sub i32) (local $ws i32)
    (local.set $sub (call $start (i32.const 20) (i32.const 7) (i32.const 0x40)))
    (local.set $ws (call $ws_new))
    (call $join (local.get $sub) (local.get $ws))
    (if (i32.ne (call $ws_wait (local.get $ws) (i32.const 0x50)) (i32.const 1))
      (then unreachable))                                        ;; EVENT_SUBTASK
    (if (i32.ne (i32.load (i32.const 0x50)) (local.get $sub)) (then unreachable))
    (if (i32.ne (i32.load (i32.const 0x54)) (i32.const 2)) (then unreachable)) ;; RETURNED
    (call $subtask_drop (local.get $sub))
    (call $ws_drop (local.get $ws))
    (call $task_return_stackful (i32.add (i32.load (i32.const 0x40)) (i32.const 1000))))

  ;; The same stackful shape, reached through an exported interface: the export
  ;; name carries `<pkg>/<iface>#<fn>` and the lift is otherwise identical.
  ;; retptrs: 0x60 = slow(10,5) result, 0x70 = wait's (index, payload) pair.
  (func (export "[async-lift-stackful]spike:gate/runner#go")
    (local $sub i32) (local $ws i32)
    (local.set $sub (call $start (i32.const 10) (i32.const 5) (i32.const 0x60)))
    (local.set $ws (call $ws_new))
    (call $join (local.get $sub) (local.get $ws))
    (if (i32.ne (call $ws_wait (local.get $ws) (i32.const 0x70)) (i32.const 1))
      (then unreachable))                                        ;; EVENT_SUBTASK
    (if (i32.ne (i32.load (i32.const 0x70)) (local.get $sub)) (then unreachable))
    (if (i32.ne (i32.load (i32.const 0x74)) (i32.const 2)) (then unreachable)) ;; RETURNED
    (call $subtask_drop (local.get $sub))
    (call $ws_drop (local.get $ws))
    (call $task_return_go (i32.add (i32.load (i32.const 0x60)) (i32.const 2000))))
)
"#;

fn encode_component() -> Vec<u8> {
    let mut resolve = Resolve::default();
    let pkg = resolve
        .push_str("gate.wit", WIT)
        .expect("parse gate WIT");
    let world = resolve
        .select_world(&[pkg], Some("gate"))
        .expect("select gate world");

    let mut module = wat::parse_str(CORE_WAT).expect("assemble core module");
    wit_component::embed_component_metadata(&mut module, &resolve, world, StringEncoding::UTF8)
        .expect("embed component metadata");

    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .expect("set module")
        .encode()
        .expect("encode component")
}

#[test]
fn p3_async_component_encodes_and_runs() {
    let component_bytes = encode_component();

    let mut config = Config::new();
    dovetail::p3::configure_p3_engine(&mut config);
    let engine = Engine::new(&config).expect("engine");

    let component = Component::new(&engine, &component_bytes).expect("load component");

    let mut linker: Linker<()> = Linker::new(&engine);
    let mut host = linker.instance("spike:gate/host").expect("host instance");
    host.func_wrap_concurrent("slow", |_acc, (ms, tag): (u32, u32)| {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(ms as u64)).await;
            Ok((tag,))
        })
    })
    .expect("define slow");

    let runtime = dovetail::p3::runtime().expect("tokio runtime");
    runtime.block_on(async {
        let mut store = Store::new(&engine, ());
        let instance = linker
            .instantiate_async(&mut store, &component)
            .await
            .expect("instantiate");
        let run = instance.get_func(&mut store, "run").expect("run export");

        let mut results = [Val::U32(0)];
        store
            .run_concurrent(async |acc| run.call_concurrent(acc, &[], &mut results).await)
            .await
            .expect("run_concurrent")
            .expect("call run");

        assert_eq!(
            results[0],
            Val::U32(242),
            "30ms op must complete first: async-lowered imports did not interleave"
        );

        let run_stackful = instance
            .get_func(&mut store, "run-stackful")
            .expect("run-stackful export");
        let mut stackful_results = [Val::U32(0)];
        store
            .run_concurrent(async |acc| {
                run_stackful
                    .call_concurrent(acc, &[], &mut stackful_results)
                    .await
            })
            .await
            .expect("run_concurrent")
            .expect("call run-stackful");

        assert_eq!(
            stackful_results[0],
            Val::U32(1007),
            "a stackful lift must be able to block mid-body and return via task.return"
        );

        // Interface-scoped: two lookups, because the export is a func inside an
        // exported instance rather than a func on the world.
        let runner_iface = instance
            .get_export_index(&mut store, None, "spike:gate/runner")
            .expect("runner interface export");
        let go_idx = instance
            .get_export_index(&mut store, Some(&runner_iface), "go")
            .expect("go export");
        let go = instance.get_func(&mut store, go_idx).expect("go func");
        let mut go_results = [Val::U32(0)];
        store
            .run_concurrent(async |acc| go.call_concurrent(acc, &[], &mut go_results).await)
            .await
            .expect("run_concurrent")
            .expect("call go");

        assert_eq!(
            go_results[0],
            Val::U32(2005),
            "an interface-scoped stackful lift must mangle as `<pkg>/<iface>#<fn>` and take its task.return from `[export]<pkg>/<iface>`"
        );
    });
}
