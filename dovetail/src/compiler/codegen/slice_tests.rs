use std::collections::BTreeMap;

use wasmparser::{CompositeInnerType, KnownCustom, Name, Operator, Parser, Payload, TypeRef};

#[test]
fn slice_parameters_returns_and_reslicing_are_flat() {
    check_flat_slice(false);
}

#[test]
fn readonly_slice_parameters_returns_and_reslicing_are_flat() {
    check_flat_slice(true);
}

fn check_flat_slice(readonly: bool) {
    let source = r#"
package a
function sliceProbe(s: Slice<Int32>): Slice<Int32> = s[|1..|]
function readProbe(s: Slice<Int32>): Int32 = s.get(0)
function main(): Unit =
    let s = sliceProbe([|1, 2, 3|][|..|])
    assert readProbe(s) == 2
"#;
    let source = if readonly {
        source
            .replace("Slice<", "ReadonlySlice<")
            .replace("[|1, 2, 3|][|..|]", "[|1, 2, 3|].readonly")
            .replace("function main(): Unit =", "function widenProbe(s: ReadonlySlice<Int32>): ReadonlySlice<Any> = s\nfunction main(): Unit =")
            .replace("assert readProbe(s) == 2", "assert readProbe(s) == 2\n    assert (widenProbe(s)[0] as Int32) == 2")
    } else {
        source.to_owned()
    };
    let wasm = super::tests::compile_core_with_prelude(&source);
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&wasm)
        .expect("valid slice module");

    let mut signatures = Vec::new();
    let mut function_types = Vec::new();
    let mut bodies = Vec::new();
    let mut names = BTreeMap::new();
    let mut imported_functions = 0;
    for payload in Parser::new(0).parse_all(&wasm) {
        match payload.expect("valid payload") {
            Payload::TypeSection(reader) => {
                for group in reader {
                    for ty in group.expect("type group").types() {
                        signatures.push(match &ty.composite_type.inner {
                            CompositeInnerType::Func(f) => {
                                Some((f.params().len(), f.results().len()))
                            }
                            _ => None,
                        });
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    if matches!(import.expect("import").ty, TypeRef::Func(_)) {
                        imported_functions += 1;
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                function_types.extend(reader.into_iter().map(|ty| ty.expect("function type")));
            }
            Payload::CodeSectionEntry(body) => bodies.push(body),
            Payload::CustomSection(reader) => {
                if let KnownCustom::Name(reader) = reader.as_known() {
                    for name in reader {
                        if let Name::Function(map) = name.expect("name subsection") {
                            for name in map {
                                let name = name.expect("function name");
                                names.insert(name.index, name.name.to_owned());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let mut checked = 0;
    let mut checked_probe = false;
    for (index, name) in names {
        let probe = name.starts_with("a.sliceProbe(") || name.starts_with("a.widenProbe(");
        let read_probe = readonly && name.starts_with("a.readProbe(");
        let slice_method = (name.starts_with("standard.prelude.Slice.")
            || name.starts_with("standard.prelude.ReadonlySlice."))
            && ["full(", "make(", "drop(", "slice("]
                .iter()
                .any(|method| name.contains(method));
        if !probe && !read_probe && !slice_method {
            continue;
        }
        let body_index = index as usize - imported_functions;
        if probe {
            checked_probe = true;
            assert_eq!(
                signatures[function_types[body_index] as usize],
                Some((3, 3))
            );
        }
        let mut native_reads = 0;
        if read_probe {
            assert_eq!(
                signatures[function_types[body_index] as usize],
                Some((3, 1))
            );
        }
        for op in bodies[body_index]
            .get_operators_reader()
            .expect("operators")
        {
            let op = op.expect("operator");
            if read_probe {
                match op {
                    Operator::ArrayGet { array_type_index } => {
                        assert_eq!(array_type_index, super::ARRAY_I32_TYPE_INDEX);
                        native_reads += 1;
                    }
                    Operator::Call { .. } | Operator::CallIndirect { .. } => {
                        panic!("native read must not call a boxing helper")
                    }
                    _ => {}
                }
            }
            if let Operator::StructNew { struct_type_index }
            | Operator::StructNewDefault { struct_type_index } = op
            {
                // Failed bounds checks construct their panic message. No slice
                // operation may allocate a tuple or any other view container.
                assert_eq!(
                    struct_type_index,
                    super::STRING_STRUCT_TYPE_INDEX,
                    "unexpected allocation in {name}"
                );
            }
        }
        if read_probe {
            assert_eq!(native_reads, 1, "known Int32 read stays native");
        }
        checked += 1;
    }
    assert!(checked_probe, "sliceProbe must be inspected");
    assert!(
        checked >= 5,
        "expected probes plus emitted view methods, found {checked}"
    );
}
