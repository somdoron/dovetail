//! Exercise the emitted implementation with a host-controlled counter. The
//! extra export exists only in these test modules, never in compiled programs.
use wasm_encoder::{ExportKind, ExportSection, Module, RawSection};
use wasmtime::{Instance, Linker, Store, TypedFunc, Val};

fn harness(source: &str) -> (Store<()>, Instance, TypedFunc<(), ()>) {
    let bytes = super::tests::compile_core_with_prelude(source);
    let mut module = Module::new();
    let mut run_name = String::new();
    for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
        let payload = payload.unwrap();
        if let wasmparser::Payload::ExportSection(exports) = payload {
            let mut section = ExportSection::new();
            for export in exports {
                let export = export.unwrap();
                let kind = match export.kind {
                    wasmparser::ExternalKind::Func => ExportKind::Func,
                    wasmparser::ExternalKind::Memory => ExportKind::Memory,
                    _ => panic!("unexpected production export"),
                };
                if export.name.ends_with("#run") {
                    run_name = export.name.to_string();
                }
                section.export(export.name, kind, export.index);
            }
            section.export(
                "test-counter",
                ExportKind::Global,
                super::GLOBAL_IDENTITY_HASH_COUNTER,
            );
            module.section(&section);
        } else if let Some((id, range)) = payload.as_section() {
            module.section(&RawSection {
                id,
                data: &bytes[range],
            });
        }
    }
    let engine = crate::p3::p3_engine().unwrap();
    let module = wasmtime::Module::new(&engine, module.finish()).unwrap();
    let mut store = Store::new(&engine, ());
    let mut linker = Linker::new(&engine);
    linker
        .define_unknown_imports_as_default_values(&mut store, &module)
        .unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    instance
        .get_typed_func::<(), ()>(&mut store, "_initialize")
        .unwrap()
        .call(&mut store, ())
        .unwrap();
    let run = instance
        .get_typed_func::<(), ()>(&mut store, &run_name)
        .unwrap();
    (store, instance, run)
}

#[test]
fn lazy_hash_assignment_wraparound_and_unsigned_extension() {
    for start in [1i32, i32::MAX, -2i32] {
        let first = start as u32 as u64;
        let second = start.wrapping_add(1) as u32 as u64;
        let next = if second == u32::MAX as u64 {
            1
        } else {
            second + 1
        };
        let source = format!(
            r#"
package identity
class Item()
let unused: Item = Item()
function main(): Unit =
    let a = Item()
    let b = Item()
    assert ClassIdentity.equals(a, a)
    assert !ClassIdentity.equals(a, b)
    assert ClassIdentity.hash(b) == {first}i64
    assert ClassIdentity.hash(b) == {first}i64
    assert ClassIdentity.hash(a) == {second}i64
    assert ClassIdentity.hash(b) == {first}i64
    assert ClassIdentity.hash(Item()) == {next}i64
"#
        );
        let (mut store, instance, run) = harness(&source);
        let counter = instance.get_global(&mut store, "test-counter").unwrap();
        assert_eq!(
            counter.get(&mut store).i32(),
            Some(1),
            "global allocation consumed a hash"
        );
        counter.set(&mut store, Val::I32(start)).unwrap();
        run.call(&mut store, ()).unwrap();
    }
}

#[test]
fn hash_collisions_preserve_identity_and_existing_hashes() {
    let (mut store, instance, run) = harness(
        r#"
package identity
class Item()
class Control(public mutable round: Int32)
let first: Item = Item()
let control: Control = Control(0)
function main(): Unit =
    if control.round == 0 then
        assert ClassIdentity.hash(first) == 1i64
        control.round = 1
    else
        let collision = Item()
        assert ClassIdentity.hash(collision) == 1i64
        assert ClassIdentity.hash(first) == 1i64
        assert !ClassIdentity.equals(first, collision)
"#,
    );
    run.call(&mut store, ()).unwrap();
    instance
        .get_global(&mut store, "test-counter")
        .unwrap()
        .set(&mut store, Val::I32(1))
        .unwrap();
    run.call(&mut store, ()).unwrap();
}
