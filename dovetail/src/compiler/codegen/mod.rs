use crate::typechecker::types::VtableMethodGroup;
mod classes;
mod component;
mod dwarf;
mod enums;
mod function_emitter;
mod globals;
#[cfg(test)]
mod identity_tests;
mod instance_key;
// Preserve the generator output; its exact contents are checked by p3-table-gen.
#[rustfmt::skip]
mod p3_imports;
mod realloc;
mod records;
#[cfg(test)]
mod runtime_type_tests;
mod runtime_types;
mod slice_checks;
mod string_functions;
mod string_literals;
mod type_graph;
mod wit_imports;

use std::collections::BTreeMap;
use std::fmt;

use smallvec::{SmallVec, smallvec};
use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, FunctionSection, GlobalSection, GlobalType,
    ImportSection, MemorySection, MemoryType, Module, NameMap, NameSection, SubType, TypeSection,
    ValType,
};

use crate::TestExportInfo;
use crate::common::span::Span;
use crate::common::types::{InterfaceMemberName, MangledName, PackagePath, TypeParamName};
use crate::compiler::witgen::WitImportUniverse;
use crate::monomorphize::substitute::apply_type_substitution;
use crate::typechecker::types::{
    CapturedVar, Type, TypeDef, TypedClosureParam, TypedExpr, TypedExprKind, TypedModule,
};
use instance_key::{InstanceKey, instance_key};
use wit_imports::WitImportRegistry;

/// Concrete type args to pass to a vtable slot's impl method at a given class
/// instantiation: substitute the class's type params with the instantiation's
/// args into the slot's `impl_type_params`. For own methods this yields the
/// instantiation's args; for a method inherited from a generic ancestor bound to
/// concrete/other types it yields that ancestor binding.
fn substitute_slot_type_params(
    class_type_params: &[TypeParamName],
    instantiation: &[Type],
    slot_impl_type_params: &[Type],
) -> Vec<Type> {
    if slot_impl_type_params.is_empty() {
        return Vec::new();
    }
    let binding: BTreeMap<TypeParamName, Type> = class_type_params
        .iter()
        .cloned()
        .zip(instantiation.iter().cloned())
        .collect();
    slot_impl_type_params
        .iter()
        .map(|t| apply_type_substitution(t, &binding))
        .collect()
}

/// Returns the number of bytes needed to LEB128-encode a u32 value.
fn leb128_u32_size(mut value: u32) -> usize {
    let mut size = 1;
    while value >= 0x80 {
        value >>= 7;
        size += 1;
    }
    size
}

/// Maps a byte offset within a function body to a source location.
struct FunctionSourceMapping {
    /// Byte offset within the function body (after locals encoding).
    byte_offset: u32,
    span: Span,
}

/// Debug info for one function: body size + source mappings.
struct FunctionDebugInfo {
    /// Total byte size of encoded function body (locals + instructions + end).
    body_byte_len: usize,
    /// Source mappings within this function body.
    mappings: Vec<FunctionSourceMapping>,
}

/// Error type for code generation failures.
#[derive(Debug)]
pub struct CodeGenError {
    pub message: String,
}

impl fmt::Display for CodeGenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

// --- Index constants ---
// The core module has a fixed layout of types, imports, and runtime functions
// before user-defined content.

/// Number of WASI imported functions — mirrors the generated p3 import table
/// (see `p3_imports.rs`, derived from the vendored WIT by `tools/p3-table-gen`).
const NUM_IMPORTS: u32 = p3_imports::NUM_P3_IMPORTS;

/// Number of runtime functions (run, run_post, realloc, initialize, string_eq, string_concat,
/// string_cmp, char_to_string, string_from_bytes, string_get_char, debug_print, panic_with_message,
/// console_print, console_eprint, console_eprintln).
const NUM_RUNTIME_FUNCS: u32 = 16;

/// First user global index (after bump allocator at index 0).
/// Runtime globals: 0 = scratch bump pointer; 1/2 = the blocking print path's
/// own stdout/stderr writable stream ends (0 = uninitialized); 3 = the private
/// stdio flush waitable-set (0 = uninitialized). 1-3 belong to
/// `string_functions::emit_flush_write` alone — `standard.io.Console` opens its
/// own stream pairs and drives them through the fiber runtime.
pub(super) const GLOBAL_STDOUT_WRITABLE: u32 = 1;
pub(super) const GLOBAL_STDERR_WRITABLE: u32 = 2;
pub(super) const GLOBAL_FLUSH_SET: u32 = 3;
/// Pinned-heap free-list head (0 = empty) and break pointer (see realloc.rs).
pub(super) const GLOBAL_PINNED_FREE_HEAD: u32 = 4;
pub(super) const GLOBAL_PINNED_BRK: u32 = 5;
/// Head of the current marshaling window's list of pinned scratch blocks
/// (0 = empty). Dynamically-sized marshaling payloads — strings, byte lists —
/// cannot live in the fixed scratch arena, so they are pinned-allocated and
/// threaded onto this list through their block header's `next` word; the
/// window's `scratch_restore` frees everything allocated inside it.
pub(super) const GLOBAL_SCRATCH_PINNED_HEAD: u32 = 6;
pub(super) const GLOBAL_IDENTITY_HASH_COUNTER: u32 = 7;
const USER_GLOBAL_BASE: u32 = 8;

/// Initial values of the runtime globals, indexed by the `GLOBAL_*` constants
/// above — the whole layout in one place. `emit_global_section` emits it, and
/// `realloc`'s test harness builds its standalone module from it, so the 15
/// allocator tests cannot go on certifying a module shape that stopped
/// shipping. Its length is `USER_GLOBAL_BASE`.
pub(super) const RUNTIME_GLOBAL_INITS: [i32; USER_GLOBAL_BASE as usize] =
    [0, 0, 0, 0, 0, SCRATCH_LIMIT, 0, 1];

/// End of the scratch bump arena: the pinned heap starts here, so a scratch
/// allocation that would reach this address must trap rather than scribble
/// over live pinned blocks.
pub(super) const SCRATCH_LIMIT: i32 = 65536;

/// Type index for run_post: (i32) -> ()
const TYPE_RUN_POST: u32 = 3;
/// Type index for realloc: (i32, i32, i32, i32) -> i32
const TYPE_REALLOC: u32 = 4;
/// Type index for initialize: () -> ()
const TYPE_INITIALIZE: u32 = 5;

/// Type index for the u8 backing array type: (array (mut i8))
/// Used by String (as backing store) and as the `$Array$i8` element storage for
/// `Array<Uint8>` (see `ARRAY_I8_TYPE_INDEX`).
pub(super) const U8_BACKING_TYPE_INDEX: u32 = 6;

/// Type index for the string struct type: (struct (ref $backing) i32)
pub(super) const STRING_STRUCT_TYPE_INDEX: u32 = 7;

/// Type index for string_eq: (ref $string_struct, ref $string_struct) -> i32
const TYPE_STRING_EQ: u32 = 8;

/// Type index for string_concat: (ref $string_struct, ref $string_struct) -> ref $string_struct
const TYPE_STRING_CONCAT: u32 = 9;

/// Type index for char_to_string: (i32) -> ref $string_struct
const TYPE_CHAR_TO_STRING: u32 = 10;

/// Type index for string_from_bytes: (ref $backing, i32, i32) -> ref $string_struct
const TYPE_STRING_FROM_BYTES: u32 = 11;

/// Type index for string_get_char: (ref $string_struct, i32) -> i32
const TYPE_STRING_GET_CHAR: u32 = 12;

/// Type index for debug_print: (ref $string_struct) -> ()
const TYPE_DEBUG_PRINT: u32 = 13;

// Boxing struct types for primitives (used when storing primitives as Any).
// Each is a final struct with one immutable field of the appropriate WASM type.
// Nine of the thirteen are byte-identical `(struct (field i32))`; they discriminate under
// `ref.test`/`ref.cast` only because they are distinct members of the module-wide rec group
// (index is part of a type's identity, structure alone is not). See emit_type_section.
const BOX_UNIT_TYPE_INDEX: u32 = 14;
const BOX_BOOL_TYPE_INDEX: u32 = 15;
const BOX_CHAR_TYPE_INDEX: u32 = 16;
const BOX_INT8_TYPE_INDEX: u32 = 17;
const BOX_INT16_TYPE_INDEX: u32 = 18;
const BOX_INT32_TYPE_INDEX: u32 = 19;
const BOX_UINT8_TYPE_INDEX: u32 = 20;
const BOX_UINT16_TYPE_INDEX: u32 = 21;
const BOX_UINT32_TYPE_INDEX: u32 = 22;
const BOX_INT64_TYPE_INDEX: u32 = 23;
const BOX_UINT64_TYPE_INDEX: u32 = 24;
const BOX_FLOAT32_TYPE_INDEX: u32 = 25;
const BOX_FLOAT64_TYPE_INDEX: u32 = 26;

// Mutable box struct types for mutable captures in closures.
// Each has a single mutable field of the appropriate WASM type.
const MUT_BOX_I32_TYPE_INDEX: u32 = 27;
const MUT_BOX_I64_TYPE_INDEX: u32 = 28;
const MUT_BOX_F32_TYPE_INDEX: u32 = 29;
const MUT_BOX_F64_TYPE_INDEX: u32 = 30;
pub(super) const MUT_BOX_REF_TYPE_INDEX: u32 = 31;

/// Dedicated boxed form of `Uint128`: `(struct (field (mut i64)) (field (mut i64)))`.
/// Holds the two halves as raw `i64` fields (lo, hi) — never the generic anyref `$Tuple_2`,
/// so boxing a `Uint128` is one `struct.new` and unboxing is two `struct.get`, with no
/// per-leaf i64-into-anyref boxing. Mutable so it can double as the closure mut-box.
pub(super) const UINT128_STRUCT_TYPE_INDEX: u32 = 33;

// The fixed array set gives every primitive a distinct runtime identity. Reference
// elements share one array type. Same-layout primitives remain distinct within the
// explicit recursive group, allowing erased readonly reads to recover box identity.
// Uint8 reuses the String backing array (6); all others are emitted below.
/// `$Array$i8` — `(array (mut i8))`, shared with the String backing store. Uint8 elements.
pub(super) const ARRAY_I8_TYPE_INDEX: u32 = U8_BACKING_TYPE_INDEX;
/// `$Array$i16` — `(array (mut i16))`. Int16 elements.
const ARRAY_I16_TYPE_INDEX: u32 = 34;
/// `$Array$i32` — `(array (mut i32))`. Int32 elements.
const ARRAY_I32_TYPE_INDEX: u32 = 35;
/// `$Array$i64` — `(array (mut i64))`. Int64 elements.
const ARRAY_I64_TYPE_INDEX: u32 = 36;
/// `$Array$f32` — `(array (mut f32))`. Float32 elements.
const ARRAY_F32_TYPE_INDEX: u32 = 37;
/// `$Array$f64` — `(array (mut f64))`. Float64 elements.
const ARRAY_F64_TYPE_INDEX: u32 = 38;
/// `$Array$u128` — `(array (mut (ref $Uint128)))`. Uint128 elements stay boxed (two-i64 run has
/// no inline array slot); dedicated so reads need only an *unbox*, no `ref.cast`.
const ARRAY_U128_TYPE_INDEX: u32 = 39;
/// `$Array$ref` — `(array (mut (ref any)))`. Every reference element type (String, record,
/// enum, class, tuple, closure, interface object, nested array, `Any`). Reads `ref.cast` back to the
/// concrete element the reader knows.
pub(super) const ARRAY_REF_TYPE_INDEX: u32 = 40;

// Distinct identities for primitives that share a physical storage layout.
const ARRAY_INT8_TYPE_INDEX: u32 = 41;
const ARRAY_UINT16_TYPE_INDEX: u32 = 42;
const ARRAY_UINT32_TYPE_INDEX: u32 = 43;
const ARRAY_BOOL_TYPE_INDEX: u32 = 44;
const ARRAY_CHAR_TYPE_INDEX: u32 = 45;
const ARRAY_UNIT_TYPE_INDEX: u32 = 46;
const ARRAY_UINT64_TYPE_INDEX: u32 = 47;
// This roster also drives emission and erased readonly-slice reads. Order follows
// fixed type indices; Uint8 is emitted earlier with the String backing type.
const ARRAY_ELEMENT_TYPES: &[(Type, u32)] = &[
    (Type::Uint8, ARRAY_I8_TYPE_INDEX),
    (Type::Int16, ARRAY_I16_TYPE_INDEX),
    (Type::Int32, ARRAY_I32_TYPE_INDEX),
    (Type::Int64, ARRAY_I64_TYPE_INDEX),
    (Type::Float32, ARRAY_F32_TYPE_INDEX),
    (Type::Float64, ARRAY_F64_TYPE_INDEX),
    (Type::Uint128, ARRAY_U128_TYPE_INDEX),
    (Type::Any, ARRAY_REF_TYPE_INDEX),
    (Type::Int8, ARRAY_INT8_TYPE_INDEX),
    (Type::Uint16, ARRAY_UINT16_TYPE_INDEX),
    (Type::Uint32, ARRAY_UINT32_TYPE_INDEX),
    (Type::Bool, ARRAY_BOOL_TYPE_INDEX),
    (Type::Char, ARRAY_CHAR_TYPE_INDEX),
    (Type::Unit, ARRAY_UNIT_TYPE_INDEX),
    (Type::Uint64, ARRAY_UINT64_TYPE_INDEX),
];

/// First user type index (after fixed types 0-47).
const USER_TYPE_BASE: u32 = 48;

// Import function indices

/// Type: `(i32) -> i32` — `pinned_alloc`'s signature.
const TYPE_PINNED_ALLOC: u32 = 32;

/// Wrap a function signature as a `SubType`, for func types that live *inside* the module-wide
/// rec group. `TypeSection::function` has no `SubType` form, and a group is emitted as one
/// `Vec<SubType>`, so the group's func types are built through here instead.
fn func_subtype(
    params: impl IntoIterator<Item = ValType>,
    results: impl IntoIterator<Item = ValType>,
) -> SubType {
    SubType {
        is_final: true,
        supertype_idx: None,
        composite_type: wasm_encoder::CompositeType {
            inner: wasm_encoder::CompositeInnerType::Func(wasm_encoder::FuncType::new(
                params.into_iter().collect::<Vec<_>>(),
                results.into_iter().collect::<Vec<_>>(),
            )),
            shared: false,
            describes: None,
            descriptor: None,
        },
    }
}

/// Info about a wrapper function for interface object dispatch. The wrapper bridges the **erased
/// vtable-slot ABI** (the trait's generic params lowered to `anyref`) to the **concrete impl ABI**:
/// it casts `self` and every erased param back to the impl's concrete type, calls the impl, and
/// boxes an erased return back to `anyref`. Slot params that are already concrete forward directly.
struct WrapperFunc {
    /// The coercion group key this wrapper belongs to — the component's
    /// `$IfaceObj$…` key, `$via$`-tagged when a sub-trait's block backs the
    /// slots. A bare-`Self` return re-boxes through THIS group's standalone
    /// vtable global when no direct (un-tagged) global exists.
    group_key: MangledName,
    /// Type index of the wrapper function type (the erased vtable-slot signature).
    func_type_index: u32,
    /// The concrete impl method this wrapper delegates to.
    impl_method_mangled: MangledName,
    /// The concrete type of the receiver (for the `self` ref.cast).
    concrete_type: Type,
    /// Per non-self param: (slot type as it appears in the erased vtable slot — a trait generic
    /// param shows as a `TypeVariable`/`GenericParam`/`Any`, concrete types as themselves; the
    /// concrete impl param type). When the two lower to different valtypes the slot is erased and
    /// the wrapper casts back.
    params: Vec<(Type, Type)>,
    /// The erased vtable-slot return type and the concrete impl return type.
    slot_return: Type,
    concrete_return: Type,
}

/// Pre-scanned closure info extracted from the typed AST (wave-order).
struct ClosureInfo {
    params: Vec<TypedClosureParam>,
    captures: Vec<CapturedVar>,
    body: TypedExpr,
    param_types: Vec<Type>,
    return_type: Type,
}

/// Pre-scanned info for a function/method reference trampoline.
struct RefTrampoline {
    target_mangled: MangledName,
    /// None for FunctionRef, Some(T) for MethodRef
    self_type: Option<Type>,
    param_types: Vec<Type>,
    return_type: Type,
}

/// Pre-scanned interface object coercion info extracted from the typed AST.
struct InterfaceObjectInfo {
    /// The SET key of the coercion target (`$IfaceObj$A` / `$IfaceObj$A&B`).
    interface_mangled_name: MangledName,
    concrete_type: Type,
    /// Grouped per component: (component per-trait key, slot entries), sorted.
    vtable_methods: Vec<VtableMethodGroup>,
}

/// Holds the state for the codegen phase.
struct Codegen<'a> {
    registry: &'a crate::typechecker::registry::Registry,
    runtime_types: runtime_types::RuntimeTypes,
    typed_module: &'a TypedModule,
    /// Number of dynamic `[task-return]` imports (one per async-lifted
    /// export: run + each test). Runtime/user function indices float above
    /// the static import table plus these.
    num_task_return_imports: u32,
    /// WIT component-import interfaces this project imports (empty for the
    /// vast majority of compiles). Threaded from the pipeline. The marshaling
    /// body reads its `resolve`/`interfaces`/`table`.
    wit_imports: &'a WitImportUniverse,
    /// Import table for the WIT component imports: `WitFuncRef` → core-import
    /// function index, plus the ordered import list and per-import func types.
    wit_registry: WitImportRegistry,
    /// Number of WIT component imports (`wit_registry.len()`). Shifts the
    /// runtime/user function index space (via `runtime_func_base`), after the
    /// static table + `[task-return]` imports.
    num_wit_imports: u32,
    module: Module,
    function_indices: BTreeMap<MangledName, u32>,
    global_indices: BTreeMap<MangledName, u32>,
    type_indices: BTreeMap<MangledName, u32>,
    /// Maps (enum_mangled_name, variant_name) → WASM struct type index.
    variant_type_indices: BTreeMap<(MangledName, String), u32>,
    main_func_index: Option<u32>,
    /// Per-entry function-type index for the static p3 import table
    /// (parallel to `p3_imports::P3_IMPORTS`), assigned at the end of
    /// `emit_type_section`.
    import_type_indices: Vec<u32>,
    /// Func-type index for `[task-return]run` — `(param i32)` (result discriminant).
    type_task_return_run: u32,
    /// Func-type index for test `[task-return]` imports — no params.
    type_task_return_unit: u32,
    next_type_index: u32,
    /// Type index where user function types begin (set after emitting struct/array types).
    func_type_base: u32,
    /// Dedup map: string content → data segment index.
    string_data_indices: BTreeMap<String, u32>,
    /// Ordered byte payloads for passive data segments.
    string_data_payloads: Vec<Vec<u8>>,
    /// (declaring_project_root, resource_name) → passive data segment index.
    /// Populated up-front from the registry; consumed by emit_intrinsic_call
    /// for `Resource.bytes(...)` to produce an `array.new_data`.
    pub(super) resource_data_segment_indices: BTreeMap<(PackagePath, String), u32>,
    /// (declaring_project_root, resource_name) → byte length of the embedded
    /// blob, used as the `array.new_data` length immediate.
    pub(super) resource_byte_lengths: BTreeMap<(PackagePath, String), usize>,
    /// interface_mangled_name → WASM type index of the interface object struct type
    interface_object_type_indices: BTreeMap<MangledName, u32>,
    /// interface_mangled_name → WASM type index of the vtable struct type
    vtable_type_indices: BTreeMap<MangledName, u32>,
    /// (concrete_type_instance_key, interface_mangled_name) → WASM global index of the vtable instance.
    /// The type key is the *per-instantiation* `instance_key` (includes type args), so two
    /// instantiations of a generic impl class get distinct vtable globals even though they share one
    /// per-trait interface-object WASM type.
    vtable_global_indices: BTreeMap<(InstanceKey, MangledName), u32>,
    /// Number of interface vtable globals actually EMITTED (the indices map
    /// may also hold aliases that don't correspond to their own global).
    num_interface_vtable_globals: u32,
    /// (interface_mangled_name, member_name) → vtable field index
    trait_method_vtable_indices: BTreeMap<(MangledName, InterfaceMemberName), u32>,
    /// (interface_mangled_name, vtable_field_index) → wrapper function type index (for call_ref)
    wrapper_func_type_indices: BTreeMap<(MangledName, u32), u32>,
    /// Wrapper functions for vtable dispatch
    wrapper_funcs: Vec<WrapperFunc>,
    /// Pre-scanned interface object coercion info
    interface_object_infos: Vec<InterfaceObjectInfo>,
    /// (type instance key, full via group key) → direct group key for
    /// bare-`Self` re-boxing. The type directly implements the same super
    /// application the group backs. See
    /// `TypedModule.direct_rebox_authorizations`.
    pub(super) direct_rebox_authorized: BTreeMap<(InstanceKey, MangledName), MangledName>,
    /// class_mangled_name → WASM type index of the class vtable struct type
    class_vtable_type_indices: BTreeMap<MangledName, u32>,
    /// (class_mangled_name, vtable_slot) → func type index (for call_ref)
    class_vtable_slot_func_types: BTreeMap<(MangledName, u32), u32>,
    /// (canonical_class_mangled_name, type_args) → WASM global index of the class vtable instance.
    /// For non-generic classes, type_args is empty. For generic classes, each instantiation
    /// has its own vtable global (with monomorphized method funcrefs) even though the vtable
    /// WASM struct *type* is shared across instantiations.
    class_vtable_global_indices: std::collections::HashMap<(MangledName, Vec<Type>), u32>,
    /// impl method mangled names that need ref.func declarations (for vtable globals)
    class_vtable_impl_func_indices: Vec<u32>,
    /// Virtual method impl_mangled → vtable slot func type index.
    /// Used so the function section declares virtual methods with the vtable slot type.
    virtual_method_func_types: BTreeMap<MangledName, u32>,
    /// For each concrete monomorphized class method registered in `virtual_method_func_types`,
    /// the corresponding `VtableSlot`'s `(param_types, return_type)` from the canonical class.
    /// `param_types` may contain `Type::TypeVariable`s (lower to anyref). Used by function-body
    /// emission to know which param positions need cast-back-from-anyref and whether the body
    /// needs to box its return for the erased vtable slot signature.
    virtual_method_slot_sigs: BTreeMap<MangledName, (Vec<Type>, Type)>,
    /// Pre-scanned closure info (wave-order)
    closure_infos: Vec<ClosureInfo>,
    /// Canonical closure types under always-erased closures: arity N → (func_type_idx, struct_type_idx).
    /// `func_type_idx` is `Func_N = (func (param anyref) ... N+1 times (result anyref))`;
    /// `struct_type_idx` is `Closure_N = (struct (field anyref) (field (ref Func_N)))`.
    /// All `Type::Function([P1..Pn], R)` lower to `(ref Closure_N)` regardless of param/return types —
    /// the closure body's prologue casts params back to their declared types and the epilogue boxes
    /// the return. Populated by `discover_closure_arities` + `emit_closure_arity_types`.
    closure_arity_indices: BTreeMap<u32, (u32, u32)>,
    /// Flattened tuple width N → the boxed `$Tuple_N = (struct (field anyref) × N)` struct type
    /// index. One shared struct per width (like `closure_arity_indices` per arity), discovered by
    /// `discover_tuple_widths` and emitted by `emit_tuple_width_types`. A tuple's boxed form is
    /// `(ref $Tuple_N)`; box = box each leaf to anyref + `struct.new`, unbox = N × `struct.get` +
    /// cast each leaf back. Replaces the per-shape `$Tuple$…` typedefs (no longer monomorphized).
    tuple_width_indices: BTreeMap<u32, u32>,
    /// Closure ID → env struct type index (None if no captures)
    closure_env_type_indices: Vec<Option<u32>>,
    /// Closure ID → lifted function index
    closure_func_indices: Vec<u32>,
    /// Counter for matching closures during code emission
    closure_emit_counter: std::cell::Cell<usize>,
    /// Pre-scanned ref trampolines (for FunctionRef/MethodRef)
    ref_trampolines: Vec<RefTrampoline>,
    /// Dedup key → WASM function index (populated during emit_function_section)
    ref_trampoline_indices: BTreeMap<String, u32>,
    /// WASM function indices of $test$ functions (for generating test wrappers)
    test_func_indices: Vec<u32>,
    /// First test wrapper function index (populated in emit_function_section)
    test_wrapper_base: u32,
    /// Debug info for user functions and closures: (code_section_offset, debug_info).
    debug_infos: Vec<(u32, FunctionDebugInfo)>,
}

/// Function-index accessors. Imports occupy the front of the index space:
/// the static WASI import table, then dynamic `[task-return]` imports, then
/// runtime functions, then user functions.
impl Codegen<'_> {
    fn runtime_func_base(&self) -> u32 {
        NUM_IMPORTS + self.num_task_return_imports + self.num_wit_imports
    }

    /// Function index of the first WIT component import.
    fn wit_import_base(&self) -> u32 {
        NUM_IMPORTS + self.num_task_return_imports
    }

    /// `[task-return]run` import index.
    pub(super) fn func_task_return_run(&self) -> u32 {
        NUM_IMPORTS
    }

    /// `[task-return]test-n{i}` import index.
    pub(super) fn func_task_return_test(&self, i: u32) -> u32 {
        NUM_IMPORTS + 1 + i
    }

    /// `run` is the first runtime function; `realloc` and `_initialize` follow.
    pub(super) fn func_run(&self) -> u32 {
        self.runtime_func_base()
    }

    pub(super) fn func_realloc(&self) -> u32 {
        self.runtime_func_base() + 1
    }
    pub(super) fn func_initialize(&self) -> u32 {
        self.runtime_func_base() + 2
    }
    pub(super) fn func_string_eq(&self) -> u32 {
        self.runtime_func_base() + 3
    }
    pub(super) fn func_string_concat(&self) -> u32 {
        self.runtime_func_base() + 4
    }
    pub(super) fn func_string_cmp(&self) -> u32 {
        self.runtime_func_base() + 5
    }
    pub(super) fn func_char_to_string(&self) -> u32 {
        self.runtime_func_base() + 6
    }
    pub(super) fn func_string_from_bytes(&self) -> u32 {
        self.runtime_func_base() + 7
    }
    pub(super) fn func_string_get_char(&self) -> u32 {
        self.runtime_func_base() + 8
    }
    pub(super) fn func_debug_print(&self) -> u32 {
        self.runtime_func_base() + 9
    }
    pub(super) fn func_panic_with_message(&self) -> u32 {
        self.runtime_func_base() + 10
    }
    pub(super) fn func_console_print(&self) -> u32 {
        self.runtime_func_base() + 11
    }
    pub(super) fn func_console_eprint(&self) -> u32 {
        self.runtime_func_base() + 12
    }
    pub(super) fn func_console_eprintln(&self) -> u32 {
        self.runtime_func_base() + 13
    }

    pub(super) fn func_pinned_alloc(&self) -> u32 {
        self.runtime_func_base() + 14
    }

    pub(super) fn func_pinned_free(&self) -> u32 {
        self.runtime_func_base() + 15
    }

    pub(super) fn user_func_base(&self) -> u32 {
        self.runtime_func_base() + NUM_RUNTIME_FUNCS
    }
}

impl<'a> Codegen<'a> {
    fn new(
        typed_module: &'a TypedModule,
        wit_imports: &'a WitImportUniverse,
        registry: &'a crate::typechecker::registry::Registry,
    ) -> Self {
        // One `[task-return]` import per async-lifted export: run + each test.
        let num_tests = typed_module
            .functions
            .keys()
            .filter(|name| name.0.starts_with("$test$"))
            .count() as u32;
        let num_task_return_imports = 1 + num_tests;
        // WIT component imports slot in right after the `[task-return]`
        // imports; their count shifts the runtime/user function indices.
        let wit_import_base = NUM_IMPORTS + num_task_return_imports;
        let wit_registry = WitImportRegistry::build(wit_imports, wit_import_base);
        let num_wit_imports = wit_registry.len();
        let user_func_base =
            NUM_IMPORTS + num_task_return_imports + num_wit_imports + NUM_RUNTIME_FUNCS;
        // User functions start at user_func_base
        let function_indices: BTreeMap<MangledName, u32> = typed_module
            .functions
            .keys()
            .enumerate()
            .map(|(i, name)| (name.clone(), user_func_base + i as u32))
            .collect();

        let global_indices: BTreeMap<MangledName, u32> = typed_module
            .globals
            .keys()
            .enumerate()
            .map(|(i, name)| (name.clone(), USER_GLOBAL_BASE + i as u32))
            .collect();

        // Find main function index
        let main_func_index = typed_module.main_function_fqn.as_ref().and_then(|fqn| {
            let main_name = MangledName::for_function_no_params(fqn);
            function_indices.get(&main_name).copied()
        });

        // Collect test function indices (in BTreeMap iteration order)
        let test_func_indices: Vec<u32> = function_indices
            .iter()
            .filter(|(name, _)| name.0.starts_with("$test$"))
            .map(|(_, &idx)| idx)
            .collect();

        let mut codegen = Self {
            registry,
            runtime_types: Default::default(),
            typed_module,
            num_task_return_imports,
            wit_imports,
            wit_registry,
            num_wit_imports,
            import_type_indices: Vec::new(),
            type_task_return_run: 0,
            type_task_return_unit: 0,
            module: Module::new(),
            function_indices,
            global_indices,
            type_indices: BTreeMap::new(),
            variant_type_indices: BTreeMap::new(),
            main_func_index,
            next_type_index: USER_TYPE_BASE,
            func_type_base: USER_TYPE_BASE,
            string_data_indices: BTreeMap::new(),
            string_data_payloads: Vec::new(),
            resource_data_segment_indices: BTreeMap::new(),
            resource_byte_lengths: BTreeMap::new(),
            interface_object_type_indices: BTreeMap::new(),
            vtable_type_indices: BTreeMap::new(),
            vtable_global_indices: BTreeMap::new(),
            num_interface_vtable_globals: 0,
            trait_method_vtable_indices: BTreeMap::new(),
            wrapper_func_type_indices: BTreeMap::new(),
            wrapper_funcs: Vec::new(),
            interface_object_infos: Vec::new(),
            direct_rebox_authorized: typed_module
                .direct_rebox_authorizations
                .iter()
                .map(|(ty, key, direct_key)| ((instance_key(ty), key.clone()), direct_key.clone()))
                .collect(),
            class_vtable_type_indices: BTreeMap::new(),
            class_vtable_slot_func_types: BTreeMap::new(),
            class_vtable_global_indices: std::collections::HashMap::new(),
            class_vtable_impl_func_indices: Vec::new(),
            virtual_method_func_types: BTreeMap::new(),
            virtual_method_slot_sigs: BTreeMap::new(),
            closure_infos: Vec::new(),
            closure_arity_indices: BTreeMap::new(),
            tuple_width_indices: BTreeMap::new(),
            closure_env_type_indices: Vec::new(),
            closure_func_indices: Vec::new(),
            closure_emit_counter: std::cell::Cell::new(0),
            ref_trampolines: Vec::new(),
            ref_trampoline_indices: BTreeMap::new(),
            test_func_indices,
            test_wrapper_base: 0,
            debug_infos: Vec::new(),
        };
        codegen.prepare_runtime_types();
        codegen.prescan_interface_objects();
        codegen.prescan_closures();
        codegen.prescan_ref_trampolines();
        codegen
    }

    /// Pre-scan typed module AST to find all InterfaceObjectCoerce nodes.
    fn prescan_interface_objects(&mut self) {
        for func in self.typed_module.functions.values() {
            self.scan_expr_for_interface_objects(&func.body);
        }
        for global in self.typed_module.globals.values() {
            self.scan_expr_for_interface_objects(&global.initializer);
        }
        // Scan class initializers and extends_args
        for type_def in self.typed_module.types.values() {
            if let TypeDef::Class(cls) = type_def {
                for stmt in &cls.initializer {
                    self.scan_expr_for_interface_objects(stmt);
                }
                if let Some(extends_args) = &cls.extends_args {
                    for arg in extends_args {
                        self.scan_expr_for_interface_objects(arg);
                    }
                }
            }
        }
        // Synthetic direct super coercions (globals/wrappers only, no
        // expression): same dedup as the expression scan, so a real direct
        // coercion anywhere in the program takes the key first.
        for syn in &self.typed_module.synthetic_interface_coercions {
            let type_key = instance_key(&syn.concrete_type);
            let global_key =
                Self::coercion_global_key(&syn.interface_mangled_name, &syn.vtable_methods);
            if !self
                .vtable_global_indices
                .contains_key(&(type_key.clone(), global_key.clone()))
            {
                self.vtable_global_indices
                    .insert((type_key, global_key), u32::MAX);
                self.interface_object_infos.push(InterfaceObjectInfo {
                    interface_mangled_name: syn.interface_mangled_name.clone(),
                    concrete_type: syn.concrete_type.clone(),
                    vtable_methods: syn.vtable_methods.clone(),
                });
            }
        }
    }

    /// Pre-scan typed module AST to find all Closure nodes in wave order.
    /// Wave 0: closures in user function/global/class-init bodies (not recursing into closure bodies).
    /// Wave 1: closures found in wave 0's closure bodies. Repeat until empty.
    fn prescan_closures(&mut self) {
        // Wave 0 must be walked in the SAME order the code section emits these
        // bodies, because a closure's id is a running counter shared by the scan
        // and the emit: `initialize` (the global initializers, in dependency
        // order) is a runtime function emitted BEFORE any user function, so the
        // globals come first here too. Scanning them in declaration order after
        // the functions — as this used to — shifted every id and handed closures
        // each other's env struct types.
        let mut current_wave_exprs: Vec<&TypedExpr> = Vec::new();
        for name in self.global_initializer_order() {
            current_wave_exprs.push(&self.typed_module.globals[name].initializer);
        }
        for func in self.typed_module.functions.values() {
            current_wave_exprs.push(&func.body);
        }
        // NOTE: class initializer statements and extends-args are NOT walked
        // here. `emit_class_hierarchy` inlines them at every `ClassNew` site, so
        // the scan walks them from the `ClassNew` arm of `scan_closures_in_expr`
        // instead — once per construction site, exactly mirroring emission. A
        // one-shot walk here would give a class's closures a single id while the
        // emitter consumed one id per site.

        let mut wave_bodies: Vec<TypedExpr> = Vec::new();
        for expr in &current_wave_exprs {
            self.scan_closures_in_expr(expr, &mut wave_bodies);
        }

        // Process subsequent waves
        loop {
            if wave_bodies.is_empty() {
                break;
            }
            let current = std::mem::take(&mut wave_bodies);
            for expr in &current {
                self.scan_closures_in_expr(expr, &mut wave_bodies);
            }
        }
    }

    /// Scan an expression for Closure nodes. Does NOT recurse into closure bodies;
    /// instead pushes them to `next_wave_bodies` for the next wave.
    fn scan_closures_in_expr(&mut self, expr: &TypedExpr, next_wave_bodies: &mut Vec<TypedExpr>) {
        match &expr.kind {
            TypedExprKind::Closure {
                params,
                body,
                captures,
            } => {
                let param_types: Vec<Type> = params.iter().map(|p| p.ty.clone()).collect();
                // Use the closure's declared return type (from expr.ty) rather than
                // the body's type. After variance casts, the closure's Function type
                // may declare a wider return (e.g. Any) than the body actually produces.
                // The WASM function signature must match the declared type.
                let return_type = match &expr.ty {
                    Type::Function(_, ret) => (**ret).clone(),
                    _ => body.ty.clone(),
                };
                self.closure_infos.push(ClosureInfo {
                    params: params.clone(),
                    captures: captures.clone(),
                    body: (**body).clone(),
                    param_types,
                    return_type,
                });
                // Push body to next wave — don't recurse into it
                next_wave_bodies.push((**body).clone());
            }
            TypedExprKind::Block(exprs) => {
                for e in exprs {
                    self.scan_closures_in_expr(e, next_wave_bodies);
                }
            }
            TypedExprKind::Let { value, .. }
            | TypedExprKind::Assign { value, .. }
            | TypedExprKind::Panic { message: value }
            | TypedExprKind::BoxToAny { inner: value }
            | TypedExprKind::NewtypeCreate { value }
            | TypedExprKind::NewtypeValue { value }
            | TypedExprKind::GlobalAssign { value, .. }
            | TypedExprKind::Return { value, .. }
            | TypedExprKind::UnaryOp { operand: value, .. }
            | TypedExprKind::FieldAccess { object: value, .. }
            | TypedExprKind::MethodRef { object: value, .. }
            | TypedExprKind::TypeTest { value, .. }
            | TypedExprKind::TypeCast { value, .. }
            | TypedExprKind::InterfaceObjectCoerce { inner: value, .. }
            | TypedExprKind::InterfaceObjectUpcast { inner: value } => {
                self.scan_closures_in_expr(value, next_wave_bodies);
            }
            TypedExprKind::BinaryOp { left, right, .. } => {
                self.scan_closures_in_expr(left, next_wave_bodies);
                self.scan_closures_in_expr(right, next_wave_bodies);
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.scan_closures_in_expr(condition, next_wave_bodies);
                self.scan_closures_in_expr(then_branch, next_wave_bodies);
                if let Some(e) = else_branch {
                    self.scan_closures_in_expr(e, next_wave_bodies);
                }
            }
            TypedExprKind::While { condition, body } => {
                self.scan_closures_in_expr(condition, next_wave_bodies);
                self.scan_closures_in_expr(body, next_wave_bodies);
            }
            TypedExprKind::Assert { condition, message } => {
                self.scan_closures_in_expr(condition, next_wave_bodies);
                if let Some(m) = message {
                    self.scan_closures_in_expr(m, next_wave_bodies);
                }
            }
            TypedExprKind::FunctionCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. }
            | TypedExprKind::ArrayLiteral { elements: args }
            | TypedExprKind::EnumCreate { args, .. }
            | TypedExprKind::EnumVariantRecordCreate { args, .. }
            | TypedExprKind::ClassSuperCall { args, .. } => {
                for a in args {
                    self.scan_closures_in_expr(a, next_wave_bodies);
                }
            }
            TypedExprKind::ClassNew {
                mangled_name, args, ..
            } => {
                // Emission order at a `ClassNew` (see the emit arm): constructor
                // args, then `emit_class_hierarchy` inlines the extends-args, the
                // parent chain, and this class's own initializer statements. Walk
                // the same shape so a closure anywhere in that expansion gets the
                // id the emitter will consume for it — at THIS site.
                for a in args {
                    self.scan_closures_in_expr(a, next_wave_bodies);
                }
                self.scan_class_hierarchy_closures(mangled_name, next_wave_bodies);
            }
            TypedExprKind::Match { subject, arms } => {
                self.scan_closures_in_expr(subject, next_wave_bodies);
                for arm in arms {
                    // Guard BEFORE body: `match_expression.rs` emits the guard test
                    // first and the body inside its `if`, so the closure counter must
                    // see them in that order too. Scanning body-first crossed the ids
                    // of an arm that had a closure in both.
                    if let Some(g) = &arm.guard {
                        self.scan_closures_in_expr(g, next_wave_bodies);
                    }
                    self.scan_closures_in_expr(&arm.body, next_wave_bodies);
                }
            }
            TypedExprKind::RecordCreate { fields, .. } => {
                for (_, e) in fields {
                    self.scan_closures_in_expr(e, next_wave_bodies);
                }
            }
            TypedExprKind::TupleLiteral { elements } => {
                for e in elements {
                    self.scan_closures_in_expr(e, next_wave_bodies);
                }
            }
            TypedExprKind::RecordWith {
                object, overrides, ..
            } => {
                self.scan_closures_in_expr(object, next_wave_bodies);
                for (_, _, e) in overrides {
                    self.scan_closures_in_expr(e, next_wave_bodies);
                }
            }
            TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. }
            | TypedExprKind::ClassVirtualCall {
                object: receiver,
                args,
                ..
            } => {
                self.scan_closures_in_expr(receiver, next_wave_bodies);
                for a in args {
                    self.scan_closures_in_expr(a, next_wave_bodies);
                }
            }
            TypedExprKind::ClosureCall { callee, args } => {
                self.scan_closures_in_expr(callee, next_wave_bodies);
                for a in args {
                    self.scan_closures_in_expr(a, next_wave_bodies);
                }
            }
            TypedExprKind::LetDestructure { value, .. } => {
                self.scan_closures_in_expr(value, next_wave_bodies);
            }
            TypedExprKind::ClassStructCreate { fields, .. } => {
                for a in fields {
                    self.scan_closures_in_expr(a, next_wave_bodies);
                }
            }
            TypedExprKind::FieldAssign { object, value, .. } => {
                self.scan_closures_in_expr(object, next_wave_bodies);
                self.scan_closures_in_expr(value, next_wave_bodies);
            }
            TypedExprKind::ImplFunctionCall { args, .. } => {
                for a in args {
                    self.scan_closures_in_expr(a, next_wave_bodies);
                }
            }
            TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. } => {
                self.scan_closures_in_expr(inner, next_wave_bodies);
            }
            _ => {}
        }
    }

    /// Scan the closures a `ClassNew` will inline: the extends-args, then the
    /// parent chain, then this class's own initializer statements. This is the
    /// exact traversal `emit_class_hierarchy` performs, and it runs once per
    /// construction site because emission does too — a class constructed twice
    /// has its initializer closures lifted twice, under two ids.
    fn scan_class_hierarchy_closures(
        &mut self,
        mangled_name: &MangledName,
        next_wave_bodies: &mut Vec<TypedExpr>,
    ) {
        // Copy the module reference out so the `&mut self` recursion below does
        // not conflict with the borrow of `cls` (`typed_module` is a `&'a` field).
        let typed_module = self.typed_module;
        let Some(TypeDef::Class(cls)) = typed_module.types.get(mangled_name) else {
            return;
        };
        if let (Some(extends_args), Some(parent)) = (&cls.extends_args, &cls.parent_mangled_name) {
            for arg in extends_args {
                self.scan_closures_in_expr(arg, next_wave_bodies);
            }
            self.scan_class_hierarchy_closures(parent, next_wave_bodies);
        }
        for stmt in &cls.initializer {
            self.scan_closures_in_expr(stmt, next_wave_bodies);
        }
    }

    /// Pre-scan typed module AST to find all FunctionRef/MethodRef nodes and register trampolines.
    fn prescan_ref_trampolines(&mut self) {
        // Collect all expressions to scan (functions, globals, class initializers)
        let mut exprs: Vec<&TypedExpr> = Vec::new();
        for func in self.typed_module.functions.values() {
            exprs.push(&func.body);
        }
        for global in self.typed_module.globals.values() {
            exprs.push(&global.initializer);
        }
        for type_def in self.typed_module.types.values() {
            if let TypeDef::Class(cls) = type_def {
                for stmt in &cls.initializer {
                    exprs.push(stmt);
                }
                if let Some(extends_args) = &cls.extends_args {
                    for arg in extends_args {
                        exprs.push(arg);
                    }
                }
            }
        }
        for expr in exprs {
            self.scan_expr_for_ref_trampolines(expr);
        }
        // Also scan closure bodies (clone to avoid borrow conflict)
        let closure_bodies: Vec<TypedExpr> =
            self.closure_infos.iter().map(|i| i.body.clone()).collect();
        for body in &closure_bodies {
            self.scan_expr_for_ref_trampolines(body);
        }
    }

    /// Recursively scan an expression for FunctionRef/MethodRef and register trampolines.
    fn scan_expr_for_ref_trampolines(&mut self, expr: &TypedExpr) {
        match &expr.kind {
            TypedExprKind::FunctionRef {
                name,
                type_params: _,
            } => {
                let (param_types, return_type) = match &expr.ty {
                    Type::Function(pts, ret) => (pts.clone(), ret.as_ref().clone()),
                    _ => unreachable!("FunctionRef must have Function type"),
                };
                let key = format!("$ref_func${}", name.0);
                if let std::collections::btree_map::Entry::Vacant(e) =
                    self.ref_trampoline_indices.entry(key)
                {
                    e.insert(0); // placeholder, set in emit_function_section
                    self.ref_trampolines.push(RefTrampoline {
                        target_mangled: name.clone(),
                        self_type: None,
                        param_types,
                        return_type,
                    });
                }
            }
            TypedExprKind::MethodRef {
                object,
                method_name,
                type_params: _,
            } => {
                self.scan_expr_for_ref_trampolines(object);
                let (param_types, return_type) = match &expr.ty {
                    Type::Function(pts, ret) => (pts.clone(), ret.as_ref().clone()),
                    _ => unreachable!("MethodRef must have Function type"),
                };
                let self_ty = self.typed_module.functions[method_name].params[0]
                    .ty
                    .clone();
                let key = format!("$ref_method${}", method_name.0);
                if let std::collections::btree_map::Entry::Vacant(e) =
                    self.ref_trampoline_indices.entry(key)
                {
                    e.insert(0); // placeholder, set in emit_function_section
                    self.ref_trampolines.push(RefTrampoline {
                        target_mangled: method_name.clone(),
                        self_type: Some(self_ty),
                        param_types,
                        return_type,
                    });
                }
            }
            // Recurse into children
            TypedExprKind::Block(exprs) => {
                for e in exprs {
                    self.scan_expr_for_ref_trampolines(e);
                }
            }
            TypedExprKind::Let { value, .. }
            | TypedExprKind::Assign { value, .. }
            | TypedExprKind::Panic { message: value }
            | TypedExprKind::BoxToAny { inner: value }
            | TypedExprKind::NewtypeCreate { value }
            | TypedExprKind::NewtypeValue { value }
            | TypedExprKind::GlobalAssign { value, .. }
            | TypedExprKind::Return { value, .. }
            | TypedExprKind::UnaryOp { operand: value, .. }
            | TypedExprKind::FieldAccess { object: value, .. }
            | TypedExprKind::TypeTest { value, .. }
            | TypedExprKind::TypeCast { value, .. }
            | TypedExprKind::InterfaceObjectCoerce { inner: value, .. }
            | TypedExprKind::InterfaceObjectUpcast { inner: value } => {
                self.scan_expr_for_ref_trampolines(value);
            }
            TypedExprKind::BinaryOp { left, right, .. } => {
                self.scan_expr_for_ref_trampolines(left);
                self.scan_expr_for_ref_trampolines(right);
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.scan_expr_for_ref_trampolines(condition);
                self.scan_expr_for_ref_trampolines(then_branch);
                if let Some(e) = else_branch {
                    self.scan_expr_for_ref_trampolines(e);
                }
            }
            TypedExprKind::While { condition, body } => {
                self.scan_expr_for_ref_trampolines(condition);
                self.scan_expr_for_ref_trampolines(body);
            }
            TypedExprKind::Assert { condition, message } => {
                self.scan_expr_for_ref_trampolines(condition);
                if let Some(m) = message {
                    self.scan_expr_for_ref_trampolines(m);
                }
            }
            TypedExprKind::FunctionCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. }
            | TypedExprKind::ArrayLiteral { elements: args }
            | TypedExprKind::EnumCreate { args, .. }
            | TypedExprKind::EnumVariantRecordCreate { args, .. }
            | TypedExprKind::ClassNew { args, .. }
            | TypedExprKind::ClassSuperCall { args, .. } => {
                for a in args {
                    self.scan_expr_for_ref_trampolines(a);
                }
            }
            TypedExprKind::Match { subject, arms } => {
                self.scan_expr_for_ref_trampolines(subject);
                for arm in arms {
                    // Guard before body, mirroring the emitter (see the closure scan).
                    // Trampolines are keyed by mangled name so order is not load-bearing
                    // here, but keeping every prescan in emission order is the invariant.
                    if let Some(g) = &arm.guard {
                        self.scan_expr_for_ref_trampolines(g);
                    }
                    self.scan_expr_for_ref_trampolines(&arm.body);
                }
            }
            TypedExprKind::RecordCreate { fields, .. } => {
                for (_, e) in fields {
                    self.scan_expr_for_ref_trampolines(e);
                }
            }
            TypedExprKind::TupleLiteral { elements } => {
                for e in elements {
                    self.scan_expr_for_ref_trampolines(e);
                }
            }
            TypedExprKind::RecordWith {
                object, overrides, ..
            } => {
                self.scan_expr_for_ref_trampolines(object);
                for (_, _, e) in overrides {
                    self.scan_expr_for_ref_trampolines(e);
                }
            }
            TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. }
            | TypedExprKind::ClassVirtualCall {
                object: receiver,
                args,
                ..
            } => {
                self.scan_expr_for_ref_trampolines(receiver);
                for a in args {
                    self.scan_expr_for_ref_trampolines(a);
                }
            }
            TypedExprKind::ClosureCall { callee, args } => {
                self.scan_expr_for_ref_trampolines(callee);
                for a in args {
                    self.scan_expr_for_ref_trampolines(a);
                }
            }
            TypedExprKind::Closure { body, .. } => {
                self.scan_expr_for_ref_trampolines(body);
            }
            TypedExprKind::LetDestructure { value, .. } => {
                self.scan_expr_for_ref_trampolines(value);
            }
            TypedExprKind::ClassStructCreate { fields, .. } => {
                for a in fields {
                    self.scan_expr_for_ref_trampolines(a);
                }
            }
            TypedExprKind::FieldAssign { object, value, .. } => {
                self.scan_expr_for_ref_trampolines(object);
                self.scan_expr_for_ref_trampolines(value);
            }
            TypedExprKind::ImplFunctionCall { args, .. } => {
                for a in args {
                    self.scan_expr_for_ref_trampolines(a);
                }
            }
            TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. } => {
                self.scan_expr_for_ref_trampolines(inner);
            }
            _ => {}
        }
    }

    /// The heap-struct type index of the mut-box that holds a captured **mutable** variable of this
    /// type. A tuple's own mutable `$Tuple_N` *is* the box (no wrapper) — its fields are written in
    /// place on reassignment; a primitive uses a `$MutBox$X`; every other reference type uses the
    /// shared anyref `$MutBox`.
    pub(super) fn mut_box_type_index_for(&self, ty: &Type) -> u32 {
        match ty {
            Type::Unit
            | Type::Bool
            | Type::Char
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Uint8
            | Type::Uint16
            | Type::Uint32 => MUT_BOX_I32_TYPE_INDEX,
            Type::Int64 | Type::Uint64 => MUT_BOX_I64_TYPE_INDEX,
            Type::Float32 => MUT_BOX_F32_TYPE_INDEX,
            Type::Float64 => MUT_BOX_F64_TYPE_INDEX,
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => self.mut_box_type_index_for(inner),
            _ if self.is_tuple(ty) => self.tuple_struct_index(ty),
            // A mutable `Uint128`'s own `$Uint128` (mutable i64 fields) doubles as its mut-box.
            _ if self.is_uint128(ty) => UINT128_STRUCT_TYPE_INDEX,
            _ => MUT_BOX_REF_TYPE_INDEX, // All other reference types: String, Record, Enum, Class, Array, Function, InterfaceObject
        }
    }

    /// The WASM ref type of the mut-box (see `mut_box_type_index_for`).
    pub(super) fn mut_box_valtype(&self, ty: &Type) -> ValType {
        ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(self.mut_box_type_index_for(ty)),
        })
    }

    fn scan_expr_for_interface_objects(&mut self, expr: &crate::typechecker::types::TypedExpr) {
        match &expr.kind {
            TypedExprKind::InterfaceObjectCoerce {
                inner,
                interface_mangled_name,
                concrete_type,
                vtable_methods,
            } => {
                self.scan_expr_for_interface_objects(inner);
                // Only record if we haven't seen this (type instantiation, trait) pair before.
                // Per-instantiation key: a generic impl class (e.g. `ArrayIterator<Int32>` vs
                // `ArrayIterator<String>`) needs its own vtable instance, but its erased
                // mangled name is shared — so key on `instance_key` (includes type args).
                let type_key = instance_key(concrete_type);
                let global_key = Self::coercion_global_key(interface_mangled_name, vtable_methods);
                if !self
                    .vtable_global_indices
                    .contains_key(&(type_key.clone(), global_key.clone()))
                {
                    // Use a placeholder global index for now — will be assigned later
                    self.vtable_global_indices
                        .insert((type_key, global_key), u32::MAX);
                    self.interface_object_infos.push(InterfaceObjectInfo {
                        interface_mangled_name: interface_mangled_name.clone(),
                        concrete_type: concrete_type.clone(),
                        vtable_methods: vtable_methods.clone(),
                    });
                }
            }
            // Recurse into all children
            TypedExprKind::Block(exprs) => {
                for e in exprs {
                    self.scan_expr_for_interface_objects(e);
                }
            }
            TypedExprKind::Let { value, .. }
            | TypedExprKind::Assign { value, .. }
            | TypedExprKind::Panic { message: value }
            | TypedExprKind::BoxToAny { inner: value }
            | TypedExprKind::NewtypeCreate { value }
            | TypedExprKind::NewtypeValue { value }
            | TypedExprKind::InterfaceObjectUpcast { inner: value }
            | TypedExprKind::GlobalAssign { value, .. } => {
                self.scan_expr_for_interface_objects(value);
            }
            TypedExprKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.scan_expr_for_interface_objects(condition);
                self.scan_expr_for_interface_objects(then_branch);
                if let Some(e) = else_branch {
                    self.scan_expr_for_interface_objects(e);
                }
            }
            TypedExprKind::While { condition, body } => {
                self.scan_expr_for_interface_objects(condition);
                self.scan_expr_for_interface_objects(body);
            }
            TypedExprKind::FunctionCall { args, .. }
            | TypedExprKind::IntrinsicCall { args, .. }
            | TypedExprKind::ArrayLiteral { elements: args } => {
                for a in args {
                    self.scan_expr_for_interface_objects(a);
                }
            }
            TypedExprKind::BinaryOp { left, right, .. } => {
                self.scan_expr_for_interface_objects(left);
                self.scan_expr_for_interface_objects(right);
            }
            TypedExprKind::UnaryOp { operand, .. } => {
                self.scan_expr_for_interface_objects(operand);
            }
            TypedExprKind::Assert { condition, message } => {
                self.scan_expr_for_interface_objects(condition);
                if let Some(m) = message {
                    self.scan_expr_for_interface_objects(m);
                }
            }
            TypedExprKind::FieldAccess { object, .. }
            | TypedExprKind::MethodRef { object, .. }
            | TypedExprKind::TypeTest { value: object, .. }
            | TypedExprKind::TypeCast { value: object, .. } => {
                self.scan_expr_for_interface_objects(object);
            }
            TypedExprKind::Match { subject, arms } => {
                self.scan_expr_for_interface_objects(subject);
                for arm in arms {
                    // Guard before body, mirroring the emitter. Vtables are keyed by
                    // (instance, trait) so order is not load-bearing here either.
                    if let Some(g) = &arm.guard {
                        self.scan_expr_for_interface_objects(g);
                    }
                    self.scan_expr_for_interface_objects(&arm.body);
                }
            }
            TypedExprKind::RecordCreate { fields, .. } => {
                for (_, e) in fields {
                    self.scan_expr_for_interface_objects(e);
                }
            }
            TypedExprKind::TupleLiteral { elements } => {
                for e in elements {
                    self.scan_expr_for_interface_objects(e);
                }
            }
            TypedExprKind::RecordWith {
                object, overrides, ..
            } => {
                self.scan_expr_for_interface_objects(object);
                for (_, _, e) in overrides {
                    self.scan_expr_for_interface_objects(e);
                }
            }
            TypedExprKind::EnumCreate { args, .. }
            | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
                for a in args {
                    self.scan_expr_for_interface_objects(a);
                }
            }
            TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
                self.scan_expr_for_interface_objects(receiver);
                for a in args {
                    self.scan_expr_for_interface_objects(a);
                }
            }
            TypedExprKind::ClassVirtualCall { object, args, .. } => {
                self.scan_expr_for_interface_objects(object);
                for a in args {
                    self.scan_expr_for_interface_objects(a);
                }
            }
            TypedExprKind::ClassSuperCall { args, .. } => {
                for a in args {
                    self.scan_expr_for_interface_objects(a);
                }
            }
            TypedExprKind::LetDestructure { value, .. } => {
                self.scan_expr_for_interface_objects(value);
            }
            TypedExprKind::Return { value, .. } => {
                self.scan_expr_for_interface_objects(value);
            }
            TypedExprKind::ClassNew { args, .. } => {
                for a in args {
                    self.scan_expr_for_interface_objects(a);
                }
            }
            TypedExprKind::Closure { body, .. } => {
                self.scan_expr_for_interface_objects(body);
            }
            TypedExprKind::ClosureCall { callee, args } => {
                self.scan_expr_for_interface_objects(callee);
                for a in args {
                    self.scan_expr_for_interface_objects(a);
                }
            }
            TypedExprKind::ClassStructCreate { fields, .. } => {
                for a in fields {
                    self.scan_expr_for_interface_objects(a);
                }
            }
            // Leaf nodes
            _ => {}
        }
    }

    /// The single WASM value type for a Dovetail value — it **boxes only tuples**, contrasting with
    /// the unboxed multi-value `type_to_valtypes` sequence. A primitive is its raw valtype (a primitive
    /// already *is* one value — no boxing), a reference type is its own ref, a tuple is its boxed
    /// `(ref $Tuple_N)` (the one form that *must* box, since a tuple has no single unboxed slot),
    /// and a type parameter / `Any` lowers to `anyref`. Used wherever a value occupies exactly one
    /// WASM slot (globals, single-value locals/temps, block results, match scrutinees). The
    /// genuinely-everything-boxed `(ref $Box$X)` form for primitives lives in
    /// `wasm_type_index_for_any_cast` (the erasure-cast boundary).
    fn single_val_type(&self, ty: &Type) -> ValType {
        match ty {
            Type::Record(_, mn)
            | Type::Enum(_, mn)
            | Type::GenericRecord {
                mangled_name: mn, ..
            }
            | Type::GenericEnum {
                mangled_name: mn, ..
            } => {
                let struct_idx = *self
                    .type_indices
                    .get(mn)
                    .unwrap_or_else(|| panic!("missing type index for: {} (type: {})", mn, ty));
                ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(struct_idx),
                })
            }
            Type::Tuple(..) => ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(self.tuple_struct_index(ty)),
            }),
            // A `Uint128` is a width-2 `[i64, i64]` flattened run; its single boxed slot is the
            // dedicated `(ref $Uint128)` struct (raw i64 fields), not the generic `$Tuple_2`.
            Type::Uint128 => ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(UINT128_STRUCT_TYPE_INDEX),
            }),
            Type::Unit | Type::Bool | Type::Char => ValType::I32,
            Type::Int8 | Type::Int16 | Type::Int32 => ValType::I32,
            Type::Uint8 | Type::Uint16 | Type::Uint32 => ValType::I32,
            Type::Int64 | Type::Uint64 => ValType::I64,
            Type::Float32 => ValType::F32,
            Type::Float64 => ValType::F64,
            Type::String => ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(STRING_STRUCT_TYPE_INDEX),
            }),
            // Shared generic fields retain symbolic extensions; their varying
            // tuple shapes occupy one boxed slot, just like a type parameter.
            Type::Any | Type::TupleProjection(..) | Type::TupleExtend(..) => {
                ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::ANY,
                })
            }
            Type::Never | Type::Error => ValType::I32,
            // A fully-generic array (element carries a type parameter) is erased, but to the
            // built-in abstract `array` heap type rather than bare `any`: every concrete array
            // type ($Array$iN / $Array$u128 / $Array$ref) is a subtype of it, so it's a valid
            // common slot, `array.len` works without a cast, and it catches non-array stores at
            // validation time. Non-null, matching the erased-slot convention (values never null).
            Type::Array(elem) if elem.contains_type_parameter() => {
                ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Abstract {
                        shared: false,
                        ty: wasm_encoder::AbstractHeapType::Array,
                    },
                })
            }
            Type::Array(elem) => {
                let array_type_index = self.array_type_index(elem);
                ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(array_type_index),
                })
            }
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => self.single_val_type(inner),
            // Interface objects are de-monomorphized: one WASM type per trait, layout independent of the
            // type args (the vtable slot signatures erase the trait's generic params). So a trait
            // object is *never* erased — even when its type args contain a type parameter, it lowers
            // to the concrete per-trait struct (keyed by the per-trait `mangled_name`).
            Type::InterfaceObject {
                mangled_name,
                traits,
                ..
            } => {
                let idx = *self.interface_object_type_indices.get(mangled_name).unwrap_or_else(|| {
                    panic!("missing interface_object_type_index for: {mangled_name} (traits: {traits:?})")
                });
                ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(idx),
                })
            }
            Type::Function(param_types, _ret) => {
                // Always-erased closures: every Type::Function lowers to (ref Closure_N) where
                // N = arity. Closure body's prologue casts each anyref param to the declared
                // type; epilogue boxes the return back to anyref. No per-signature struct exists.
                let arity = param_types.len() as u32;
                let (_, struct_idx) = *self.closure_arity_indices.get(&arity).unwrap_or_else(|| {
                    panic!("missing Closure_N type for arity {arity} — discover_closure_arities did not find this function type")
                });
                ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(struct_idx),
                })
            }
            Type::Class(_, mn)
            | Type::GenericClass {
                mangled_name: mn, ..
            } => {
                let struct_idx = *self
                    .type_indices
                    .get(mn)
                    .unwrap_or_else(|| panic!("no type_index for class mangled_name: {}", mn));
                ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(struct_idx),
                })
            }
            Type::SelfType => unreachable!("SelfType should be resolved before codegen"),
            // Erased slots: type parameters and any composite carrying one lower to anyref.
            Type::TypeVariable(_, _) | Type::GenericParam(_, _, _) => {
                ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::ANY,
                })
            }
            Type::TypeConstructor { .. } | Type::AssociatedProjection(_) => {
                unreachable!("TypeConstructor should be expanded before codegen")
            }
        }
    }

    /// Flattened ("unboxed") lowering: a Dovetail type → an ordered sequence of WASM
    /// value types. Tuples flatten transitively; everything else is width-1 and
    /// identical to `single_val_type`. The single-value `single_val_type` is retained
    /// for the boxed single-slot boundaries (array elements, anyref-erased positions).
    ///
    /// See docs/tuple-multivalue-codegen-design.md.
    fn type_to_valtypes(&self, ty: &Type) -> SmallVec<[ValType; 2]> {
        match ty {
            // Every tuple flattens transitively into its leaf values. A type-parameter leaf lowers to
            // `anyref` (via `single_val_type`), so `(Int, T)` → `[i32, anyref]` and `(A, B)` →
            // `[anyref, anyref]` — the boxed form is the shared `$Tuple_N`.
            Type::Tuple(elems, _) => elems
                .iter()
                .flat_map(|e| self.type_to_valtypes(e))
                .collect(),
            // Newtypes are transparent (mirror type_to_valtype) so a newtype over a
            // tuple inherits the flattened representation.
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => self.type_to_valtypes(inner),
            // `Uint128` is the smallest non-tuple flattened value: a width-2 `[i64, i64]` (lo, hi)
            // run. Its boxed single slot (`single_val_type`) is the dedicated `(ref $Uint128)`.
            Type::Uint128 => smallvec![ValType::I64, ValType::I64],
            _ => smallvec![self.single_val_type(ty)],
        }
    }

    /// Resolve a type to the tuple `(elements, mangled_name)` it lowers to under the flattened
    /// representation, transparently unwrapping newtypes. Returns `None` for anything that is not a
    /// tuple. Every tuple flattens now (type-parameter leaves lower to `anyref`), so this resolves
    /// `(Int, T)` and `(A, B)` too.
    fn resolve_tuple<'t>(&self, ty: &'t Type) -> Option<(&'t [Type], &'t MangledName)> {
        match ty {
            Type::Tuple(elems, mn) => Some((elems, mn)),
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => self.resolve_tuple(inner),
            _ => None,
        }
    }

    /// Whether `ty` is a tuple (transparently through newtypes) — i.e. lowers to a flattened
    /// multi-value sequence. Every tuple flattens now, so this is just "is it a tuple".
    fn is_tuple(&self, ty: &Type) -> bool {
        self.resolve_tuple(ty).is_some()
    }

    /// Whether `ty` is a `Uint128` (transparently through newtypes). Like a tuple it lowers to a
    /// flattened `[i64, i64]` run, but it boxes into the dedicated `$Uint128` struct (raw i64
    /// fields), so the box/unbox seams special-case it ahead of the generic tuple path.
    fn is_uint128(&self, ty: &Type) -> bool {
        match ty {
            Type::Uint128 => true,
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => self.is_uint128(inner),
            _ => false,
        }
    }

    /// Flattened width (number of WASM values) of a type under the unboxed representation.
    fn flat_width(&self, ty: &Type) -> u32 {
        self.type_to_valtypes(ty).len() as u32
    }

    /// For a flattenable tuple's elements, the `(start, width)` sub-range of element `i` within
    /// the tuple's flattened values: `start` = Σ widths of elements `0..i`, `width` = element i's width.
    fn tuple_elem_offset(&self, elems: &[Type], i: usize) -> (u32, u32) {
        let start: u32 = elems[..i].iter().map(|e| self.flat_width(e)).sum();
        (start, self.flat_width(&elems[i]))
    }

    /// The shared boxed `$Tuple_N` struct index for a flattenable tuple type, keyed by its flattened
    /// leaf count (`flat_width`). One struct serves every concrete tuple of that width.
    fn tuple_struct_index(&self, ty: &Type) -> u32 {
        let width = self.flat_width(ty);
        *self.tuple_width_indices.get(&width).unwrap_or_else(|| {
            panic!("missing $Tuple_{width} struct for {ty} — discover_tuple_widths did not find this width")
        })
    }

    /// The ordered concrete leaf types of a flattenable tuple — the per-leaf static types backing
    /// its flattened values (and the `anyref` fields of its `$Tuple_N`). A concrete sub-tuple flattens
    /// transitively; a newtype is transparent; a `Uint128` contributes its two `i64` halves (so a
    /// boxed tuple's `Uint128` element occupies two `Box$i64` fields, keeping every leaf width-1 and
    /// matching `type_to_valtypes`); every other element is a single leaf. Each leaf is width-1, so
    /// `flatten_to_leaf_types(ty).len() == flat_width(ty)` and leaf `k` corresponds to value `k` /
    /// `$Tuple_N` field `k`.
    fn flatten_to_leaf_types(&self, ty: &Type) -> Vec<Type> {
        match ty {
            Type::Tuple(elems, _) => elems
                .iter()
                .flat_map(|e| self.flatten_to_leaf_types(e))
                .collect(),
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => self.flatten_to_leaf_types(inner),
            // A `Uint128` leaf flattens into its two `i64` halves (lo, hi) so the generic boxed
            // `$Tuple_N` stays a flat run of width-1 anyref fields.
            Type::Uint128 => vec![Type::Uint64, Type::Uint64],
            _ => vec![ty.clone()],
        }
    }

    /// A class field is spliced (its tuple flattened into N struct fields) whenever it is a tuple —
    /// mutable tuple fields splice into N **mutable** sub-fields (reassigned leaf-by-leaf in place),
    /// just like immutable ones, exactly as record fields always splice.
    fn class_field_is_spliced(&self, f: &crate::typechecker::types::ClassFieldDef) -> bool {
        self.is_tuple(&f.ty)
    }

    /// WASM-field width of a Dovetail class field: N for a spliced tuple field, else 1.
    fn class_field_wasm_width(&self, f: &crate::typechecker::types::ClassFieldDef) -> u32 {
        if self.class_field_is_spliced(f) {
            self.flat_width(&f.ty)
        } else {
            1
        }
    }

    /// The `(wasm_start, width)` range of WASM struct fields backing Dovetail field `field_idx` of a
    /// record or class (both always splice tuple fields; class data fields follow the common
    /// header). Used wherever a record/class struct field index is computed for
    /// `struct.get`/`struct.set`/copy.
    fn struct_field_range(&self, mn: &MangledName, field_idx: usize) -> (u32, u32) {
        match &self.typed_module.types[mn] {
            TypeDef::Record(r) => {
                let start: u32 = r.fields[..field_idx]
                    .iter()
                    .map(|(_, t)| self.flat_width(t))
                    .sum();
                (
                    self.id_prefix(mn) + start,
                    self.flat_width(&r.fields[field_idx].1),
                )
            }
            TypeDef::Class(c) => {
                let start: u32 = self.class_header_size(mn)
                    + c.fields[..field_idx]
                        .iter()
                        .map(|f| self.class_field_wasm_width(f))
                        .sum::<u32>();
                (start, self.class_field_wasm_width(&c.fields[field_idx]))
            }
            _ => unreachable!("struct_field_range on non-record/class typedef: {mn}"),
        }
    }

    /// The `(wasm_start, width)` range of WASM fields backing payload `field_idx` of an enum variant
    /// (variant structs splice tuple payloads, like records, with no prefix field). Used to read a
    /// variant payload by `struct.get`.
    fn enum_payload_range(
        &self,
        enum_mn: &MangledName,
        variant_name: &str,
        field_idx: usize,
    ) -> (u32, u32) {
        match &self.typed_module.types[enum_mn] {
            TypeDef::Enum(e) => {
                let variant = e
                    .variants
                    .iter()
                    .find(|v| v.name == variant_name)
                    .unwrap_or_else(|| {
                        panic!("enum_payload_range: no variant {variant_name} in {enum_mn}")
                    });
                let start: u32 = variant.payload_types[..field_idx]
                    .iter()
                    .map(|t| self.flat_width(t))
                    .sum();
                (
                    self.id_prefix(enum_mn) + start,
                    self.flat_width(&variant.payload_types[field_idx]),
                )
            }
            _ => unreachable!("enum_payload_range on non-enum typedef: {enum_mn}"),
        }
    }

    /// Instructions to **unbox** a boxed tuple: a single `(ref $Tuple_N)` on top of the stack is
    /// exploded into its flattened values of N WASM values (matching `type_to_valtypes`). The shared
    /// `$Tuple_N` has `anyref` fields, so each leaf is read with `struct.get` and then cast back
    /// from `anyref` to its concrete leaf type (`ref.cast` + a trailing `struct.get 0` for boxed
    /// primitives). Uses one temp local at `base` (to re-load the ref); returns the values and the
    /// temp valtype.
    fn tuple_unbox_instrs(
        &self,
        ty: &Type,
        base: u32,
    ) -> (Vec<wasm_encoder::Instruction<'static>>, Vec<ValType>) {
        let (mut instrs, mut temps) = self.tuple_unbox_fields_instrs(ty, base);
        self.validate_unboxed_slices(ty, base, &mut instrs, &mut temps);
        (instrs, temps)
    }

    fn tuple_unbox_fields_instrs(
        &self,
        ty: &Type,
        base: u32,
    ) -> (Vec<wasm_encoder::Instruction<'static>>, Vec<ValType>) {
        let idx = self.tuple_struct_index(ty);
        let ref_vt = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(idx),
        });
        let leaves = self.flatten_to_leaf_types(ty);
        let mut instrs = vec![wasm_encoder::Instruction::LocalSet(base)];
        for (k, leaf) in leaves.iter().enumerate() {
            instrs.push(wasm_encoder::Instruction::LocalGet(base));
            instrs.push(wasm_encoder::Instruction::StructGet {
                struct_type_index: idx,
                field_index: k as u32,
            });
            // Cast the `anyref` field back to the leaf's concrete representation. An `Any`
            // (or type-parameter) leaf already *is* anyref — leave the field value as-is.
            match leaf {
                Type::Any | Type::TypeVariable(..) | Type::GenericParam(..) => {}
                Type::Never | Type::Error => instrs.push(wasm_encoder::Instruction::Unreachable),
                _ => {
                    let leaf_idx = self.wasm_type_index_for_any_cast(leaf);
                    instrs.push(wasm_encoder::Instruction::RefCastNonNull(
                        wasm_encoder::HeapType::Concrete(leaf_idx),
                    ));
                    if !leaf.is_reference_type() {
                        instrs.push(wasm_encoder::Instruction::StructGet {
                            struct_type_index: leaf_idx,
                            field_index: 0,
                        });
                    }
                }
            }
        }
        (instrs, vec![ref_vt])
    }

    /// Instructions to **unbox** a boxed `Uint128`: a single `(ref $Uint128)` on top of the stack is
    /// exploded into its flattened `[i64, i64]` (lo, hi). The struct's fields are raw `i64`, so this
    /// is two `struct.get`s (no per-leaf cast). Uses one temp local at `base` (to re-load the ref);
    /// returns the instructions and the temp valtype. Mirrors `tuple_unbox_instrs` for the
    /// `wasm_encoder::Function` const-init contexts (trampolines).
    fn uint128_unbox_instrs(
        &self,
        base: u32,
    ) -> (Vec<wasm_encoder::Instruction<'static>>, Vec<ValType>) {
        let ref_vt = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(UINT128_STRUCT_TYPE_INDEX),
        });
        let instrs = vec![
            wasm_encoder::Instruction::LocalSet(base),
            wasm_encoder::Instruction::LocalGet(base),
            wasm_encoder::Instruction::StructGet {
                struct_type_index: UINT128_STRUCT_TYPE_INDEX,
                field_index: 0,
            },
            wasm_encoder::Instruction::LocalGet(base),
            wasm_encoder::Instruction::StructGet {
                struct_type_index: UINT128_STRUCT_TYPE_INDEX,
                field_index: 1,
            },
        ];
        (instrs, vec![ref_vt])
    }

    /// Instructions to **rebox** a flattened tuple: the N values of its values on top of the stack are
    /// reassembled into a single `(ref $Tuple_N)`. Since `$Tuple_N` has `anyref` fields, each leaf
    /// is first boxed to `anyref` (a `struct.new $Box$X` for a primitive; a free upcast for a ref)
    /// before `struct.new $Tuple_N`. The values are spilled into N temp locals at `base..base+N` (their
    /// valtypes are returned) so each leaf can be reloaded and boxed in order.
    fn tuple_rebox_instrs(
        &self,
        ty: &Type,
        base: u32,
    ) -> (Vec<wasm_encoder::Instruction<'static>>, Vec<ValType>) {
        let idx = self.tuple_struct_index(ty);
        let leaves = self.flatten_to_leaf_types(ty);
        let run_vts: Vec<ValType> = self.type_to_valtypes(ty).into_vec();
        let n = leaves.len() as u32;
        let mut instrs = Vec::new();
        // Spill the values into locals base..base+N (LocalSet pops top-of-stack = last leaf first).
        for k in (0..n).rev() {
            instrs.push(wasm_encoder::Instruction::LocalSet(base + k));
        }
        // Reload each leaf and box it to anyref. A reference type (and `Any` / a type parameter,
        // which are already anyref) needs no `struct.new $Box` — only a raw primitive does.
        for (k, leaf) in leaves.iter().enumerate() {
            instrs.push(wasm_encoder::Instruction::LocalGet(base + k as u32));
            let already_boxed = leaf.is_reference_type()
                || matches!(
                    leaf,
                    Type::Any | Type::TypeVariable(..) | Type::GenericParam(..)
                );
            if !already_boxed {
                instrs.push(wasm_encoder::Instruction::StructNew(
                    self.box_type_index_for(leaf),
                ));
            }
        }
        instrs.push(wasm_encoder::Instruction::StructNew(idx));
        (instrs, run_vts)
    }

    /// Map an array element type to its WASM-GC array type index — one of the fixed array set
    /// (`emit_array_types`). Primitives route to their packed/native array; every reference element
    /// (String, record, enum, class, tuple, closure, interface object, nested array, `Any`) collapses
    /// to the shared `$Array$ref`; `Uint128` to its dedicated boxed array. Newtypes unwrap to inner.
    fn array_type_index(&self, elem: &Type) -> u32 {
        let elem = Self::array_element_type(elem);
        ARRAY_ELEMENT_TYPES
            .iter()
            .find_map(|(element, index)| (element == elem).then_some(*index))
            .unwrap_or(ARRAY_REF_TYPE_INDEX)
    }

    /// Newtypes are transparent to storage, packed reads, and erased dispatch.
    fn array_element_type(mut element: &Type) -> &Type {
        while let Type::Newtype(_, inner)
        | Type::GenericNewtype {
            concrete_inner_type: inner,
            ..
        } = element
        {
            element = inner;
        }
        element
    }

    /// Returns true if a TypeDef field of this type lowers to `anyref` under full type
    /// erasure — i.e., values written into the slot must box primitives, and reads from
    /// the slot must `ref.cast` (and unbox primitives) back to the expression's static type.
    /// Mirrors the erasure logic in `single_val_type`.
    pub(super) fn is_erased_slot(ty: &Type) -> bool {
        match ty {
            // A bare type parameter is erased to `anyref`, and so is an explicit `Any` — the two
            // lower to exactly the same slot, so anything stored into one has to be boxed on the
            // way in and cast on the way out either way. `Any` used to be missing here, which left
            // every caller that gates on this predicate either special-casing it by hand (five did)
            // or silently storing a raw i32 into an anyref slot — a module that fails WASM
            // validation, not a wrong answer.
            //
            // So is a fully-generic `Array<T>` (which has no single concrete WASM array type when
            // the element is a type parameter — it lowers to the abstract `(ref array)`). A
            // `Function` is *not* erased — every closure is the uniform `(ref Closure_N)`; nor is a
            // `InterfaceObject` — it is de-monomorphized to one concrete WASM type per trait. Mirrors
            // `single_val_type`.
            Type::Any
            | Type::TypeVariable(_, _)
            | Type::GenericParam(_, _, _)
            | Type::TupleExtend(..)
            | Type::TupleProjection(..) => true,
            Type::Array(elem) => elem.contains_type_parameter(),
            // Interface objects are de-monomorphized to one concrete WASM type per trait — never erased.
            Type::Newtype(_, inner) => Self::is_erased_slot(inner),
            Type::GenericNewtype {
                concrete_inner_type,
                ..
            } => Self::is_erased_slot(concrete_inner_type),
            _ => false,
        }
    }

    /// Return the boxing struct type index for a primitive type.
    #[allow(clippy::only_used_in_recursion)]
    pub(super) fn box_type_index_for(&self, ty: &Type) -> u32 {
        match ty {
            Type::Unit => BOX_UNIT_TYPE_INDEX,
            Type::Bool => BOX_BOOL_TYPE_INDEX,
            Type::Char => BOX_CHAR_TYPE_INDEX,
            Type::Int8 => BOX_INT8_TYPE_INDEX,
            Type::Int16 => BOX_INT16_TYPE_INDEX,
            Type::Int32 => BOX_INT32_TYPE_INDEX,
            Type::Uint8 => BOX_UINT8_TYPE_INDEX,
            Type::Uint16 => BOX_UINT16_TYPE_INDEX,
            Type::Uint32 => BOX_UINT32_TYPE_INDEX,
            Type::Int64 => BOX_INT64_TYPE_INDEX,
            Type::Uint64 => BOX_UINT64_TYPE_INDEX,
            // A `Uint128` boxes into its dedicated `(struct (mut i64) (mut i64))`.
            Type::Uint128 => UINT128_STRUCT_TYPE_INDEX,
            Type::Float32 => BOX_FLOAT32_TYPE_INDEX,
            Type::Float64 => BOX_FLOAT64_TYPE_INDEX,
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => self.box_type_index_for(inner),
            // Never has no inhabitants; any boxing code is dead. Use I32 box since Never is stored as I32.
            Type::Never => BOX_INT32_TYPE_INDEX,
            _ => unreachable!("box_type_index_for called on non-primitive type: {}", ty),
        }
    }

    /// Return the WASM type index for ref.test / ref.cast on a given Dovetail type.
    /// Primitives use their boxing struct type index.
    /// Reference types use their concrete type index.
    pub(super) fn wasm_type_index_for_any_cast(&self, ty: &Type) -> u32 {
        match ty {
            Type::Unit
            | Type::Bool
            | Type::Char
            | Type::Int8
            | Type::Int16
            | Type::Int32
            | Type::Uint8
            | Type::Uint16
            | Type::Uint32
            | Type::Int64
            | Type::Uint64
            | Type::Uint128
            | Type::Float32
            | Type::Float64 => self.box_type_index_for(ty),
            Type::String => STRING_STRUCT_TYPE_INDEX,
            Type::Record(_, mn)
            | Type::Enum(_, mn)
            | Type::GenericRecord {
                mangled_name: mn, ..
            }
            | Type::GenericEnum {
                mangled_name: mn, ..
            } => self.type_indices[mn],
            Type::Class(_, mn)
            | Type::GenericClass {
                mangled_name: mn, ..
            } => self.type_indices[mn],
            Type::Array(elem) => self.array_type_index(elem),
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => self.wasm_type_index_for_any_cast(inner),
            Type::Tuple(..) => self.tuple_struct_index(ty),
            Type::Function(params, _ret) => {
                let arity = params.len() as u32;
                self.closure_arity_indices[&arity].1
            }
            Type::InterfaceObject { mangled_name, .. } => {
                self.interface_object_type_indices[mangled_name]
            }
            _ => unreachable!("unsupported type for any cast: {ty}"),
        }
    }

    /// Get the WASM struct type index for a class type.
    fn wasm_type_index_for_class(&self, ty: &Type) -> u32 {
        match ty {
            Type::Class(_, mn)
            | Type::GenericClass {
                mangled_name: mn, ..
            } => self.type_indices[mn],
            _ => unreachable!("wasm_type_index_for_class: expected Class, got {ty}"),
        }
    }

    /// Allocate the next type index and return the previous value.
    fn next_type_index(&mut self) -> u32 {
        let idx = self.next_type_index;
        self.next_type_index += 1;
        idx
    }

    /// Emit the WASM custom name section with human-readable function names.
    fn emit_name_section(&mut self) {
        let mut names = NameSection::new();
        let mut func_names = NameMap::new();

        // Import functions (indices 0-14)
        // Import names from the p3 table
        for (i, import) in p3_imports::P3_IMPORTS.iter().enumerate() {
            func_names.append(i as u32, &format!("{}::{}", import.module, import.name));
        }
        func_names.append(self.func_task_return_run(), "[task-return]run");
        for i in 0..self.num_task_return_imports - 1 {
            func_names.append(
                self.func_task_return_test(i),
                &format!("[task-return]test-n{i}"),
            );
        }

        // Runtime functions
        func_names.append(self.func_run(), "run");
        func_names.append(self.func_realloc(), "realloc");
        func_names.append(self.func_initialize(), "initialize");
        func_names.append(self.func_string_eq(), "string_eq");
        func_names.append(self.func_string_concat(), "string_concat");
        func_names.append(self.func_string_cmp(), "string_cmp");
        func_names.append(self.func_char_to_string(), "char_to_string");
        func_names.append(self.func_string_from_bytes(), "string_from_bytes");
        func_names.append(self.func_string_get_char(), "string_get_char");
        func_names.append(self.func_debug_print(), "debug_print");
        func_names.append(self.func_panic_with_message(), "panic_with_message");
        func_names.append(self.func_console_print(), "console_print");
        func_names.append(self.func_console_eprint(), "console_eprint");
        func_names.append(self.func_console_eprintln(), "console_eprintln");
        func_names.append(self.func_pinned_alloc(), "pinned_alloc");
        func_names.append(self.func_pinned_free(), "pinned_free");

        // User functions
        for (mangled, &func_idx) in &self.function_indices {
            if let Some(func) = self.typed_module.functions.get(mangled) {
                func_names.append(func_idx, &func.display_name);
            }
        }

        // Wrapper functions for interface object vtable dispatch
        let num_user_funcs = self.typed_module.functions.len() as u32;
        let wrapper_base = self.user_func_base() + num_user_funcs;
        for (i, wrapper) in self.wrapper_funcs.iter().enumerate() {
            let target_name = self
                .typed_module
                .functions
                .get(&wrapper.impl_method_mangled)
                .map(|f| f.display_name.as_str())
                .unwrap_or(&wrapper.impl_method_mangled.0);
            func_names.append(
                wrapper_base + i as u32,
                &format!("wrapper::{}", target_name),
            );
        }

        // Closure functions
        let num_wrappers = self.wrapper_funcs.len() as u32;
        let lifted_base = wrapper_base + num_wrappers;
        for (i, _) in self.closure_infos.iter().enumerate() {
            func_names.append(lifted_base + i as u32, &format!("closure#{}", i));
        }

        // Ref trampolines
        let num_closures = self.closure_infos.len() as u32;
        let trampoline_base = lifted_base + num_closures;
        for (i, tramp) in self.ref_trampolines.iter().enumerate() {
            let target_name = self
                .typed_module
                .functions
                .get(&tramp.target_mangled)
                .map(|f| f.display_name.as_str())
                .unwrap_or(&tramp.target_mangled.0);
            func_names.append(
                trampoline_base + i as u32,
                &format!("trampoline::{}", target_name),
            );
        }

        // Test wrappers
        let test_wrapper_base = self.test_wrapper_base;
        for (i, &test_func_idx) in self.test_func_indices.iter().enumerate() {
            // Find the display_name of the test function by its index
            let test_name = self
                .function_indices
                .iter()
                .find(|&(_, &idx)| idx == test_func_idx)
                .and_then(|(mn, _)| self.typed_module.functions.get(mn))
                .map(|f| f.display_name.as_str())
                .unwrap_or("unknown");
            func_names.append(
                test_wrapper_base + i as u32,
                &format!("test_wrapper::{}", test_name),
            );
        }

        names.functions(&func_names);
        self.module.section(&names);
    }

    fn generate(mut self) -> Vec<u8> {
        self.collect_string_literals();
        self.emit_type_section();
        // After type emission: register every concrete monomorphized class-method function
        // with its vtable slot's erased func-type index. Consumers (`emit_function_section`
        // and the per-function body emitter) need this to type virtual methods with the
        // shared vtable slot signature instead of their concrete (per-instantiation) sig.
        self.register_virtual_method_func_types();
        self.emit_import_section();
        self.emit_function_section();
        self.emit_memory_section();
        self.emit_global_section();
        self.emit_export_section();
        self.emit_element_section();
        self.emit_data_count_section();
        let codes = self.build_code_section();
        self.module.section(&codes);
        self.emit_data_section();
        self.emit_name_section();
        self.emit_dwarf_sections();
        self.module.finish()
    }

    fn emit_dwarf_sections(&mut self) {
        let sections = dwarf::generate_dwarf_sections(
            &self.debug_infos,
            &self.typed_module.functions,
            &self.function_indices,
        );
        for (name, data) in sections {
            self.module.section(&wasm_encoder::CustomSection {
                name: std::borrow::Cow::Owned(name),
                data: std::borrow::Cow::Owned(data),
            });
        }
    }

    /// Type section, in three regions:
    ///
    /// 1. **Types 0-5** — the host-facing func types (the two WASI imports; the `run`,
    ///    `run_post`, `initialize` and `realloc` exports). These stay *outside* the rec group:
    ///    a func type inside a group takes on that group's identity, and would then no longer
    ///    match the canonical signature wasmtime expects at the import/export boundary.
    /// 2. **Types 6 .. `func_type_base` - 1** — one single module-wide rec group holding every
    ///    GC type (string types, boxes, mut-boxes, `$Uint128`, the fixed array set,
    ///    `Closure_N`/`$Tuple_N`, all user types, closure envs) plus the func types that
    ///    participate in GC type cycles (`Func_N`, class vtable slots, interface-object wrappers)
    ///    and the internal string/alloc helper signatures that reference GC types.
    ///
    ///    Grouping is what gives Dovetail nominal type identity: WASM-GC canonicalizes rec
    ///    groups structurally, so same-shape types in *separate* groups are the same type and
    ///    `ref.test`/`ref.cast` cannot tell them apart — while two members of *one* group at
    ///    different indices are always distinct. See `docs/nominal-type-identity.md`.
    /// 3. **`func_type_base` ..** — user function types, then the p3/WIT import signatures.
    ///    Host-facing, and they only reference group members backwards, which is legal.
    ///
    /// Emission order inside region 2 is unchanged from when it was many groups, so every
    /// fixed `*_TYPE_INDEX` constant keeps its value.
    fn emit_type_section(&mut self) {
        let mut types = TypeSection::new();

        // Type 0: get-stdout () -> i32
        types.ty().function(vec![], vec![ValType::I32]);
        // Type 1: blocking-write-and-flush (i32, i32, i32, i32) -> ()
        types.ty().function(
            vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            vec![],
        );
        // Type 2: run () -> i32
        types.ty().function(vec![], vec![ValType::I32]);
        // Type 3: run_post (i32) -> ()
        types.ty().function(vec![ValType::I32], vec![]);
        // Type 4: realloc (i32, i32, i32, i32) -> i32
        types.ty().function(
            vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            vec![ValType::I32],
        );
        // Type 5: initialize () -> ()
        types.ty().function(vec![], vec![]);

        // ── The single module-wide rec group starts here (index 6) ──────────────────────
        // Everything below is accumulated into `group` and emitted with one `rec` at the end,
        // so that structurally identical types stay nominally distinct.
        let mut group: Vec<SubType> = Vec::new();

        // Type 6: string backing array type (array (mut i8))
        group.push(SubType {
            is_final: true,
            supertype_idx: None,
            composite_type: wasm_encoder::CompositeType {
                inner: wasm_encoder::CompositeInnerType::Array(wasm_encoder::ArrayType(
                    wasm_encoder::FieldType {
                        element_type: wasm_encoder::StorageType::I8,
                        mutable: true,
                    },
                )),
                shared: false,
                describes: None,
                descriptor: None,
            },
        });

        // Type 7: string struct type (struct (ref $backing) i32)
        let backing_ref = wasm_encoder::FieldType {
            element_type: wasm_encoder::StorageType::Val(ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
            })),
            mutable: false,
        };
        let utf8_field = wasm_encoder::FieldType {
            element_type: wasm_encoder::StorageType::Val(ValType::I32),
            mutable: false,
        };
        group.push(SubType {
            is_final: true,
            supertype_idx: None,
            composite_type: wasm_encoder::CompositeType {
                inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                    fields: Box::new([backing_ref, utf8_field]),
                }),
                shared: false,
                describes: None,
                descriptor: None,
            },
        });

        // Types 8-13: signatures of the internal string helpers. They take/return GC refs, so
        // they can never cross the host boundary — only `functions.function(..)` declarations of
        // compiler-emitted helpers reference them — and they must sit in the group anyway, since
        // a type outside it may not reference a member of a group defined later.
        // Type 8: string_eq (ref $string_struct, ref $string_struct) -> i32
        let str_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(STRING_STRUCT_TYPE_INDEX),
        });
        group.push(func_subtype([str_ref, str_ref], [ValType::I32]));

        // Type 9: string_concat (ref $string_struct, ref $string_struct) -> ref $string_struct
        group.push(func_subtype([str_ref, str_ref], [str_ref]));

        // Type 10: char_to_string (i32) -> ref $string_struct
        group.push(func_subtype([ValType::I32], [str_ref]));

        // Type 11: string_from_bytes (ref $backing, i32, i32) -> ref $string_struct
        let backing_arr_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(U8_BACKING_TYPE_INDEX),
        });
        group.push(func_subtype(
            [backing_arr_ref, ValType::I32, ValType::I32],
            [str_ref],
        ));

        // Type 12: string_get_char (ref $string_struct, i32) -> i32
        group.push(func_subtype([str_ref, ValType::I32], [ValType::I32]));

        // Type 13: debug_print (ref $string_struct) -> ()
        group.push(func_subtype([str_ref], []));

        // Types 14-26: Boxing struct types for primitives (used when storing as Any)
        for box_field_type in [
            ValType::I32, // BoxUnit (14)
            ValType::I32, // BoxBool (15)
            ValType::I32, // BoxChar (16)
            ValType::I32, // BoxInt8 (17)
            ValType::I32, // BoxInt16 (18)
            ValType::I32, // BoxInt32 (19)
            ValType::I32, // BoxUint8 (20)
            ValType::I32, // BoxUint16 (21)
            ValType::I32, // BoxUint32 (22)
            ValType::I64, // BoxInt64 (23)
            ValType::I64, // BoxUint64 (24)
            ValType::F32, // BoxFloat32 (25)
            ValType::F64, // BoxFloat64 (26)
        ] {
            group.push(SubType {
                is_final: true,
                supertype_idx: None,
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                        fields: Box::new([wasm_encoder::FieldType {
                            element_type: wasm_encoder::StorageType::Val(box_field_type),
                            mutable: false,
                        }]),
                    }),
                    shared: false,
                    describes: None,
                    descriptor: None,
                },
            });
        }

        // Types 27-31: Mutable box struct types for closure mutable captures
        for mut_box_field_type in [
            ValType::I32, // MutBoxI32 (27) — Unit, Bool, Char, Int8-32, Uint8-32
            ValType::I64, // MutBoxI64 (28) — Int64, Uint64
            ValType::F32, // MutBoxF32 (29) — Float32
            ValType::F64, // MutBoxF64 (30) — Float64
            ValType::Ref(wasm_encoder::RefType {
                nullable: true,
                heap_type: wasm_encoder::HeapType::ANY,
            }), // MutBoxRef (31) — all reference types
        ] {
            group.push(SubType {
                is_final: true,
                supertype_idx: None,
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                        fields: Box::new([wasm_encoder::FieldType {
                            element_type: wasm_encoder::StorageType::Val(mut_box_field_type),
                            mutable: true,
                        }]),
                    }),
                    shared: false,
                    describes: None,
                    descriptor: None,
                },
            });
        }

        // Type 32: (i32) -> i32 — pinned_alloc. Internal to the bump allocator; in the group
        // only so the fixed indices keep their values.
        group.push(func_subtype([ValType::I32], [ValType::I32]));

        // Type 33: dedicated boxed `Uint128` — (struct (field (mut i64)) (field (mut i64)))
        // (lo, hi) raw i64 fields, mutable so it can double as a closure mut-box.
        {
            let i64_field = wasm_encoder::FieldType {
                element_type: wasm_encoder::StorageType::Val(ValType::I64),
                mutable: true,
            };
            group.push(SubType {
                is_final: true,
                supertype_idx: None,
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                        fields: Box::new([i64_field, i64_field]),
                    }),
                    shared: false,
                    describes: None,
                    descriptor: None,
                },
            });
        }

        // Types 34-40: the fixed array set ($Array$i16/i32/i64/f32/f64/u128/ref). $Array$i8 is
        // index 6 (the String backing array), already emitted above. Predeclared unconditionally —
        // arrays have no open axis to discover (unlike closures/tuples). Reference arrays all share
        // `$Array$ref` `(array (mut (ref null any)))`; `$Array$u128` holds the boxed `(ref $Uint128)`.
        self.emit_array_types(&mut group);

        // Canonical Closure_N/Func_N pairs — one per discovered arity. All `Type::Function`
        // values lower to `(ref Closure_N)` regardless of param/return types; see
        // `closure_arity_indices`.
        self.emit_closure_arity_types(&mut group);

        // Canonical `$Tuple_N` boxed structs — one shared `(struct (field anyref) × N)` per
        // discovered flattened tuple width.
        self.emit_tuple_width_types(&mut group);

        // ── User-defined types ─────────────────────────────────────────────────────────
        // One pass over `emission_order`: pre-allocate every slot's index, then build every
        // SubType. Because all of them land in the same rec group, cross-references resolve
        // whether they point forward or backward, so no dependency ordering is needed — the
        // order only has to put a class after its parent (see `type_graph::emission_order`).
        let order = type_graph::emission_order(&self.typed_module.types);
        // Cloned once, up front: both passes need the defs while `self` is borrowed mutably.
        let type_defs: Vec<(MangledName, TypeDef)> = order
            .iter()
            .map(|name| (name.clone(), self.typed_module.types[name].clone()))
            .collect();

        // Pass 1: assign consecutive indices for every slot — enum variants, class vtable func
        // types + vtable structs, interface-object wrappers + vtables — so that pass 2 can resolve
        // any cross-reference, including the forward ones a single group now permits.
        for (name, td) in &type_defs {
            match td {
                TypeDef::Record(_) => {
                    let idx = self.next_type_index();
                    self.type_indices.insert(name.clone(), idx);
                }
                TypeDef::Enum(e) => {
                    let base_index = self.next_type_index();
                    self.type_indices.insert(name.clone(), base_index);
                    for variant in &e.variants {
                        let idx = self.next_type_index();
                        self.variant_type_indices
                            .insert((name.clone(), variant.name.clone()), idx);
                    }
                }
                TypeDef::Class(cls) => {
                    // Only slots beyond the parent's vtable need a new func type; the rest are
                    // inherited from the parent by `create_class_vtable` in pass 2, which is
                    // why `emission_order` puts a class after its parent.
                    let parent_vtable_len =
                        cls.parent_mangled_name.as_ref().map_or(0, |parent_mn| {
                            match &self.typed_module.types[parent_mn] {
                                TypeDef::Class(parent_cls) => parent_cls.vtable_methods.len(),
                                _ => 0,
                            }
                        });
                    // Slots the parent already declared keep the parent's func type — an
                    // override reuses the signature it overrides.
                    if let Some(parent_mn) = &cls.parent_mangled_name {
                        for slot_idx in 0..parent_vtable_len {
                            if let Some(&idx) = self
                                .class_vtable_slot_func_types
                                .get(&(parent_mn.clone(), slot_idx as u32))
                            {
                                self.class_vtable_slot_func_types
                                    .insert((name.clone(), slot_idx as u32), idx);
                            }
                        }
                    }
                    for slot_idx in parent_vtable_len..cls.vtable_methods.len() {
                        let idx = self.next_type_index();
                        self.class_vtable_slot_func_types
                            .insert((name.clone(), slot_idx as u32), idx);
                    }
                    let vtable_idx = self.next_type_index();
                    self.class_vtable_type_indices
                        .insert(name.clone(), vtable_idx);
                    let class_idx = self.next_type_index();
                    self.type_indices.insert(name.clone(), class_idx);
                }
                TypeDef::InterfaceObject(to) => {
                    // Field 0..supers.len() are the nested super-vtable refs;
                    // own member slots follow.
                    let offset = to.supers.len() as u32;
                    for (i, (member_name, _, _)) in to.vtable_members.iter().enumerate() {
                        let idx = self.next_type_index();
                        let field_idx = offset + i as u32;
                        self.wrapper_func_type_indices
                            .insert((name.clone(), field_idx), idx);
                        self.trait_method_vtable_indices
                            .insert((name.clone(), member_name.clone()), field_idx);
                    }
                    let vtable_idx = self.next_type_index();
                    self.vtable_type_indices.insert(name.clone(), vtable_idx);
                    let object_idx = self.next_type_index();
                    self.interface_object_type_indices
                        .insert(name.clone(), object_idx);
                }
                TypeDef::InterfaceIntersection(_) => {
                    // Set vtable struct (one ref field per component vtable), then the
                    // fat-pointer struct. No wrapper func types or slot map — those
                    // live on the components.
                    let vtable_idx = self.next_type_index();
                    self.vtable_type_indices.insert(name.clone(), vtable_idx);
                    let object_idx = self.next_type_index();
                    self.interface_object_type_indices
                        .insert(name.clone(), object_idx);
                }
                TypeDef::Array(_) => {
                    unreachable!("arrays are predeclared as the fixed array set, never a user type")
                }
            }
        }

        // Pass 2: build the SubTypes, in the same order the indices were assigned.
        for (name, td) in &type_defs {
            match td {
                TypeDef::Record(rec) => {
                    group.push(records::build_record_subtype(rec, self));
                }
                TypeDef::Enum(e) => {
                    let enum_base_idx = self.type_indices[name];
                    group.push(enums::build_enum_base_subtype(self.id_prefix(name) != 0));
                    for variant in &e.variants {
                        group.push(enums::build_enum_variant_subtype(
                            variant,
                            enum_base_idx,
                            self.id_prefix(name) != 0,
                            self,
                        ));
                    }
                }
                TypeDef::Class(cls) => {
                    // Vtable func types + vtable struct + class struct.
                    self.build_class_vtable_subtypes(cls, name, &mut group);
                }
                TypeDef::InterfaceObject(to) => {
                    group.extend(self.build_interface_object_subtypes(to));
                }
                TypeDef::InterfaceIntersection(toi) => {
                    group.extend(self.build_interface_object_intersection_subtypes(toi));
                }
                TypeDef::Array(_) => {
                    unreachable!("arrays are predeclared as the fixed array set, never a user type")
                }
            }
        }

        // Build wrapper functions for interface object dispatch (all type indices are assigned).
        self.build_wrapper_funcs();

        // Closure env struct types (one per closure with captures). Last members of the group.
        self.emit_closure_env_types(&mut group);

        // ── End of the module-wide rec group ───────────────────────────────────────────
        types.ty().rec(group);

        // Record where user function types begin (after struct/array/interface-object/closure types)
        self.func_type_base = self.next_type_index;

        // User function types
        for func in self.typed_module.functions.values() {
            self.next_type_index += 1;
            // Phase 3: tuple params flatten to a sequence of N WASM params (direct-call ABI). The
            // vtable `self` (param 0) stays the single base-class ref.
            let params: Vec<ValType> = func
                .params
                .iter()
                .enumerate()
                .flat_map(|(i, p)| {
                    if i == 0
                        && let Some(ref vst) = func.vtable_self_type
                    {
                        return smallvec::smallvec![self.single_val_type(vst)];
                    }
                    self.type_to_valtypes(&p.ty)
                })
                .collect();
            // Phase 2: tuple returns flatten to multi-value results.
            let result: Vec<ValType> = self.type_to_valtypes(&func.return_type).into_vec();
            types.ty().function(params, result);
        }

        // Wrapper function types (after user function types so they can reference user types)
        // Actually, wrapper func types are already emitted in emit_interface_object_types

        // p3 import function types, deduplicated, appended after everything
        // else (type indices are position-only; imports may reference them).
        let mut sig_dedup: BTreeMap<(Vec<ValType>, Vec<ValType>), u32> = BTreeMap::new();
        let mut import_type_indices = Vec::with_capacity(p3_imports::P3_IMPORTS.len());
        for import in p3_imports::P3_IMPORTS {
            let key = (import.params.to_vec(), import.results.to_vec());
            let idx = *sig_dedup.entry(key).or_insert_with(|| {
                let idx = self.next_type_index;
                self.next_type_index += 1;
                types.ty().function(
                    import.params.iter().copied(),
                    import.results.iter().copied(),
                );
                idx
            });
            import_type_indices.push(idx);
        }
        self.import_type_indices = import_type_indices;
        self.type_task_return_run = *sig_dedup
            .entry((vec![ValType::I32], vec![]))
            .or_insert_with(|| {
                let idx = self.next_type_index;
                self.next_type_index += 1;
                types.ty().function([ValType::I32], []);
                idx
            });
        self.type_task_return_unit = *sig_dedup.entry((vec![], vec![])).or_insert_with(|| {
            let idx = self.next_type_index;
            self.next_type_index += 1;
            types.ty().function([], []);
            idx
        });
        // WIT component-import func types, deduplicated and appended last (only
        // referenced by the conditional WIT imports), so they never shift a
        // function or existing import type index.
        let mut wit_type_indices = Vec::with_capacity(self.wit_registry.entries.len());
        for entry in &self.wit_registry.entries {
            let key = (entry.params.clone(), entry.results.clone());
            let idx = *sig_dedup.entry(key).or_insert_with(|| {
                let idx = self.next_type_index;
                self.next_type_index += 1;
                types
                    .ty()
                    .function(entry.params.iter().copied(), entry.results.iter().copied());
                idx
            });
            wit_type_indices.push(idx);
        }
        self.wit_registry.type_indices = wit_type_indices;
        self.module.section(&types);
    }

    /// Emit the fixed array set (types 34-47). `$Array$i8`
    /// is the String backing array at index 6, emitted with the other fixed types. Each is a final,
    /// mutable, leaf array type — none reference a user type (`$Array$ref` holds `(ref any)`,
    /// `$Array$u128` holds the fixed `(ref $Uint128)`).
    /// Must be emitted at `next_type_index == ARRAY_I16_TYPE_INDEX`, before closures/tuples/user types.
    fn emit_array_types(&self, group: &mut Vec<SubType>) {
        for (element, _) in &ARRAY_ELEMENT_TYPES[1..] {
            let storage = match element {
                Type::Int8 | Type::Uint8 => wasm_encoder::StorageType::I8,
                Type::Int16 | Type::Uint16 => wasm_encoder::StorageType::I16,
                _ => wasm_encoder::StorageType::Val(self.single_val_type(element)),
            };
            group.push(SubType {
                is_final: true,
                supertype_idx: None,
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Array(wasm_encoder::ArrayType(
                        wasm_encoder::FieldType {
                            element_type: storage,
                            mutable: true,
                        },
                    )),
                    shared: false,
                    describes: None,
                    descriptor: None,
                },
            });
        }
    }

    /// Build the SubTypes for a interface object: one wrapper func type per vtable member, the
    /// vtable struct, then the interface object struct. All three index families were registered
    /// in the pre-allocation pass of `emit_type_section`, so this only reads them back.
    fn build_interface_object_subtypes(
        &self,
        to: &crate::typechecker::types::InterfaceObjectTypeDef,
    ) -> Vec<SubType> {
        let anyref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::ANY,
        });

        let mut subtypes = Vec::new();
        let mut vtable_field_types = Vec::new();

        // Nested super-vtable ref fields come first (extends). Their type
        // indices were pre-allocated in pass 1 like every other vtable.
        for super_mn in &to.supers {
            let super_vtable_idx = self.vtable_type_indices[super_mn];
            vtable_field_types.push(wasm_encoder::FieldType {
                element_type: wasm_encoder::StorageType::Val(ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(super_vtable_idx),
                })),
                mutable: false,
            });
        }

        let offset = to.supers.len() as u32;
        for (i, (_, param_types, return_type)) in to.vtable_members.iter().enumerate() {
            let func_type_idx =
                self.wrapper_func_type_indices[&(to.mangled_name.clone(), offset + i as u32)];

            // Tuple params splice into a sequence of WASM params (the impl's direct-call ABI),
            // matching class vtable slots; the wrapper forwards them through without reboxing.
            let mut params = vec![anyref];
            for pt in param_types {
                params.extend(self.type_to_valtypes(pt));
            }
            // Phase 2: tuple returns flatten to multi-value results (the wrapper forwards them).
            let result: Vec<ValType> = self.type_to_valtypes(return_type).into_vec();

            subtypes.push(func_subtype(params, result));

            vtable_field_types.push(wasm_encoder::FieldType {
                element_type: wasm_encoder::StorageType::Val(ValType::Ref(wasm_encoder::RefType {
                    nullable: false,
                    heap_type: wasm_encoder::HeapType::Concrete(func_type_idx),
                })),
                mutable: false,
            });
        }

        // Vtable struct type
        let vtable_type_idx = self.vtable_type_indices[&to.mangled_name];
        subtypes.push(SubType {
            is_final: true,
            supertype_idx: None,
            composite_type: wasm_encoder::CompositeType {
                inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                    fields: vtable_field_types.into_boxed_slice(),
                }),
                shared: false,
                describes: None,
                descriptor: None,
            },
        });

        // Interface object struct type: (anyref, ref $vtable)
        subtypes.push(SubType {
            is_final: true,
            supertype_idx: None,
            composite_type: wasm_encoder::CompositeType {
                inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                    fields: Box::new([
                        wasm_encoder::FieldType {
                            element_type: wasm_encoder::StorageType::Val(anyref),
                            mutable: false,
                        },
                        wasm_encoder::FieldType {
                            element_type: wasm_encoder::StorageType::Val(ValType::Ref(
                                wasm_encoder::RefType {
                                    nullable: false,
                                    heap_type: wasm_encoder::HeapType::Concrete(vtable_type_idx),
                                },
                            )),
                            mutable: false,
                        },
                    ]),
                }),
                shared: false,
                describes: None,
                descriptor: None,
            },
        });

        subtypes
    }

    /// Build the SubTypes for an intersection interface object: the set vtable
    /// struct — one immutable non-null ref field per COMPONENT vtable, in the
    /// set's sorted order — then the fat-pointer struct `(anyref, ref $set-vtable)`.
    /// Component vtable/wrapper types are emitted by the components' own
    /// `build_interface_object_subtypes`; forward refs are legal inside the single
    /// module-wide rec group because all indices were pre-allocated in pass 1.
    fn build_interface_object_intersection_subtypes(
        &self,
        toi: &crate::typechecker::types::InterfaceIntersectionTypeDef,
    ) -> Vec<SubType> {
        let anyref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::ANY,
        });

        let mut subtypes = Vec::new();

        let vtable_field_types: Vec<wasm_encoder::FieldType> = toi
            .components
            .iter()
            .map(|(_, component_mn)| {
                let component_vtable_idx = self.vtable_type_indices[component_mn];
                wasm_encoder::FieldType {
                    element_type: wasm_encoder::StorageType::Val(ValType::Ref(
                        wasm_encoder::RefType {
                            nullable: false,
                            heap_type: wasm_encoder::HeapType::Concrete(component_vtable_idx),
                        },
                    )),
                    mutable: false,
                }
            })
            .collect();

        // Set vtable struct type
        let vtable_type_idx = self.vtable_type_indices[&toi.mangled_name];
        subtypes.push(SubType {
            is_final: true,
            supertype_idx: None,
            composite_type: wasm_encoder::CompositeType {
                inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                    fields: vtable_field_types.into_boxed_slice(),
                }),
                shared: false,
                describes: None,
                descriptor: None,
            },
        });

        // Intersection object struct type: (anyref, ref $set-vtable)
        subtypes.push(SubType {
            is_final: true,
            supertype_idx: None,
            composite_type: wasm_encoder::CompositeType {
                inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                    fields: Box::new([
                        wasm_encoder::FieldType {
                            element_type: wasm_encoder::StorageType::Val(anyref),
                            mutable: false,
                        },
                        wasm_encoder::FieldType {
                            element_type: wasm_encoder::StorageType::Val(ValType::Ref(
                                wasm_encoder::RefType {
                                    nullable: false,
                                    heap_type: wasm_encoder::HeapType::Concrete(vtable_type_idx),
                                },
                            )),
                            mutable: false,
                        },
                    ]),
                }),
                shared: false,
                describes: None,
                descriptor: None,
            },
        });

        subtypes
    }

    /// Build wrapper functions for interface object dispatch.
    /// Called after the user-type passes, when all type indices are assigned.
    /// DFS path of nested super-vtable ref fields from the `from` component's
    /// vtable to `target`'s: a list of (vtable owner, field index) struct.gets
    /// to emit in order. Empty when `from == target`; `None` when unreachable.
    fn super_vtable_path(
        &self,
        from: &MangledName,
        target: &MangledName,
    ) -> Option<Vec<(MangledName, u32)>> {
        if from == target {
            return Some(vec![]);
        }
        let Some(TypeDef::InterfaceObject(to)) = self.typed_module.types.get(from) else {
            return None;
        };
        for (i, super_mn) in to.supers.iter().enumerate() {
            if let Some(mut rest) = self.super_vtable_path(super_mn, target) {
                let mut path = vec![(from.clone(), i as u32)];
                path.append(&mut rest);
                return Some(path);
            }
        }
        None
    }

    /// The vtable-global key for one coercion. Historically just the target's
    /// set key, which assumed at most one implementation of a trait per type.
    /// Tagged groups (`$via$` providers, `$inst$` sibling instantiations)
    /// break that assumption, so the key is discriminated by the group keys —
    /// untagged coercions keep the plain set key, byte-identically.
    fn coercion_global_key(
        interface_mangled_name: &MangledName,
        vtable_methods: &[VtableMethodGroup],
    ) -> MangledName {
        let any_tagged = vtable_methods
            .iter()
            .any(|(k, _)| k.0.contains("$via$") || k.0.contains("$inst$"));
        if !any_tagged {
            return interface_mangled_name.clone();
        }
        let joined: Vec<&str> = vtable_methods.iter().map(|(k, _)| k.0.as_str()).collect();
        MangledName(format!("{}${}", interface_mangled_name, joined.join("&")))
    }

    /// A coercion group key is the component's `$IfaceObj$…` key, optionally
    /// suffixed `$via$<provider trait fqn>` when a different trait's impl
    /// block backs the slots (extends). Lookups use the component part;
    /// wrapper names use the full key so a via-backed vtable never collides
    /// with a direct coercion's wrappers.
    fn split_group_key(key: &MangledName) -> MangledName {
        let cut = match (key.0.find("$via$"), key.0.find("$inst$")) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        match cut {
            Some(i) => MangledName(key.0[..i].to_string()),
            None => key.clone(),
        }
    }

    /// For a DFS post-order group list, the length of the subtree ending at
    /// each index (1 for a leaf; 1 + children subtree lengths otherwise —
    /// children count = the component's direct super count).
    fn group_subtree_lengths(&self, groups: &[VtableMethodGroup]) -> Vec<usize> {
        let mut lengths: Vec<usize> = Vec::with_capacity(groups.len());
        let mut stack: Vec<usize> = Vec::new();
        for (key, _) in groups {
            let component_mn = Self::split_group_key(key);
            let num_supers = match self.typed_module.types.get(&component_mn) {
                Some(TypeDef::InterfaceObject(to)) => to.supers.len(),
                _ => 0,
            };
            let mut len = 1usize;
            for _ in 0..num_supers.min(stack.len()) {
                len += stack.pop().unwrap();
            }
            stack.push(len);
            lengths.push(len);
        }
        lengths
    }

    fn build_wrapper_funcs(&mut self) {
        if self.interface_object_infos.is_empty() {
            return;
        }

        let interface_object_infos = std::mem::take(&mut self.interface_object_infos);

        let num_user_funcs = self.typed_module.functions.len() as u32;
        let wrapper_base = self.user_func_base() + num_user_funcs;
        let mut wrapper_idx = self.wrapper_funcs.len() as u32;

        for info in &interface_object_infos {
            let type_key = instance_key(&info.concrete_type);
            for (group_key, entries) in &info.vtable_methods {
                let component_mn = &Self::split_group_key(group_key);
                // The per-trait (component) TypeDef holds the erased vtable-slot signatures.
                let slot_def = match self.typed_module.types.get(component_mn) {
                    Some(TypeDef::InterfaceObject(to)) => Some(to),
                    _ => None,
                };
                for (member_name, impl_mangled, _) in entries {
                    if let Some(func) = self.typed_module.functions.get(impl_mangled) {
                        let concrete_params: Vec<Type> =
                            func.params.iter().skip(1).map(|p| p.ty.clone()).collect();
                        // Slot (erased) param/return types for this member, from the per-trait vtable.
                        let (slot_params, slot_return): (Vec<Type>, Type) = slot_def
                            .and_then(|to| {
                                to.vtable_members
                                    .iter()
                                    .find(|(mn, _, _)| mn == member_name)
                            })
                            .map(|(_, ps, r)| (ps.clone(), r.clone()))
                            .unwrap_or_else(|| (concrete_params.clone(), func.return_type.clone()));
                        // Pair each slot param with its concrete impl counterpart. (Lengths match: both
                        // are the method's non-self params; the slot just erases generic ones.)
                        let params: Vec<(Type, Type)> = slot_params
                            .into_iter()
                            .zip(concrete_params.iter().cloned())
                            .collect();

                        let field_idx = self.trait_method_vtable_indices
                            [&(component_mn.clone(), member_name.clone())];
                        let func_type_index =
                            self.wrapper_func_type_indices[&(component_mn.clone(), field_idx)];

                        let wrapper_name = MangledName(format!(
                            "$wrapper${}${}${}",
                            group_key, type_key, member_name
                        ));

                        if let std::collections::btree_map::Entry::Vacant(e) =
                            self.function_indices.entry(wrapper_name)
                        {
                            e.insert(wrapper_base + wrapper_idx);
                            self.wrapper_funcs.push(WrapperFunc {
                                group_key: group_key.clone(),
                                func_type_index,
                                impl_method_mangled: impl_mangled.clone(),
                                concrete_type: info.concrete_type.clone(),
                                params,
                                slot_return,
                                concrete_return: func.return_type.clone(),
                            });
                            wrapper_idx += 1;
                        }
                    }
                }
            }
        }

        self.interface_object_infos = interface_object_infos;
    }

    /// Emit closure env struct types (per closure with captures).
    /// Called AFTER user-defined types so env fields can reference user type valtypes.
    fn emit_closure_env_types(&mut self, group: &mut Vec<SubType>) {
        if self.closure_infos.is_empty() {
            return;
        }

        let closure_captures: Vec<Vec<CapturedVar>> = self
            .closure_infos
            .iter()
            .map(|info| info.captures.clone())
            .collect();
        for captures in &closure_captures {
            if captures.is_empty() {
                self.closure_env_type_indices.push(None);
            } else {
                let env_type_index = self.next_type_index();
                let mut fields: Vec<wasm_encoder::FieldType> = Vec::new();
                for cap in captures {
                    if cap.mutable {
                        // Mutable capture: one field holding the mut-box ref (a `(ref $Tuple_N)` for a
                        // tuple — its own struct serves as the mut-box — else a `(ref $MutBox)`).
                        fields.push(wasm_encoder::FieldType {
                            element_type: wasm_encoder::StorageType::Val(
                                self.mut_box_valtype(&cap.ty),
                            ),
                            mutable: false,
                        });
                    } else {
                        // Immutable capture: splice into its flattened WASM value types (a tuple
                        // occupies a run of fields, no `(ref $Tuple)` box), exactly like class fields.
                        for vt in self.type_to_valtypes(&cap.ty) {
                            fields.push(wasm_encoder::FieldType {
                                element_type: wasm_encoder::StorageType::Val(vt),
                                mutable: false,
                            });
                        }
                    }
                }
                group.push(SubType {
                    is_final: true,
                    supertype_idx: None,
                    composite_type: wasm_encoder::CompositeType {
                        inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                            fields: fields.into_boxed_slice(),
                        }),
                        shared: false,
                        describes: None,
                        descriptor: None,
                    },
                });
                self.closure_env_type_indices.push(Some(env_type_index));
            }
        }
    }

    /// Import section: WASI imports.
    fn emit_import_section(&mut self) {
        let mut imports = ImportSection::new();

        // Static p3 WASI import table (indices 0..NUM_IMPORTS).
        debug_assert_eq!(self.import_type_indices.len(), p3_imports::P3_IMPORTS.len());
        for (import, ty_idx) in p3_imports::P3_IMPORTS.iter().zip(&self.import_type_indices) {
            imports.import(
                import.module,
                import.name,
                wasm_encoder::EntityType::Function(*ty_idx),
            );
        }

        // Dynamic `[task-return]` imports, one per async-lifted export:
        // `run` first, then each test. Their indices follow the static table
        // (see `runtime_func_base`).
        imports.import(
            &format!("[export]wasi:cli/run@{}", component::P3_VERSION),
            "[task-return]run",
            wasm_encoder::EntityType::Function(self.type_task_return_run),
        );
        let num_tests = self.num_task_return_imports - 1;
        for i in 0..num_tests {
            imports.import(
                "[export]$root",
                &format!("[task-return]test-n{i}"),
                wasm_encoder::EntityType::Function(self.type_task_return_unit),
            );
        }

        // WIT component imports, right after the `[task-return]` imports and
        // before the runtime functions. Order matches `wit_registry.entries`
        // (and thus `func_index`), so the marshaling call targets resolve.
        debug_assert_eq!(
            self.wit_registry.type_indices.len(),
            self.wit_registry.entries.len()
        );
        let wit_base = self.wit_import_base();
        for (i, (entry, &ty_idx)) in self
            .wit_registry
            .entries
            .iter()
            .zip(&self.wit_registry.type_indices)
            .enumerate()
        {
            if std::env::var_os("DOVETAIL_DUMP_WIT_IMPORTS").is_some() {
                eprintln!(
                    "[wit-import] fn#{} {} {} params={:?} results={:?}",
                    wit_base + i as u32,
                    entry.module,
                    entry.field,
                    entry.params,
                    entry.results
                );
            }
            imports.import(
                &entry.module,
                &entry.field,
                wasm_encoder::EntityType::Function(ty_idx),
            );
        }

        self.module.section(&imports);
    }

    /// Function section: declare runtime functions + user functions.
    fn emit_function_section(&mut self) {
        let mut functions = FunctionSection::new();

        // Runtime functions (after NUM_IMPORTS imports)
        functions.function(TYPE_INITIALIZE); // run () -> () (stackful async lift)
        functions.function(TYPE_REALLOC); // realloc
        functions.function(TYPE_INITIALIZE); // initialize
        functions.function(TYPE_STRING_EQ); // string_eq
        functions.function(TYPE_STRING_CONCAT); // string_concat
        functions.function(TYPE_STRING_EQ); // string_cmp (same signature as string_eq)
        functions.function(TYPE_CHAR_TO_STRING); // char_to_string
        functions.function(TYPE_STRING_FROM_BYTES); // string_from_bytes
        functions.function(TYPE_STRING_GET_CHAR); // string_get_char
        functions.function(TYPE_DEBUG_PRINT); // debug_print
        functions.function(TYPE_DEBUG_PRINT); // panic_with_message (same signature)
        functions.function(TYPE_DEBUG_PRINT); // console_print (same signature)
        functions.function(TYPE_DEBUG_PRINT); // console_eprint (same signature)
        functions.function(TYPE_DEBUG_PRINT); // console_eprintln (same signature)
        functions.function(TYPE_PINNED_ALLOC); // pinned_alloc
        functions.function(TYPE_RUN_POST); // pinned_free ((i32) -> ())

        // User functions — type indices follow user-defined struct/array types.
        // Virtual methods use the vtable slot func type for correct ref.func typing.
        let func_type_base = self.func_type_base;
        for (i, (name, _)) in self.typed_module.functions.iter().enumerate() {
            if let Some(&vtable_func_type) = self.virtual_method_func_types.get(name) {
                functions.function(vtable_func_type);
            } else {
                functions.function(func_type_base + i as u32);
            }
        }

        // Wrapper functions for interface object vtable dispatch
        for wrapper in &self.wrapper_funcs {
            functions.function(wrapper.func_type_index);
        }

        // Lifted closure functions — every closure uses the canonical Func_N for its arity.
        let num_user_funcs = self.typed_module.functions.len() as u32;
        let num_wrappers = self.wrapper_funcs.len() as u32;
        let lifted_base = self.user_func_base() + num_user_funcs + num_wrappers;
        for (i, info) in self.closure_infos.iter().enumerate() {
            let arity = info.param_types.len() as u32;
            let (func_type_index, _) = self.closure_arity_indices[&arity];
            functions.function(func_type_index);
            self.closure_func_indices.push(lifted_base + i as u32);
        }

        // Ref trampolines for FunctionRef/MethodRef — also Func_N.
        let num_closures = self.closure_infos.len() as u32;
        let trampoline_base = lifted_base + num_closures;
        for (i, tramp) in self.ref_trampolines.iter().enumerate() {
            let arity = tramp.param_types.len() as u32;
            let (func_type_index, _) = self.closure_arity_indices[&arity];
            functions.function(func_type_index);
            let func_idx = trampoline_base + i as u32;
            // Update the dedup map with the actual WASM function index
            let key = if tramp.self_type.is_some() {
                format!("$ref_method${}", tramp.target_mangled.0)
            } else {
                format!("$ref_func${}", tramp.target_mangled.0)
            };
            self.ref_trampoline_indices.insert(key, func_idx);
        }

        // Test wrapper functions: async-lift entry `[] -> [code]`
        let num_ref_trampolines = self.ref_trampolines.len() as u32;
        self.test_wrapper_base = trampoline_base + num_ref_trampolines;
        for _ in &self.test_func_indices {
            functions.function(TYPE_INITIALIZE); // test wrapper: [] -> [] (stackful)
        }

        self.module.section(&functions);
    }

    /// Memory section: 1 page min, 256 pages max.
    fn emit_memory_section(&mut self) {
        let mut memory = MemorySection::new();
        memory.memory(MemoryType {
            minimum: 1,
            maximum: Some(256),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        self.module.section(&memory);
    }

    /// Global section: global 0 = bump allocator pointer, then user globals.
    fn emit_global_section(&mut self) {
        let mut globals = GlobalSection::new();

        // Global 0: bump allocator pointer (i32 mutable, init 0)
        // Globals 1-3: stdio stream state (see GLOBAL_* consts)
        // Globals 4-5: pinned heap free-list head and brk (starts at page 1;
        // page 0 is the scratch arena)
        // Global 6: current marshaling window's pinned-scratch list head
        // Global 7: next lazily assigned class identity hash (zero is reserved)
        for init in RUNTIME_GLOBAL_INITS {
            globals.global(
                GlobalType {
                    val_type: ValType::I32,
                    mutable: true,
                    shared: false,
                },
                &wasm_encoder::ConstExpr::i32_const(init),
            );
        }

        self.emit_user_globals(&mut globals);
        self.emit_vtable_globals(&mut globals);
        self.emit_class_vtable_globals(&mut globals);
        self.emit_runtime_type_globals(&mut globals);

        self.module.section(&globals);
    }

    /// Emit vtable globals for interface object dispatch.
    fn emit_vtable_globals(&mut self, globals: &mut GlobalSection) {
        if self.interface_object_infos.is_empty() {
            return;
        }

        let num_user_globals = self.typed_module.globals.len() as u32;
        let vtable_global_base = USER_GLOBAL_BASE + num_user_globals;
        let mut vtable_global_idx = 0u32;

        let interface_object_infos = std::mem::take(&mut self.interface_object_infos);
        let mut seen = std::collections::BTreeSet::new();
        #[allow(clippy::type_complexity)]
        let mut deferred_via_standalones: Vec<(InstanceKey, Vec<VtableMethodGroup>)> = Vec::new();

        for info in &interface_object_infos {
            let type_key = instance_key(&info.concrete_type);
            let global_key =
                Self::coercion_global_key(&info.interface_mangled_name, &info.vtable_methods);
            let key = (type_key.clone(), global_key);
            if !seen.insert(key.clone()) {
                continue;
            }
            // Already emitted as a standalone component global of an earlier
            // intersection coercion — reuse it (keeps the map length equal to
            // the emitted-global count, which downstream bases rely on).
            if self
                .vtable_global_indices
                .get(&key)
                .is_some_and(|&idx| idx != u32::MAX)
            {
                continue;
            }

            let vtable_type_idx = self.vtable_type_indices[&info.interface_mangled_name];
            let global_idx = vtable_global_base + vtable_global_idx;
            self.vtable_global_indices.insert(key, global_idx);
            vtable_global_idx += 1;

            // Build constant expression. Groups arrive in DFS post-order per
            // component tree: each group pushes its wrapper refs and a
            // struct.new whose leading fields consume the nested super vtables
            // already on the stack — bottom-up construction by pure stack
            // discipline. For an intersection TARGET, a final struct.new packs
            // the component-root vtables (exactly what remains on the stack).
            // A plain single interface with no supers is one group — the
            // emitted instruction sequence is byte-identical to the
            // historical form.
            let target_is_intersection = matches!(
                self.typed_module.types.get(&info.interface_mangled_name),
                Some(TypeDef::InterfaceIntersection(_))
            );
            let mut insns: Vec<wasm_encoder::Instruction> = Vec::new();

            for (group_key, entries) in &info.vtable_methods {
                let component_mn = Self::split_group_key(group_key);
                for (member_name, _impl_mangled, _) in entries {
                    // Find the wrapper function index
                    let wrapper_name = MangledName(format!(
                        "$wrapper${}${}${}",
                        group_key, type_key, member_name
                    ));
                    let wrapper_func_idx = *self.function_indices.get(&wrapper_name).unwrap_or_else(|| {
                        panic!("missing wrapper function: {wrapper_name} for trait={component_mn} type={type_key}")
                    });
                    insns.push(wasm_encoder::Instruction::RefFunc(wrapper_func_idx));
                }
                let component_vtable_idx = self.vtable_type_indices[&component_mn];
                insns.push(wasm_encoder::Instruction::StructNew(component_vtable_idx));
            }
            if target_is_intersection {
                insns.push(wasm_encoder::Instruction::StructNew(vtable_type_idx));
            }
            let init_expr = wasm_encoder::ConstExpr::extended(insns);

            let vtable_ref_type = ValType::Ref(wasm_encoder::RefType {
                nullable: false,
                heap_type: wasm_encoder::HeapType::Concrete(vtable_type_idx),
            });

            globals.global(
                GlobalType {
                    val_type: vtable_ref_type,
                    mutable: false,
                    shared: false,
                },
                &init_expr,
            );

            // A single-interface target whose ROOT group is tagged registered
            // its main global under the DISCRIMINATED key only — alias the
            // root group key to the same global so Self-return re-boxes (which
            // look up by group key) can find it. Aliases don't emit a global,
            // so the emitted-count bookkeeping below must not use map length.
            if !target_is_intersection && let Some((root_key, _)) = info.vtable_methods.last() {
                let main_key =
                    Self::coercion_global_key(&info.interface_mangled_name, &info.vtable_methods);
                if *root_key != main_key {
                    // Tagged root, or untagged root over a tagged subtree
                    // (a direct impl of an extending trait): either way
                    // the root group key is the one re-boxes look up. An
                    // UNTAGGED root key is safe to alias — sibling direct
                    // impls would have `$inst$`-tagged it.
                    let alias_key = (type_key.clone(), root_key.clone());
                    self.vtable_global_indices
                        .entry(alias_key)
                        .or_insert(global_idx);
                }
            }

            // ALSO emit a standalone vtable global per non-root group (keyed
            // (type_key, component_mn)) so Self-return wrappers can re-box the
            // concrete value into their component's interface object
            // regardless of how the value was first coerced. For an
            // intersection every group gets one; for a single-interface
            // target the root's standalone IS the main global above, so only
            // nested super groups (extends) add standalones. Each standalone
            // rebuilds its full subtree (nested super vtables included).
            // `$via$`-tagged groups (a super's slots backed by a sub-trait's
            // block) are DEFERRED to a second phase: when a type also
            // implements the super directly, the direct global must own the
            // (type, component) key regardless of coercion order — a re-boxed
            // super object is a direct-super context, so the direct impl wins
            // deterministically.
            if info.vtable_methods.len() > 1 {
                let subtree_lengths = self.group_subtree_lengths(&info.vtable_methods);
                let last = info.vtable_methods.len() - 1;
                for (i, (group_key, _)) in info.vtable_methods.iter().enumerate() {
                    // The root group of a single-interface target is the main
                    // global emitted above.
                    if !target_is_intersection
                        && i == last
                        && subtree_lengths[i] == info.vtable_methods.len()
                    {
                        continue;
                    }
                    let subtree_start = i + 1 - subtree_lengths[i];
                    if group_key.0.contains("$via$") || group_key.0.contains("$inst$") {
                        deferred_via_standalones.push((
                            type_key.clone(),
                            info.vtable_methods[subtree_start..=i].to_vec(),
                        ));
                        continue;
                    }
                    let component_mn = Self::split_group_key(group_key);
                    let comp_key = (type_key.clone(), component_mn.clone());
                    if self.vtable_global_indices.contains_key(&comp_key)
                        && self.vtable_global_indices[&comp_key] != u32::MAX
                    {
                        continue;
                    }
                    let component_vtable_idx = self.vtable_type_indices[&component_mn];
                    let comp_global_idx = vtable_global_base + vtable_global_idx;
                    self.vtable_global_indices.insert(comp_key, comp_global_idx);
                    vtable_global_idx += 1;

                    let mut comp_insns: Vec<wasm_encoder::Instruction> = Vec::new();
                    for (sub_key, sub_entries) in &info.vtable_methods[subtree_start..=i] {
                        let sub_component_mn = Self::split_group_key(sub_key);
                        for (member_name, _impl_mangled, _) in sub_entries {
                            let wrapper_name = MangledName(format!(
                                "$wrapper${}${}${}",
                                sub_key, type_key, member_name
                            ));
                            let wrapper_func_idx = self.function_indices[&wrapper_name];
                            comp_insns.push(wasm_encoder::Instruction::RefFunc(wrapper_func_idx));
                        }
                        comp_insns.push(wasm_encoder::Instruction::StructNew(
                            self.vtable_type_indices[&sub_component_mn],
                        ));
                    }
                    globals.global(
                        GlobalType {
                            val_type: ValType::Ref(wasm_encoder::RefType {
                                nullable: false,
                                heap_type: wasm_encoder::HeapType::Concrete(component_vtable_idx),
                            }),
                            mutable: false,
                            shared: false,
                        },
                        &wasm_encoder::ConstExpr::extended(comp_insns),
                    );
                }
            }
        }

        // Phase 2: via-backed standalone super vtables — only for keys not
        // already owned by a direct global.
        for (type_key, subtree) in deferred_via_standalones {
            let (root_key, _) = subtree.last().expect("non-empty subtree");
            let component_mn = Self::split_group_key(root_key);
            // Keyed by the FULL `$via$`-tagged group key: each provider's
            // subtree gets its own standalone, so an inherited bare-`Self`
            // re-box deterministically uses its OWN provider's vtable (the
            // un-tagged key stays reserved for direct globals — those win
            // whenever a direct impl's coercion exists, per the spec).
            let comp_key = (type_key.clone(), root_key.clone());
            if self.vtable_global_indices.contains_key(&comp_key)
                && self.vtable_global_indices[&comp_key] != u32::MAX
            {
                continue;
            }
            let component_vtable_idx = self.vtable_type_indices[&component_mn];
            let comp_global_idx = vtable_global_base + vtable_global_idx;
            self.vtable_global_indices.insert(comp_key, comp_global_idx);
            vtable_global_idx += 1;

            let mut comp_insns: Vec<wasm_encoder::Instruction> = Vec::new();
            for (sub_key, sub_entries) in &subtree {
                let sub_component_mn = Self::split_group_key(sub_key);
                for (member_name, _impl_mangled, _) in sub_entries {
                    let wrapper_name =
                        MangledName(format!("$wrapper${}${}${}", sub_key, type_key, member_name));
                    let wrapper_func_idx = self.function_indices[&wrapper_name];
                    comp_insns.push(wasm_encoder::Instruction::RefFunc(wrapper_func_idx));
                }
                comp_insns.push(wasm_encoder::Instruction::StructNew(
                    self.vtable_type_indices[&sub_component_mn],
                ));
            }
            globals.global(
                GlobalType {
                    val_type: ValType::Ref(wasm_encoder::RefType {
                        nullable: false,
                        heap_type: wasm_encoder::HeapType::Concrete(component_vtable_idx),
                    }),
                    mutable: false,
                    shared: false,
                },
                &wasm_encoder::ConstExpr::extended(comp_insns),
            );
        }

        self.num_interface_vtable_globals = vtable_global_idx;
        self.interface_object_infos = interface_object_infos;
    }

    /// Emit vtable globals for class virtual dispatch.
    fn emit_class_vtable_globals(&mut self, globals: &mut GlobalSection) {
        if self.class_vtable_type_indices.is_empty() {
            return;
        }

        // Discover every (canonical_class, type_args) instance that needs a vtable global.
        // For non-generic classes, type_args is empty. For generic classes, each construction
        // site contributes one (class_mn, type_args) tuple.
        let instances = self.discover_class_vtable_instances();

        let num_user_globals = self.typed_module.globals.len() as u32;
        let num_trait_vtable_globals = self.num_interface_vtable_globals;
        let class_vtable_global_base =
            USER_GLOBAL_BASE + num_user_globals + num_trait_vtable_globals;
        let mut global_idx = 0u32;

        // Iterate in the same order as type emission so vtable global indices line up.
        let order = type_graph::emission_order(&self.typed_module.types);
        {
            for name in &order {
                if let TypeDef::Class(cls) = &self.typed_module.types[name] {
                    let cls = cls.clone();
                    let vtable_type_idx = self.class_vtable_type_indices[name];

                    // Find all (name, type_args) instances for this class.
                    // For non-generic classes (no type_params), use a single empty-type-args entry.
                    // For generic classes with no discovered instantiations, emit nothing —
                    // the class is dead in this build, no vtable needed.
                    let mut class_instances: Vec<Vec<Type>> = instances
                        .iter()
                        .filter(|(mn, _)| mn == name)
                        .map(|(_, type_args)| type_args.clone())
                        .collect();
                    if class_instances.is_empty() && cls.type_params.is_empty() {
                        class_instances.push(Vec::new());
                    }
                    if class_instances.is_empty() {
                        continue;
                    }

                    for type_args in class_instances {
                        // Skip instances whose methods weren't monomorphized — under full
                        // type erasure these vtable entries are unreachable (the instance
                        // appears only in some type position, never constructed at runtime).
                        // Common case post-Phase 6: deeply-nested class types like
                        // `Fiber<Fiber<Any, Never>, Never>` appear in `expr.ty` via parent-
                        // class type chains but are never `ClassNew`'d.
                        let all_methods_exist = cls.vtable_methods.iter().all(|slot| {
                            let slot_ta = substitute_slot_type_params(
                                &cls.type_params,
                                &type_args,
                                &slot.impl_type_params,
                            );
                            let concrete_mn =
                                MangledName::for_function(&slot.impl_fqn, &slot.param_types)
                                    .with_type_args(&slot_ta);
                            self.function_indices.contains_key(&concrete_mn)
                        });
                        if !all_methods_exist {
                            continue;
                        }

                        let vtable_global_idx = class_vtable_global_base + global_idx;
                        self.class_vtable_global_indices
                            .insert((name.clone(), type_args.clone()), vtable_global_idx);
                        global_idx += 1;

                        // Build constant expression: ref.func for each slot, then struct.new.
                        // Concrete function name = MangledName::for_function(impl_fqn, param_types)
                        // + with_type_args(type_args). For non-generic classes type_args is empty
                        // and `with_type_args` is identity.
                        let mut insns: Vec<wasm_encoder::Instruction> = Vec::new();
                        for slot in &cls.vtable_methods {
                            let slot_ta = substitute_slot_type_params(
                                &cls.type_params,
                                &type_args,
                                &slot.impl_type_params,
                            );
                            let concrete_mn =
                                MangledName::for_function(&slot.impl_fqn, &slot.param_types)
                                    .with_type_args(&slot_ta);
                            let func_idx = *self.function_indices.get(&concrete_mn).unwrap_or_else(|| {
                                panic!(
                                    "missing vtable function: {} method {} in class {} (type_args: {:?})",
                                    concrete_mn.0, slot.method_name.0, name.0, type_args,
                                )
                            });
                            if !self.class_vtable_impl_func_indices.contains(&func_idx) {
                                self.class_vtable_impl_func_indices.push(func_idx);
                            }
                            insns.push(wasm_encoder::Instruction::RefFunc(func_idx));
                        }
                        insns.push(wasm_encoder::Instruction::StructNew(vtable_type_idx));
                        let init_expr = wasm_encoder::ConstExpr::extended(insns);

                        let vtable_ref_type = ValType::Ref(wasm_encoder::RefType {
                            nullable: false,
                            heap_type: wasm_encoder::HeapType::Concrete(vtable_type_idx),
                        });

                        globals.global(
                            GlobalType {
                                val_type: vtable_ref_type,
                                mutable: false,
                                shared: false,
                            },
                            &init_expr,
                        );
                    }
                }
            }
        }
    }

    /// Discover every `Type::Function` arity used in the typed module. Walks every TypeDef
    /// field/payload type, every function signature + body, every global type + initializer,
    /// every test body, and every nested expression's static type. Each `Type::Function`
    /// contributes its `params.len()` to the set. Used by `emit_closure_arity_types` to
    /// allocate exactly the canonical `Closure_N`/`Func_N` types the program needs.
    fn discover_closure_arities(&self) -> std::collections::BTreeSet<u32> {
        let mut out: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        self.for_each_module_type(&mut |ty| {
            if let Type::Function(params, _) = ty {
                out.insert(params.len() as u32);
            }
        });
        out
    }

    /// Discover every flattened **tuple width** used in the module — the leaf count of every
    /// concrete (non-type-parameter) tuple type. Used by `emit_tuple_width_types` to allocate the
    /// shared `$Tuple_N` boxed structs. Over-discovery (e.g. a nested tuple's own width that is
    /// never boxed on its own) is harmless: an unused struct is valid and unreferenced.
    fn discover_tuple_widths(&self) -> std::collections::BTreeSet<u32> {
        /// Flattened leaf count of a concrete tuple (mirrors `type_to_valtypes`/`flatten_to_leaf_types`
        /// length without needing `&self`): a concrete tuple sums its elements' leaf counts; a newtype
        /// is transparent; a `Uint128` is two `i64` leaves; everything else is one leaf.
        fn leaf_count(ty: &Type) -> u32 {
            match ty {
                Type::Tuple(elems, _) => elems.iter().map(leaf_count).sum(),
                Type::Newtype(_, inner)
                | Type::GenericNewtype {
                    concrete_inner_type: inner,
                    ..
                } => leaf_count(inner),
                Type::Uint128 => 2,
                _ => 1,
            }
        }
        let mut out: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        self.for_each_module_type(&mut |ty| {
            if let Type::Tuple(..) = ty {
                out.insert(leaf_count(ty));
            }
        });
        out
    }

    /// Walk every `Type` node reachable in the typed module — every TypeDef field/payload type,
    /// every function signature + body, every global type + initializer, every test body, every
    /// class vtable slot signature, and every nested expression's static type — invoking `f` on
    /// each. Shared by `discover_closure_arities` and `discover_tuple_widths`.
    fn for_each_module_type(&self, f: &mut dyn FnMut(&Type)) {
        fn visit_type(ty: &Type, f: &mut dyn FnMut(&Type)) {
            f(ty);
            match ty {
                Type::Function(params, ret) => {
                    for p in params {
                        visit_type(p, f);
                    }
                    visit_type(ret, f);
                }
                Type::GenericClass { type_args, .. }
                | Type::GenericRecord { type_args, .. }
                | Type::GenericEnum { type_args, .. } => {
                    for (_, t) in type_args {
                        visit_type(t, f);
                    }
                }
                Type::Array(elem) => visit_type(elem, f),
                Type::Tuple(elems, _) => {
                    for e in elems {
                        visit_type(e, f);
                    }
                }
                Type::Newtype(_, inner)
                | Type::GenericNewtype {
                    concrete_inner_type: inner,
                    ..
                } => {
                    visit_type(inner, f);
                }
                Type::InterfaceObject { traits, .. } => {
                    for c in traits {
                        for t in &c.trait_type_args {
                            visit_type(t, f);
                        }
                    }
                }
                _ => {}
            }
        }

        fn visit_expr(e: &TypedExpr, f: &mut dyn FnMut(&Type)) {
            visit_type(&e.ty, f);
            use crate::typechecker::types::TypedExprKind as K;
            match &e.kind {
                K::Block(exprs) => exprs.iter().for_each(|c| visit_expr(c, f)),
                K::Let { value, var_ty, .. } => {
                    visit_type(var_ty, f);
                    visit_expr(value, f);
                }
                K::Assign { value, .. } => visit_expr(value, f),
                K::FunctionCall { args, .. }
                | K::IntrinsicCall { args, .. }
                | K::ClassNew { args, .. }
                | K::ClassSuperCall { args, .. }
                | K::ClassStructCreate { fields: args, .. }
                | K::EnumCreate { args, .. }
                | K::EnumVariantRecordCreate { args, .. }
                | K::ArrayLiteral { elements: args }
                | K::ClosureCall { args, .. }
                | K::ClassVirtualCall { args, .. }
                | K::InterfaceObjectMethodCall { args, .. } => {
                    args.iter().for_each(|a| visit_expr(a, f));
                    match &e.kind {
                        K::ClosureCall { callee, .. } => visit_expr(callee, f),
                        K::ClassVirtualCall { object, .. } => visit_expr(object, f),
                        K::InterfaceObjectMethodCall { receiver, .. } => visit_expr(receiver, f),
                        _ => {}
                    }
                }
                K::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    visit_expr(condition, f);
                    visit_expr(then_branch, f);
                    if let Some(e2) = else_branch {
                        visit_expr(e2, f);
                    }
                }
                K::While { condition, body } => {
                    visit_expr(condition, f);
                    visit_expr(body, f);
                }
                K::NewtypeCreate { value, .. }
                | K::NewtypeValue { value, .. }
                | K::TypeCast { value, .. }
                | K::TypeTest { value, .. }
                | K::BoxToAny { inner: value }
                | K::Panic { message: value }
                | K::Return { value, .. }
                | K::GlobalAssign { value, .. }
                | K::UnaryOp { operand: value, .. } => visit_expr(value, f),
                K::BinaryOp { left, right, .. } => {
                    visit_expr(left, f);
                    visit_expr(right, f);
                }
                K::FieldAccess { object, .. } | K::MethodRef { object, .. } => {
                    visit_expr(object, f)
                }
                K::FieldAssign { object, value, .. } => {
                    visit_expr(object, f);
                    visit_expr(value, f);
                }
                K::Match { subject, arms } => {
                    visit_expr(subject, f);
                    for arm in arms {
                        if let Some(g) = &arm.guard {
                            visit_expr(g, f);
                        }
                        visit_expr(&arm.body, f);
                    }
                }
                K::Closure {
                    body,
                    params,
                    captures,
                } => {
                    for p in params {
                        visit_type(&p.ty, f);
                    }
                    for c in captures {
                        visit_type(&c.ty, f);
                    }
                    visit_expr(body, f);
                }
                K::Assert { condition, message } => {
                    visit_expr(condition, f);
                    if let Some(m) = message {
                        visit_expr(m, f);
                    }
                }
                K::RecordCreate { fields, .. } => {
                    for (_, e2) in fields {
                        visit_expr(e2, f);
                    }
                }
                K::TupleLiteral { elements } => {
                    for e2 in elements {
                        visit_expr(e2, f);
                    }
                }
                K::RecordWith {
                    object, overrides, ..
                } => {
                    visit_expr(object, f);
                    for (_, _, e2) in overrides {
                        visit_expr(e2, f);
                    }
                }
                K::LetDestructure { value, .. } => visit_expr(value, f),
                K::InterfaceObjectCoerce { inner, .. }
                | K::TemplateInterfaceObjectCoerce { inner, .. }
                | K::InterfaceObjectUpcast { inner } => visit_expr(inner, f),
                K::ImplFunctionCall { args, .. } | K::ExtFunctionCall { args, .. } => {
                    args.iter().for_each(|a| visit_expr(a, f));
                }
                K::ForLoop { iterable, body, .. } => {
                    visit_expr(iterable, f);
                    visit_expr(body, f);
                }
                K::AsyncBlock { body, .. } => visit_expr(body, f),
                K::Try { operand, .. } | K::Await { operand, .. } => visit_expr(operand, f),
                _ => {}
            }
        }

        for td in self.typed_module.types.values() {
            match td {
                TypeDef::Record(r) => {
                    for (_, ty) in &r.fields {
                        visit_type(ty, f);
                    }
                }
                TypeDef::Enum(e) => {
                    for v in &e.variants {
                        for ty in &v.payload_types {
                            visit_type(ty, f);
                        }
                    }
                }
                TypeDef::Class(cls) => {
                    if let Some(parent) = &cls.parent_type {
                        visit_type(parent, f);
                    }
                    for fld in &cls.fields {
                        visit_type(&fld.ty, f);
                    }
                    for p in &cls.constructor_params {
                        visit_type(&p.ty, f);
                    }
                    for (_, ty) in &cls.initializer_fields {
                        visit_type(ty, f);
                    }
                    for stmt in &cls.initializer {
                        visit_expr(stmt, f);
                    }
                    if let Some(args) = &cls.extends_args {
                        for a in args {
                            visit_expr(a, f);
                        }
                    }
                }
                _ => {}
            }
        }
        for func in self.typed_module.functions.values() {
            for p in &func.params {
                visit_type(&p.ty, f);
            }
            visit_type(&func.return_type, f);
            visit_expr(&func.body, f);
        }
        for global in self.typed_module.globals.values() {
            visit_type(&global.ty, f);
            visit_expr(&global.initializer, f);
        }
        for test in &self.typed_module.tests {
            visit_expr(&test.body, f);
        }
        // Class vtable slot signatures — type-param-erased signatures contribute their
        // arity/width through the vtable funcref types.
        for td in self.typed_module.types.values() {
            if let TypeDef::Class(cls) = td {
                for slot in &cls.vtable_methods {
                    for p in &slot.param_types {
                        visit_type(p, f);
                    }
                    visit_type(&slot.return_type, f);
                }
            }
        }
    }

    /// Emit `Closure_N`/`Func_N` rec groups for every arity discovered. Each rec group is:
    /// ```text
    /// rec
    ///   Func_N    = (func (param anyref) ... N+1 times (result anyref))
    ///   Closure_N = (struct (field anyref) (field (ref Func_N)))
    /// ```
    /// Indices are recorded in `closure_arity_indices`. Called after the fixed types have been
    /// emitted and before user types start, so it consumes indices starting at `next_type_index`.
    fn emit_closure_arity_types(&mut self, group: &mut Vec<SubType>) {
        let arities = self.discover_closure_arities();
        // Env is nullable: closures with no captures pass `ref.null` here.
        let nullable_anyref = ValType::Ref(wasm_encoder::RefType {
            nullable: true,
            heap_type: wasm_encoder::HeapType::ANY,
        });
        // Regular params and return: non-null `(ref any)`. Dovetail values are never null at
        // runtime, and using non-null lets `struct.get`/`call_ref` results flow into other
        // non-null contexts without an extra ref.as_non_null.
        let any_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::ANY,
        });
        for arity in arities {
            let func_idx = self.next_type_index;
            let struct_idx = self.next_type_index + 1;
            self.next_type_index += 2;

            // Func_N: env (nullable anyref) + N non-null anyref params, returns non-null anyref.
            let mut params = vec![nullable_anyref];
            for _ in 0..arity {
                params.push(any_ref);
            }
            let closure_func_subtype = SubType {
                is_final: true,
                supertype_idx: None,
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Func(wasm_encoder::FuncType::new(
                        params,
                        vec![any_ref],
                    )),
                    shared: false,
                    describes: None,
                    descriptor: None,
                },
            };

            // Closure_N: (struct (field anyref-nullable env) (field (ref Func_N) funcref)).
            let closure_struct_subtype = SubType {
                is_final: true,
                supertype_idx: None,
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                        fields: Box::new([
                            wasm_encoder::FieldType {
                                element_type: wasm_encoder::StorageType::Val(nullable_anyref),
                                mutable: false,
                            },
                            wasm_encoder::FieldType {
                                element_type: wasm_encoder::StorageType::Val(ValType::Ref(
                                    wasm_encoder::RefType {
                                        nullable: false,
                                        heap_type: wasm_encoder::HeapType::Concrete(func_idx),
                                    },
                                )),
                                mutable: false,
                            },
                        ]),
                    }),
                    shared: false,
                    describes: None,
                    descriptor: None,
                },
            };

            group.push(closure_func_subtype);
            group.push(closure_struct_subtype);
            self.closure_arity_indices
                .insert(arity, (func_idx, struct_idx));
        }
    }

    /// Emit one shared boxed tuple struct per discovered flattened width:
    /// ```text
    /// $Tuple_N = (struct (field (ref any)) × N)
    /// ```
    /// All concrete tuples of the same leaf count share their `$Tuple_N`; a value is boxed by
    /// boxing each leaf to `anyref` then `struct.new`, and unboxed by N × `struct.get` + casting
    /// each leaf back. Indices are recorded in `tuple_width_indices`. Called after the fixed types
    /// and `emit_closure_arity_types`, before user types start, so it consumes indices starting at
    /// `next_type_index`.
    fn emit_tuple_width_types(&mut self, group: &mut Vec<SubType>) {
        let widths = self.discover_tuple_widths();
        // Non-null `(ref any)` fields: a boxed leaf (a `$Box$X` ref, a string/record ref, etc.) is
        // never null, matching how the values flows into other non-null contexts.
        let any_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::ANY,
        });
        for width in widths {
            let struct_idx = self.next_type_index;
            self.next_type_index += 1;
            // Fields are **mutable** so a `$Tuple_N` can double as the heap mut-box for a tuple
            // captured by a closure (reassignment writes the fields in place — see the mutable-
            // capture paths in `expressions.rs`). The struct is final and its fields are `anyref`
            // (the top type), so mutability costs no subtyping; value tuples simply never write them.
            let fields: Vec<wasm_encoder::FieldType> = (0..width)
                .map(|_| wasm_encoder::FieldType {
                    element_type: wasm_encoder::StorageType::Val(any_ref),
                    mutable: true,
                })
                .collect();
            group.push(SubType {
                is_final: true,
                supertype_idx: None,
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                        fields: fields.into_boxed_slice(),
                    }),
                    shared: false,
                    describes: None,
                    descriptor: None,
                },
            });
            self.tuple_width_indices.insert(width, struct_idx);
        }
    }

    /// Map each concrete monomorphized class-method function to the vtable slot func type
    /// index of the class it belongs to. After this pass, `virtual_method_func_types[fn_mn]`
    /// tells the function section + body emitter to type the function with the erased vtable
    /// slot signature, matching the funcref stored in each instantiation's vtable global.
    ///
    /// This replaces the earlier prefix-matching approach (`fn_mn.0.starts_with(&prefix)`)
    /// with explicit enumeration: for each class, for each instantiation, for each vtable
    /// slot, compute the concrete function name from the slot's `(impl_fqn, param_types)` +
    /// the instantiation's `type_args`.
    fn register_virtual_method_func_types(&mut self) {
        let instances = self.discover_class_vtable_instances();
        // For each class, accumulate its instantiations.
        let mut by_class: std::collections::HashMap<MangledName, Vec<Vec<Type>>> =
            Default::default();
        for (class_mn, type_args) in &instances {
            by_class
                .entry(class_mn.clone())
                .or_default()
                .push(type_args.clone());
        }

        for (class_mn, td) in &self.typed_module.types {
            let cls = match td {
                TypeDef::Class(cls) => cls,
                _ => continue,
            };
            // Non-generic classes without any expression-typed reference still need their
            // vtable methods registered. Treat "no discovered instantiation" as a single
            // empty-type-args instantiation for non-generic classes.
            let mut class_instances: Vec<Vec<Type>> =
                by_class.get(class_mn).cloned().unwrap_or_default();
            if class_instances.is_empty() && cls.type_params.is_empty() {
                class_instances.push(Vec::new());
            }
            if class_instances.is_empty() {
                continue;
            }

            for (slot_idx, slot) in cls.vtable_methods.iter().enumerate() {
                let func_type_idx =
                    self.class_vtable_slot_func_types[&(class_mn.clone(), slot_idx as u32)];
                for type_args in &class_instances {
                    let slot_ta = substitute_slot_type_params(
                        &cls.type_params,
                        type_args,
                        &slot.impl_type_params,
                    );
                    let concrete_mn = MangledName::for_function(&slot.impl_fqn, &slot.param_types)
                        .with_type_args(&slot_ta);
                    self.virtual_method_func_types
                        .insert(concrete_mn.clone(), func_type_idx);
                    self.virtual_method_slot_sigs.insert(
                        concrete_mn,
                        (slot.param_types.clone(), slot.return_type.clone()),
                    );
                }
            }
        }
    }

    /// Discover all (canonical_class_mangled_name, type_args) instances referenced anywhere
    /// in the module. Non-generic classes appear with empty type_args. Generic classes appear
    /// once per concrete instantiation seen at any ClassNew/ClassStructCreate site or as the
    /// type of any expression / parameter / global / field.
    fn discover_class_vtable_instances(&self) -> Vec<(MangledName, Vec<Type>)> {
        // Insertion-ordered dedup set: iteration order must be deterministic because it
        // decides vtable global index assignment (and therefore the emitted bytes).
        let mut out = OrderedInstances::default();

        fn visit_type(ty: &Type, out: &mut OrderedInstances) {
            match ty {
                Type::Class(_, mn) => {
                    out.insert((mn.clone(), Vec::new()));
                }
                Type::GenericClass {
                    mangled_name,
                    type_args,
                    ..
                } => {
                    if !type_args.iter().any(|(_, t)| t.contains_type_parameter()) {
                        let plain: Vec<Type> = type_args.iter().map(|(_, t)| t.clone()).collect();
                        out.insert((mangled_name.clone(), plain));
                    }
                    for (_, t) in type_args {
                        visit_type(t, out);
                    }
                }
                Type::GenericRecord { type_args, .. } | Type::GenericEnum { type_args, .. } => {
                    for (_, t) in type_args {
                        visit_type(t, out);
                    }
                }
                Type::Array(elem) => visit_type(elem, out),
                Type::Tuple(elems, _) => {
                    for e in elems {
                        visit_type(e, out);
                    }
                }
                Type::Function(params, ret) => {
                    for p in params {
                        visit_type(p, out);
                    }
                    visit_type(ret, out);
                }
                Type::Newtype(_, inner)
                | Type::GenericNewtype {
                    concrete_inner_type: inner,
                    ..
                } => {
                    visit_type(inner, out);
                }
                Type::InterfaceObject { traits, .. } => {
                    for c in traits {
                        for t in &c.trait_type_args {
                            visit_type(t, out);
                        }
                    }
                }
                _ => {}
            }
        }

        fn visit_expr(e: &TypedExpr, out: &mut OrderedInstances) {
            visit_type(&e.ty, out);
            // Recurse into immediate children; we only care about types touched by exprs.
            use crate::typechecker::types::TypedExprKind as K;
            match &e.kind {
                K::ClassNew {
                    mangled_name,
                    type_params,
                    ..
                }
                | K::ClassStructCreate {
                    target_mangled_name: mangled_name,
                    type_params,
                    ..
                } if !type_params.iter().any(Type::contains_type_parameter) => {
                    out.insert((mangled_name.clone(), type_params.clone()));
                }
                _ => {}
            }
            match &e.kind {
                K::Block(exprs) => exprs.iter().for_each(|c| visit_expr(c, out)),
                K::Let { value, var_ty, .. } => {
                    visit_type(var_ty, out);
                    visit_expr(value, out);
                }
                K::Assign { value, .. } => visit_expr(value, out),
                K::FunctionCall { args, .. }
                | K::IntrinsicCall { args, .. }
                | K::ClassNew { args, .. }
                | K::ClassSuperCall { args, .. }
                | K::ClassStructCreate { fields: args, .. }
                | K::EnumCreate { args, .. }
                | K::EnumVariantRecordCreate { args, .. }
                | K::ArrayLiteral { elements: args }
                | K::ClosureCall { args, .. }
                | K::ClassVirtualCall { args, .. }
                | K::InterfaceObjectMethodCall { args, .. } => {
                    args.iter().for_each(|a| visit_expr(a, out));
                    match &e.kind {
                        K::ClosureCall { callee, .. } => visit_expr(callee, out),
                        K::ClassVirtualCall { object, .. } => visit_expr(object, out),
                        K::InterfaceObjectMethodCall { receiver, .. } => visit_expr(receiver, out),
                        _ => {}
                    }
                }
                K::If {
                    condition,
                    then_branch,
                    else_branch,
                } => {
                    visit_expr(condition, out);
                    visit_expr(then_branch, out);
                    if let Some(e2) = else_branch {
                        visit_expr(e2, out);
                    }
                }
                K::While { condition, body } => {
                    visit_expr(condition, out);
                    visit_expr(body, out);
                }
                K::NewtypeCreate { value, .. }
                | K::NewtypeValue { value, .. }
                | K::TypeCast { value, .. }
                | K::TypeTest { value, .. }
                | K::BoxToAny { inner: value }
                | K::Panic { message: value }
                | K::Return { value, .. }
                | K::GlobalAssign { value, .. }
                | K::UnaryOp { operand: value, .. } => visit_expr(value, out),
                K::BinaryOp { left, right, .. } => {
                    visit_expr(left, out);
                    visit_expr(right, out);
                }
                K::FieldAccess { object, .. } | K::MethodRef { object, .. } => {
                    visit_expr(object, out)
                }
                K::FieldAssign { object, value, .. } => {
                    visit_expr(object, out);
                    visit_expr(value, out);
                }
                K::Match { subject, arms } => {
                    visit_expr(subject, out);
                    for arm in arms {
                        if let Some(g) = &arm.guard {
                            visit_expr(g, out);
                        }
                        visit_expr(&arm.body, out);
                    }
                }
                K::Closure { body, .. } => visit_expr(body, out),
                K::Assert { condition, message } => {
                    visit_expr(condition, out);
                    if let Some(m) = message {
                        visit_expr(m, out);
                    }
                }
                K::RecordCreate { fields, .. } => {
                    for (_, e2) in fields {
                        visit_expr(e2, out);
                    }
                }
                K::TupleLiteral { elements } => {
                    for e2 in elements {
                        visit_expr(e2, out);
                    }
                }
                K::RecordWith {
                    object, overrides, ..
                } => {
                    visit_expr(object, out);
                    for (_, _, e2) in overrides {
                        visit_expr(e2, out);
                    }
                }
                K::LetDestructure { value, .. } => visit_expr(value, out),
                K::InterfaceObjectCoerce { inner, .. }
                | K::TemplateInterfaceObjectCoerce { inner, .. }
                | K::InterfaceObjectUpcast { inner } => visit_expr(inner, out),
                K::ImplFunctionCall { args, .. } | K::ExtFunctionCall { args, .. } => {
                    args.iter().for_each(|a| visit_expr(a, out));
                }
                K::ForLoop { iterable, body, .. } => {
                    visit_expr(iterable, out);
                    visit_expr(body, out);
                }
                K::AsyncBlock { body, .. } => visit_expr(body, out),
                K::Try { operand, .. } | K::Await { operand, .. } => visit_expr(operand, out),
                _ => {}
            }
        }

        for func in self.typed_module.functions.values() {
            for p in &func.params {
                visit_type(&p.ty, &mut out);
            }
            visit_type(&func.return_type, &mut out);
            visit_expr(&func.body, &mut out);
        }
        for global in self.typed_module.globals.values() {
            visit_type(&global.ty, &mut out);
            visit_expr(&global.initializer, &mut out);
        }
        for test in &self.typed_module.tests {
            visit_expr(&test.body, &mut out);
        }
        // Class TypeDef parent_type / extends_args / initializer / fields contribute too.
        for td in self.typed_module.types.values() {
            if let TypeDef::Class(cls) = td {
                if let Some(parent) = &cls.parent_type {
                    visit_type(parent, &mut out);
                }
                if let Some(args) = &cls.extends_args {
                    for a in args {
                        visit_expr(a, &mut out);
                    }
                }
                for stmt in &cls.initializer {
                    visit_expr(stmt, &mut out);
                }
                for f in &cls.fields {
                    visit_type(&f.ty, &mut out);
                }
            }
        }
        out.items
    }

    /// Element section: declare functions used in ref.func (wrappers + class vtable impl methods).
    fn emit_element_section(&mut self) {
        // Collect all trait wrapper function indices
        let num_user_funcs = self.typed_module.functions.len() as u32;
        let wrapper_base = self.user_func_base() + num_user_funcs;
        let mut func_indices: Vec<u32> = (0..self.wrapper_funcs.len() as u32)
            .map(|i| wrapper_base + i)
            .collect();

        // Add class vtable impl method indices
        func_indices.extend_from_slice(&self.class_vtable_impl_func_indices);

        // Add lifted closure function indices (needed for ref.func)
        func_indices.extend_from_slice(&self.closure_func_indices);

        // Add ref trampoline function indices (needed for ref.func)
        func_indices.extend(self.ref_trampoline_indices.values().copied());

        if func_indices.is_empty() {
            return;
        }

        let mut elements = wasm_encoder::ElementSection::new();
        elements.declared(wasm_encoder::Elements::Functions(
            std::borrow::Cow::Borrowed(&func_indices),
        ));

        self.module.section(&elements);
    }

    /// Export section: cm32p2 names for WASI CLI component model.
    fn emit_export_section(&mut self) {
        let mut exports = ExportSection::new();

        // Legacy-mangled p3 exports: stackful async lifts (no callback).
        //
        // Stackful rather than callback-lifted because the core `run`/`test-n*`
        // bodies block on `waitable-set.wait` in the middle of their own control
        // flow: a callback lift must return to the host at every suspension
        // point and be re-entered, which the Dovetail runtime's scheduler loop
        // cannot be expressed as. The consequence is that the export's value
        // does NOT come back as a core return: it is handed over with
        // `task.return` before the body ends, and the host must have
        // `wasm_component_model_async_stackful` enabled or the component will
        // not instantiate (see `p3::configure_p3_engine`).
        exports.export(
            &format!(
                "[async-lift-stackful]wasi:cli/run@{}#run",
                component::P3_VERSION
            ),
            ExportKind::Func,
            self.func_run(),
        );
        exports.export("cabi_realloc", ExportKind::Func, self.func_realloc());
        exports.export("_initialize", ExportKind::Func, self.func_initialize());
        exports.export("memory", ExportKind::Memory, 0);

        // Test wrapper exports: stackful async lifts.
        for (i, _) in self.test_func_indices.iter().enumerate() {
            exports.export(
                &format!("[async-lift-stackful]test-n{i}"),
                ExportKind::Func,
                self.test_wrapper_base + i as u32,
            );
        }

        self.module.section(&exports);
    }

    /// Code section: runtime function bodies + user function bodies.
    fn build_code_section(&mut self) -> CodeSection {
        let mut codes = CodeSection::new();

        // Compute the function count and its LEB128 size for DWARF address offset.
        // DWARF addresses must be relative to code section content start, which begins
        // with the function count LEB128 that CodeSection::byte_len() doesn't include.
        let total_func_count = NUM_RUNTIME_FUNCS as usize
            + self.typed_module.functions.len()
            + self.wrapper_funcs.len()
            + self.closure_infos.len()
            + self.ref_trampolines.len()
            + self.test_func_indices.len();
        let func_count_leb_size = leb128_u32_size(total_func_count as u32);

        // Helper: add a runtime function and skip its bytes (no debug info needed)
        macro_rules! add_runtime_func {
            ($codes:expr, $func:expr) => {
                $codes.function(&$func);
            };
        }

        // run: call main, convert Unit result (0) to WASI result (0=Ok, 1=Err)
        add_runtime_func!(codes, self.generate_run_body());

        // realloc: bump allocator
        add_runtime_func!(
            codes,
            realloc::generate_realloc(self.func_pinned_alloc(), self.func_pinned_free())
        );

        // initialize: evaluate global initializers
        let (init_func, _) = self.generate_initialize_body();
        add_runtime_func!(codes, init_func);

        // string_eq: byte-by-byte string comparison
        add_runtime_func!(codes, string_functions::generate_string_eq());

        // string_concat: allocate + copy
        add_runtime_func!(codes, string_functions::generate_string_concat());

        // string_cmp: lexicographic byte comparison
        add_runtime_func!(codes, string_functions::generate_string_cmp());

        // char_to_string
        add_runtime_func!(codes, string_functions::generate_char_to_string());

        // string_from_bytes
        add_runtime_func!(codes, string_functions::generate_string_from_bytes());

        // string_get_char
        add_runtime_func!(codes, string_functions::generate_string_get_char());

        // debug_print
        add_runtime_func!(
            codes,
            string_functions::generate_debug_print(
                self.func_pinned_alloc(),
                self.func_pinned_free()
            )
        );

        // panic_with_message
        add_runtime_func!(
            codes,
            string_functions::generate_panic_with_message(self.func_pinned_alloc())
        );

        // console_print (stdout, no newline)
        add_runtime_func!(
            codes,
            string_functions::generate_console_print(
                self.func_pinned_alloc(),
                self.func_pinned_free()
            )
        );

        // console_eprint (stderr, no newline)
        add_runtime_func!(
            codes,
            string_functions::generate_console_eprint(
                self.func_pinned_alloc(),
                self.func_pinned_free()
            )
        );

        // console_eprintln (stderr, with newline)
        add_runtime_func!(
            codes,
            string_functions::generate_console_eprintln(
                self.func_pinned_alloc(),
                self.func_pinned_free()
            )
        );

        // pinned allocator
        add_runtime_func!(codes, realloc::generate_pinned_alloc());
        add_runtime_func!(codes, realloc::generate_pinned_free());

        // User functions
        for (func_name, func) in &self.typed_module.functions {
            // WIT-import binding: replace the generated stub body with a
            // canonical-ABI lower/call/lift against the imported interface.
            if let Some(wit_ref) = self.wit_imports.table.funcs.get(func_name) {
                let mut emitter = function_emitter::FunctionEmitter::new(&func.params, None, self);
                emitter.emit_wit_import_body(wit_ref, &func.params, &func.return_type);
                let (wasm_func, debug_info) = emitter.build();
                let before = codes.byte_len();
                codes.function(&wasm_func);
                let entry_size = codes.byte_len() - before;
                let body_offset_in_entry = entry_size - debug_info.body_byte_len;
                let code_section_offset =
                    (func_count_leb_size + before + body_offset_in_entry) as u32;
                self.debug_infos.push((code_section_offset, debug_info));
                continue;
            }
            // Every function flattens tuple params (direct-call *and* vtable ABI). A vtable method
            // uses the erased slot signature for its param indices.
            let slot_sig = self.virtual_method_slot_sigs.get(func_name).cloned();
            let mut emitter = function_emitter::FunctionEmitter::new(
                &func.params,
                slot_sig.as_ref().map(|(p, _)| p.as_slice()),
                self,
            );
            if let Some(ref vst) = func.vtable_self_type
                && let Some(first_param) = func.params.first()
                && &first_param.ty != vst
            {
                let concrete_type_idx = self.wasm_type_index_for_class(&first_param.ty);
                let concrete_valtype = self.single_val_type(&first_param.ty);
                emitter.emit_self_cast_prologue(concrete_valtype, concrete_type_idx);
            }
            // For a monomorphized class method emitted with the erased vtable slot signature, any
            // partially-erased non-self param (a type-param scalar, or a tuple with type-param
            // leaves) arrives in the slot's (anyref-bearing) layout; coerce each back to the body's
            // concrete layout. A no-op for fully-concrete params.
            if let Some((slot_params, _slot_ret)) = &slot_sig {
                emitter.emit_param_coerce_prologue(slot_params, &func.params);
            }
            // The body produces its value in `func.return_type`'s representation: a flattened values
            // for a concrete tuple (matching the multi-value result signature), a single value
            // otherwise.
            emitter.emit_body(&func.body, &func.return_type);

            // Coerce the body's concrete return value into the vtable slot's (possibly erased)
            // return layout — a no-op for a concrete return, boxing erased leaves otherwise.
            if let Some((_slot_params, slot_ret)) = &slot_sig {
                emitter.coerce_value(&func.return_type, slot_ret);
            }
            let (wasm_func, debug_info) = emitter.build();
            let before = codes.byte_len();
            codes.function(&wasm_func);
            let entry_size = codes.byte_len() - before;
            // The body starts after the LEB128 size prefix within the entry.
            // Add func_count_leb_size because CodeSection::byte_len() doesn't include
            // the function count LEB128, but DWARF addresses are relative to the start
            // of the code section content (which begins with the function count).
            let body_offset_in_entry = entry_size - debug_info.body_byte_len;
            let code_section_offset = (func_count_leb_size + before + body_offset_in_entry) as u32;
            self.debug_infos.push((code_section_offset, debug_info));
        }

        // Wrapper functions for interface object vtable dispatch
        for i in 0..self.wrapper_funcs.len() {
            let (wasm_func, debug_info) = self.build_wrapper_function(&self.wrapper_funcs[i]);
            let before = codes.byte_len();
            codes.function(&wasm_func);
            let entry_size = codes.byte_len() - before;
            let body_offset_in_entry = entry_size - debug_info.body_byte_len;
            let code_section_offset = (func_count_leb_size + before + body_offset_in_entry) as u32;
            self.debug_infos.push((code_section_offset, debug_info));
        }

        // Lifted closure function bodies — collect debug info
        for closure_id in 0..self.closure_infos.len() {
            let params = self.closure_infos[closure_id].params.clone();
            let captures = self.closure_infos[closure_id].captures.clone();
            let body = self.closure_infos[closure_id].body.clone();
            let return_type = self.closure_infos[closure_id].return_type.clone();
            let env_type_index = self.closure_env_type_indices[closure_id];

            // Under always-erased closures, the WASM function signature is Func_N (all anyref).
            // Prologue casts env to concrete capture struct (if any captures), then casts each
            // declared param from anyref to its declared type. Body emits in the declared
            // return type. Epilogue boxes the body's value back to anyref for the WASM return.
            let mut emitter = function_emitter::FunctionEmitter::new_for_closure(&params, self);
            if let Some(env_idx) = env_type_index {
                emitter.emit_closure_env_prologue(env_idx, &captures);
            }
            emitter.emit_closure_param_prologue(&params);
            emitter.emit_closure_body(&body, &return_type);
            let (wasm_func, debug_info) = emitter.build();
            let before = codes.byte_len();
            codes.function(&wasm_func);
            let entry_size = codes.byte_len() - before;
            let body_offset_in_entry = entry_size - debug_info.body_byte_len;
            let code_section_offset = (before + body_offset_in_entry) as u32;
            self.debug_infos.push((code_section_offset, debug_info));
        }

        // Ref trampoline function bodies
        for i in 0..self.ref_trampolines.len() {
            let tramp = &self.ref_trampolines[i];
            codes.function(&self.build_ref_trampoline(tramp));
        }

        // Test wrapper function bodies (stackful, () -> ()): call the test,
        // deliver completion via this test's [task-return] import, return.
        for (i, &test_func_idx) in self.test_func_indices.iter().enumerate() {
            let mut f = wasm_encoder::Function::new(vec![]);
            f.instruction(&wasm_encoder::Instruction::Call(test_func_idx));
            f.instruction(&wasm_encoder::Instruction::Drop);
            f.instruction(&wasm_encoder::Instruction::Call(
                self.func_task_return_test(i as u32),
            ));
            f.instruction(&wasm_encoder::Instruction::End);
            codes.function(&f);
        }

        codes
    }

    /// Build a wrapper function for interface object vtable dispatch.
    ///
    /// The wrapper receives `(anyref, non_self_params...)` and:
    /// 1. Casts anyref back to the concrete type (ref.cast for ref types, or unbox for primitives)
    /// 2. Passes all params to the impl method
    /// 3. Returns the result
    ///
    /// Build an interface-object dispatch wrapper. Its WASM params are the erased vtable-slot signature
    /// (anyref self + erased slot params); the body bridges to the concrete impl ABI. Built with a
    /// `FunctionEmitter` so `coerce_value` handles every param/return shape — including a
    /// partially-erased tuple param like `(Int32, T)` (a width-changing per-element coercion).
    fn build_wrapper_function(
        &self,
        wrapper: &WrapperFunc,
    ) -> (wasm_encoder::Function, FunctionDebugInfo) {
        let impl_func_idx = self.function_indices[&wrapper.impl_method_mangled];
        let mut emitter = function_emitter::FunctionEmitter::new(&[], None, self);
        emitter.self_return_group_key = Some(wrapper.group_key.clone());
        emitter.emit_interface_object_wrapper(
            &wrapper.concrete_type,
            &wrapper.params,
            impl_func_idx,
            &wrapper.concrete_return,
            &wrapper.slot_return,
        );
        emitter.build()
    }

    /// Build a trampoline function for FunctionRef/MethodRef.
    ///
    /// FunctionRef: ignores env (param 0), forwards params 1..N to target.
    /// MethodRef: casts env (param 0) from anyref to self type, forwards (self, params 1..N) to target.
    fn build_ref_trampoline(&self, tramp: &RefTrampoline) -> wasm_encoder::Function {
        // WASM signature is Func_N: (anyref env, anyref ×N) -> anyref. The trampoline:
        // - For MethodRef, casts env (param 0) → self type and pushes it as the call's first arg.
        // - For FunctionRef, env is null and ignored.
        // - For each non-self param at WASM index i+1, casts anyref → declared type.
        // - Calls the target function (concrete signature).
        // - Phase 2: if the target returns a concrete tuple it now does so as a multi-value sequence;
        //   rebox it into (ref $Tuple) before the box-to-anyref. Temp locals for that rebox sit
        //   after the (1 + arity) anyref params and must be declared up front.
        // The target uses the boxed (vtable slot) signature iff it is a registered vtable method;
        // otherwise it uses the direct-call ABI with flattened tuple params, so each boxed anyref
        // tuple param must be cast and exploded into its flattened values before the call. Temp locals (param
        // unbox + return rebox) sit after the `1 + arity` anyref params.
        let boxed_params = self
            .virtual_method_slot_sigs
            .contains_key(&tramp.target_mangled);
        let mut local_decls: Vec<(u32, ValType)> = vec![];
        let mut next_local = 1 + tramp.param_types.len() as u32;

        // Plan per-param unbox (non-vtable flattened targets only).
        let mut param_unbox: Vec<Option<(u32, Vec<wasm_encoder::Instruction<'static>>)>> =
            Vec::new();
        for declared_ty in &tramp.param_types {
            if !boxed_params && self.is_tuple(declared_ty) {
                let base = next_local;
                let (instrs, temps) = self.tuple_unbox_instrs(declared_ty, base);
                for t in &temps {
                    local_decls.push((1, *t));
                }
                next_local += temps.len() as u32;
                param_unbox.push(Some((base, instrs)));
            } else if !boxed_params && self.is_uint128(declared_ty) {
                // A flattened `Uint128` param: cast the boxed anyref to `(ref $Uint128)` then explode.
                let base = next_local;
                let (instrs, temps) = self.uint128_unbox_instrs(base);
                for t in &temps {
                    local_decls.push((1, *t));
                }
                next_local += temps.len() as u32;
                param_unbox.push(Some((base, instrs)));
            } else {
                param_unbox.push(None);
            }
        }

        // The return rebox now boxes each leaf to anyref via N spill locals (the `$Tuple_N` has
        // anyref fields); declare them after the param-unbox temps.
        let needs_rebox = self.is_tuple(&tramp.return_type);
        let rebox_base = next_local;
        if needs_rebox {
            let (_instrs, temps) = self.tuple_rebox_instrs(&tramp.return_type, rebox_base);
            for t in &temps {
                local_decls.push((1, *t));
            }
        }
        let mut f = wasm_encoder::Function::new(local_decls);

        if let Some(ref self_type) = tramp.self_type {
            f.instruction(&wasm_encoder::Instruction::LocalGet(0));
            self.emit_cast_back_const(&mut f, self_type);
        }

        for (i, declared_ty) in tramp.param_types.iter().enumerate() {
            f.instruction(&wasm_encoder::Instruction::LocalGet((i + 1) as u32));
            if let Some((_base, instrs)) = &param_unbox[i] {
                // Flattened tuple param: cast the boxed anyref to `(ref $Tuple)`, then explode.
                let type_idx = self.wasm_type_index_for_any_cast(declared_ty);
                f.instruction(&wasm_encoder::Instruction::RefCastNonNull(
                    wasm_encoder::HeapType::Concrete(type_idx),
                ));
                for inst in instrs {
                    f.instruction(inst);
                }
            } else {
                self.emit_cast_back_const(&mut f, declared_ty);
            }
        }

        let target_idx = self.function_indices[&tramp.target_mangled];
        f.instruction(&wasm_encoder::Instruction::Call(target_idx));

        // Rebox a multi-value tuple return into a single `(ref $Tuple_N)`: spill the values into the
        // pre-declared `rebox_base` locals, box each leaf to anyref, then `struct.new`.
        if needs_rebox {
            let (instrs, _temps) = self.tuple_rebox_instrs(&tramp.return_type, rebox_base);
            for inst in &instrs {
                f.instruction(inst);
            }
        }

        // Box return to anyref (a no-op upcast for the reboxed struct ref).
        self.emit_box_to_any_const(&mut f, &tramp.return_type);

        f.instruction(&wasm_encoder::Instruction::End);
        f
    }

    /// `emit_cast_back_from_any` analogue for `wasm_encoder::Function` const-init contexts
    /// (trampolines etc., which build a `Function` directly without going through the
    /// `FunctionEmitter` instruction stream).
    fn emit_cast_back_const(&self, f: &mut wasm_encoder::Function, target_ty: &Type) {
        if matches!(
            target_ty,
            Type::Any | Type::TypeVariable(..) | Type::GenericParam(..)
        ) {
            return;
        }
        if matches!(target_ty, Type::Never | Type::Error) {
            f.instruction(&wasm_encoder::Instruction::Unreachable);
            return;
        }
        let type_idx = self.wasm_type_index_for_any_cast(target_ty);
        f.instruction(&wasm_encoder::Instruction::RefCastNonNull(
            wasm_encoder::HeapType::Concrete(type_idx),
        ));
        // Primitives unbox (struct.get field 0). Reference types — and tuples / `Uint128`, which a
        // boxed-param vtable target receives as their boxed `(ref $Tuple_N)` / `(ref $Uint128)` —
        // keep the ref.
        if !target_ty.is_reference_type()
            && !self.is_tuple(target_ty)
            && !self.is_uint128(target_ty)
        {
            f.instruction(&wasm_encoder::Instruction::StructGet {
                struct_type_index: type_idx,
                field_index: 0,
            });
        }
    }

    /// `emit_box_to_any` analogue for `wasm_encoder::Function` direct emission.
    fn emit_box_to_any_const(&self, f: &mut wasm_encoder::Function, value_ty: &Type) {
        if matches!(
            value_ty,
            Type::Any | Type::TypeVariable(..) | Type::GenericParam(..)
        ) {
            return;
        }
        if matches!(value_ty, Type::Never | Type::Error) {
            f.instruction(&wasm_encoder::Instruction::Unreachable);
            return;
        }
        // A tuple value reaches this `_const` path already reboxed into `(ref $Tuple_N)` by
        // the caller (the trampoline's `needs_rebox` block); that ref upcasts to anyref for
        // free, so there is nothing to emit.
        if self.is_tuple(value_ty) {
            return;
        }
        if value_ty.is_reference_type() {
            return;
        }
        let box_idx = self.box_type_index_for(value_ty);
        f.instruction(&wasm_encoder::Instruction::StructNew(box_idx));
    }

    /// Generate the `run` function body: call main, return 0 (Ok).
    /// The WASI CLI run interface expects: 0 = Ok (success), 1 = Err (failure).
    /// Dovetail's main() returns Unit (i32 0), so we call main and return 0.
    /// In test mode ($test$ functions present), run is a no-op — tests are called
    /// individually by the host via their own exports.
    fn generate_run_body(&self) -> wasm_encoder::Function {
        // Stackful async lift: the core `run` function has signature `() -> ()`.
        // It runs main to completion (blocking on `waitable-set.wait` inside
        // the runtime as needed — legal because the export is async-typed),
        // delivers the result via `task.return`, then returns normally.
        let mut f = wasm_encoder::Function::new(vec![]);
        if self.test_func_indices.is_empty()
            && let Some(main_idx) = self.main_func_index
        {
            f.instruction(&wasm_encoder::Instruction::Call(main_idx));
            f.instruction(&wasm_encoder::Instruction::Drop);
        }
        // Deliver the run result (0 = ok discriminant) and finish the task.
        f.instruction(&wasm_encoder::Instruction::I32Const(0));
        f.instruction(&wasm_encoder::Instruction::Call(
            self.func_task_return_run(),
        ));
        f.instruction(&wasm_encoder::Instruction::End);
        f
    }

    /// Generate the `initialize` function body.
    /// Creates the emitter and delegates to per-concern initialization methods.
    fn generate_initialize_body(&self) -> (wasm_encoder::Function, FunctionDebugInfo) {
        let mut emitter = function_emitter::FunctionEmitter::new(&[], None, self);

        self.emit_global_initializers(&mut emitter);

        emitter.build()
    }
}

fn validate_tuple_projections(module: &TypedModule) -> Result<(), CodeGenError> {
    fn unresolved(expr: &TypedExpr) -> Option<CodeGenError> {
        if matches!(
            &expr.kind,
            TypedExprKind::IntrinsicCall {
                intrinsic: crate::typechecker::types::IntrinsicKind::TupleProjection(_),
                ..
            }
        ) {
            return Some(CodeGenError {
                message: format!(
                    "unresolved tuple accessor on '{}' at {}:{}; tuple shape must be known before code generation",
                    expr.ty, expr.span.file, expr.span.line
                ),
            });
        }
        let mut error = None;
        crate::monomorphize::visit_expr_children(expr, |child| {
            if error.is_none() {
                error = unresolved(child);
            }
        });
        error
    }
    for function in module
        .functions
        .values()
        .filter(|f| f.type_params.is_empty())
    {
        if let Some(error) = unresolved(&function.body) {
            return Err(error);
        }
    }
    for global in module.globals.values().filter(|g| g.type_params.is_empty()) {
        if let Some(error) = unresolved(&global.initializer) {
            return Err(error);
        }
    }
    Ok(())
}

/// Generate a WASM component from the typed module.
///
/// First generates a core module with WASI CLI imports/exports,
/// then wraps it into a WASM component targeting the WASI CLI command world.
/// In test mode, each test gets its own export in the component.
pub fn generate_component(
    typed_module: &TypedModule,
    registry: &crate::typechecker::registry::Registry,
    test_exports: &[TestExportInfo],
    wit_imports: &WitImportUniverse,
    component_bytes: &[(String, Vec<u8>)],
) -> Result<Vec<u8>, CodeGenError> {
    validate_tuple_projections(typed_module)?;
    let codegen = Codegen::new(typed_module, wit_imports, registry);
    let core_bytes = codegen.generate();
    let component = component::encode_p3_with_imports(
        core_bytes,
        if test_exports.is_empty() {
            None
        } else {
            Some(test_exports.len())
        },
        wit_imports,
    )?;
    // When component dependencies are declared, plug their binaries into the
    // just-emitted imports so the produced component imports only WASI.
    if component_bytes.is_empty() {
        Ok(component)
    } else {
        component::compose_components(component, component_bytes)
    }
}

/// Generate core WASM module bytes (without component wrapping).
/// Useful for debugging and testing.
#[cfg(test)]
pub fn generate_core_module(
    typed_module: &TypedModule,
    registry: &crate::typechecker::registry::Registry,
) -> Vec<u8> {
    let empty = WitImportUniverse::empty();
    let codegen = Codegen::new(typed_module, &empty, registry);
    codegen.generate()
}

#[cfg(test)]
mod slice_tests;

#[cfg(test)]
mod tests {
    /// Validate with the component-model async feature set the p3 output uses.
    fn validate_p3(wasm: &[u8]) -> Result<(), wasmparser::BinaryReaderError> {
        let mut validator =
            wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all());
        validator.validate_all(wasm).map(|_| ())
    }

    use super::*;
    use crate::common::span::FilePath;
    use crate::common::types::PackagePath;
    use crate::layout::LayoutFilter;
    use crate::lexer::Lexer;
    use crate::lexer::attach_doc_comments;
    use crate::parser::Parser;
    use crate::parser::ast::PackageAst;
    use crate::typechecker::registry::Registry;

    fn compile(source: &str) -> Vec<u8> {
        let mut lexer = Lexer::new(source, FilePath::from("test.dove"));
        let tokens = lexer.tokenize();
        let tokens = attach_doc_comments(tokens);
        let mut filter = LayoutFilter::new(tokens);
        let filtered = filter.filter();
        let mut parser = Parser::new(filtered);
        let source_file = parser.parse_source_file();
        let package_path = PackagePath(
            source_file
                .package
                .path
                .iter()
                .map(|s| s.value.clone())
                .collect(),
        );
        let package_ast = PackageAst {
            package_path: package_path.clone(),
            files: vec![source_file],
        };
        let base_registry = Registry::new();
        let mut result = crate::typechecker::typecheck(&package_ast, &base_registry);
        assert!(!result.has_errors());
        let mut diagnostics = crate::common::diagnostics::Diagnostics::new();
        let main_fqn = crate::typechecker::rules::resolve_main_function(
            &result.typed_module,
            None,
            &package_path,
            &mut diagnostics,
        );
        result.typed_module.main_function_fqn = main_fqn;

        let empty = WitImportUniverse::empty();
        generate_component(&result.typed_module, &result.registry, &[], &empty, &[])
            .expect("component generation failed")
    }

    /// Build a `Type::Tuple` from element types, computing its mangled name the same
    /// way inference does.
    fn tuple(elems: Vec<Type>) -> Type {
        let mn = MangledName::for_tuple(&elems);
        Type::Tuple(elems, mn)
    }

    /// Flatten a type with an empty module — scalar-leaf tuples never touch
    /// `type_indices`, so no type registration is required.
    fn flatten(ty: &Type) -> Vec<ValType> {
        let module = TypedModule::empty();
        let empty = WitImportUniverse::empty();
        let registry = Registry::new();
        let codegen = Codegen::new(&module, &empty, &registry);
        codegen.type_to_valtypes(ty).into_vec()
    }

    #[test]
    fn type_to_valtypes_scalar_is_width_one() {
        assert_eq!(flatten(&Type::Int32), vec![ValType::I32]);
        assert_eq!(flatten(&Type::Int64), vec![ValType::I64]);
        assert_eq!(flatten(&Type::Float64), vec![ValType::F64]);
    }

    #[test]
    fn type_to_valtypes_flat_pair() {
        assert_eq!(
            flatten(&tuple(vec![Type::Int32, Type::Bool])),
            vec![ValType::I32, ValType::I32]
        );
    }

    #[test]
    fn type_to_valtypes_mixed_widths() {
        assert_eq!(
            flatten(&tuple(vec![Type::Int64, Type::Float64])),
            vec![ValType::I64, ValType::F64]
        );
    }

    #[test]
    fn type_to_valtypes_nested_is_transitive() {
        let nested = tuple(vec![tuple(vec![Type::Int32, Type::Bool]), Type::Int64]);
        assert_eq!(
            flatten(&nested),
            vec![ValType::I32, ValType::I32, ValType::I64]
        );
    }

    #[test]
    fn type_to_valtypes_deeply_nested() {
        let deep = tuple(vec![
            Type::Int32,
            tuple(vec![Type::Bool, tuple(vec![Type::Int64, Type::Float32])]),
        ]);
        assert_eq!(
            flatten(&deep),
            vec![ValType::I32, ValType::I32, ValType::I64, ValType::F32]
        );
    }

    #[test]
    fn type_to_valtypes_newtype_over_tuple_is_transparent() {
        let inner = tuple(vec![Type::Int32, Type::Bool]);
        let nt = Type::Newtype(
            crate::common::types::Fqn {
                package: PackagePath(vec!["test".to_string()]),
                symbol: crate::common::types::SymbolName("Pair".to_string()),
            },
            Box::new(inner.clone()),
        );
        assert_eq!(flatten(&nt), flatten(&inner));
    }

    #[test]
    fn type_to_valtypes_uint128_is_two_i64() {
        // `Uint128` is the smallest non-tuple flattened value: a width-2 `[i64, i64]` (lo, hi) run.
        assert_eq!(flatten(&Type::Uint128), vec![ValType::I64, ValType::I64]);
        // Inside a tuple it flattens transitively: `(Uint128, Int32)` → `[i64, i64, i32]`.
        assert_eq!(
            flatten(&tuple(vec![Type::Uint128, Type::Int32])),
            vec![ValType::I64, ValType::I64, ValType::I32]
        );
    }

    #[test]
    fn type_to_valtypes_tuple_with_type_param_flattens() {
        // Every tuple flattens now: a type-parameter leaf lowers to `anyref`, so `(Int32, T)`
        // is the two-value sequence `[i32, anyref]` (boxed form is the shared `$Tuple_2`).
        let param = Type::GenericParam(
            crate::common::types::TypeParamName("T".to_string()),
            vec![],
            0,
        );
        let flat = flatten(&tuple(vec![Type::Int32, param]));
        let any_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::ANY,
        });
        assert_eq!(flat, vec![ValType::I32, any_ref]);
    }

    #[test]
    fn test_generate_valid_wasm() {
        let wasm = compile("package a\n\nfunction main(): Unit = ()");
        // Validate that the output is a valid WASM component
        validate_p3(&wasm).expect("produced invalid WASM component");
    }

    #[test]
    fn test_forward_reference_valid_wasm() {
        let wasm = compile(
            r#"package a

record Line =
    start: Point
    end: Point

record Point =
    x: Int32
    y: Int32

function main(): Unit = ()"#,
        );
        validate_p3(&wasm).expect("forward reference produced invalid WASM");
    }

    #[test]
    fn test_mutually_recursive_records_valid_wasm() {
        let wasm = compile(
            r#"package a

record Foo =
    value: Int32
    bar: Bar

record Bar =
    value: Int32
    foo: Foo

function main(): Unit = ()"#,
        );
        validate_p3(&wasm).expect("mutually recursive records produced invalid WASM");
    }

    #[test]
    fn test_self_referential_record_valid_wasm() {
        let wasm = compile(
            r#"package a

record Node =
    value: Int32
    next: Node

function main(): Unit = ()"#,
        );
        validate_p3(&wasm).expect("self-referential record produced invalid WASM");
    }

    #[test]
    fn test_string_literal_valid_wasm() {
        let wasm = compile(
            r#"package a

function main(): Unit =
    let s: String = "hello"
    ()"#,
        );
        validate_p3(&wasm).expect("string literal produced invalid WASM");
    }

    /// Compile with prelude support and return core module bytes (not component).
    pub(super) fn compile_core_with_prelude(source: &str) -> Vec<u8> {
        let file: FilePath = std::sync::Arc::from("test.dove");
        let mut lexer = Lexer::new(source, file);
        let tokens = lexer.tokenize();
        let tokens = attach_doc_comments(tokens);
        let mut filter = LayoutFilter::new(tokens);
        let filtered = filter.filter();
        let mut parser = Parser::new(filtered);
        let source_file = parser.parse_source_file();
        let package_path = PackagePath(
            source_file
                .package
                .path
                .iter()
                .map(|s| s.value.clone())
                .collect(),
        );
        let package_ast = PackageAst {
            package_path: package_path.clone(),
            files: vec![source_file],
        };
        let prelude = &*crate::compiler::PRELUDE_OUTPUT;
        let base_registry = prelude.1.clone();
        let mut tc_result = crate::typechecker::typecheck(&package_ast, &base_registry);
        assert!(
            !tc_result.has_errors(),
            "typecheck failed: {:?}",
            tc_result.diagnostics.iter().collect::<Vec<_>>()
        );
        let mut diagnostics = crate::common::diagnostics::Diagnostics::new();
        let main_fqn = crate::typechecker::rules::resolve_main_function(
            &tc_result.typed_module,
            None,
            &package_path,
            &mut diagnostics,
        );
        tc_result.typed_module.main_function_fqn = main_fqn;
        tc_result.typed_module.merge_from(prelude.2.clone());
        crate::desugar::desugar_all(&mut tc_result.typed_module);

        let merged_registry = base_registry.merge(&tc_result.registry);
        let mut mono_module =
            crate::monomorphize::monomorphize(tc_result.typed_module, &merged_registry)
                .expect("monomorphize");
        crate::desugar::coerce_and_capture(&mut mono_module);
        crate::coerce::elaborate_coercions(&mut mono_module, &merged_registry);
        crate::monomorphize::ensure_vtable_functions(&mut mono_module, &merged_registry)
            .expect("materialize interface implementations");
        generate_core_module(&mono_module, &merged_registry)
    }

    #[test]
    fn test_nested_closure_valid_core_wasm() {
        let core_bytes = compile_core_with_prelude(
            r#"
package a

function main(): Unit =
    let x = 100
    let outer: Unit => Int32 => Int32 = (_: Unit) =>
        (z: Int32) => x + z
    let inner = outer(())
    assert inner(1) == 101
"#,
        );
        wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
            .validate_all(&core_bytes)
            .expect("nested closure core module validation failed");
    }

    #[test]
    fn test_name_section_present_with_function_names() {
        let core_bytes = compile_core_with_prelude(
            r#"package a

function add(x: Int32, y: Int32): Int32 = x + y

function main(): Unit =
    let _ = add(1, 2)
    ()"#,
        );

        // Parse the core module and extract function names from the name section
        let mut func_names: Vec<(u32, String)> = Vec::new();
        for payload in wasmparser::Parser::new(0).parse_all(&core_bytes) {
            let payload = payload.expect("valid payload");
            if let wasmparser::Payload::CustomSection(reader) = payload
                && reader.name() == "name"
                && let wasmparser::KnownCustom::Name(name_reader) = reader.as_known()
            {
                for name in name_reader {
                    let name = name.expect("valid name subsection");
                    if let wasmparser::Name::Function(map) = name {
                        for naming in map {
                            let naming = naming.expect("valid naming");
                            func_names.push((naming.index, naming.name.to_string()));
                        }
                    }
                }
            }
        }

        // Should have at least import + runtime + user functions
        assert!(!func_names.is_empty(), "name section should have entries");

        // Check runtime function names
        let names_map: std::collections::HashMap<u32, &str> = func_names
            .iter()
            .map(|(idx, name)| (*idx, name.as_str()))
            .collect();
        assert_eq!(
            names_map[&p3_imports::FUNC_P3_ROOT_WAITABLE_SET_NEW],
            "$root::[waitable-set-new]"
        );
        // Fixture has no tests: runtime funcs start after the static table
        // plus the single `[task-return]run` import.
        let runtime_base = NUM_IMPORTS + 1;
        assert_eq!(names_map[&runtime_base], "run");
        assert_eq!(names_map[&(runtime_base + 2)], "initialize");
        assert_eq!(names_map[&(runtime_base + 3)], "string_eq");

        // Check user function names appear
        let user_names: Vec<&str> = func_names
            .iter()
            .filter(|(idx, _)| *idx >= runtime_base + NUM_RUNTIME_FUNCS)
            .map(|(_, name)| name.as_str())
            .collect();
        assert!(
            user_names.contains(&"a.main"),
            "should contain 'a.main', got: {:?}",
            user_names
        );
        assert!(
            user_names.contains(&"a.add(Int32, Int32)"),
            "should contain 'a.add(Int32, Int32)', got: {:?}",
            user_names
        );
        let add_index = func_names
            .iter()
            .find(|(_, name)| name == "a.add(Int32, Int32)")
            .unwrap()
            .0;
        let mut function_index = NUM_IMPORTS + 1;
        for payload in wasmparser::Parser::new(0).parse_all(&core_bytes) {
            if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
                if function_index == add_index {
                    let ops: Vec<_> = body
                        .get_operators_reader()
                        .unwrap()
                        .into_iter()
                        .map(Result::unwrap)
                        .collect();
                    assert!(
                        ops.iter()
                            .any(|op| matches!(op, wasmparser::Operator::I32Add))
                    );
                    assert!(
                        !ops.iter()
                            .any(|op| matches!(op, wasmparser::Operator::Call { .. }))
                    );
                }
                function_index += 1;
            }
        }
    }

    /// Every GC type must live in **one** rec group. WASM-GC canonicalizes rec groups
    /// structurally, so two same-shape types in separate groups are the same type and
    /// `ref.test`/`ref.cast` cannot separate them; two members of one group never are.
    /// See `docs/nominal-type-identity.md`.
    #[test]
    fn all_gc_types_share_one_rec_group() {
        let core_bytes = compile_core_with_prelude(
            r#"
package a

record Point = x: Int32; y: Int32
record Vec2 = x: Int32; y: Int32

enum Color =
    Red
    Shade(Int32)

interface Greeter =
    function greet(self: Self): String

implement Greeter for Point =
    function greet(self: Point): String = "point"

abstract class Shape()
class Circle(public radius: Float64) extends Shape()

function main(): Unit =
    let p = Point { x = 1; y = 2 }
    let v = Vec2 { x = 1; y = 2 }
    let c = Color.Shade(3)
    let s: Shape = Circle(1.0)
    let g: Greeter = p
    let f = (n: Int32) => n + p.x
    assert f(1) == 2
"#,
        );

        // (start index, member count) of every explicit rec group, plus the indices of every
        // struct/array type, walked in declaration order.
        let mut explicit_groups: Vec<(u32, usize)> = Vec::new();
        let mut gc_type_indices: Vec<u32> = Vec::new();
        let mut next_index = 0u32;
        let mut saw_type_section = false;

        for payload in wasmparser::Parser::new(0).parse_all(&core_bytes) {
            let wasmparser::Payload::TypeSection(reader) = payload.expect("valid payload") else {
                continue;
            };
            saw_type_section = true;
            for rec_group in reader {
                let rec_group = rec_group.expect("valid rec group");
                let start = next_index;
                let mut count = 0usize;
                for sub_type in rec_group.types() {
                    if matches!(
                        sub_type.composite_type.inner,
                        wasmparser::CompositeInnerType::Struct(_)
                            | wasmparser::CompositeInnerType::Array(_)
                    ) {
                        gc_type_indices.push(next_index);
                    }
                    next_index += 1;
                    count += 1;
                }
                if rec_group.is_explicit_rec_group() {
                    explicit_groups.push((start, count));
                }
            }
        }

        assert!(saw_type_section, "core module should have a type section");
        assert_eq!(
            explicit_groups.len(),
            1,
            "expected exactly one rec group, got {:?}",
            explicit_groups
        );

        let (group_start, group_len) = explicit_groups[0];
        assert_eq!(
            group_start, U8_BACKING_TYPE_INDEX,
            "the rec group must start right after the host-facing func types 0-5"
        );
        assert!(
            group_len > 1,
            "a one-member group would not give nominal identity"
        );

        // Nothing that `ref.test` can name may sit outside the group — a struct or array in its
        // own singleton group would canonicalize against any same-shape type in the store.
        let group_end = group_start + group_len as u32;
        let stragglers: Vec<u32> = gc_type_indices
            .iter()
            .copied()
            .filter(|idx| *idx < group_start || *idx >= group_end)
            .collect();
        assert!(
            stragglers.is_empty(),
            "GC types outside the rec group ({}..{}): {:?}",
            group_start,
            group_end,
            stragglers
        );

        // And the group really does hold the whole program: fixed types through user types.
        assert!(
            group_end > USER_TYPE_BASE,
            "group should extend past the fixed types into user types, ended at {}",
            group_end
        );
    }
}

/// Insertion-ordered dedup set of `(class mangled name, type args)` vtable instances.
///
/// Deliberately not a `HashSet`: the iteration order of the discovered instances decides
/// which vtable global index each instantiation gets, so a randomly-seeded hash order made
/// the emitted WASM differ byte-for-byte between otherwise identical builds.
#[derive(Default)]
struct OrderedInstances {
    items: Vec<(MangledName, Vec<Type>)>,
    seen: std::collections::HashSet<(MangledName, Vec<Type>)>,
}

impl OrderedInstances {
    fn insert(&mut self, key: (MangledName, Vec<Type>)) {
        if self.seen.insert(key.clone()) {
            self.items.push(key);
        }
    }
}
