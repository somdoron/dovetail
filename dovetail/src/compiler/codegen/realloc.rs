use wasm_encoder::{BlockType, Function, Instruction, MemArg, ValType};

/// Generate the `cabi_realloc` export over the pinned heap.
/// Signature: (old_ptr: i32, old_size: i32, align: i32, new_size: i32) -> i32
///
/// Canonical-ABI lowerings from the host may land at ANY time under p3 (an
/// async import's result strings arrive when the task completes), so realloc
/// must not hand out scratch memory (global 0), whose bump pointer is
/// save/restored per marshaling window. Blocks come from the pinned free-list
/// instead, and every lift must free what it copied out of — including the
/// sync ones (environment, arguments, initial-cwd, preopens), which are not
/// call-once: a program is free to poll `Environment.variables` in a loop, and
/// an unfreed block per call runs the heap out.
///
/// A grow that still fits the block it already has is answered in place. The
/// host reallocs its way up a list it is lifting (a `list<string>` grown
/// element by element), and every one of those steps used to allocate, copy and
/// free — with `pinned_alloc` rounding sizes up to a multiple of 8 and never
/// splitting off a remainder under 16 bytes, most of them had the room already.
/// Nothing may assume realloc moves: the canonical ABI's contract is that the
/// returned pointer is where the data now lives, not that it differs.
pub(super) fn generate_realloc(pinned_alloc: u32, pinned_free: u32) -> Function {
    // Locals: 0=old_ptr, 1=old_size, 2=align, 3=new_size, 4=result
    let mut f = Function::new(vec![(1, ValType::I32)]);
    let old_ptr = 0;
    let old_size = 1;
    let align = 2;
    let new_size = 3;
    let result = 4;

    // The pinned heap guarantees 8-byte alignment and nothing more, so an
    // alignment above that is a request this allocator cannot honour. The
    // canonical ABI's maximum alignment is 8 today, which is why the parameter
    // could be ignored at all; a WIT type or a canon change that raised it would
    // otherwise be answered with a misaligned pointer and no complaint. Trap
    // instead — a wrong answer here is a fault in the host's lifted value, far
    // from its cause.
    f.instruction(&Instruction::LocalGet(align));
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32GtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);

    // A non-zero `old_ptr` below the pinned base did not come out of this
    // allocator, and both paths that touch it — the header read at `old_ptr - 8`
    // and the `pinned_free` at the end — would take a garbage size word for a
    // real one and thread scratch memory, or page 0, onto the free list.
    // Defensive, not a live bug: the canonical ABI hands back only pointers it
    // received from `cabi_realloc` (or 0, which the in-place test already
    // handles), `cabi_realloc` only ever returns pinned blocks, and nothing in
    // the guest calls it. Same reasoning as the `align > 8` trap above — the
    // cost is one comparison on a path that already does several, and the
    // alternative to trapping is a corrupted free list discovered somewhere
    // else entirely.
    f.instruction(&Instruction::LocalGet(old_ptr));
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::I32Ne);
    f.instruction(&Instruction::LocalGet(old_ptr));
    f.instruction(&Instruction::I32Const(SCRATCH_LIMIT));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);

    // In place when the existing block is already big enough: its capacity is
    // the header size minus the header itself. A shrink lands here too and
    // keeps the surplus rather than splitting it off — realloc is called on the
    // way up, and the tail would be handed straight back on the next grow.
    f.instruction(&Instruction::LocalGet(old_ptr));
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(old_ptr));
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32Sub);
    f.instruction(&Instruction::I32Load(HDR_SIZE));
    f.instruction(&Instruction::I32Const(SIZE_MASK));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32Sub);
    f.instruction(&Instruction::LocalGet(new_size));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(old_ptr));
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::End);

    // result = pinned_alloc(new_size)
    f.instruction(&Instruction::LocalGet(new_size));
    f.instruction(&Instruction::Call(pinned_alloc));
    f.instruction(&Instruction::LocalSet(result));

    // copy min(old_size, new_size) bytes from old_ptr, then free it
    f.instruction(&Instruction::LocalGet(old_ptr));
    f.instruction(&Instruction::If(BlockType::Empty));
    // clamp copy size
    f.instruction(&Instruction::LocalGet(old_size));
    f.instruction(&Instruction::LocalGet(new_size));
    f.instruction(&Instruction::LocalGet(old_size));
    f.instruction(&Instruction::LocalGet(new_size));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::Select);
    f.instruction(&Instruction::LocalSet(old_size));
    // memory.copy(result, old_ptr, old_size) — bulk memory is always enabled
    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::LocalGet(old_ptr));
    f.instruction(&Instruction::LocalGet(old_size));
    f.instruction(&Instruction::MemoryCopy { src_mem: 0, dst_mem: 0 });
    // free the old block
    f.instruction(&Instruction::LocalGet(old_ptr));
    f.instruction(&Instruction::Call(pinned_free));
    f.instruction(&Instruction::End);

    f.instruction(&Instruction::LocalGet(result));
    f.instruction(&Instruction::End);
    f
}

// ── Pinned allocator ────────────────────────────────────────────────
//
// Allocations that must survive across suspension points (async-call
// parameter/retptr blocks, posted stream buffers) cannot live in the
// scratch bump arena (global 0), which is reclaimed per marshaling window.
// They come from a dedicated pinned heap: a classic first-fit free list
// with 8-byte-aligned blocks and an 8-byte header [size:i32][next:i32],
// growing upward from PINNED_BASE via memory.grow.
//
// Globals (see GLOBAL_* consts in mod.rs):
//   GLOBAL_PINNED_FREE_HEAD — first free block (0 = empty list)
//   GLOBAL_PINNED_BRK       — end of the pinned region in use
//
// The free list is kept sorted by address so that `pinned_free` can merge a
// block with its neighbours on the way in. Without that, splitting is
// one-way: every allocation that does not fill a block exactly leaves a
// smaller one behind, and a server that runs for a week ends up with a
// pinned high-water mark well above its live set even though its allocation
// pattern is a loop. The insertion walk is O(list), which is the right trade
// here — the list is short (sizes repeat, and a marshaling window frees its
// blocks in one adjacent burst at teardown, which is exactly the shape
// coalescing collapses back to a single block).

use super::{GLOBAL_PINNED_BRK, GLOBAL_PINNED_FREE_HEAD, SCRATCH_LIMIT};

/// Block header word 0: the block's total size, header included.
const HDR_SIZE: MemArg = MemArg { offset: 0, align: 2, memory_index: 0 };

/// Block header word 1: the free-list `next` link while the block is free.
///
/// While the block is *allocated* this word belongs to whoever holds it —
/// `scratch_pinned_alloc` threads the marshaling window's list through it —
/// so the allocator may only read or write it on blocks that are on the free
/// list. That is what rules the header out as a place to record "this block
/// has been freed", and why the tag below lives in the size word instead.
const HDR_NEXT: MemArg = MemArg { offset: 4, align: 2, memory_index: 0 };

/// Set in the size word while the block is on the free list.
///
/// Block sizes are multiples of 8, so the low three bits are always clear in a
/// real size; the size word is the allocator's own, unlike the payload, whose
/// first word would sooner or later hold whatever sentinel we picked and trap a
/// correct program. `pinned_free` traps on entry if the tag is already set,
/// which turns a double free from a free-list cycle (an infinite first-fit
/// walk) or a block handed out twice (silent corruption, discovered much later
/// and somewhere else) into a trap at the offending free.
const FREE_TAG: i32 = 1;

/// Strips [`FREE_TAG`] from a size word.
const SIZE_MASK: i32 = -8;

/// `pinned_alloc(size: i32) -> i32` — allocate from the pinned heap.
pub(super) fn generate_pinned_alloc() -> Function {
    // Locals: 0=size(param), 1=need, 2=prev, 3=cur, 4=blk_size, 5=grow_pages,
    // 6=new_brk
    let mut f = Function::new(vec![(6, ValType::I32)]);
    let size = 0;
    let need = 1;
    let prev = 2;
    let cur = 3;
    let blk_size = 4;
    let grow_pages = 5;
    let new_brk = 6;

    // need = max(align8(size) + 8, 16)
    f.instruction(&Instruction::LocalGet(size));
    f.instruction(&Instruction::I32Const(7));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(-8));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalTee(need));

    // The rounding above wraps for a size within 7 of 2^32, and a wrapped `need`
    // is worse than the out-of-memory it stands for: a request for ~4GiB rounds
    // to a 16-byte block, which the host then memcpys 4GiB into — over the live
    // pinned heap, over the scratch arena, and only then out of bounds. Trapping
    // on the wrap keeps an impossible request an impossible request. Unsigned,
    // because `need` is a byte count: `need < size` can only mean it wrapped.
    f.instruction(&Instruction::LocalGet(size));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);

    // A block is never smaller than its header plus a free-list link.
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Const(16));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::I32Const(16));
    f.instruction(&Instruction::LocalSet(need));
    f.instruction(&Instruction::End);

    // First-fit walk of the free list.
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(prev));
    f.instruction(&Instruction::GlobalGet(GLOBAL_PINNED_FREE_HEAD));
    f.instruction(&Instruction::LocalSet(cur));

    f.instruction(&Instruction::Block(BlockType::Empty)); // $miss
    f.instruction(&Instruction::Loop(BlockType::Empty)); // $walk
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::BrIf(1)); // -> $miss

    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Load(HDR_SIZE));
    f.instruction(&Instruction::I32Const(SIZE_MASK)); // drop FREE_TAG
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::LocalTee(blk_size));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::If(BlockType::Empty));
    // Fits. Split if the remainder can hold a minimal block.
    f.instruction(&Instruction::LocalGet(blk_size));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Sub);
    f.instruction(&Instruction::I32Const(16));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::If(BlockType::Empty));
    // split: tail block at cur+need inherits the remainder and cur's next.
    // The tail stays on the free list, so its size word keeps the tag; cur's
    // is stamped with the exact allocated size, which clears it.
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Add); // tail ptr
    f.instruction(&Instruction::LocalGet(blk_size));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Sub);
    f.instruction(&Instruction::I32Const(FREE_TAG));
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::I32Store(HDR_SIZE));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Load(HDR_NEXT));
    f.instruction(&Instruction::I32Store(HDR_NEXT));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Store(HDR_SIZE));
    // link tail where cur was
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Store(HDR_NEXT));
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::GlobalSet(GLOBAL_PINNED_FREE_HEAD));
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::Else);
    // no split: unlink cur, and rewrite its size untagged — the block is
    // leaving the free list, and the tag is what the next free checks.
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalGet(blk_size));
    f.instruction(&Instruction::I32Store(HDR_SIZE));
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Load(HDR_NEXT));
    f.instruction(&Instruction::I32Store(HDR_NEXT));
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Load(HDR_NEXT));
    f.instruction(&Instruction::GlobalSet(GLOBAL_PINNED_FREE_HEAD));
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::End);
    // return cur + 8
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::Return);
    f.instruction(&Instruction::End);

    // advance
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalSet(prev));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Load(HDR_NEXT));
    f.instruction(&Instruction::LocalSet(cur));
    f.instruction(&Instruction::Br(0));
    f.instruction(&Instruction::End); // $walk
    f.instruction(&Instruction::End); // $miss

    // No fit: extend the brk, growing memory as required.
    // cur = brk
    f.instruction(&Instruction::GlobalGet(GLOBAL_PINNED_BRK));
    f.instruction(&Instruction::LocalSet(cur));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalTee(new_brk));

    // The rounding guard above only rules out a `need` that wrapped; the brk
    // advance can still wrap on a `need` that did not. A size of 0xFFFF0000
    // rounds cleanly to a `need` of 0xFFFF0008, misses the free list, and lands
    // a brk of ~8 — inside the scratch arena, below the pinned base — at which
    // point the grow loop sees nothing to grow and every later allocation
    // extends from there, over page 0 and over live pinned blocks. Unsigned
    // again, and against `cur` rather than `need`: addresses only ever move up.
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32LtU);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable);
    f.instruction(&Instruction::End);

    f.instruction(&Instruction::LocalGet(new_brk));
    f.instruction(&Instruction::GlobalSet(GLOBAL_PINNED_BRK));

    // while brk > memory.size * 64K: grow by the shortfall (at least 1 page)
    f.instruction(&Instruction::Block(BlockType::Empty));
    f.instruction(&Instruction::Loop(BlockType::Empty));
    f.instruction(&Instruction::GlobalGet(GLOBAL_PINNED_BRK));
    f.instruction(&Instruction::MemorySize(0));
    f.instruction(&Instruction::I32Const(16));
    f.instruction(&Instruction::I32Shl);
    f.instruction(&Instruction::I32LeU);
    f.instruction(&Instruction::BrIf(1));
    // grow_pages = (brk - mem_bytes + 65535) / 65536
    f.instruction(&Instruction::GlobalGet(GLOBAL_PINNED_BRK));
    f.instruction(&Instruction::MemorySize(0));
    f.instruction(&Instruction::I32Const(16));
    f.instruction(&Instruction::I32Shl);
    f.instruction(&Instruction::I32Sub);
    f.instruction(&Instruction::I32Const(65535));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(16));
    f.instruction(&Instruction::I32ShrU);
    f.instruction(&Instruction::LocalSet(grow_pages));
    f.instruction(&Instruction::LocalGet(grow_pages));
    f.instruction(&Instruction::MemoryGrow(0));
    f.instruction(&Instruction::I32Const(-1));
    f.instruction(&Instruction::I32Eq);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable); // out of memory
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::Br(0));
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::End);

    // stamp header (untagged: the block is allocated), return cur + 8
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalGet(need));
    f.instruction(&Instruction::I32Store(HDR_SIZE));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::End);
    f
}

/// `pinned_free(ptr: i32)` — return a pinned block to the free list.
///
/// Inserts by address and merges with either neighbour it turns out to be
/// adjacent to. Every byte between the base of the pinned region and the brk
/// belongs to exactly one block, so "adjacent" is decided by arithmetic on the
/// sizes: no boundary tags and no back pointers are needed.
///
/// Only free blocks' headers are read or written here — `prev` and `cur` both
/// come off the free list. An allocated neighbour's `next` word carries the
/// marshaling window's link (see `scratch_pinned_alloc`), and touching it would
/// corrupt a list the window is still going to walk.
///
/// The block that ends exactly at the brk is *not* returned to the brk. It
/// would take carrying the predecessor of `prev` through the walk to unlink the
/// merged block again, for a saving that address-ordered coalescing has already
/// made small — the block stays first-fit reusable either way.
pub(super) fn generate_pinned_free() -> Function {
    // Locals: 0=ptr(param), 1=blk, 2=size, 3=prev, 4=cur
    let mut f = Function::new(vec![(4, ValType::I32)]);
    let ptr = 0;
    let blk = 1;
    let size = 2;
    let prev = 3;
    let cur = 4;

    // blk = ptr - 8. Trap on a block that is already free: the tag is still
    // set from the previous free of it.
    f.instruction(&Instruction::LocalGet(ptr));
    f.instruction(&Instruction::I32Const(8));
    f.instruction(&Instruction::I32Sub);
    f.instruction(&Instruction::LocalTee(blk));
    f.instruction(&Instruction::I32Load(HDR_SIZE));
    f.instruction(&Instruction::LocalTee(size)); // exact: an allocated block is untagged
    f.instruction(&Instruction::I32Const(FREE_TAG));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::Unreachable); // double free
    f.instruction(&Instruction::End);

    // Walk to the insertion point: prev = last block below blk, cur = first
    // block above it (either may be 0).
    f.instruction(&Instruction::I32Const(0));
    f.instruction(&Instruction::LocalSet(prev));
    f.instruction(&Instruction::GlobalGet(GLOBAL_PINNED_FREE_HEAD));
    f.instruction(&Instruction::LocalSet(cur));
    f.instruction(&Instruction::Block(BlockType::Empty)); // $found
    f.instruction(&Instruction::Loop(BlockType::Empty)); // $walk
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Eqz);
    f.instruction(&Instruction::BrIf(1)); // -> $found (end of list)
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalGet(blk));
    f.instruction(&Instruction::I32GeU);
    f.instruction(&Instruction::BrIf(1)); // -> $found
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::LocalSet(prev));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Load(HDR_NEXT));
    f.instruction(&Instruction::LocalSet(cur));
    f.instruction(&Instruction::Br(0));
    f.instruction(&Instruction::End); // $walk
    f.instruction(&Instruction::End); // $found

    // blk.next = cur, absorbing cur if it starts where blk ends.
    f.instruction(&Instruction::LocalGet(blk));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Store(HDR_NEXT));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(blk));
    f.instruction(&Instruction::LocalGet(size));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Eq);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(size));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Load(HDR_SIZE));
    f.instruction(&Instruction::I32Const(SIZE_MASK));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalSet(size));
    f.instruction(&Instruction::LocalGet(blk));
    f.instruction(&Instruction::LocalGet(cur));
    f.instruction(&Instruction::I32Load(HDR_NEXT));
    f.instruction(&Instruction::I32Store(HDR_NEXT));
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::LocalGet(blk));
    f.instruction(&Instruction::LocalGet(size));
    f.instruction(&Instruction::I32Const(FREE_TAG));
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::I32Store(HDR_SIZE));

    // Link blk in behind prev, absorbing it into prev if they are adjacent.
    // blk's own header is left tagged when that happens: it is interior to the
    // merged block now, and a stray free of it still traps.
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::LocalGet(blk));
    f.instruction(&Instruction::I32Store(HDR_NEXT));
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::I32Load(HDR_SIZE));
    f.instruction(&Instruction::I32Const(SIZE_MASK));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::LocalGet(blk));
    f.instruction(&Instruction::I32Eq);
    f.instruction(&Instruction::If(BlockType::Empty));
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::I32Load(HDR_SIZE));
    f.instruction(&Instruction::I32Const(SIZE_MASK));
    f.instruction(&Instruction::I32And);
    f.instruction(&Instruction::LocalGet(size));
    f.instruction(&Instruction::I32Add);
    f.instruction(&Instruction::I32Const(FREE_TAG));
    f.instruction(&Instruction::I32Or);
    f.instruction(&Instruction::I32Store(HDR_SIZE));
    f.instruction(&Instruction::LocalGet(prev));
    f.instruction(&Instruction::LocalGet(blk));
    f.instruction(&Instruction::I32Load(HDR_NEXT));
    f.instruction(&Instruction::I32Store(HDR_NEXT));
    f.instruction(&Instruction::End);
    f.instruction(&Instruction::Else);
    f.instruction(&Instruction::LocalGet(blk));
    f.instruction(&Instruction::GlobalSet(GLOBAL_PINNED_FREE_HEAD));
    f.instruction(&Instruction::End);

    f.instruction(&Instruction::End);
    f
}

#[cfg(test)]
mod tests {
    //! The allocator is hand-written WASM that no Dovetail program can call
    //! directly — `pinned_alloc`/`pinned_free` are emitted by the compiler
    //! around marshaling, so a Dovetail-source test can only reach them
    //! incidentally and can never provoke a double free at all. These tests
    //! assemble the three generated functions into a standalone core module
    //! and drive them from the host, which is the only place the free list,
    //! the brk and the double-free trap are observable.

    use super::*;
    use wasm_encoder::{
        CodeSection, ConstExpr, ExportKind, ExportSection, FunctionSection, GlobalSection,
        GlobalType, MemorySection, MemoryType, Module, TypeSection,
    };
    use wasmtime::{Engine, Instance, Module as WasmModule, Store, TypedFunc};

    /// Base of the pinned region: page 1. Read out of the shipped global
    /// inits rather than written out again, so the harness cannot quietly
    /// disagree with `emit_global_section` about where the heap starts.
    const PINNED_BASE: i32 =
        super::super::RUNTIME_GLOBAL_INITS[super::super::GLOBAL_PINNED_BRK as usize];

    struct Heap {
        store: Store<()>,
        alloc: TypedFunc<i32, i32>,
        free: TypedFunc<i32, ()>,
        realloc: TypedFunc<(i32, i32, i32, i32), i32>,
        instance: Instance,
    }

    impl Heap {
        fn new() -> Heap {
            let engine = Engine::default();
            let module = WasmModule::new(&engine, build_module()).expect("module is invalid");
            let mut store = Store::new(&engine, ());
            let instance =
                Instance::new(&mut store, &module, &[]).expect("instantiation failed");
            let alloc = instance
                .get_typed_func::<i32, i32>(&mut store, "alloc")
                .unwrap();
            let free = instance
                .get_typed_func::<i32, ()>(&mut store, "free")
                .unwrap();
            let realloc = instance
                .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "realloc")
                .unwrap();
            Heap { store, alloc, free, realloc, instance }
        }

        fn alloc(&mut self, size: i32) -> i32 {
            self.alloc.call(&mut self.store, size).expect("alloc trapped")
        }

        fn free(&mut self, ptr: i32) {
            self.free.call(&mut self.store, ptr).expect("free trapped")
        }

        /// `free` that is expected to trap, returning which trap it was — an
        /// out-of-bounds access from a mangled free list would satisfy a
        /// weaker "it failed somehow" check just as well.
        fn free_expecting_trap(&mut self, ptr: i32) -> wasmtime::Trap {
            match self.free.call(&mut self.store, ptr) {
                Ok(()) => panic!("expected the free to trap, but it returned"),
                Err(e) => *e
                    .downcast_ref::<wasmtime::Trap>()
                    .unwrap_or_else(|| panic!("failed without trapping: {e}")),
            }
        }

        fn realloc(&mut self, old_ptr: i32, old_size: i32, new_size: i32) -> i32 {
            self.realloc_aligned(old_ptr, old_size, 4, new_size)
        }

        fn realloc_aligned(
            &mut self,
            old_ptr: i32,
            old_size: i32,
            align: i32,
            new_size: i32,
        ) -> i32 {
            self.realloc
                .call(&mut self.store, (old_ptr, old_size, align, new_size))
                .expect("realloc trapped")
        }

        /// `realloc` that is expected to trap, returning which trap it was.
        fn realloc_expecting_trap(
            &mut self,
            old_ptr: i32,
            old_size: i32,
            align: i32,
            new_size: i32,
        ) -> wasmtime::Trap {
            match self
                .realloc
                .call(&mut self.store, (old_ptr, old_size, align, new_size))
            {
                Ok(_) => panic!("expected the realloc to trap, but it returned"),
                Err(e) => *e
                    .downcast_ref::<wasmtime::Trap>()
                    .unwrap_or_else(|| panic!("failed without trapping: {e}")),
            }
        }

        /// `alloc` that is expected to trap, returning which trap it was.
        fn alloc_expecting_trap(&mut self, size: i32) -> wasmtime::Trap {
            match self.alloc.call(&mut self.store, size) {
                Ok(_) => panic!("expected the alloc to trap, but it returned"),
                Err(e) => *e
                    .downcast_ref::<wasmtime::Trap>()
                    .unwrap_or_else(|| panic!("failed without trapping: {e}")),
            }
        }

        fn brk(&mut self) -> i32 {
            self.instance
                .get_global(&mut self.store, "brk")
                .unwrap()
                .get(&mut self.store)
                .i32()
                .unwrap()
        }

        fn write(&mut self, ptr: i32, bytes: &[u8]) {
            let memory = self.instance.get_memory(&mut self.store, "memory").unwrap();
            memory
                .write(&mut self.store, ptr as usize, bytes)
                .expect("write out of bounds");
        }

        fn read(&mut self, ptr: i32, len: usize) -> Vec<u8> {
            let memory = self.instance.get_memory(&mut self.store, "memory").unwrap();
            let mut buf = vec![0u8; len];
            memory
                .read(&mut self.store, ptr as usize, &mut buf)
                .expect("read out of bounds");
            buf
        }
    }

    /// A core module holding exactly the three generated functions, with the
    /// globals and memory they expect at the indices they expect.
    fn build_module() -> Vec<u8> {
        let mut module = Module::new();

        let mut types = TypeSection::new();
        types.ty().function(vec![ValType::I32], vec![ValType::I32]); // alloc
        types.ty().function(vec![ValType::I32], vec![]); // free
        types.ty().function(
            vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            vec![ValType::I32],
        ); // realloc
        module.section(&types);

        let mut functions = FunctionSection::new();
        functions.function(0);
        functions.function(1);
        functions.function(2);
        module.section(&functions);

        let mut memory = MemorySection::new();
        memory.memory(MemoryType {
            minimum: 1,
            maximum: Some(256),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        module.section(&memory);

        // The runtime globals themselves, not a copy of them: the generated
        // code hard-codes indices, and a layout change that moved the pinned
        // pair would otherwise leave these tests passing against a module the
        // compiler no longer emits.
        let mut globals = GlobalSection::new();
        for init in super::super::RUNTIME_GLOBAL_INITS {
            globals.global(
                GlobalType { val_type: ValType::I32, mutable: true, shared: false },
                &ConstExpr::i32_const(init),
            );
        }
        module.section(&globals);

        let mut exports = ExportSection::new();
        exports.export("alloc", ExportKind::Func, 0);
        exports.export("free", ExportKind::Func, 1);
        exports.export("realloc", ExportKind::Func, 2);
        exports.export("memory", ExportKind::Memory, 0);
        exports.export("free_head", ExportKind::Global, GLOBAL_PINNED_FREE_HEAD);
        exports.export("brk", ExportKind::Global, GLOBAL_PINNED_BRK);
        module.section(&exports);

        let mut codes = CodeSection::new();
        codes.function(&generate_pinned_alloc());
        codes.function(&generate_pinned_free());
        codes.function(&generate_realloc(0, 1));
        module.section(&codes);

        module.finish()
    }

    #[test]
    fn allocations_are_aligned_distinct_and_writable() {
        let mut heap = Heap::new();
        let a = heap.alloc(10);
        let b = heap.alloc(10);
        assert!(a >= PINNED_BASE && b >= PINNED_BASE);
        assert_eq!(a % 8, 0, "canonical ABI alignment goes up to 8");
        assert_eq!(b % 8, 0);
        assert_ne!(a, b);
        heap.write(a, b"0123456789");
        heap.write(b, b"abcdefghij");
        assert_eq!(heap.read(a, 10), b"0123456789");
        assert_eq!(heap.read(b, 10), b"abcdefghij");
    }

    #[test]
    fn a_freed_block_is_reused_rather_than_extending_the_brk() {
        let mut heap = Heap::new();
        let a = heap.alloc(32);
        let after_first = heap.brk();
        heap.free(a);
        assert_eq!(heap.alloc(32), a);
        assert_eq!(heap.brk(), after_first);
    }

    #[test]
    fn freeing_the_lower_neighbour_last_merges_upward() {
        // Free `b` first, then `a`: `a` is inserted before `b` and absorbs it.
        let mut heap = Heap::new();
        let a = heap.alloc(32);
        let b = heap.alloc(32);
        let guard = heap.alloc(32); // keeps the merged pair away from the brk
        let brk = heap.brk();
        heap.free(b);
        heap.free(a);
        // 32 rounds to a 40-byte block, so the merged block holds 80 bytes:
        // 72 of payload. Uncoalesced, this could only come from the brk.
        assert_eq!(heap.alloc(72), a);
        assert_eq!(heap.brk(), brk, "the merged block covered it");
        heap.free(guard);
    }

    #[test]
    fn freeing_the_upper_neighbour_last_merges_downward() {
        // Free `a` first, then `b`: `b` is inserted after `a` and merges into it.
        let mut heap = Heap::new();
        let a = heap.alloc(32);
        let b = heap.alloc(32);
        let guard = heap.alloc(32);
        let brk = heap.brk();
        heap.free(a);
        heap.free(b);
        assert_eq!(heap.alloc(72), a);
        assert_eq!(heap.brk(), brk);
        heap.free(guard);
    }

    #[test]
    fn a_block_merges_with_both_neighbours_at_once() {
        let mut heap = Heap::new();
        let a = heap.alloc(32);
        let b = heap.alloc(32);
        let c = heap.alloc(32);
        let guard = heap.alloc(32);
        let brk = heap.brk();
        heap.free(a);
        heap.free(c);
        heap.free(b); // lands between two free blocks and joins them
        assert_eq!(heap.alloc(112), a, "three 40-byte blocks make 120");
        assert_eq!(heap.brk(), brk);
        heap.free(guard);
    }

    #[test]
    fn a_window_of_mixed_sizes_freed_in_reverse_comes_back_as_one_block() {
        // The shape `scratch_restore` produces: a burst of adjacent blocks
        // released newest-first at the close of a marshaling window.
        let mut heap = Heap::new();
        let base = heap.alloc(16);
        let sizes = [24, 100, 8, 4096, 40];
        let mut ptrs = vec![];
        for size in sizes {
            ptrs.push(heap.alloc(size));
        }
        let guard = heap.alloc(16);
        let brk = heap.brk();
        for ptr in ptrs.iter().rev() {
            heap.free(*ptr);
        }
        heap.free(base);
        // Everything below the guard is one block again: the sum of the
        // rounded sizes, less the 8-byte header the single block still needs.
        let total: i32 = sizes.iter().map(|s| (s + 7) / 8 * 8 + 8).sum::<i32>() + 24;
        assert_eq!(heap.alloc(total - 8), base);
        assert_eq!(heap.brk(), brk);
        heap.free(guard);
    }

    #[test]
    fn repeated_alloc_free_cycles_do_not_drift_the_brk() {
        // A round allocates two blocks of varying size and frees them, which
        // is the shape of a server loop. Splitting alone answers each round
        // out of the previous round's leftovers and never quite fits, so
        // without merging the brk climbs on a large fraction of rounds; with
        // it, each round hands the region back whole.
        let mut heap = Heap::new();
        let round = |heap: &mut Heap, n: i32| {
            let p = heap.alloc(24 + (n % 5) * 8);
            let q = heap.alloc(300);
            heap.free(p);
            heap.free(q);
        };
        // Settle first: the heap is empty at the start, so the opening rounds
        // legitimately grow it to the working set.
        for n in 0..10 {
            round(&mut heap, n);
        }
        let brk = heap.brk();
        for n in 10..500 {
            round(&mut heap, n);
        }
        assert_eq!(heap.brk(), brk, "steady-state allocation must not grow the heap");
    }

    #[test]
    fn a_grow_that_fits_the_existing_block_stays_put() {
        let mut heap = Heap::new();
        // 20 bytes rounds to a 32-byte block: 24 of capacity.
        let p = heap.alloc(20);
        let brk = heap.brk();
        heap.write(p, b"payload!");
        assert_eq!(heap.realloc(p, 20, 21), p);
        assert_eq!(heap.realloc(p, 21, 24), p, "the whole capacity is usable");
        assert_eq!(heap.realloc(p, 24, 12), p, "a shrink keeps the block");
        assert_eq!(heap.brk(), brk, "nothing was allocated");
        assert_eq!(heap.read(p, 8), b"payload!");
    }

    #[test]
    fn a_grow_past_the_capacity_moves_and_copies() {
        let mut heap = Heap::new();
        let p = heap.alloc(20);
        let pin = heap.alloc(8); // stops the move from being answered in place
        heap.write(p, b"0123456789abcdefghij");
        let q = heap.realloc(p, 20, 40);
        assert_ne!(q, p);
        assert_eq!(heap.read(q, 20), b"0123456789abcdefghij");
        // The old block went back to the free list and is handed out again.
        assert_eq!(heap.alloc(20), p);
        heap.free(pin);
    }

    #[test]
    fn realloc_from_null_allocates_without_copying() {
        let mut heap = Heap::new();
        let p = heap.realloc(0, 0, 16);
        assert!(p >= PINNED_BASE);
        heap.write(p, b"sixteen bytes...");
        assert_eq!(heap.read(p, 16), b"sixteen bytes...");
    }

    #[test]
    fn freeing_a_block_twice_traps() {
        let mut heap = Heap::new();
        let p = heap.alloc(16);
        heap.free(p);
        assert_eq!(
            heap.free_expecting_trap(p),
            wasmtime::Trap::UnreachableCodeReached
        );
    }

    #[test]
    fn freeing_a_block_that_was_merged_into_its_neighbour_traps() {
        // `b` stops existing as a block when `a` absorbs it; its stale header
        // is inside a live free block, and a second free of it must not splice
        // that block's interior into the list.
        let mut heap = Heap::new();
        let a = heap.alloc(32);
        let b = heap.alloc(32);
        heap.free(a);
        heap.free(b);
        assert_eq!(
            heap.free_expecting_trap(b),
            wasmtime::Trap::UnreachableCodeReached
        );
    }

    #[test]
    fn an_alignment_the_pinned_heap_cannot_honour_traps() {
        let mut heap = Heap::new();
        // Unreachable from the canonical ABI as it stands (its maximum
        // alignment is 8), which is the point: the day a WIT type or a canon
        // revision asks for 16, this says so instead of returning a pointer
        // that merely happens to be 8-aligned.
        assert_eq!(
            heap.realloc_expecting_trap(0, 0, 16, 8),
            wasmtime::Trap::UnreachableCodeReached
        );
        // Everything at or below 8 is still served.
        for align in [1, 2, 4, 8] {
            let p = heap.realloc_aligned(0, 0, align, 16);
            assert_eq!(p % 8, 0);
        }
    }

    #[test]
    fn an_old_pointer_below_the_pinned_base_traps() {
        let mut heap = Heap::new();
        // 8 is a plausible-looking pointer that never came out of this
        // allocator: its "header" at 0 is page 0, so the size word reads as
        // whatever happens to be there and the block joins the free list on the
        // way out. Like the alignment trap above, this is unreachable from a
        // conforming host — it hands back only what `cabi_realloc` returned —
        // and the point is that a host that does not, or a lowering that
        // invents a pointer for an empty list, says so here rather than in the
        // next unrelated allocation.
        assert_eq!(
            heap.realloc_expecting_trap(8, 0, 4, 16),
            wasmtime::Trap::UnreachableCodeReached
        );
        // A fresh heap (the trap poisoned that store): the two shapes that must
        // still work are the null `old_ptr` of a first allocation and a real
        // pinned block growing.
        let mut heap = Heap::new();
        let p = heap.realloc(0, 0, 16);
        assert!(p >= PINNED_BASE);
        heap.write(p, &[7u8; 16]);
        let q = heap.realloc(p, 16, 4096);
        assert!(q >= PINNED_BASE);
        assert_eq!(heap.read(q, 16), vec![7u8; 16]);
    }

    #[test]
    fn a_size_that_wraps_the_header_rounding_traps() {
        let mut heap = Heap::new();
        // -1 as an unsigned byte count is 0xFFFFFFFF: rounding it up to a
        // multiple of 8 and adding the header wraps to 8, and the 16-byte
        // minimum would then hand back a block the caller believes is 4GiB.
        assert_eq!(
            heap.alloc_expecting_trap(-1),
            wasmtime::Trap::UnreachableCodeReached
        );
        // The largest size that does NOT wrap is still refused, but as the
        // out-of-memory it is: the heap cannot grow that far, so `pinned_alloc`
        // reaches its own trap rather than corrupting anything on the way.
        assert_eq!(
            heap.alloc_expecting_trap(0x7FFF_FFF0),
            wasmtime::Trap::UnreachableCodeReached
        );
        // And an ordinary size after it, from the same instance: the guard is
        // ahead of every state change, so a refused request leaves nothing
        // behind. (A fresh `Heap`, because a trap poisons this store.)
        let mut heap = Heap::new();
        assert!(heap.alloc(64) >= PINNED_BASE);
    }

    #[test]
    fn a_size_that_wraps_the_brk_advance_traps() {
        let mut heap = Heap::new();
        // 0xFFFF0000 is already a multiple of 8, so it threads the rounding
        // guard intact as a `need` of 0xFFFF0008 — and then `brk + need` wraps
        // to roughly the header size. Left unchecked, that brk is *below* the
        // pinned base, so the grow loop finds nothing to grow and the heap
        // starts handing out page 0 and its own live blocks. Hence the trap
        // *kind*: with the check removed this heap dies on the header store
        // instead, out of bounds of its one page — the shape a live component,
        // whose memory is long since bigger than that, would take silently.
        assert_eq!(
            heap.alloc_expecting_trap(0xFFFF_0000u32 as i32),
            wasmtime::Trap::UnreachableCodeReached
        );
        // Nothing was committed on the way to the trap. (A fresh `Heap`,
        // because a trap poisons this store.)
        let mut heap = Heap::new();
        assert!(heap.alloc(64) >= PINNED_BASE);
    }

    #[test]
    fn reallocation_clears_the_free_tag_so_the_next_free_is_accepted() {
        // The negative control for the two tests above: a block that goes
        // round the alloc/free cycle repeatedly must never trip the guard,
        // whether it comes back whole (no split) or as the head of a split.
        let mut heap = Heap::new();
        for _ in 0..10 {
            let p = heap.alloc(64);
            heap.free(p);
            let whole = heap.alloc(64);
            let split = heap.alloc(8);
            heap.free(split);
            heap.free(whole);
        }
    }
}
