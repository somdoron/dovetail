// @derive(ToSqlValue) — generates `toSqlValue(self): SqlValue`.
//
// Newtypes (transparent): forward to the inner value's implementation.
//   newtype UserId = Int64
//   → self.value.toSqlValue()
//
// Payload-free enums: the variant name as TEXT.
//   enum Status = Active; Inactive
//   → SqlValue.Text("Active") / SqlValue.Text("Inactive")
//
// Records and payload-carrying enums are rejected: neither has an unambiguous
// single-column form, and guessing one would silently pick a schema.
//
// Generic targets receive `T: ToSqlValue` bounds for every type parameter.

fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}

const NL = "\n";
let i12 = "            ";

let tp_decl = "";
let tp_args = "";
let where_clause = "";
if !input.type_params.is_empty() {
    let bound_parts = [];
    for t in input.type_params { bound_parts.push(`${t}: ToSqlValue`); }
    tp_decl = "<" + join(input.type_params, ", ") + "> ";
    tp_args = "<" + join(input.type_params, ", ") + ">";
    where_clause = " where " + join(bound_parts, ", ");
}
let for_type = `${input.name}${tp_args}`;

let body = "";

if input.kind == "newtype" {
    body = "self.value.toSqlValue()";
} else if input.kind == "record" {
    throw "ToSqlValue cannot be derived for record '" + input.name +
          "': a record spans several columns, so it has no single-column form. " +
          "Use @derive(FromRow) to map a whole result row, or implement ToSqlValue by hand.";
} else {
    if input.variants.is_empty() {
        throw "ToSqlValue cannot be derived for enum '" + input.name + "': it has no variants.";
    }
    let arms = [];
    for v in input.variants {
        if v.kind != "none" {
            throw "ToSqlValue cannot be derived for enum '" + input.name + "': variant '" +
                  v.name + "' carries a payload, so there is no unambiguous single-column " +
                  "form. Implement ToSqlValue by hand (see docs/SQL.md, \"Manual implementation\").";
        }
        arms.push(`case ${input.name}.${v.name} => SqlValue.Text("${v.name}")`);
    }
    body = "match self with" + NL + i12 + join(arms, NL + i12);
}

`implement ${tp_decl}ToSqlValue for ${for_type}${where_clause} =
    public function toSqlValue(self: ${for_type}): SqlValue =
        ${body}
`
