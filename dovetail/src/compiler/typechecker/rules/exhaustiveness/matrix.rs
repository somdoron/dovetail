//! Maranget's usefulness algorithm, specialized to the wildcard query: is the
//! all-wildcard row still useful against the matrix of arm patterns? If so, the
//! match is non-exhaustive and the witness says which value escapes.

use crate::typechecker::types::Type;

use super::ctor::{Bail, Ctor, CtorSet, PatCx};
use super::witness::Witness;

/// A pattern in matrix form.
#[derive(Debug, Clone)]
pub(super) enum Pat {
    Wild,
    Ctor { ctor: Ctor, fields: Vec<Pat> },
}

type Row = Vec<Pat>;
pub(super) type Matrix = Vec<Row>;

/// How many uncovered values to report before truncating.
const MAX_WITNESSES: usize = 3;
/// Backstop against a malformed typed AST. Real depth is bounded by how deeply
/// the *user* nested their patterns, not by the recursion in the type.
const MAX_DEPTH: usize = 64;

/// Does a row's constructor cover the constructor being split on?
///
/// Equality, except that a type test against a sealed intermediate covers every
/// leaf beneath it.
fn covers(cx: &PatCx<'_>, row: &Ctor, split: &Ctor) -> bool {
    match (row, split) {
        (Ctor::ClassTest(tested), Ctor::ClassLeaf(leaf)) => leaf.covered_by(cx.registry, tested),
        _ => row == split,
    }
}

/// The distinct constructors appearing in the head column, in first-seen order.
fn head_ctors(matrix: &Matrix) -> Vec<Ctor> {
    let mut seen: Vec<Ctor> = Vec::new();
    for row in matrix {
        if let Some(Pat::Ctor { ctor, .. }) = row.first()
            && !seen.contains(ctor)
        {
            seen.push(ctor.clone());
        }
    }
    seen
}

/// `S(c, P)`: keep rows whose head matches `c`, splicing its fields in front.
fn specialize(cx: &PatCx<'_>, matrix: &Matrix, ctor: &Ctor, arity: usize) -> Matrix {
    matrix
        .iter()
        .filter_map(|row| {
            let (head, tail) = row.split_first()?;
            let mut new = match head {
                Pat::Wild => vec![Pat::Wild; arity],
                Pat::Ctor { ctor: c, fields } if covers(cx, c, ctor) => {
                    // A ClassTest covers a leaf but carries no fields of its own.
                    if fields.len() == arity {
                        fields.clone()
                    } else {
                        vec![Pat::Wild; arity]
                    }
                }
                _ => return None,
            };
            new.extend_from_slice(tail);
            Some(new)
        })
        .collect()
}

/// `D(P)`: keep only wildcard-headed rows, dropping the head column.
fn default_matrix(matrix: &Matrix) -> Matrix {
    matrix
        .iter()
        .filter(|row| matches!(row.first(), Some(Pat::Wild)))
        .map(|row| row[1..].to_vec())
        .collect()
}

/// Rebuilds a witness row by folding the first `arity` entries under `ctor`.
fn reassemble(ctor: &Ctor, arity: usize, mut witness: Vec<Witness>) -> Vec<Witness> {
    let rest = witness.split_off(arity.min(witness.len()));
    let mut out = vec![Witness::Ctor {
        ctor: ctor.clone(),
        fields: witness,
    }];
    out.extend(rest);
    out
}

/// Witnesses of non-exhaustiveness for `matrix` over columns `col_types`.
///
/// An empty result means exhaustive. `Err(Bail)` means the analysis could not
/// be completed and nothing should be reported.
///
/// Terminates on recursive types: the split branch is taken only when every
/// constructor of the signature already appears in the head column, which
/// strictly reduces the number of constructor nodes in the matrix; the default
/// branch strictly reduces the column count. An all-wildcard column never
/// splits, so `List` never unrolls.
pub(super) fn missing_witnesses(
    cx: &PatCx<'_>,
    matrix: &Matrix,
    col_types: &[Type],
    depth: usize,
) -> Result<Vec<Vec<Witness>>, Bail> {
    if depth > MAX_DEPTH {
        return Err(Bail);
    }

    // Base case: the empty query is useful exactly when no row survived.
    let Some((head_type, rest_types)) = col_types.split_first() else {
        return Ok(if matrix.is_empty() {
            vec![vec![]]
        } else {
            vec![]
        });
    };

    let present = head_ctors(matrix);

    let constructors = cx.partitioned_ctor_set(head_type, &present);
    let opaque = matches!(constructors, CtorSet::Opaque);
    let (complete, missing): (Vec<Ctor>, Vec<Ctor>) = match constructors {
        CtorSet::Unknown => return Err(Bail),
        CtorSet::Opaque => (vec![], vec![Ctor::Single]), // never complete; placeholder
        CtorSet::Finite(all) => {
            let missing = all
                .iter()
                .filter(|c| !present.iter().any(|p| covers(cx, p, c)))
                .cloned()
                .collect::<Vec<_>>();
            (all, missing)
        }
    };

    if !opaque && missing.is_empty() {
        // The signature is complete: exhaustive iff exhaustive after
        // specializing on every constructor.
        let mut out = Vec::new();
        for ctor in complete {
            let field_types = cx.ctor_field_types(head_type, &ctor)?;
            let spec = specialize(cx, matrix, &ctor, field_types.len());
            let mut sub_types = field_types.clone();
            sub_types.extend_from_slice(rest_types);
            for w in missing_witnesses(cx, &spec, &sub_types, depth + 1)? {
                out.push(reassemble(&ctor, field_types.len(), w));
                if out.len() >= MAX_WITNESSES {
                    return Ok(out);
                }
            }
        }
        return Ok(out);
    }

    // The signature is incomplete. Maranget: U(P, (_ q)) <=> U(D(P), q), so the
    // default matrix answers it — no need to descend into present constructors.
    let sub = missing_witnesses(cx, &default_matrix(matrix), rest_types, depth + 1)?;
    if sub.is_empty() {
        return Ok(vec![]);
    }

    let mut out = Vec::new();
    if opaque {
        // Nothing enumerable to name; `_` stands for every uncovered value.
        for w in &sub {
            let mut row = vec![Witness::Wild];
            row.extend(w.iter().cloned());
            out.push(row);
            if out.len() >= MAX_WITNESSES {
                break;
            }
        }
        return Ok(out);
    }

    'outer: for ctor in missing.iter() {
        let arity = cx
            .ctor_field_types(head_type, ctor)
            .map(|f| f.len())
            .unwrap_or(0);
        for w in &sub {
            let mut row = vec![Witness::Ctor {
                ctor: ctor.clone(),
                fields: vec![Witness::Wild; arity],
            }];
            row.extend(w.iter().cloned());
            out.push(row);
            if out.len() >= MAX_WITNESSES {
                break 'outer;
            }
        }
    }
    Ok(out)
}
