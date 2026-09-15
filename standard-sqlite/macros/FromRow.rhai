// @derive(FromRow) — generates `fromRow(row: Row): Result<Self, SqliteError>`.
//
// Records: one column per field, looked up by the field's own NAME and decoded
// through `FromSqlValue`. Align field names with column names, or alias the
// column with `AS` in the query — Dovetail has no field-level attributes yet, so
// there is no `@column("...")` override.
//
// Why by name and not by position: a record's fields and a SELECT list are two
// independently edited lists. Positional mapping silently swaps two same-typed
// columns when either is reordered — `record Person = name: String; email:
// String` against `SELECT email, name` decodes without error and puts the email
// in `name`. Names make that a hard failure instead. The trade is that the query
// must actually produce the field names, which `AS` fixes.
//
// When names are the wrong key — computed columns, or a join where two tables
// both contribute `id` — decode into a tuple instead. Tuples have no field
// names, so their `FromRow` implementations map by index; see
// src/FromRow.dove.
//
// Newtypes and payload-free enums: a single-column row decoded through the
// type's own `FromSqlValue`. That is what makes `fetchOne<UserId>(conn)` work
// on a `RETURNING id` query.
//
// Payload-carrying enums are rejected: there is no obvious column mapping.

fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}

const NL = "\n";
let i8 = "        ";

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
    body = input.name + ".fromSqlValue(try row.scalar())";
} else if input.kind == "record" {
    if input.fields.is_empty() {
        body = "Ok(" + input.name + " {})";
    } else {
        let bind_lines = [];
        let inits = [];
        for f in input.fields {
            bind_lines.push(
                i8 + "let " + f.name + " = try " + f.ty +
                ".fromSqlValue(try row.column(\"" + f.name + "\"))"
            );
            inits.push(f.name + " = " + f.name);
        }
        body =
            join(bind_lines, NL) + NL +
            i8 + "Ok(" + input.name + " { " + join(inits, "; ") + " })";
        // The first line is already indented by the template below.
        body = body.sub_string(8);
    }
} else {
    if input.variants.is_empty() {
        throw "FromRow cannot be derived for enum '" + input.name + "': it has no variants.";
    }
    for v in input.variants {
        if v.kind != "none" {
            throw "FromRow cannot be derived for enum '" + input.name + "': variant '" +
                  v.name + "' carries a payload. Derive FromRow only for records " +
                  "(field per column), payload-free enums, and newtypes.";
        }
    }
    body = input.name + ".fromSqlValue(try row.scalar())";
}

`implement ${tp_decl}FromRow for ${for_type}${where_clause} =
    public function fromRow(row: Row): Result<${for_type}, SqliteError> =
${i8}${body}
`
