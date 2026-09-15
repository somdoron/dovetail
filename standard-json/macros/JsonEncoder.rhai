// @derive(JsonEncoder) — generates `toJson(self): Json`.
//
// Records:
//   record Point = x: Int32; y: Int32
//   → Json.Object([("x", self.x.toJson()), ("y", self.y.toJson())])
//
// Newtypes (transparent): forward to the inner value's encoder.
//   newtype Cents = Int32
//   → self.value.toJson()
//
// Enums (internally tagged with `"type"` discriminator):
//   No payload:   Empty                  → {"type": "Empty"}
//   1-tuple:      Single(x)              → {"type": "Single", "value": x.toJson()}
//   n-tuple:      Pair(x, y)             → {"type": "Pair", "value": [x.toJson(), y.toJson()]}
//   record-style: Centered { x, y }      → {"type": "Centered", "x": x.toJson(), "y": y.toJson()}
//
// Generic targets receive `T: JsonEncoder` bounds for every type parameter.

fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}

let tp_decl = "";
let tp_args = "";
let where_clause = "";
if !input.type_params.is_empty() {
    let bound_parts = [];
    for t in input.type_params { bound_parts.push(`${t}: JsonEncoder`); }
    tp_decl = "<" + join(input.type_params, ", ") + "> ";
    tp_args = "<" + join(input.type_params, ", ") + ">";
    where_clause = " where " + join(bound_parts, ", ");
}
let for_type = `${input.name}${tp_args}`;

let body = "";

if input.kind == "newtype" {
    body = "self.value.toJson()";
} else if input.kind == "record" {
    if input.fields.is_empty() {
        body = "Json.Object([])";
    } else {
        let parts = [];
        for f in input.fields {
            parts.push(`("${f.name}", self.${f.name}.toJson())`);
        }
        body = `Json.Object([${join(parts, ", ")}])`;
    }
} else {
    // enum: build a `match self with` expression, one arm per variant.
    if input.variants.is_empty() {
        body = "Json.Object([])";
    } else {
        let arms = [];
        for v in input.variants {
            let tag_part = `("type", Json.Str("${v.name}"))`;
            if v.kind == "none" {
                arms.push(
                    `case ${input.name}.${v.name} => ` +
                    `Json.Object([${tag_part}])`
                );
            } else if v.kind == "tuple" {
                if v.types.len() == 1 {
                    arms.push(
                        `case ${input.name}.${v.name}(__a0) => ` +
                        `Json.Object([${tag_part}, ("value", __a0.toJson())])`
                    );
                } else {
                    let binds = [];
                    let elems = [];
                    for i in 0..v.types.len() {
                        binds.push(`__a${i}`);
                        elems.push(`__a${i}.toJson()`);
                    }
                    arms.push(
                        `case ${input.name}.${v.name}(${join(binds, ", ")}) => ` +
                        `Json.Object([${tag_part}, ("value", Json.Array([${join(elems, ", ")}]))])`
                    );
                }
            } else {
                // record-style variant: hoist fields next to "type".
                let binds = [];
                let parts = [tag_part];
                for f in v.fields {
                    let bind = `__a_${f.name}`;
                    binds.push(`${f.name} = ${bind}`);
                    parts.push(`("${f.name}", ${bind}.toJson())`);
                }
                arms.push(
                    `case ${input.name}.${v.name} { ${join(binds, ", ")} } => ` +
                    `Json.Object([${join(parts, ", ")}])`
                );
            }
        }
        body = `match self with
            ${join(arms, "\n            ")}`;
    }
}

`implement ${tp_decl}JsonEncoder for ${for_type}${where_clause} =
    public function toJson(self: ${for_type}): Json =
        ${body}
`
