# Cranelift + Garbage Collection Research

> Research notes for moving Dovetail from WASM codegen to Cranelift with a tracing GC.

## Table of Contents

### Part 1: GC and Stack Maps
1. [The Big Picture](#the-big-picture)
2. [What Are Stack Maps?](#what-are-stack-maps)
3. [Cranelift's User Stack Maps API](#cranelift-user-stack-maps-api)
4. [End-to-End Example](#end-to-end-example)
5. [Extracting Stack Maps After Compilation](#extracting-stack-maps-after-compilation)
6. [Safepoints in Loops](#safepoints-in-loops)
7. [Building a GC Runtime](#building-a-gc-runtime)
8. [Multi-Threading and GC](#multi-threading-and-gc)
9. [Write Barriers (for Concurrent/Generational GC)](#write-barriers)
10. [How Wasmtime Does It](#how-wasmtime-does-it)
11. [How Other Runtimes Do It](#how-other-runtimes-do-it)

### Part 2: Linking Runtime with Compiled Code
12. [JIT Mode (cranelift-jit)](#jit-mode-cranelift-jit)
13. [AOT Mode (cranelift-object)](#aot-mode-cranelift-object)
14. [The VMContext Pattern](#the-vmcontext-pattern)
15. [GlobalValues in Cranelift](#globalvalues-in-cranelift)
16. [ABI and Calling Conventions](#abi-and-calling-conventions)

### Part 3: Putting It Together
17. [What Dovetail Would Need](#what-dovetail-would-need)
18. [Sources](#sources)

---

## The Big Picture

When you compile a language with GC to native code, you have a fundamental problem:
the GC needs to find every live object reference on every thread's stack, but the
compiler puts values in registers and stack slots wherever it wants. **Stack maps**
solve this by recording, for each "pause point" in the code, exactly which stack
slots contain GC references.

The flow looks like this:

```
Your compiler (Dovetail)
    |
    | generates Cranelift IR, marking which values are GC refs
    v
Cranelift
    |
    | compiles to native code + stack map metadata
    v
Native code + Stack Maps
    |
    | at runtime, when GC triggers...
    v
GC Runtime
    1. Pauses the thread(s)
    2. Walks each thread's stack frames
    3. At each frame, looks up the stack map by return address
    4. Reads GC references from the recorded stack slots
    5. Traces from those roots, collects garbage
    6. (If moving GC) Updates the references on the stack
    7. Resumes the thread(s)
```

The key insight: **Cranelift handles the compiler side** (generating stack maps),
but **you build the runtime side** (the actual GC).

---

## What Are Stack Maps?

A stack map is a table that says:

> "At code offset X, with frame size Y, stack slots at offsets [A, B, C] from
> the stack pointer contain live GC references."

Concretely, imagine a function's stack frame:

```
High addresses
┌──────────────────┐
│   return addr     │  ← saved by call instruction
├──────────────────┤
│   saved rbp       │
├──────────────────┤
│   local: counter  │  SP+24  (just an int, not a GC ref)
├──────────────────┤
│   local: name     │  SP+16  ← GC reference (String object)
├──────────────────┤
│   local: user     │  SP+8   ← GC reference (User object)
├──────────────────┤
│   local: temp     │  SP+0   (just an int, not a GC ref)
└──────────────────┘
Low addresses (SP points here)
```

The stack map for a safepoint in this function would be:

```
code_offset: 0x42, frame_size: 32, entries: [(I64, 8), (I64, 16)]
                                               ^          ^
                                           user @ SP+8  name @ SP+16
```

The GC reads these two slots, follows the pointers to the heap objects, and marks
them as live. If it's a **moving GC**, it relocates the objects and writes the new
addresses back into SP+8 and SP+16.

---

## Cranelift User Stack Maps API

Since 2024, Cranelift uses a **"user stack maps"** design. The key idea: **your
frontend** (Dovetail's codegen) tells Cranelift which values are GC references.
Cranelift then **automatically** handles spilling them to the stack before
safepoints (calls) and reloading them after. The mid-end, backends, and register
allocator don't need to know anything about GC.

### The Two Key Methods on FunctionBuilder

```rust
// Mark a variable as GC-managed. All SSA values derived from it will be
// automatically spilled before safepoints and reloaded after.
builder.declare_var_needs_stack_map(my_var);

// Mark a single SSA value as GC-managed. Same spill/reload behavior.
builder.declare_value_needs_stack_map(my_value);
```

That's it. You call one of these, and Cranelift does the rest.

### What Cranelift Does Behind the Scenes

When you call `builder.finalize()`, Cranelift runs a **safepoint spiller** pass:

1. **Liveness analysis** (backward dataflow): figures out which GC values are live
   at each safepoint (call instruction).

2. **Spill insertion**: before each safepoint, stores live GC values to stack slots.

3. **Stack map annotation**: attaches `stack_map = [...]` to each safepoint,
   listing which stack slots hold GC refs.

4. **Reload insertion**: after each safepoint, loads GC values back from the stack
   slots (since a moving GC might have updated them).

### What the CLIF IR Looks Like

Before the safepoint pass (what you write):
```
v0 = <some GC reference>       ;; you marked this with declare_value_needs_stack_map
call $might_trigger_gc()        ;; this is a safepoint
use v0                          ;; v0 might be stale if GC moved the object!
```

After the safepoint pass (what Cranelift transforms it into):
```
v0 = <some GC reference>
store.i64 notrap (stack_addr ss0), v0       ;; spill to stack slot ss0

call $might_trigger_gc(), stack_map = [i64 @ ss0]   ;; safepoint annotated

v1 = load.i64 notrap (stack_addr ss0)       ;; reload (GC may have updated ss0)
use v1                                       ;; v1 is the correct, updated ref
```

### The Data Structures

```rust
/// One entry in a stack map: "stack slot X contains a GC ref of type T"
pub struct UserStackMapEntry {
    pub ty: ir::Type,        // e.g., I64 for a 64-bit pointer
    pub slot: ir::StackSlot, // which stack slot (e.g., ss0)
    pub offset: u32,         // byte offset within the slot (usually 0)
}

/// A compiled stack map — the final form after code generation.
/// Internally stores entries as a bitset indexed by SP-relative offset.
pub struct UserStackMap { /* ... */ }

impl UserStackMap {
    /// Iterate over entries: yields (Type, SP-relative offset) pairs.
    /// Example: (I64, 16) means "SP+16 contains a live GC ref of type I64"
    pub fn entries(&self) -> impl Iterator<Item = (ir::Type, u32)>;
}
```

The compiled output gives you a list of these per function:

```rust
/// After compilation, you get a slice of:
///   (code_offset, frame_size, stack_map)
///
/// code_offset: byte offset within the function's machine code
/// frame_size:  size of the stack frame at that point
/// stack_map:   which SP-relative offsets contain GC refs
&[(CodeOffset, u32, UserStackMap)]
```

---

## End-to-End Example

Here's a complete example showing how to build a Cranelift function with GC
reference tracking:

```rust
use cranelift::prelude::*;
use cranelift_codegen::ir::types;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Module, Linkage};

fn build_function_with_gc(module: &mut JITModule) {
    let ptr_ty = module.target_config().pointer_type(); // I64 on 64-bit

    // Function signature: fn(gc_ref: I64) -> I64
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(ptr_ty));
    sig.returns.push(AbiParam::new(ptr_ty));

    let func_id = module
        .declare_function("my_func", Linkage::Export, &sig)
        .unwrap();

    let mut ctx = module.make_context();
    ctx.func.signature = sig;

    let mut fb_ctx = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);

        // Declare a variable for the GC reference
        let gc_var = Variable::new(0);
        builder.declare_var(gc_var, ptr_ty);

        // *** THIS IS THE KEY LINE ***
        // Tell Cranelift this variable holds a GC reference.
        // Cranelift will auto-spill it before calls and reload after.
        builder.declare_var_needs_stack_map(gc_var);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        // Bind the function parameter to our GC variable
        let param_val = builder.block_params(entry_block)[0];
        builder.def_var(gc_var, param_val);

        // Declare an external function that might trigger GC
        let mut callee_sig = module.make_signature();
        callee_sig.params.push(AbiParam::new(ptr_ty));
        let callee_id = module
            .declare_function("may_trigger_gc", Linkage::Import, &callee_sig)
            .unwrap();
        let callee_ref = module.declare_func_in_func(callee_id, builder.func);

        // Use gc_var — Cranelift sees it's live across the upcoming call
        let gc_ref = builder.use_var(gc_var);

        // This call is a SAFEPOINT. Cranelift will automatically:
        //   1. Spill gc_ref to a stack slot before this call
        //   2. Annotate the call with stack_map = [I64 @ ss0]
        //   3. Reload gc_ref from the stack slot after this call
        builder.ins().call(callee_ref, &[gc_ref]);

        // Use gc_var again — gets the reloaded value (safe even if GC moved it)
        let gc_ref_after = builder.use_var(gc_var);
        builder.ins().return_(&[gc_ref_after]);

        // finalize() triggers the safepoint spiller pass
        builder.finalize();
    }

    // Compile — stack maps are generated as part of this
    module.define_function(func_id, &mut ctx).unwrap();
    module.clear_context(&mut ctx);
    module.finalize_definitions().unwrap();
}
```

### Marking Individual Values (Alternative)

Instead of marking a whole variable, you can mark specific values:

```rust
let gc_ref = builder.ins().load(ptr_ty, MemFlags::new(), some_ptr, 0);
builder.declare_value_needs_stack_map(gc_ref);  // just this value
```

This is what Wasmtime does when loading GC refs from struct fields:

```rust
// From Wasmtime's GC compiler
let gc_ref = builder.ins().load(ir::types::I32, flags, ptr_to_gc_ref, 0);
if ty != WasmHeapType::I31 {
    builder.declare_value_needs_stack_map(gc_ref);
}
```

---

## Extracting Stack Maps After Compilation

After compiling a function, you need to extract the stack maps and store them
in a lookup table for your GC runtime.

```rust
// Compile the function
let compiled_code = ctx.compile(isa, &mut ctrl_plane)?;

// Get the stack maps
let stack_maps: &[(CodeOffset, u32, UserStackMap)] =
    compiled_code.buffer.user_stack_maps();

// Build a lookup table: instruction_pointer -> (frame_size, gc_root_offsets)
let mut gc_table: HashMap<usize, (u32, Vec<u32>)> = HashMap::new();

let function_base_address: usize = /* where the function's code lives in memory */;

for (code_offset, frame_size, stack_map) in stack_maps {
    let absolute_pc = function_base_address + *code_offset as usize;

    let gc_root_offsets: Vec<u32> = stack_map
        .entries()
        .map(|(_ty, sp_offset)| sp_offset)
        .collect();

    // At this PC, these SP-relative offsets contain GC refs
    gc_table.insert(absolute_pc, (*frame_size, gc_root_offsets));
}

// Or, to take ownership (moves the data out):
let owned: SmallVec<[(CodeOffset, u32, UserStackMap); 8]> =
    compiled_code.buffer.take_user_stack_maps();
```

At GC time, your runtime does:

```rust
fn find_gc_roots(stack_frames: &[StackFrame]) -> Vec<*mut u8> {
    let mut roots = Vec::new();
    for frame in stack_frames {
        if let Some((frame_size, offsets)) = gc_table.get(&frame.return_address) {
            for offset in offsets {
                // The GC reference lives at SP + offset
                let root_ptr = (frame.sp as usize + *offset as usize) as *mut *mut u8;
                let gc_ref = unsafe { *root_ptr };
                if !gc_ref.is_null() {
                    roots.push(gc_ref);
                }
            }
        }
        // Frames without stack maps have no GC refs — skip them
    }
    roots
}
```

---

## Safepoints in Loops

**Problem:** Cranelift only treats **call instructions** as safepoints. A tight
loop with no calls will never hit a safepoint, meaning GC can't run and the
thread can't be paused.

```
// This loop never reaches a safepoint!
loop:
    v1 = load.i64 v_array, v_index
    v_index = iadd_imm v_index, 1
    v_cmp = icmp ult v_index, v_len
    brif v_cmp, loop, exit
```

**Solution:** Insert an explicit GC poll at loop back-edges. Three strategies,
from simplest to most optimized:

### Strategy 1: Call a GC Poll Function (Simplest)

Your compiler inserts a call to a runtime function at every loop back-edge:

```
loop:
    ;; ... loop body ...

    ;; GC poll: this is a call, so Cranelift treats it as a safepoint
    ;; and auto-generates stack maps for any live GC refs
    call $gc_safepoint_poll()

    brif v_cmp, loop, exit
```

The `gc_safepoint_poll` function checks a flag and returns immediately if no GC
is needed. Cost: one function call per loop iteration (~5-10ns). Simple and correct.

```rust
// The runtime function:
extern "C" fn gc_safepoint_poll() {
    if GC_REQUESTED.load(Ordering::Relaxed) {
        // Full GC pause: save state, do collection, resume
        do_gc_collection();
    }
    // Otherwise: return immediately (fast path)
}
```

### Strategy 2: Inline Flag Check (Faster)

Check a flag inline, only call the runtime on the slow path:

```
loop:
    ;; ... loop body ...

    ;; Fast path: load flag, branch over slow path
    v_flag = load.i8 notrap gv_gc_flag
    brif v_flag, block_gc_slow, block_continue

block_gc_slow:
    ;; Slow path: this call is the safepoint (stack maps generated here)
    call $gc_safepoint_slow()
    jump block_continue

block_continue:
    brif v_cmp, loop, exit
```

Cost: one memory load + one (predicted-not-taken) branch per iteration. ~1-2ns.

### Strategy 3: Page-Trap Poll (Fastest, JVM-Style)

Load from a memory page that the runtime can make inaccessible:

```
loop:
    ;; ... loop body ...

    ;; Just load from the poll page. Costs ~1 cycle (L1 cache hit).
    ;; When GC needed: runtime calls mprotect(PROT_NONE) on this page,
    ;; causing a SIGSEGV. Signal handler parks the thread.
    v_poll = load.i32 notrap gv_poll_page

    brif v_cmp, loop, exit
```

Cost: ~1 cycle when GC is not requested (cached load). The SIGSEGV handler
must save register state and coordinate the GC pause. More complex to implement
but essentially zero overhead on the fast path.

---

## Building a GC Runtime

Here are the pieces you need to build, explained step by step.

### Piece 1: The Heap

A contiguous region of memory where GC-managed objects live.

```rust
struct GcHeap {
    memory: *mut u8,     // mmap'd region
    size: usize,         // total size
    bump_ptr: *mut u8,   // next allocation point (for bump allocator)
}

impl GcHeap {
    fn alloc(&mut self, size: usize, align: usize) -> *mut u8 {
        let aligned = align_up(self.bump_ptr, align);
        let end = aligned.add(size);
        if end > self.memory.add(self.size) {
            // Out of space — trigger GC, or grow heap
            return std::ptr::null_mut();
        }
        self.bump_ptr = end;
        aligned
    }
}
```

### Piece 2: Object Layout

Every object on the heap needs a header so the GC knows its type and size.

```
┌──────────────┬──────────────┬──────────────────┐
│  GC header   │  Type info   │   Fields...      │
│  (mark bits, │  (vtable or  │                  │
│   fwd ptr)   │   type ID)   │                  │
└──────────────┴──────────────┴──────────────────┘
```

```rust
#[repr(C)]
struct GcHeader {
    mark: u8,           // 0=white, 1=grey, 2=black (for tri-color marking)
    type_id: u32,       // index into a type info table
    // For moving GC, add: forwarding_ptr: *mut u8
}
```

### Piece 3: The Stack Map Table

Built from Cranelift's output (see [Extracting Stack Maps](#extracting-stack-maps-after-compilation)).
Maps instruction pointers to stack slot locations of GC refs.

### Piece 4: Stack Walking

At GC time, walk each thread's stack to find all frames and look up their stack maps.

```rust
fn walk_stack_and_collect_roots(gc_table: &GcTable) -> Vec<*mut *mut u8> {
    let mut roots = Vec::new();

    // Use platform unwinding to walk frames
    // (backtrace crate, or manual frame-pointer walking)
    backtrace::trace(|frame| {
        let pc = frame.ip() as usize;
        if let Some((frame_size, offsets)) = gc_table.lookup(pc) {
            let sp = frame.sp() as usize;
            for offset in offsets {
                // This is a pointer TO the stack slot that contains the GC ref.
                // The GC can read it (to find the object) and write it (to
                // update if the object moves).
                let slot_addr = (sp + offset as usize) as *mut *mut u8;
                roots.push(slot_addr);
            }
        }
        true // continue walking
    });

    roots
}
```

### Piece 5: The Collector (Mark-Sweep Example)

```rust
fn collect(heap: &mut GcHeap, roots: &[*mut *mut u8]) {
    // === MARK PHASE ===
    let mut worklist: Vec<*mut u8> = Vec::new();

    // Start from roots
    for root in roots {
        let obj = unsafe { **root };
        if !obj.is_null() {
            mark_grey(obj);
            worklist.push(obj);
        }
    }

    // Trace: follow references in grey objects
    while let Some(obj) = worklist.pop() {
        let header = obj as *mut GcHeader;
        unsafe { (*header).mark = BLACK; }

        // For each reference field in this object (using type_id to find layout):
        for field_offset in get_reference_fields(unsafe { (*header).type_id }) {
            let child = unsafe { *(obj.add(field_offset) as *mut *mut u8) };
            if !child.is_null() && get_mark(child) == WHITE {
                mark_grey(child);
                worklist.push(child);
            }
        }
    }

    // === SWEEP PHASE ===
    // Walk the heap, free all WHITE objects, reset BLACK objects to WHITE
    sweep(heap);
}
```

### Piece 6: Putting It Together

```
1. Mutator runs compiled code
2. Allocation fails (or safepoint poll triggers)
3. → collect() is called
4. Walk stack → find roots via stack maps
5. Mark phase: trace from roots through object graph
6. Sweep phase: reclaim unreachable objects
7. Return to mutator
```

---

## Multi-Threading and GC

### The Problem

With multiple threads, the GC must:
1. **Stop all threads** at safe points (can't walk a running thread's stack)
2. **Walk all threads' stacks** to find all roots
3. **Collect garbage** with knowledge of the complete root set
4. **Resume all threads**

### How to Stop All Threads (Stop-the-World)

**Approach A: Cooperative polling (simplest)**

Each thread checks a flag at safepoints. When GC is needed:

```
Thread requesting GC:
    1. Set global flag: GC_REQUESTED = true
    2. Wait until all threads have parked
    3. Do the GC
    4. Clear the flag, wake all threads

Every other thread (at safepoint polls):
    1. Check GC_REQUESTED
    2. If true: save state, signal "I'm parked", wait
    3. When woken: reload state, continue
```

This is simple but has a problem: if a thread is in a tight loop with no
safepoints, it never checks the flag. That's why you need loop back-edge polls
(see [Safepoints in Loops](#safepoints-in-loops)).

**Approach B: Signal-based suspension (for stuck threads)**

Send an OS signal (e.g., `SIGUSR1`) to force-pause a thread:

```
GC thread:
    1. For each mutator thread:
       - Send SIGUSR1
    2. Signal handler fires on the target thread:
       - Checks if at a known safepoint (using stack map table)
       - If yes: save registers, park
       - If no: set a thread-local flag so the thread parks
         at the next safepoint
    3. Wait for all threads to park
    4. Do the GC
```

This is how Go handles goroutines stuck in tight loops (it uses `SIGURG`).

**Approach C: Hybrid (production runtimes)**

Use cooperative polling for the common case (fast), signal-based as a fallback.
This is what Go and modern JVMs do.

### Thread-Local vs Shared Heaps

| | Thread-Local Heaps | Shared Heap |
|---|---|---|
| **Model** | Each thread has its own heap | All threads share one heap |
| **Allocation** | No synchronization needed (fast) | Need thread-local allocation buffers (TLABs) |
| **Cross-thread refs** | Expensive (need barriers or copying) | Free (any thread can reference any object) |
| **GC pauses** | Can collect one thread independently | Must stop all threads |
| **Complexity** | Simpler GC, harder sharing | Harder GC, simpler sharing |
| **Who uses this** | Erlang, OCaml 5 (minor heap) | Java, Go, .NET |

**For Dovetail (recommended starting point):** Thread-local heaps with message
passing, like Erlang. Each thread/actor has its own heap and GC. No cross-thread
GC coordination needed. Shared data is copied when sent between threads.

### Stack Walking Across Threads

At stop-the-world, you need to walk every thread's stack:

```rust
fn stop_the_world_gc() {
    // 1. Signal all threads to pause
    request_gc_pause();
    wait_for_all_threads_parked();

    // 2. Walk each thread's stack
    let mut all_roots = Vec::new();
    for thread in &mutator_threads {
        let roots = walk_stack_and_collect_roots(
            thread.saved_sp,     // stack pointer when thread parked
            thread.saved_pc,     // program counter when thread parked
            &gc_table,
        );
        all_roots.extend(roots);
    }

    // 3. Also include non-stack roots (globals, thread-local roots, etc.)
    all_roots.extend(get_global_roots());

    // 4. Collect
    collect(&mut heap, &all_roots);

    // 5. Resume all threads
    resume_all_threads();
}
```

Each thread must save its SP and PC when it parks (the signal handler or poll
code does this). The GC then walks from each thread's saved SP using the stack
map table.

---

## Write Barriers

Write barriers are only needed for **concurrent** or **generational** collectors.
A simple stop-the-world mark-sweep does NOT need them.

### What Are They?

A write barrier is extra code at every pointer store that notifies the GC about
the mutation. Without barriers, a concurrent GC might miss live objects.

### When You Need Them

- **Generational GC**: Need barriers to track old→young pointers (so you can
  collect the young generation without scanning the entire old generation).
- **Concurrent GC**: Need barriers to maintain tri-color invariant while mutators
  and collector run simultaneously.
- **Simple STW mark-sweep**: NO barriers needed. Everything stops during GC.

### Types of Write Barriers

**Card marking (generational GC, simplest):**

When storing a pointer, mark the memory region ("card") as dirty:

```
// Pseudocode: *slot = new_ref
*slot = new_ref
card_table[slot >> CARD_SHIFT] = DIRTY
```

In Cranelift IR:
```
store.i64 notrap v_slot, v_new_ref          ;; the actual store
v_card = ushr_imm v_slot, 9                 ;; 512-byte cards
v_card_addr = iadd gv_card_table, v_card
store.i8 notrap v_card_addr, 1              ;; mark dirty
```

Cost: ~2 extra instructions per pointer store.

**Dijkstra insertion barrier (concurrent GC):**

When storing a pointer, shade the new target so the GC sees it:

```
// Pseudocode: *slot = new_ref
if gc_is_marking:
    mark_grey(new_ref)
*slot = new_ref
```

**Yuasa deletion barrier (snapshot-at-beginning):**

Before overwriting a pointer, shade the old target:

```
// Pseudocode: *slot = new_ref
if gc_is_marking:
    old = *slot
    mark_grey(old)
*slot = new_ref
```

### In Cranelift IR (Inline Card Marking Example)

```
;; Your compiler emits this for every heap pointer store:

;; The actual store
store.i64 notrap v_field_addr, v_new_ref

;; Card marking barrier (2 extra instructions)
v_card_index = ushr_imm v_field_addr, 9
v_card_ptr = iadd gv_card_table_base, v_card_index
store.i8 notrap v_card_ptr, 1
```

Cranelift does NOT insert barriers for you — your compiler frontend must emit
them as part of the IR for every pointer store to the heap.

---

## How Wasmtime Does It

Wasmtime's approach is useful as a reference, though simpler than what Dovetail
would eventually need.

### Architecture

- **Per-Store isolation**: Each `Store` (single-threaded) has its own GC heap.
  No cross-thread GC coordination.
- **Pluggable collectors** via `GcCompiler` (compile-time) and `GcRuntime` traits.
- **Two collectors**:
  - **DRC (Deferred Reference Counting)**: Default. Avoids refcount manipulation
    for stack-only refs. Uses an over-approximation table + precise stack scan
    at collection time. Cannot collect cycles.
  - **Null**: Bump allocates, never collects. Traps on OOM. For short-lived
    workloads.

### How Wasmtime Uses Stack Maps

1. During compilation, Wasmtime's `GcCompiler` calls
   `builder.declare_value_needs_stack_map(gc_ref)` when loading GC refs.

2. Cranelift generates stack maps as part of compilation.

3. At GC time, Wasmtime walks the Wasm stack using `.eh_frame` unwind info,
   looks up stack maps by return address, and finds all live GC refs.

4. For DRC: compares precise roots against the over-approximation table and
   adjusts reference counts.

### Wasmtime's Type System for GC Objects

Three type hierarchies:
- `any` → `eq` → `{i31, struct, array}` → `none` (internal data)
- `func` → concrete function types → `nofunc` (functions)
- `extern` → `noextern` (external references)

Every GC object has a `VMGcHeader` with a kind discriminator and type info.

---

## How Other Runtimes Do It

### Go

- **Collector**: Concurrent tri-color mark-sweep (no compaction/moving)
- **Safepoints**: Cooperative (piggybacks on stack-growth checks in function
  prologues) + signal-based fallback (`SIGURG`) for tight loops
- **Write barrier**: Hybrid Dijkstra+Yuasa. Compiles to 2 instructions on the
  fast path (check flag + branch, almost always not taken)
- **Stack maps**: Custom unwind tables, not DWARF

### Java (HotSpot)

- **Collectors**: G1 (default), ZGC (concurrent, sub-ms pauses), Shenandoah
- **Safepoints**: Load from a poll page at method returns + loop back-edges.
  When GC needed: `mprotect(PROT_NONE)` → SIGSEGV → handler parks thread
- **Write barriers**: Card marking (G1), load barriers (ZGC), Brooks pointers
  (Shenandoah)
- **Stack maps**: OopMaps generated by JIT, precise root identification

### OCaml 5 (Multicore)

- **Minor heap**: Thread-local, stop-the-world parallel collection
- **Major heap**: Shared, mostly-concurrent mark-sweep with deletion barrier
- **Safepoints**: At allocation sites (every allocation checks GC flag). OCaml
  allocates frequently (functional style), so this works well.
- **No write barriers for minor GC**: Uses virtual memory tricks to detect
  cross-heap pointers

---

## JIT Mode (cranelift-jit)

This section covers how JIT-compiled Dovetail code communicates with a Rust runtime.

### The Core Question

When Cranelift JITs a Dovetail function, the machine code lives in executable memory.
When that code needs to allocate an object, print a string, or trigger GC — how does
it call into our Rust runtime? And how does the runtime pass state (heap pointers,
GC metadata) to the compiled code?

### Registering Runtime Functions

`JITBuilder` lets you register Rust functions by name. The JIT resolves these names
when compiled code calls them, just like a linker resolves symbols.

```rust
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::default_libcall_names;

// Runtime functions MUST be extern "C" — Rust's default ABI is unstable
extern "C" fn gc_alloc(ctx: *mut RuntimeContext, size: u64) -> *mut u8 {
    let ctx = unsafe { &mut *ctx };
    // ... allocate from GC heap ...
}

extern "C" fn runtime_println(ptr: *const u8, len: u64) {
    let s = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    println!("{}", String::from_utf8_lossy(s));
}

extern "C" fn gc_safepoint_poll(ctx: *mut RuntimeContext) {
    // Check if GC is requested, collect if so
}

// Register them when building the JIT
let mut builder = JITBuilder::with_isa(isa, default_libcall_names());

// Option A: one at a time
builder.symbol("gc_alloc", gc_alloc as *const u8);
builder.symbol("runtime_println", runtime_println as *const u8);
builder.symbol("gc_safepoint_poll", gc_safepoint_poll as *const u8);

// Option B: batch
builder.symbols([
    ("gc_alloc", gc_alloc as *const u8),
    ("runtime_println", runtime_println as *const u8),
    ("gc_safepoint_poll", gc_safepoint_poll as *const u8),
]);

// Option C: dynamic lookup (fallback if symbol not found in table)
builder.symbol_lookup_fn(Box::new(|name| {
    match name {
        "gc_alloc" => Some(gc_alloc as *const u8),
        _ => None,
    }
}));

let module = JITModule::new(builder);
```

### Calling Runtime Functions from Compiled Code

In your codegen, declare the runtime function as an import, then call it:

```rust
// 1. Declare the runtime function signature
let mut alloc_sig = module.make_signature();
alloc_sig.params.push(AbiParam::new(pointer_type)); // ctx
alloc_sig.params.push(AbiParam::new(types::I64));    // size
alloc_sig.returns.push(AbiParam::new(pointer_type)); // result ptr

// 2. Declare it as an import (resolved from the symbol table)
let alloc_func_id = module
    .declare_function("gc_alloc", Linkage::Import, &alloc_sig)
    .unwrap();

// 3. Inside a function body, import and call it
let alloc_ref = module.declare_func_in_func(alloc_func_id, builder.func);

let ctx_val = /* the runtime context pointer */;
let size_val = builder.ins().iconst(types::I64, 64);
let call = builder.ins().call(alloc_ref, &[ctx_val, size_val]);
let new_obj = builder.inst_results(call)[0];

// 4. Mark the result as a GC reference (for stack maps)
builder.declare_value_needs_stack_map(new_obj);
```

### Passing Runtime State: Three Strategies

The compiled code needs access to the GC heap, string table, etc. How?

**Strategy 1: Explicit context parameter (simplest, start here)**

Every compiled function takes a hidden first parameter — a pointer to the runtime context:

```rust
#[repr(C)]  // MUST be repr(C) so field offsets are predictable
struct RuntimeContext {
    gc_heap_base: *mut u8,
    gc_heap_size: u64,
    alloc_cursor: u64,
    string_table: *mut StringTable,
}

// In codegen: every function signature gets a hidden first param
fn emit_signature(module: &JITModule, dovetail_sig: &FuncSig) -> ir::Signature {
    let ptr = module.isa().pointer_type();
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(ptr));  // hidden: *mut RuntimeContext
    for param in &dovetail_sig.params {
        sig.params.push(AbiParam::new(to_cranelift_type(param)));
    }
    // ... return type ...
    sig
}
```

When one compiled function calls another, it forwards the context pointer.
When calling a runtime function, it passes it as the first argument.

Pros: Simple, easy to debug, explicit.
Cons: Extra parameter threaded through every call.

**Strategy 2: VMContext (Wasmtime-style, more efficient)**

Use Cranelift's built-in `ArgumentPurpose::VMCtx` mechanism:

```rust
// The vmctx is a special parameter that Cranelift knows about
sig.params.push(AbiParam::special(pointer_type, ArgumentPurpose::VMCtx));
```

Inside the function, access fields through `GlobalValue` chains:

```rust
// Get the vmctx pointer
let vmctx = func.create_global_value(ir::GlobalValueData::VMContext);

// Load a field: ctx->gc_heap_base (at offset 0)
let heap_base = func.create_global_value(ir::GlobalValueData::Load {
    base: vmctx,
    offset: Offset32::new(0),
    global_type: pointer_type,
    flags: ir::MemFlags::trusted(),
});

// Materialize the value in the function body
let heap_base_val = builder.ins().global_value(pointer_type, heap_base);
```

Cranelift can optimize these loads (hoist out of loops, CSE, etc.).

Pros: Cranelift-native, optimizer-friendly.
Cons: More complex setup, harder to debug initially.

**Strategy 3: Global data symbol (for truly global state)**

```rust
let data_id = module.declare_data("gc_state", Linkage::Export, true, false)?;
// ... define data ...
// Compiled code loads from this global address
```

Only useful for singleton state. Less flexible than a context pointer.

**Recommendation:** Start with Strategy 1 (explicit parameter). Move to Strategy 2
(VMContext) when optimizing.

### Complete JIT Example

```rust
use cranelift::prelude::*;
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module};
use std::mem;

// ---- The runtime context ----

#[repr(C)]
struct RuntimeContext {
    gc_heap: *mut u8,
    gc_heap_size: u64,
    alloc_cursor: u64,
}

// ---- Runtime functions (extern "C"!) ----

extern "C" fn gc_alloc(ctx: *mut RuntimeContext, size: u64) -> *mut u8 {
    let ctx = unsafe { &mut *ctx };
    let aligned = (size + 7) & !7;
    if ctx.alloc_cursor + aligned > ctx.gc_heap_size {
        panic!("OOM"); // In real code: trigger GC here
    }
    let ptr = unsafe { ctx.gc_heap.add(ctx.alloc_cursor as usize) };
    ctx.alloc_cursor += aligned;
    ptr
}

// ---- JIT setup ----

fn main() {
    // 1. Create the JIT with runtime symbols
    let isa = cranelift_native::builder().unwrap()
        .finish(settings::Flags::new(settings::builder())).unwrap();
    let mut builder = JITBuilder::with_isa(isa, default_libcall_names());
    builder.symbol("gc_alloc", gc_alloc as *const u8);
    let mut module = JITModule::new(builder);

    let ptr_ty = module.isa().pointer_type();

    // 2. Declare the runtime function as an import
    let mut alloc_sig = module.make_signature();
    alloc_sig.params.push(AbiParam::new(ptr_ty));      // ctx
    alloc_sig.params.push(AbiParam::new(types::I64));   // size
    alloc_sig.returns.push(AbiParam::new(ptr_ty));      // result
    let alloc_id = module.declare_function("gc_alloc", Linkage::Import, &alloc_sig).unwrap();

    // 3. Declare and build our function: fn my_func(ctx: *RuntimeContext) -> *u8
    let mut my_sig = module.make_signature();
    my_sig.params.push(AbiParam::new(ptr_ty));     // ctx
    my_sig.returns.push(AbiParam::new(ptr_ty));    // result
    let my_func_id = module.declare_function("my_func", Linkage::Export, &my_sig).unwrap();

    let mut ctx = module.make_context();
    ctx.func.signature = my_sig;
    let mut fb_ctx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        b.seal_block(block);

        let ctx_param = b.block_params(block)[0];  // the runtime context

        // Import gc_alloc into this function
        let alloc_ref = module.declare_func_in_func(alloc_id, b.func);

        // Call gc_alloc(ctx, 64) — allocate 64 bytes
        let size = b.ins().iconst(types::I64, 64);
        let call = b.ins().call(alloc_ref, &[ctx_param, size]);
        let obj = b.inst_results(call)[0];

        // Mark as GC ref (for stack maps)
        b.declare_value_needs_stack_map(obj);

        b.ins().return_(&[obj]);
        b.finalize();
    }

    // 4. Compile
    module.define_function(my_func_id, &mut ctx).unwrap();
    module.clear_context(&mut ctx);
    module.finalize_definitions().unwrap();

    // 5. Run it!
    let code_ptr = module.get_finalized_function(my_func_id);
    let func: extern "C" fn(*mut RuntimeContext) -> *mut u8 =
        unsafe { mem::transmute(code_ptr) };

    let heap = vec![0u8; 1024 * 1024];
    let mut rt = RuntimeContext {
        gc_heap: heap.as_ptr() as *mut u8,
        gc_heap_size: heap.len() as u64,
        alloc_cursor: 0,
    };

    let result = func(&mut rt);
    println!("Allocated at: {:?}, cursor: {}", result, rt.alloc_cursor);
    // Output: Allocated at: 0x..., cursor: 64
}
```

### Memory Ownership

- `JITModule` owns all compiled code memory
- Function pointers from `get_finalized_function()` are valid until `JITModule` is dropped
- `free_memory(self)` consumes the module and frees everything — all function pointers become invalid
- No way to free individual functions (all-or-nothing)
- The `JITModule` must outlive any use of its function pointers

---

## AOT Mode (cranelift-object)

For producing standalone executables instead of JIT.

### How It Works

`ObjectModule` implements the same `Module` trait as `JITModule`, but writes to
an object file (.o) instead of executable memory. Same codegen code, different backend.

```rust
use cranelift_object::{ObjectBuilder, ObjectModule};

let obj_builder = ObjectBuilder::new(
    isa,
    "dovetail_output",           // module name
    default_libcall_names(),
).unwrap();
let mut module = ObjectModule::new(obj_builder);

// Use module exactly like JITModule:
// declare_function, define_function, etc.

// At the end, produce the .o file
let product = module.finish();
let bytes = product.object.write().unwrap();
std::fs::write("dovetail_output.o", bytes).unwrap();
```

### The Runtime as a Static Library

Create a separate Rust crate that compiles to a static library:

```toml
# dovetail-runtime/Cargo.toml
[package]
name = "dovetail-runtime"

[lib]
crate-type = ["staticlib"]  # produces libdovetail_runtime.a
```

```rust
// dovetail-runtime/src/lib.rs

// All functions must be #[no_mangle] extern "C"
// so the linker can find them by name

#[repr(C)]
pub struct DovetailContext {
    pub gc_heap: *mut u8,
    pub gc_heap_size: u64,
    pub alloc_cursor: u64,
}

#[no_mangle]
pub extern "C" fn dovetail_gc_alloc(ctx: *mut DovetailContext, size: u64) -> *mut u8 {
    // ... allocation logic ...
}

#[no_mangle]
pub extern "C" fn dovetail_gc_collect(ctx: *mut DovetailContext) {
    // ... GC logic ...
}

#[no_mangle]
pub extern "C" fn dovetail_println(ptr: *const u8, len: u64) {
    let s = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    print!("{}", String::from_utf8_lossy(s));
}

/// The actual entry point — sets up the runtime, then calls compiled Dovetail code
#[no_mangle]
pub extern "C" fn main() -> i32 {
    let heap = vec![0u8; 64 * 1024 * 1024]; // 64MB
    let mut ctx = DovetailContext {
        gc_heap: heap.as_ptr() as *mut u8,
        gc_heap_size: heap.len() as u64,
        alloc_cursor: 0,
    };

    // Call into the compiled Dovetail code
    extern "C" { fn dovetail_main(ctx: *mut DovetailContext) -> i32; }
    unsafe { dovetail_main(&mut ctx) }
}
```

### Linking It Together

```bash
# Step 1: Compile Dovetail source to .o file
dovetail build --emit-obj -o dovetail_code.o

# Step 2: Build the runtime
cargo build --release -p dovetail-runtime
# Produces: target/release/libdovetail_runtime.a

# Step 3: Link
cc dovetail_code.o -L target/release -ldovetail_runtime -o my_program

# Step 4: Run
./my_program
```

The linker resolves all the cross-references:
- `dovetail_code.o` has unresolved references to `dovetail_gc_alloc`, `dovetail_println`, etc.
- `libdovetail_runtime.a` provides those symbols
- `libdovetail_runtime.a` has an unresolved reference to `dovetail_main`
- `dovetail_code.o` provides that symbol (it's the compiled Dovetail `main` function)

### The Key Insight: Same Codegen, Different Module

Because `JITModule` and `ObjectModule` both implement the `Module` trait, your
codegen code works unchanged for both modes:

```rust
fn compile_function(module: &mut dyn Module, dovetail_func: &TypedFunction) {
    // This code is identical for JIT and AOT
    let sig = /* ... */;
    let func_id = module.declare_function(name, Linkage::Export, &sig).unwrap();
    // ... build IR, define function ...
}

// JIT mode:
let mut jit_module = JITModule::new(builder);
compile_function(&mut jit_module, &func);

// AOT mode:
let mut obj_module = ObjectModule::new(obj_builder);
compile_function(&mut obj_module, &func);
```

---

## The VMContext Pattern

This is how Wasmtime structures the boundary between host and compiled code.
Useful to understand even if we start simpler.

### The Idea

Instead of passing individual pieces of state (heap pointer, string table,
dispatch tables) as separate arguments, pack everything into a single **context
struct** and pass a pointer to it. Compiled code accesses fields by known offsets.

```
┌─── DovetailVMContext ───────────────────────────┐
│ offset 0:   *mut GcState                      │  ← GC heap metadata
│ offset 8:   u64 stack_limit                   │  ← for stack overflow checks
│ offset 16:  *mut StringTable                  │  ← interned strings
│ offset 24:  *const DispatchTable              │  ← trait dispatch / vtables
│ offset 32:  *mut StdoutWriter                 │  ← I/O
└───────────────────────────────────────────────┘
```

### How Wasmtime Does It

Wasmtime's `VMContext` is a **dynamically-sized struct** — its layout depends on
the module being compiled (how many memories, tables, globals, etc.). A `VMOffsets`
struct computes byte offsets for each field.

For Dovetail, our context would be **fixed-size** (we control the layout), which is
simpler. We don't need dynamic offsets.

### Accessing VMContext Fields in Cranelift

```rust
// 1. The vmctx is a special parameter
sig.params.push(AbiParam::special(pointer_type, ArgumentPurpose::VMCtx));

// 2. Create a GlobalValue for the vmctx base address
let vmctx_gv = func.create_global_value(ir::GlobalValueData::VMContext);

// 3. Chain loads to access fields

// ctx->gc_state (offset 0)
let gc_state_gv = func.create_global_value(ir::GlobalValueData::Load {
    base: vmctx_gv,
    offset: Offset32::new(0),
    global_type: pointer_type,
    flags: ir::MemFlags::trusted(),
});

// ctx->stack_limit (offset 8)
let stack_limit_gv = func.create_global_value(ir::GlobalValueData::Load {
    base: vmctx_gv,
    offset: Offset32::new(8),
    global_type: types::I64,
    flags: ir::MemFlags::trusted(),
});

// 4. Materialize values in the function body
let gc_state = builder.ins().global_value(pointer_type, gc_state_gv);
let stack_limit = builder.ins().global_value(types::I64, stack_limit_gv);
```

Cranelift can optimize these — hoist loads out of loops, eliminate redundant loads, etc.

### Nested Access (Two-Level Indirection)

To access `ctx->gc_state->free_list_head`:

```rust
// Level 1: ctx->gc_state (pointer at offset 0)
let gc_state_gv = func.create_global_value(ir::GlobalValueData::Load {
    base: vmctx_gv,
    offset: Offset32::new(0),
    global_type: pointer_type,
    flags: ir::MemFlags::trusted(),
});

// Level 2: gc_state->free_list_head (pointer at offset 16 within GcState)
let free_list_gv = func.create_global_value(ir::GlobalValueData::Load {
    base: gc_state_gv,
    offset: Offset32::new(16),
    global_type: pointer_type,
    flags: ir::MemFlags::trusted(),
});

let free_list = builder.ins().global_value(pointer_type, free_list_gv);
```

---

## GlobalValues in Cranelift

GlobalValues represent values that aren't known at compile time — they're resolved
at runtime. They're the mechanism for compiled code to access runtime-provided data.

| Variant | What It Does | Use Case |
|---------|-------------|----------|
| `VMContext` | Address of the context struct | Root of all runtime data |
| `Load { base, offset }` | Dereference: `*(base + offset)` | Loading fields from structs |
| `IAddImm { base, offset }` | Compute `base + offset` (no deref) | Getting address of a sub-struct |
| `Symbol { name }` | Address of a named symbol | Accessing global data |

You can chain them (vmctx → load field → load sub-field) but not create cycles.

---

## ABI and Calling Conventions

### The Critical Rule

**Runtime functions exposed to Cranelift MUST be `extern "C"`.**

Without `extern "C"`, Rust uses an internal, unstable calling convention that
Cranelift doesn't know about. With `extern "C"`, both sides agree on the platform's
C ABI (System V on Linux/macOS, Windows fastcall on Windows).

```rust
// WRONG — Rust ABI, Cranelift can't call this
fn gc_alloc(ctx: *mut Ctx, size: u64) -> *mut u8 { ... }

// CORRECT — C ABI, Cranelift can call this
extern "C" fn gc_alloc(ctx: *mut Ctx, size: u64) -> *mut u8 { ... }
```

### Calling Conventions in Cranelift

```rust
// For calls between compiled Dovetail functions (both sides controlled by us):
// Use Fast — Cranelift's internal convention, best performance
let call_conv = isa::CallConv::Fast;

// For calls to/from extern "C" runtime functions:
// Use the platform default
let call_conv = module.isa().default_call_conv();
// Returns SystemV on Linux/macOS x86_64, WindowsFastcall on Windows
```

You can mix conventions — use `Fast` for Dovetail→Dovetail calls and `SystemV` for
Dovetail→Runtime calls. Just make sure each function's signature uses the right one.

### Struct Layout for Cross-Boundary Data

Any struct shared between Rust runtime and Cranelift code needs `#[repr(C)]`:

```rust
#[repr(C)]  // Guarantees field order and alignment match what Cranelift expects
struct RuntimeContext {
    gc_heap: *mut u8,      // offset 0
    gc_heap_size: u64,     // offset 8
    alloc_cursor: u64,     // offset 16
}
```

Without `#[repr(C)]`, Rust may reorder fields, add padding differently, etc.

---

## What Dovetail Would Need

### Phase 1: JIT with Simple STW Mark-Sweep (Start Here)

Get something working end-to-end with minimum complexity.

**Compiler side:**
1. Switch codegen from `wasm-encoder` to `cranelift-jit` (same `Module` trait pattern).
2. Every compiled function gets a hidden `*mut RuntimeContext` first parameter.
3. Call `declare_var_needs_stack_map()` for every local that holds a heap reference.
4. Insert `call $gc_safepoint_poll()` at loop back-edges.
5. Runtime functions (`gc_alloc`, `println`, etc.) declared as `Linkage::Import`.

**Runtime side (a `dovetail-runtime` crate):**
1. `RuntimeContext` struct (`#[repr(C)]`) with GC heap state, string table, etc.
2. `extern "C"` functions: `gc_alloc`, `gc_safepoint_poll`, `dovetail_println`.
3. Simple mark-sweep collector: bump allocator, walk stack on OOM, mark from roots, sweep.
4. Stack map table built from Cranelift's `user_stack_maps()` output.
5. Stack walking via the `backtrace` crate.
6. No write barriers needed.

**Build flow:** JIT mode — compile and run in-process. Runtime functions registered
via `JITBuilder::symbols()`. Single-threaded.

### Phase 2: AOT Mode (cranelift-object)

1. Add `ObjectModule` backend (same codegen code, different `Module`).
2. Compile `dovetail-runtime` as a `staticlib` (`libdovetail_runtime.a`).
3. Runtime provides `main()` that sets up context and calls `dovetail_main()`.
4. Link with `cc dovetail_code.o -ldovetail_runtime -o program`.
5. Same GC, just AOT instead of JIT.

### Phase 3: Generational GC

Add a young generation with fast bump allocation + card marking barrier:

1. **Nursery** (young gen): Bump allocate. Collect frequently (minor GC).
2. **Old gen**: Promote surviving objects. Collect infrequently (major GC).
3. **Card marking write barrier**: Track old→young pointers.
4. Still stop-the-world for both minor and major collections.

### Phase 4: VMContext Pattern

Switch from explicit context parameter to Cranelift's `ArgumentPurpose::VMCtx` +
`GlobalValueData::VMContext` for better performance. Access runtime state through
offset-based loads that Cranelift can optimize.

### Phase 5: Multi-Threading

Options (pick one):

- **Thread-local heaps** (Erlang model): Each thread has its own heap. Message
  passing copies data. No shared GC. Simplest.
- **Shared heap with STW**: All threads share a heap. Stop all threads for GC.
  Need safepoint polls + signal fallback. More complex but allows sharing.

### Phase 6: Concurrent Collection (Advanced)

Reduce pause times by running marking concurrently with mutators:

1. Add Dijkstra or Yuasa write barriers.
2. Concurrent marking phase (mutators run alongside marker).
3. Brief STW for root scanning and mark termination.
4. Concurrent sweep.

---

## Sources

### Primary References
- [New Stack Maps for Wasmtime and Cranelift (Bytecode Alliance)](https://bytecodealliance.org/articles/new-stack-maps-for-wasmtime) — the definitive design article
- [New Stack Maps (Nick Fitzgerald's blog)](https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html) — same content, more detail
- [Wasmtime GC RFC](https://github.com/bytecodealliance/rfcs/blob/main/accepted/wasm-gc.md)

### API Documentation
- [FunctionBuilder docs](https://docs.rs/cranelift-frontend/latest/cranelift_frontend/struct.FunctionBuilder.html) — `declare_var_needs_stack_map`, `declare_value_needs_stack_map`
- [UserStackMap docs](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/ir/struct.UserStackMap.html)
- [MachBufferFinalized docs](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/struct.MachBufferFinalized.html) — `user_stack_maps()`, `take_user_stack_maps()`
- [Wasmtime Collector enum](https://docs.wasmtime.dev/api/wasmtime/enum.Collector.html)

### API Documentation (Linking & Modules)
- [JITBuilder docs](https://docs.rs/cranelift-jit/latest/cranelift_jit/struct.JITBuilder.html) — `symbol()`, `symbols()`, `symbol_lookup_fn()`
- [JITModule docs](https://docs.rs/cranelift-jit/latest/cranelift_jit/struct.JITModule.html)
- [ObjectBuilder docs](https://docs.rs/cranelift-object/latest/cranelift_object/struct.ObjectBuilder.html)
- [Linkage enum docs](https://docs.rs/cranelift-module/latest/cranelift_module/enum.Linkage.html) — Import, Export, Local
- [GlobalValueData docs](https://docs.rs/cranelift/latest/cranelift/prelude/enum.GlobalValueData.html) — VMContext, Load, IAddImm, Symbol
- [CallConv docs](https://docs.rs/cranelift-codegen/latest/cranelift_codegen/isa/enum.CallConv.html) — Fast, SystemV, WindowsFastcall

### Source Code
- [user_stack_maps.rs](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/codegen/src/ir/user_stack_maps.rs) — `UserStackMap`, `UserStackMapEntry`
- [frontend.rs](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/frontend/src/frontend.rs) — `FunctionBuilder` stack map methods
- [safepoints.rs](https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/frontend/src/frontend/safepoints.rs) — liveness analysis + spill insertion
- [gc/enabled.rs](https://github.com/bytecodealliance/wasmtime/blob/main/crates/cranelift/src/func_environ/gc/enabled.rs) — Wasmtime's real GC compiler usage
- [cranelift-jit-demo](https://github.com/bytecodealliance/cranelift-jit-demo) — complete example of using Cranelift as a JIT backend
- [Wasmtime func_environ.rs](https://github.com/bytecodealliance/wasmtime/blob/main/crates/cranelift/src/func_environ.rs) — VMContext pattern
- [rustc_codegen_cranelift](https://github.com/rust-lang/rust/tree/master/compiler/rustc_codegen_cranelift) — Rust's Cranelift backend (AOT example)

### Other Runtime GC Designs
- [JVM Anatomy Quark #22: Safepoint Polls](https://shipilev.net/jvm/anatomy-quarks/22-safepoint-polls/)
- [Go Hybrid Write Barrier Proposal](https://github.com/golang/proposal/blob/master/design/17503-eliminate-rescan.md)
- [Deep Dive into Multicore OCaml GC](https://kcsrk.info/multicore/gc/2017/07/06/multicore-ocaml-gc/)
- [WebAssembly Reference Types in Wasmtime](https://bytecodealliance.org/articles/reference-types-in-wasmtime)
- [Wasmtime GC tracking issue](https://github.com/bytecodealliance/wasmtime/issues/5032)
- [Using Cranelift as a language backend](https://github.com/bytecodealliance/wasmtime/issues/5141)

### Practical Cranelift Examples
- [Building a Brainfuck compiler with Cranelift](https://rodrigodd.github.io/2022/11/26/bf_compiler-part3.html)
- [Brainfuck Cranelift compiler (tiedt.dev)](https://blog.tiedt.dev/article/brainfuck_compiler)
- [Calling Rust from Cranelift (Rust Users Forum)](https://users.rust-lang.org/t/calling-a-rust-function-from-cranelift/103948)
