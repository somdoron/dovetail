// @derive(Equatable) — generates a structural `equals` implementation.
//
// Records:
//   implement Equatable for R =
//       function equals(self: R, other: R): Bool =
//           self.f1 == other.f1 && self.f2 == other.f2 && ...
//
// Enums:
//   implement Equatable for E =
//       function equals(self: E, other: E): Bool =
//           match (self, other) with
//               case (E.A, E.A) => true
//               case (E.B(__eq_a0), E.B(__eq_b0)) => __eq_a0 == __eq_b0
//               ...
//               case _ => false
//
// Generic targets: every type parameter T gets a `T: Equatable` bound.
// Empty records / empty enums fall back to `true`.

fn join(items, sep) {
    if items.is_empty() { return ""; }
    let s = items[0];
    for i in 1..items.len() { s += sep + items[i]; }
    s
}

// Dovetail's implement<...> only takes bare type-parameter names; trait
// bounds live in the `where` clause, not inside the `<...>`.
let tp_decl = "";
let tp_args = "";
let where_clause = "";
if !input.type_params.is_empty() {
    let bound_parts = [];
    for t in input.type_params { bound_parts.push(`${t}: Equatable`); }
    tp_decl = "<" + join(input.type_params, ", ") + "> ";
    tp_args = "<" + join(input.type_params, ", ") + ">";
    where_clause = " where " + join(bound_parts, ", ");
}

let for_type = `${input.name}${tp_args}`;
let body = "";

if input.kind == "newtype" {
    // newtype Cents = Int32 → compare via the inner value.
    body = "self.value == other.value";
} else if input.kind == "record" {
    if input.fields.is_empty() {
        body = "true";
    } else {
        let parts = [];
        for f in input.fields { parts.push(`self.${f.name} == other.${f.name}`); }
        body = join(parts, " && ");
    }
} else {
    // enum
    if input.variants.is_empty() {
        body = "true";
    } else {
        let arms = [];
        for v in input.variants {
            if v.kind == "none" {
                arms.push(`case (${input.name}.${v.name}, ${input.name}.${v.name}) => true`);
            } else if v.kind == "tuple" {
                let lhs = [];
                let rhs = [];
                let comps = [];
                for i in 0..v.types.len() {
                    lhs.push(`__eq_a${i}`);
                    rhs.push(`__eq_b${i}`);
                    comps.push(`__eq_a${i} == __eq_b${i}`);
                }
                arms.push(
                    `case (${input.name}.${v.name}(${join(lhs, ", ")}), ` +
                    `${input.name}.${v.name}(${join(rhs, ", ")})) => ${join(comps, " && ")}`
                );
            } else {
                // record-style payload
                let lhs_pats = [];
                let rhs_pats = [];
                let comps = [];
                for f in v.fields {
                    lhs_pats.push(`${f.name} = __eq_a_${f.name}`);
                    rhs_pats.push(`${f.name} = __eq_b_${f.name}`);
                    comps.push(`__eq_a_${f.name} == __eq_b_${f.name}`);
                }
                arms.push(
                    `case (${input.name}.${v.name} { ${join(lhs_pats, ", ")} }, ` +
                    `${input.name}.${v.name} { ${join(rhs_pats, ", ")} }) => ${join(comps, " && ")}`
                );
            }
        }
        if input.variants.len() > 1 {
            arms.push("case _ => false");
        }
        body = `match (self, other) with
            ${join(arms, "\n            ")}`;
    }
}

`implement ${tp_decl}Equatable for ${for_type}${where_clause} =
    public function equals(self: ${for_type}, other: ${for_type}): Bool =
        ${body}
`
