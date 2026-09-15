use wasmparser::{CompositeInnerType, Operator, Parser, Payload, StorageType, ValType};

#[test]
fn reified_layouts_are_immutable_and_duplicate_checks_share_one_table() {
    let bytes = super::tests::compile_core_with_prelude(
        r#"
package reifiedLayout
record Box<T> = value: T
class Root()
final class Child<T>(public value: T) extends Root()
final class Sibling() extends Root()
function first(value: Any): Bool = value is Box<Int32>
function second(value: Any): Bool = value is Box<Int32>
function main(): Unit =
    let child = Child(42)
    let sibling = Sibling()
    assert ClassIdentity.hash(child) != ClassIdentity.hash(sibling)
    let value: Any = Box { value = 42 }
    assert first(value)
    assert second(value)
"#,
    );
    let mut tables = 0;
    let mut reified_records = 0;
    let mut rec_groups = 0;
    let mut reified_classes = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                for group in section {
                    let group = group.unwrap();
                    if group.is_explicit_rec_group() {
                        rec_groups += 1;
                    }
                    for ty in group.types() {
                        if let CompositeInnerType::Struct(s) = &ty.composite_type.inner {
                            if s.fields.len() >= 3
                                && s.fields[0].element_type == StorageType::Val(ValType::I32)
                                && !s.fields[0].mutable
                                && matches!(
                                    s.fields[1].element_type,
                                    StorageType::Val(ValType::Ref(_))
                                )
                                && !s.fields[1].mutable
                                && s.fields[2].element_type == StorageType::Val(ValType::I32)
                                && s.fields[2].mutable
                            {
                                reified_classes += 1;
                            }
                            if s.fields.len() == 2
                                && s.fields[0].element_type == StorageType::Val(ValType::I32)
                                && !s.fields[0].mutable
                                && matches!(
                                    s.fields[1].element_type,
                                    StorageType::Val(ValType::Ref(_))
                                )
                            {
                                reified_records += 1;
                            }
                        }
                    }
                }
            }
            Payload::GlobalSection(section) => {
                for global in section {
                    let global = global.unwrap();
                    let mut ops = global.init_expr.get_operators_reader();
                    while !ops.eof() {
                        if let Operator::ArrayNewFixed { .. } = ops.read().unwrap() {
                            assert!(!global.ty.mutable);
                            tables += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(rec_groups, 1);
    assert!(
        reified_classes >= 3,
        "root, child and sibling must share the ID/vtable/hash header"
    );
    assert!(reified_records > 0, "missing immutable ID prefix");
    assert_eq!(
        tables, 1,
        "identical expected types must share membership storage"
    );
}
