// @derive(FromSqlValue) — generates
// `fromSqlValue(value: SqlValue): Result<Self, SqliteError>`.
//
// Mirrors what @derive(ToSqlValue) writes:
//   newtype UserId = Int64          → Int64.fromSqlValue(value), re-wrapped
//   enum Status = Active; Inactive  → TEXT matched against the variant names
//
// Records and payload-carrying enums are rejected, as they are for ToSqlValue.

fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}

const NL = "\n";
let i12 = "            ";
let i16 = "                ";
let i20 = "                    ";

let tp_decl = "";
let tp_args = "";
let where_clause = "";
if !input.type_params.is_empty() {
    let bound_parts = [];
    for t in input.type_params { bound_parts.push(`${t}: FromSqlValue`); }
    tp_decl = "<" + join(input.type_params, ", ") + "> ";
    tp_args = "<" + join(input.type_params, ", ") + ">";
    where_clause = " where " + join(bound_parts, ", ");
}
let for_type = `${input.name}${tp_args}`;

let body = "";

if input.kind == "newtype" {
    body =
        "match " + input.inner + ".fromSqlValue(value) with" + NL +
        i12 + "case Ok(inner) => Ok(" + input.name + "(inner))" + NL +
        i12 + "case Error(e) => Error(e)";
} else if input.kind == "record" {
    throw "FromSqlValue cannot be derived for record '" + input.name +
          "': a record spans several columns. Use @derive(FromRow) instead.";
} else {
    if input.variants.is_empty() {
        throw "FromSqlValue cannot be derived for enum '" + input.name + "': it has no variants.";
    }
    let chain_lines = [];
    let first = true;
    for v in input.variants {
        if v.kind != "none" {
            throw "FromSqlValue cannot be derived for enum '" + input.name + "': variant '" +
                  v.name + "' carries a payload. Implement FromSqlValue by hand.";
        }
        let prefix = if first { "if" } else { "else if" };
        first = false;
        chain_lines.push(
            i16 + prefix + " __tag == \"" + v.name + "\" then Ok(" +
            input.name + "." + v.name + ")"
        );
    }
    body =
        "match value with" + NL +
        i12 + "case SqlValue.Text(__tag) =>" + NL +
        join(chain_lines, NL) + NL +
        i16 + "else Error(SqliteError { code = 20; message = \"unknown " + input.name +
        " variant: \" ++ __tag })" + NL +
        i12 + "case _ => Error(SqliteError { code = 20; message = \"expected Text for " +
        input.name + ", got \" ++ value.storageClassName() })";
}

`implement ${tp_decl}FromSqlValue for ${for_type}${where_clause} =
    public function fromSqlValue(value: SqlValue): Result<${for_type}, SqliteError> =
        ${body}
`
