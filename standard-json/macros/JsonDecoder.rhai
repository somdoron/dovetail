// @derive(JsonDecoder) — generates `fromJson(json: Json): Result<Self, JsonError>`.
//
// Mirrors the encoding produced by `@derive(JsonEncoder)`:
//
// Records: read fields by name from a `Json.Object`. Missing fields →
//   `JsonError.MissingField`. Wrong type for `json` → `JsonError.TypeError`.
//
// Newtypes: forward to the inner type's decoder and re-wrap.
//
// Enums (internally tagged):
//   {"type": "Empty"}                              → Empty
//   {"type": "Single", "value": v}                 → Single(T.fromJson(v))
//   {"type": "Pair", "value": [a, b]}              → Pair(A.fromJson(a), B.fromJson(b))
//   {"type": "Centered", "x": ..., "y": ...}       → Centered { x, y }
//
// Generic targets receive `T: JsonDecoder` bounds for every type parameter.

fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}

// Rhai backtick templates don't process escape sequences; double-quoted
// strings do. We use `"\n"` here so we get an actual newline character
// when composing multi-line generated source.
const NL = "\n";

let tp_decl = "";
let tp_args = "";
let where_clause = "";
if !input.type_params.is_empty() {
    let bound_parts = [];
    for t in input.type_params { bound_parts.push(`${t}: JsonDecoder`); }
    tp_decl = "<" + join(input.type_params, ", ") + "> ";
    tp_args = "<" + join(input.type_params, ", ") + ">";
    where_clause = " where " + join(bound_parts, ", ");
}
let for_type = `${input.name}${tp_args}`;

// Indentation levels (in spaces, inside the function body):
//     match json with     → 8
//         case ... =>     → 12
//             stmts       → 16   (inside case body)
//                 nested  → 20
let i8 = "        ";
let i12 = "            ";
let i16 = "                ";
let i20 = "                    ";
let i24 = "                        ";
let i28 = "                            ";
let i32 = "                                ";

let body = "";

if input.kind == "newtype" {
    body =
        "match " + input.inner + ".fromJson(json) with" + NL +
        i12 + "case Ok(v) => Ok(" + input.name + "(v))" + NL +
        i12 + "case Error(e) => Error(e)";
} else if input.kind == "record" {
    if input.fields.is_empty() {
        body =
            "match json with" + NL +
            i12 + "case Json.Object(_) => Ok(" + input.name + " {})" + NL +
            i12 + "case _ => Error(JsonError.TypeError(\"Object\", json.typeName()))";
    } else {
        let bind_lines = [];
        let inits = [];
        for f in input.fields {
            bind_lines.push(
                i16 + "let " + f.name +
                " = try " + f.ty + ".fromJson(try Json.getField(__pairs, \"" + f.name + "\"))"
            );
            inits.push(f.name + " = " + f.name);
        }
        body =
            "match json with" + NL +
            i12 + "case Json.Object(__pairs) =>" + NL +
            join(bind_lines, NL) + NL +
            i16 + "Ok(" + input.name + " { " + join(inits, "; ") + " })" + NL +
            i12 + "case _ => Error(JsonError.TypeError(\"Object\", json.typeName()))";
    }
} else {
    // enum
    if input.variants.is_empty() {
        body = "Error(JsonError.Custom(\"cannot decode empty enum\"))";
    } else {
        let chain_lines = [];
        let first = true;
        for v in input.variants {
            let prefix = if first { "if" } else { "else if" };
            first = false;
            if v.kind == "none" {
                chain_lines.push(
                    i16 + prefix + " __tag == \"" + v.name + "\" then Ok(" +
                    input.name + "." + v.name + ")"
                );
            } else if v.kind == "tuple" {
                if v.types.len() == 1 {
                    let ty = v.types[0];
                    chain_lines.push(
                        i16 + prefix + " __tag == \"" + v.name + "\" then" + NL +
                        i20 + "let __v = try " + ty + ".fromJson(try Json.getField(__pairs, \"value\"))" + NL +
                        i20 + "Ok(" + input.name + "." + v.name + "(__v))"
                    );
                } else {
                    let n = v.types.len();
                    let binds = [];
                    let elems = [];
                    let decode_lines = [];
                    for i in 0..n {
                        binds.push("__v" + i);
                        elems.push("__e" + i);
                        // A list pattern binds each element directly, so the
                        // arity check and the destructuring are the same step.
                        decode_lines.push(
                            i28 + "let __v" + i +
                            " = try " + v.types[i] + ".fromJson(__e" + i + ")"
                        );
                    }
                    chain_lines.push(
                        i16 + prefix + " __tag == \"" + v.name + "\" then" + NL +
                        i20 + "let __valJson = try Json.getField(__pairs, \"value\")" + NL +
                        i20 + "match __valJson with" + NL +
                        i24 + "case Json.Array([" + join(elems, ", ") + "]) =>" + NL +
                        join(decode_lines, NL) + NL +
                        i28 + "Ok(" + input.name + "." + v.name + "(" + join(binds, ", ") + "))" + NL +
                        i24 + "case Json.Array(_) => Error(JsonError.Custom(\"expected array of length " + n + " for " + v.name + "\"))" + NL +
                        i24 + "case _ => Error(JsonError.TypeError(\"Array\", __valJson.typeName()))"
                    );
                }
            } else {
                // record-style payload
                let bind_lines = [];
                let inits = [];
                for f in v.fields {
                    bind_lines.push(
                        i20 + "let " + f.name +
                        " = try " + f.ty + ".fromJson(try Json.getField(__pairs, \"" + f.name + "\"))"
                    );
                    inits.push(f.name + " = " + f.name);
                }
                chain_lines.push(
                    i16 + prefix + " __tag == \"" + v.name + "\" then" + NL +
                    join(bind_lines, NL) + NL +
                    i20 + "Ok(" + input.name + "." + v.name + " { " + join(inits, "; ") + " })"
                );
            }
        }
        body =
            "match json with" + NL +
            i12 + "case Json.Object(__pairs) =>" + NL +
            i16 + "let __tag = try String.fromJson(try Json.getField(__pairs, \"type\"))" + NL +
            join(chain_lines, NL) + NL +
            i16 + "else Error(JsonError.Custom(\"unknown variant for " + input.name + ": \" ++ __tag))" + NL +
            i12 + "case _ => Error(JsonError.TypeError(\"Object\", json.typeName()))";
    }
}

`implement ${tp_decl}JsonDecoder for ${for_type}${where_clause} =
    public function fromJson(json: Json): Result<${for_type}, JsonError> =
${i8}${body}
`
