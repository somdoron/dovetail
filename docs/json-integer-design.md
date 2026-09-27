# Exact JSON integer design proposal

Status: proposed representation change; strict Float64-to-integer decoding is
implemented independently. The public representation change awaits user review.

## Findings

The original parser accumulated every numeric token into Float64. Both Int32 and
Int64 encoders converted their input to Float64, and their decoders unconditionally
called the float-to-integer conversion. That conversion truncates toward zero and
uses trapping WebAssembly instructions when the truncated value is unrepresentable
(or the input is NaN/infinite). Returning `Ok` around it does not catch a trap.

Int32 values themselves fit exactly in Float64, but fractional input was truncated
and out-of-range input could trap. Int64 values above the binary64 exact-integer
range can change during encoding or parsing; 9007199254740993 cannot be represented
as Float64, and Int64.MAX rounds to 2^63. Int64.MIN is representable as Float64 and passed the targeted full wire round-trip
experiment. Int64.MAX and 9007199254740993 failed that experiment. The fractional
token `1.0000000000000001` was accepted as 1 even with strict float checks.
Derived implementations delegate to these primitive instances, inheriting their
limitations.

The independent decoder fix introduces `JsonError.NonIntegralNumber` and
`JsonError.OutOfRange(String)`. It checks the range before converting and checks
integrality afterward. The Int64 upper bound is exclusive 2^63, since converting
Int64.MAX to Float64 produces that out-of-range bound. This fixes conversions from
the supplied float, but cannot detect rounding that already happened in parsing
or encoding. In particular, a fractional token that rounds to an integral float
can still be accepted. Non-finite floats return OutOfRange.

## Recommended representation

Retain `Json.Number(Float64)` and add `Json.ExactNumber(JsonNumber)`.
`JsonNumber` should be a public record with private construction, retaining a
validated JSON numeric token. Its module exposes a checked text constructor and
exact Int32/Int64 constructors; callers can inspect its text but cannot forge or
modify it. This avoids an unchecked raw-string variant that can emit invalid JSON.

All parsed numeric tokens use ExactNumber, without an intermediate Float64.
Int32/Int64 encoders also use ExactNumber. Serialization emits the validated text;
no float conversion occurs anywhere in an integer round trip. Retaining the
original token also preserves numbers outside Int64 and Float64 ranges in the tree.

Integer decoding normalizes decimal digits and exponent directly. Accept a value
if it is mathematically integral and in the target range: `1.0`, `10e-1`, and
`1e3` are integers; `1.5` and `1e-3` are not. Check discarded decimal digits before
range checking, and accumulate negatively with checked bounds to include Int64.MIN.
Bound exponent accumulation using token length and target range, with explicit
zero handling, so huge exponents cannot overflow counters or cause unbounded
power-of-ten loops. Reject nonintegral values and overflow with typed errors.

Float64 decoding accepts both variants. Number returns its supplied float;
ExactNumber converts text with documented binary64 rounding. Float encoders keep
using Number. Conversion to floating point is explicitly allowed to lose precision;
it must never be used as the intermediate for exact integer decoding.

Derived record encoders/decoders already dispatch through primitive traits, so
ordinary integer fields need no wrapper and no macro changes. Regression tests
must exercise the complete toJson → encode → parse → fromJson path, including
both integer extrema and values on both sides of ±2^53.

## Compatibility

Existing `Json.Number(1.0)` construction stays valid and strict integer decoding
continues accepting integral, in-range floats. Float64.fromJson is the preferred
representation-independent numeric reader. `typeName()` returns "Number" for both.

Adding a variant affects exhaustive matches. Code that matches only Number will
no longer recognize parsed numbers or encoded integer fields; it must use a typed
decoder or handle ExactNumber. Serialized integer text stays ordinary unquoted
JSON numbers. Parsed numeric spelling may now be preserved instead of normalized.
Adding error variants also affects exhaustive JsonError matches. These are public
compatibility changes and should be called out in release notes.

A replacement `Number(JsonNumber)` is conceptually cleaner but breaks existing
construction as well as matching; the additive design limits that disruption.
Adding only `Integer(Int64)` is insufficient: out-of-range and fractional/exponent
text still cannot be checked exactly after a Float64 fallback.

## YAML

YAML already has `YamlValue.Integer(Int64)`, checked negative accumulation, typed
Int64 parse overflow, and checked Int32 narrowing. Float nodes are rejected for
integer targets with TypeMismatch, including mathematically integral `1.0` and
`1e3`. Derived integer fields inherit these guarantees. Its Float64 decoder
intentionally rounds integer nodes. No YAML encoder exists, so an encoding and
serialization round-trip guarantee would require separate new functionality.
No YAML representation or decoding change is needed for exact integer tokens.
