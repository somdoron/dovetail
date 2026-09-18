use super::*;

fn format(source: &str) -> String {
    match format_source(source, "test.dove".into()) {
        Ok(output) => output,
        Err(error) => {
            let candidate = syntax::parse(source, "test.dove".into())
                .and_then(|parsed| parsed.document(source))
                .map(|document| document::render(&document));
            panic!("{error}\nCandidate:\n{candidate:?}");
        }
    }
}

#[test]
fn canonical_function_body() {
    let expected = r#"package a

function add(x: Int32, y: Int32): Int32 = x + y
"#;
    assert_eq!(
        format(
            r#"package a
function add( x:Int32,y:Int32 ):Int32 =
  x+y
"#
        ),
        expected
    );
    assert_eq!(format(expected), expected);
}

#[test]
fn comments_and_blocks() {
    let source = r#"package a

// description
function f(): Int32 =
    let x = 1 // value

    x + 2
"#;
    let expected = r#"package a

// description
function f(): Int32 =
    let x = 1 // value

    x + 2
"#;
    assert_eq!(format(source), expected);
    assert_eq!(format(expected), expected);
}

#[test]
fn nested_record_and_call() {
    let source = r#"package a
function f(): SocketAddress =
    SocketAddress.V4(Ipv4SocketAddress {
        port = port
        address = Ipv4Address { a = 127u8; b = 0u8; c = 0u8; d = 1u8 }
    })
"#;
    let output = format(source);
    assert_eq!(format(&output), output);
}

#[test]
fn interpolation_code_and_literal_text() {
    let source = r#"package a
function f(): String = "keep  spaces ${ 1+2 } and \n"
"#;
    let output = format(source);
    assert!(output.contains(r#""keep  spaces ${1 + 2} and \n""#));
    assert_eq!(format(&output), output);
}

#[test]
fn file_module_and_comments() {
    let source = r#"module a.M
/// First
public function f(): Int32 = intrinsic
/// Second
public function g(): Int32 = intrinsic
"#;
    let output = format(source);
    assert!(output.starts_with("module a.M\n\n/// First\npublic function"));
    assert_eq!(format(&output), output);
}

#[test]
fn grouping_parentheses_preserve_precedence() {
    let source = r#"package a
function f(a: Int32, b: Int32, c: Int32): Int32 = ((a + b)) * c
function g(a: Int32, b: Int32, c: Int32): Int32 = ((a)) + (b * c)
function h(f: (Int32) => Int32): Int32 = f(1)
"#;
    let output = format(source);
    assert!(output.contains("= ((a + b)) * c"));
    assert!(output.contains("= ((a)) + (b * c)"));
    assert!(output.contains("function h(f: (Int32) => Int32)"));
    assert_eq!(format(&output), output);
}

#[test]
fn wrapped_signature_keeps_short_body_inline() {
    let source = r#"package a
function f(firstParameterWithAQuiteLongName: SomeLongTypeName, secondParameterWithAQuiteLongName: AnotherLongTypeName): Int32 = 1
"#;
    let output = format(source);
    assert!(output.contains("\n): Int32 = 1\n"));
    assert_eq!(format(&output), output);
}

#[test]
fn long_binary_expression_wraps_at_operators() {
    let source = r#"package a
function f(): Int32 = firstVariableWithALongName + secondVariableWithALongName + thirdVariableWithALongName + fourthVariableWithALongName
"#;
    let output = format(source);
    assert!(output.lines().all(|line| line.chars().count() <= 100));
    assert_eq!(format(&output), output);
}

#[test]
fn empty_comments_and_line_endings() {
    assert_eq!(format(""), "\n");
    assert_eq!(format("// only a comment\r\n"), "// only a comment\n");
    assert!(format_source("function (", "invalid.dove".into()).is_err());
}

#[test]
fn scoped_use_is_neither_introduced_nor_removed() {
    let source = r#"package a
function f(): Unit =
    let resource = use acquireResourceWithAnExtremelyLongName().map((value: SomeLongTypeName) => value)
    resource.close()
"#;
    let output = format(source);
    assert!(output.contains("let resource = use "));
    assert_eq!(format(&output), output);
}

#[test]
fn block_arguments_close_before_commas() {
    let source = r#"package a
function f(): Unit = call(async do 21, "error")
function g(): Unit = call((value: Int32) =>
    let result = value + 1
    result
, 2)
"#;
    let output = format(source);
    assert_eq!(format(&output), output);
}

#[test]
fn record_updates_and_fields_converge() {
    let inline = r#"package a
function f(p: Point): Point = p with x = 1; y = 2
function g(): Point = Point { x = 1; y = 2 }
"#;
    let multiline = r#"package a
function f(p: Point): Point =
    p with
        x = 1
        y = 2
function g(): Point =
    Point {
        x = 1
        y = 2
    }
"#;
    let output = format(inline);
    assert_eq!(format(multiline), output);
    assert_eq!(format(&output), output);
}

#[test]
fn workspace_corpus() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let mut failures = vec![];
    let mut count = 0;
    for directory in crate::manifest::formatting_directories(root).unwrap() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "dove") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let file: FilePath = path.to_string_lossy().as_ref().into();
            match format_source(&source, file.clone()) {
                Ok(output) => match format_source(&output, file) {
                    Ok(second) if second == output => {}
                    Ok(_) => failures.push(format!("{}: not idempotent", path.display())),
                    Err(error) => {
                        failures.push(format!("{}: second pass: {error}", path.display()))
                    }
                },
                Err(error) => failures.push(format!("{}: {error}", path.display())),
            }
            count += 1;
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {count} files failed:\n{}",
        failures.len(),
        failures
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn multiline_receiver_keeps_postfix_outside_its_block() {
    let source = r#"package a
function f(): Unit = async do
    let observer = await (async do
        while !started do await yieldNow()
        progressed = !finished
    ).forkBackground()
    await observer.join()
"#;
    let output = format(source);
    assert_eq!(format(&output), output);
}

#[test]
fn end_of_block_comments_stay_in_their_container() {
    let source = r#"package a
function f(): Int32 =
    1
    // belongs to f
function g(): Point =
    Point {
        x = 1
        // belongs to the record
    }
"#;
    let output = format(source);
    assert!(output.contains("    1\n    // belongs to f\n\nfunction g"));
    assert!(output.contains("        // belongs to the record\n    }"));
    assert_eq!(format(&output), output);
}

#[test]
fn comments_after_separators_survive_wrapping() {
    let source = r#"package a
function f(): Point = Point { x = 1; // x
    y = 2 }
function g(): Unit = call(1, // first
    2)
function empty(): Point = Point {
    // empty record
}
"#;
    let output = format(source);
    assert!(output.contains("// x"));
    assert!(output.contains("// first"));
    assert!(output.contains("    // empty record"));
    assert_eq!(format(&output), output);
}

#[test]
fn structural_validation_retains_literal_marker() {
    let marked = r#"package a
@stringLiteral public type sql = SqlBuilder
"#;
    let unmarked = r#"package a
public type sql = SqlBuilder
"#;
    let marked_ast = validate(marked, "test.dove".into()).unwrap();
    let unmarked_ast = validate(unmarked, "test.dove".into()).unwrap();
    assert_ne!(
        structure(&marked_ast).unwrap(),
        structure(&unmarked_ast).unwrap()
    );
    let output = format(marked);
    assert!(output.contains("@stringLiteral"), "{output}");
    assert_eq!(format(&output), output);
}

#[test]
fn else_if_chains_stay_aligned() {
    let source = r#"package a
function choose(x: Int32): Int32 =
    if x == 0 then 111111111
    else if x == 1 then 222222222
    else if x == 2 then 333333333
    else if x == 3 then 444444444
    else 555555555
"#;
    let output = format(source);
    assert!(output.contains("\n    else if"), "{output}");
    assert!(!output.contains("else\n"), "{output}");
    assert!(output.contains("\n    else 555555555"), "{output}");
    assert_eq!(format(&output), output);
}

#[test]
fn short_conditionals_remain_inline() {
    let source = r#"package a
function f(x: Int32): Int32 = if x == 0 then 1 else if x == 1 then 2 else 3
"#;
    let output = format(source);
    assert!(
        output.contains("if x == 0 then 1 else if x == 1 then 2 else 3"),
        "{output}"
    );
    assert_eq!(format(&output), output);
}

#[test]
fn where_conditions_align_with_the_first_condition() {
    let source = r#"package a
implement<T, U> Default for T ~U
    where T: TupleWithAnExtraordinarilyLongName,
T: DefaultWithAnExtraordinarilyLongName,
U: Default =
    public property default: T ~U = T.default ~ U.default
"#;
    let output = format(source);
    assert!(
        output.contains("    where T: TupleWithAnExtraordinarilyLongName,\n          T: DefaultWithAnExtraordinarilyLongName,\n          U: Default ="),
        "{output}"
    );
    assert_eq!(format(&output), output);
}

#[test]
fn multiline_branch_expands_every_body_in_the_chain() {
    let source = r#"package a
function f(x: Int32): Int32 =
    let mutable result = 0
    if x == 0 then result = 1
    else if x == 1 then
        result = 2
        result = result + 1
    else result = 4
    result
"#;
    let output = format(source);
    assert!(
        output.contains("if x == 0 then\n        result = 1"),
        "{output}"
    );
    assert!(output.contains("else\n        result = 4"), "{output}");
    assert_eq!(format(&output), output);
}

#[test]
fn blank_lines_between_statements_are_preserved_and_collapsed() {
    let source = r#"package a
function f(): Int32 =
    let x = 1


    // next group
    let y = 2
    x + y
"#;
    let expected = r#"package a

function f(): Int32 =
    let x = 1

    // next group
    let y = 2
    x + y
"#;
    assert_eq!(format(source), expected);
    assert_eq!(format(expected), expected);
}

#[test]
fn blank_lines_between_import_groups_are_preserved() {
    let source = r#"package a
import standard.prelude.Hashable


import standard.prelude.Equatable
import standard.prelude.Option
"#;
    let expected = r#"package a

import standard.prelude.Hashable

import standard.prelude.Equatable
import standard.prelude.Option
"#;
    assert_eq!(format(source), expected);
    assert_eq!(format(expected), expected);
}

#[test]
fn interpolation_retains_explicit_parentheses() {
    let source = r#"package a
function f(): String = "value: ${ ((1+2)) }"
"#;
    let output = format(source);
    assert!(output.contains("${((1 + 2))}"), "{output}");
    assert_eq!(format(&output), output);
}

#[test]
fn long_return_type_wraps_after_colon() {
    let source = r#"package a
function nodeArrayInsert<K, V>(arr: Array<MapNode<K, V>>, idx: Int32, value: MapNode<K, V>): Array<MapNode<K, V>> =
    let result = arr
    result
"#;
    let output = format(source);
    assert!(
        output.contains("value: MapNode<K, V>):\n    Array<MapNode<K, V>> ="),
        "{output}"
    );
    assert_eq!(format(&output), output);
}

#[test]
fn short_where_clause_stays_with_signature_above_multiline_body() {
    let source = r#"package a
implement<T> Default for Array<T>
    where T: Default =
    public property default: Array<T> = Array.empty()
"#;
    let output = format(source);
    assert!(
        output.contains("implement<T> Default for Array<T> where T: Default =\n"),
        "{output}"
    );
    assert_eq!(format(&output), output);
}

#[test]
fn named_arguments_preserve_labels_order_and_comments() {
    let source = r#"package a
function main(): Unit =
    send(
        urgent = true, // preserve this comment
        message = "hello"
    )
"#;
    let formatted = format(source);
    assert!(formatted.contains("urgent = true"));
    assert!(formatted.contains("message = \"hello\""));
    assert!(formatted.contains("// preserve this comment"));
    assert!(formatted.find("urgent").unwrap() < formatted.find("message").unwrap());
    assert_eq!(format(&formatted), formatted);
}
