use super::Inference;
use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName};
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

const BASE: u64 = 1_000_000_000;
// Bound compiler allocation for compact inputs such as 1e2147483647dec.
const MAX_COEFFICIENT_DIGITS: usize = 1_000_000;

struct ExactNumber {
    positive: bool,
    limbs: Vec<i32>,
    scale: Option<i32>,
}

impl Inference<'_> {
    pub(super) fn infer_exact_number(&mut self, text: &str, span: &Span) -> TypedExpr {
        let number = match decode(text) {
            Ok(number) => number,
            Err(message) => {
                self.diagnostics.error(span.clone(), message.to_string());
                return TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                };
            }
        };
        let sign = typed(
            TypedExprKind::BoolLiteral(number.positive),
            Type::Bool,
            span,
        );
        let elements = number
            .limbs
            .into_iter()
            .map(|limb| typed(TypedExprKind::Int32Literal(limb), Type::Int32, span))
            .collect();
        let magnitude = typed(
            TypedExprKind::ArrayLiteral { elements },
            Type::Array(Box::new(Type::Int32)),
            span,
        );
        let coefficient = self.exact_record(
            "BigInt",
            vec![("sign".into(), sign), ("magnitude".into(), magnitude)],
            span,
        );
        match number.scale {
            None => coefficient,
            Some(scale) => {
                let scale = typed(TypedExprKind::Int32Literal(scale), Type::Int32, span);
                self.exact_record(
                    "Decimal",
                    vec![("unscaled".into(), coefficient), ("scale".into(), scale)],
                    span,
                )
            }
        }
    }

    fn exact_record(
        &mut self,
        name: &str,
        fields: Vec<(String, TypedExpr)>,
        span: &Span,
    ) -> TypedExpr {
        let fqn = Fqn::from_dotted(&format!("standard.prelude.{name}")).unwrap();
        if self
            .registry
            .lookup_record_type(&fqn, &self.package_path, &self.current_file)
            .is_none()
        {
            self.diagnostics
                .error(span.clone(), format!("numeric literal requires '{fqn}'"));
            return typed(TypedExprKind::UnitLiteral, Type::Error, span);
        }
        let ty = Type::Record(fqn.clone(), MangledName::for_type(&fqn));
        typed(
            TypedExprKind::RecordCreate {
                fqn,
                fields,
                type_params: vec![],
            },
            ty,
            span,
        )
    }
}

fn typed(kind: TypedExprKind, ty: Type, span: &Span) -> TypedExpr {
    TypedExpr {
        kind,
        ty,
        span: span.clone(),
    }
}

fn decode(text: &str) -> Result<ExactNumber, &'static str> {
    let positive = !text.starts_with('-');
    let text = text.strip_prefix('-').unwrap_or(text);
    let (digits, radix, scale) = if let Some(body) = text.strip_suffix("big") {
        let (digits, radix) = match body.get(..2) {
            Some("0x" | "0X") => (&body[2..], 16),
            Some("0b" | "0B") => (&body[2..], 2),
            Some("0o" | "0O") => (&body[2..], 8),
            _ => (body, 10),
        };
        (digits.to_string(), radix, None)
    } else if let Some(body) = text.strip_suffix("dec") {
        let (digits, scale) = decimal_digits(body)?;
        (digits, 10, Some(scale))
    } else {
        return Err("invalid exact numeric suffix; expected 'big' or 'dec'");
    };
    let limbs = parse_limbs(&digits, radix)?;
    let zero = limbs == [0];
    Ok(ExactNumber {
        positive: positive || zero,
        limbs,
        scale: scale.map(|s| if zero { 0 } else { s }),
    })
}

fn decimal_digits(body: &str) -> Result<(String, i32), &'static str> {
    let (mantissa, exponent) = decimal_exponent(body)?;
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
        || (mantissa.contains('.') && fraction.is_empty())
    {
        return Err("invalid decimal literal; expected base-10 digits");
    }
    let mut digits = format!("{whole}{fraction}")
        .trim_start_matches('0')
        .to_string();
    let mut scale = (fraction.len() as i64)
        .checked_sub(exponent)
        .ok_or("decimal scale out of range")?;
    if digits.is_empty() {
        return Ok(("0".into(), 0));
    }
    while scale > 0 && digits.ends_with('0') {
        digits.pop();
        scale -= 1;
    }
    if scale < 0 {
        let zeros = scale
            .checked_neg()
            .and_then(|v| usize::try_from(v).ok())
            .ok_or("decimal coefficient exceeds compiler resource limit")?;
        if zeros > MAX_COEFFICIENT_DIGITS.saturating_sub(digits.len()) {
            return Err("decimal coefficient exceeds compiler resource limit (1000000 digits)");
        }
        digits.extend(std::iter::repeat_n('0', zeros));
        scale = 0;
    }
    let scale =
        i32::try_from(scale).map_err(|_| "decimal scale out of range (maximum 2147483647)")?;
    Ok((digits, scale))
}

fn decimal_exponent(body: &str) -> Result<(&str, i64), &'static str> {
    match body.find(['e', 'E']) {
        Some(index) => {
            let exponent = &body[index + 1..];
            let unsigned = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            if unsigned.is_empty() || !unsigned.bytes().all(|b| b.is_ascii_digit()) {
                return Err("invalid decimal exponent");
            }
            let exponent = exponent
                .parse::<i64>()
                .map_err(|_| "decimal exponent out of range")?;
            Ok((&body[..index], exponent))
        }
        None => Ok((body, 0)),
    }
}

fn parse_limbs(digits: &str, radix: u32) -> Result<Vec<i32>, &'static str> {
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_digit(radix)) {
        return Err("invalid bigint digits; fractional and exponent forms are not supported");
    }
    if digits.len() > MAX_COEFFICIENT_DIGITS {
        return Err("numeric coefficient exceeds compiler resource limit (1000000 digits)");
    }
    if radix == 10 {
        let mut limbs: Vec<i32> = digits
            .as_bytes()
            .rchunks(9)
            .map(|chunk| {
                chunk
                    .iter()
                    .fold(0, |value, digit| value * 10 + i32::from(digit - b'0'))
            })
            .collect();
        while limbs.len() > 1 && limbs.last() == Some(&0) {
            limbs.pop();
        }
        return Ok(limbs);
    }
    let mut limbs = vec![0i32];
    for digit in digits.chars() {
        let mut carry = u64::from(digit.to_digit(radix).unwrap());
        for limb in &mut limbs {
            let value = *limb as u64 * u64::from(radix) + carry;
            *limb = (value % BASE) as i32;
            carry = value / BASE;
        }
        if carry != 0 {
            limbs.push(carry as i32);
        }
    }
    Ok(limbs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_coefficients_and_normalization() {
        assert_eq!(
            decode("123456789012345678901234567890big").unwrap().limbs,
            vec![234567890, 345678901, 456789012, 123]
        );
        assert_eq!(decode("0xFFbig").unwrap().limbs, vec![255]);
        assert_eq!(decode("1.200dec").unwrap().scale, Some(1));
        assert_eq!(decode("1.25e3dec").unwrap().limbs, vec![1250]);
        assert!(decode("-0dec").unwrap().positive);
        assert_eq!(decode("1e-2147483647dec").unwrap().scale, Some(i32::MAX));
    }

    #[test]
    fn rejects_malformed_and_unbounded_literals() {
        for text in [
            "1.2big",
            "1e3big",
            "0b2big",
            "0x0bigfoo",
            "1e+dec",
            "1.2.3dec",
            "1e-2147483648dec",
            "1e2147483647dec",
            "1e-9223372036854775808dec",
        ] {
            assert!(decode(text).is_err(), "{text}");
        }
    }
}
