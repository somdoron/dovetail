# Dovetail Language Grammar

This document defines the context-free grammar (CFG) of the Dovetail programming language.

## Notation

- `UPPER_CASE` — Terminal symbols (tokens)
- `lower_case` — Non-terminal symbols (grammar rules)
- `|` — Alternation (choice)
- `[ ... ]` — Optional (zero or one)
- `{ ... }` — Repetition (zero or more)
- `( ... )` — Grouping
- `"..."` — Literal keyword or symbol
- `/* ... */` — Comments in grammar

---

## Lexical Grammar

### Whitespace and Comments

```
WHITESPACE      = ( SPACE | TAB )+
NEWLINE         = "\n" | "\r\n"
COMMENT         = "//" { any-char-except-newline }
DOC_COMMENT     = "///" { any-char-except-newline }
```

**Note:** Tabs in indentation are compile errors (spaces only for indentation).

### Keywords

```
FUNCTION    = "function"
LET         = "let"
MUTABLE     = "mutable"
IF          = "if"
THEN        = "then"
ELSE        = "else"
MATCH       = "match"
CASE        = "case"
WITH        = "with"
FOR         = "for"
WHILE       = "while"
IN          = "in"
DO          = "do"
RECORD      = "record"
ENUM        = "enum"
CLASS       = "class"
TRAIT       = "trait"
INTERFACE   = "interface"
IMPLEMENT   = "implement"
EXTENSION   = "extension"
ABSTRACT    = "abstract"
STATIC      = "static"
PUBLIC      = "public"
PRIVATE     = "private"
INTERNAL    = "internal"
IMPORT      = "import"
PACKAGE     = "package"
ASYNC       = "async"
COROUTINE   = "coroutine"
TEST        = "test"
NEWTYPE     = "newtype"
TYPE        = "type"
USE         = "use"
PANIC       = "panic"
ASSERT      = "assert"
EXTENDS     = "extends"
IMPLEMENTS  = "implements"
AND_KW      = "and"         /* for multiple traits: implements A and B */
WHERE       = "where"
AS          = "as"
CONSTRUCTOR = "constructor"
BEGIN_KW    = "begin"       /* explicit begin keyword */
END_KW      = "end"         /* explicit end keyword */
```

### Literals

```
INT_LIT     = digit { digit } [ INT_SUFFIX ]
            | "0x" hex_digit { hex_digit } [ INT_SUFFIX ]
            | "0b" bin_digit { bin_digit } [ INT_SUFFIX ]
            | "0o" oct_digit { oct_digit } [ INT_SUFFIX ]

INT_SUFFIX  = "i8" | "i16" | "i32" | "i64"
            | "u8" | "u16" | "u32" | "u64"

FLOAT_LIT   = digit { digit } "." digit { digit } [ exponent ] [ FLOAT_SUFFIX ]
            | digit { digit } exponent [ FLOAT_SUFFIX ]

FLOAT_SUFFIX = "f32" | "f64"

BIGINT_LIT  = digit { digit } "big"
            | "0x" hex_digit { hex_digit } "big"
            | "0b" bin_digit { bin_digit } "big"
            | "0o" oct_digit { oct_digit } "big"
DECIMAL_LIT = digit { digit } [ "." digit { digit } ] [ exponent ] "dec"

exponent    = ( "e" | "E" ) [ "+" | "-" ] digit { digit }

BOOL_LIT    = "true" | "false"

STRING_LIT  = '"' { string_char | escape_seq | interpolation } '"'
            | '"""' { any_char } '"""'          /* multi-line string */

string_char = any_char_except( '"' | '\' | '$' | newline )

escape_seq  = '\' ( 'n' | 'r' | 't' | '\' | '"' | '$' | '0' )
            | '\u' '{' hex_digit { hex_digit } '}'

interpolation = '$' IDENT
              | '${' expression '}'

/* A prefixed string literal: an identifier immediately followed (no
   whitespace) by a string. The prefix must not be a keyword, and `test` is
   reserved for test declarations. The prefix is an ordinary name in scope,
   declared with `@stringLiteral`, naming the builder type the literal is
   lowered onto; see book/24-prefixed-literals.md. */
PREFIXED_STRING_LIT
            = IDENT '"' { string_char | escape_seq | prefixed_interpolation } '"'
            | IDENT '"""' { any_char | escape_seq | prefixed_interpolation } '"""'

prefixed_interpolation
            = interpolation                     /* -> builder.value(expr) */
            | '$..' IDENT                       /* -> builder.spread(expr) */
            | '$..{' expression '}'

CHAR_LIT    = "'" ( char | escape_seq ) "'"

digit       = '0'..'9'
hex_digit   = '0'..'9' | 'a'..'f' | 'A'..'F'
bin_digit   = '0' | '1'
oct_digit   = '0'..'7'
```

### Identifiers

```
IDENT       = ident_start { ident_continue }

ident_start    = 'a'..'z' | 'A'..'Z' | '_'
ident_continue = ident_start | '0'..'9'
```

### Operators

```
/* Arithmetic */
PLUS        = "+"
PLUS_PLUS   = "++"          /* concatenation; see the Concat trait */
MINUS       = "-"
STAR        = "*"
SLASH       = "/"
PERCENT     = "%"

/* Comparison */
EQ          = "=="
NE          = "!="
LT          = "<"
LE          = "<="
GT          = ">"
GE          = ">="

/* Logical */
AND         = "&&"
OR          = "||"
NOT         = "!"

/* Bitwise */
BIT_AND     = "&"
BIT_OR      = "|"
BIT_XOR     = "^"
BIT_NOT     = "~"
SHL         = "<<"
SHR         = ">>"

/* Other */
RANGE       = ".."
RANGE_INCL  = "..="
```

### Punctuation

```
LPAREN      = "("
RPAREN      = ")"
LBRACKET    = "["
RBRACKET    = "]"
LBRACKET_PIPE = "[|"    /* opens an array literal */
PIPE_RBRACKET = "|]"    /* closes an array literal; requires adjacency */
LBRACE      = "{"
RBRACE      = "}"
LANGLE      = "<"
RANGLE      = ">"
COMMA       = ","
COLON       = ":"
COLONCOLON  = "::"     /* cons; right-associative */
SEMICOLON   = ";"
ASSIGN      = "="
FAT_ARROW   = "=>"     /* match arms */
ARROW       = "->"     /* lambdas (layout opener) */
DOT         = "."
DOTDOT      = ".."
AT          = "@"
UNDERSCORE  = "_"
QUESTION    = "?"
PIPE        = "|"
```

### Layout Tokens (Virtual)

These tokens are inserted by the layout filter, not present in source:

```
BEGIN       /* Opens an indentation block */
END         /* Closes an indentation block */
SEP         /* Separates expressions in a seq-block */
```

**Note:** The explicit semicolon `;` in source code produces the same `SEP` token.

**Layout Openers:** Tokens that trigger block opening:
- `=` (assignment/definition)
- `then`
- `else`
- `with`
- `->` (lambda arrow)
- `{` (record construction)

---

## Syntactic Grammar

### Program Structure

```
program             = { declaration }

declaration         = package_decl
                    | import_decl
                    | function_decl
                    | record_decl
                    | enum_decl
                    | class_decl
                    | trait_decl
                    | interface_decl
                    | implement_decl
                    | extension_decl
                    | newtype_decl
                    | type_alias_decl
                    | test_decl
```

### Package and Imports

```
package_decl        = "package" package_path

import_decl         = "import" import_path [ "as" IDENT ]

package_path        = IDENT { "." IDENT }

import_path         = IDENT { "." IDENT }
```

### Type Declarations

#### Attributes

Attributes precede the visibility keyword on the declarations that accept them.

```
declaration_attrs   = { derive_attr } [ string_literal_attr ]

/* Applies a derive macro; see book/16-macros.md.
   Allowed on: record, enum, newtype. */
derive_attr         = "@" "derive" "(" IDENT { "." IDENT } ")"

/* Marks the declared name as the prefix of a prefixed string literal, so
   `IDENT"..."` is available wherever the name is in scope. Takes no argument —
   the prefix IS the name. Allowed on: type alias, record, class.
   See book/24-prefixed-literals.md. */
string_literal_attr = "@" "stringLiteral"
```

#### Records

```
record_decl         = [ declaration_attrs ] [ doc_comment ] [ visibility ] "record" IDENT [ variant_type_params ] [ "private" ] [ where_clause ] [ "=" record_body ]

record_body         = BEGIN { record_field SEP } record_field [ SEP ] END

record_field        = [ doc_comment ] IDENT ":" type
```

#### Enums

```
enum_decl           = [ doc_comment ] [ visibility ] "enum" IDENT [ variant_type_params ] [ "private" ] [ where_clause ] "=" enum_body

enum_body           = BEGIN { enum_variant SEP } enum_variant [ SEP ] END

enum_variant        = [ doc_comment ] IDENT [ "(" type_list ")" ]

type_list           = type { "," type }

```

#### Classes

```
class_decl          = [ doc_comment ] [ "abstract" ] "class" IDENT [ type_params ]
                      [ constructor_params ]
                      [ "extends" type [ call_args ] ]
                      [ "implements" trait_list ]
                      [ "=" class_body ]

trait_list          = type { "and" type }

constructor_params  = "(" [ constructor_param { "," constructor_param } ] ")"

constructor_param   = [ visibility ] [ "mutable" ] IDENT ":" type [ "=" expression ]

class_body          = BEGIN { class_member SEP } class_member [ SEP ] END
                    | class_member { SEP class_member }

class_member        = field_decl
                    | method_decl
                    | static_method_decl
                    | secondary_constructor

field_decl          = [ doc_comment ] [ visibility ] [ "mutable" ] IDENT ":" type

method_decl         = [ doc_comment ] [ visibility ] "function" IDENT
                      [ type_params ] "(" [ param_list ] ")" [ ":" type ]
                      [ where_clause ] "=" block_expr

static_method_decl  = [ doc_comment ] [ visibility ] "static" "function" IDENT
                      [ type_params ] "(" [ param_list ] ")" [ ":" type ]
                      [ where_clause ] "=" block_expr

secondary_constructor = "constructor" "(" [ param_list ] ")" "=" constructor_call

constructor_call    = IDENT "(" [ arg_list ] ")"
```

#### Traits

```
trait_decl          = [ doc_comment ] "trait" IDENT [ type_params ]
                      [ "extends" named_type { "and" named_type } ]
                      [ "=" trait_body ]

interface_decl      = [ doc_comment ] "interface" IDENT [ type_params ]
                      [ "extends" named_type { "and" named_type } ]  (* interfaces only *)
                      [ "=" trait_body ]

trait_body          = BEGIN { trait_member SEP } trait_member [ SEP ] END

trait_member        = trait_method | trait_property | trait_assoc_type

(* an "=" body on a trait method or property is a default implementation
   (trait-design-appendix §4); implementors may omit such members *)
trait_method        = [ doc_comment ] "function" IDENT [ type_params ]
                      "(" [ param_list ] ")" [ ":" type ] [ where_clause ]
                      [ "=" block_expr ]

trait_assoc_type    = "type" IDENT [ type_params ]
```

#### Trait Implementations

```
implement_decl      = [ doc_comment ] "implement" [ type_params ]
                      trait_type "for" type [ "as" IDENT ] [ where_clause ] [ "=" impl_body ]

impl_body           = BEGIN { impl_member SEP } impl_member [ SEP ] END

impl_member         = method_decl | property_decl | assoc_type_def

assoc_type_def      = "type" IDENT [ type_params ] "=" type
```

#### Extensions

```
extension_decl      = [ doc_comment ] "extension" [ IDENT ] [ type_params ]
                      "for" type [ where_clause ] [ "=" extension_body ]

extension_body      = BEGIN { extension_member SEP } extension_member [ SEP ] END

extension_member    = method_decl | property_decl

property_decl       = [ visibility ] "let" "property" IDENT ":" type "=" expression

/* File-as-extension: if file starts with 'extension for Type',
   the whole file is that extension (no = or body needed) */
```

#### Newtypes

```
newtype_decl        = [ doc_comment ] [ visibility ] "newtype" IDENT [ variant_type_params ] [ "private" ] [ where_clause ] "=" type
```

#### Type Aliases

```
type_alias_decl     = [ string_literal_attr ] [ doc_comment ] [ visibility ] "type" IDENT [ type_params ] "=" type
```

### Functions

```
function_decl       = [ doc_comment ] [ visibility ] [ function_modifier ]
                      "function" IDENT [ type_params ]
                      "(" [ param_list ] ")" [ ":" return_type ]
                      [ where_clause ] "=" block_expr

function_modifier   = "async"
                    | "coroutine"
                    | "async" "coroutine"

return_type         = type
                    | type "," type      /* async return: T, E */

param_list          = param { "," param }

param               = IDENT ":" type [ "=" expression ]

/* Default parameter expressions are planned, not yet implemented. */

where_clause        = "where" constraint { "," constraint }

constraint          = IDENT ":" type_bound { "+" type_bound }

type_bound          = "class" | IDENT [ "<" bound_arguments ">" ]
bound_arguments     = type { "," type } [ "," associated_bindings ] | associated_bindings
associated_bindings = IDENT "=" type { "," IDENT "=" type }
```

### Tests

```
test_decl           = { test_attribute } "test" STRING_LIT [ ":" type ] "=" block_expr

test_attribute      = "@" IDENT [ "(" attr_arg ")" ]

attr_arg            = STRING_LIT | INT_LIT
```

### Type Parameters and Generics

```
type_params         = "<" type_param { "," type_param } ">"

type_param          = IDENT [ ":" type_bound { "+" type_bound } ]

variant_type_params = "<" variant_type_param { "," variant_type_param } ">"

variant_type_param  = [ "out" | "in" ] IDENT [ ":" type_bound { "+" type_bound } ]
```

Note: `variant_type_params` (with optional variance prefix) are used in `record_decl` and `enum_decl`. All other declarations use `type_params`.

### Types

```
type                = tuple_extension_type [ "=>" type ]
                    | "(" [ type { "," type } ] ")" "=>" type

tuple_extension_type = atomic_type { "~" atomic_type }

atomic_type         = named_type
                    | tuple_type
                    | "(" type ")"

named_type          = IDENT { "." IDENT } [ type_args ]

type_args           = "<" type { "," type } ">"

tuple_type          = "(" type "," type { "," type } ")"
                    | "(" named_tuple_field { "," named_tuple_field } ")"

named_tuple_field   = IDENT ":" type

function_type       = "(" [ type { "," type } ] ")" "=>" type
                    | type "=>" type
```

A dotted type name whose root is an in-scope type parameter denotes an associated
type through that parameter's bounds: `P.Output` or `W.Wrapped<T>`. Otherwise,
resolve it as a package-qualified type name. Associated references must match
the declaration's type-parameter count and identify one declaring trait
application. See [Advanced Generics](book/26-advanced-generics.md#268-associated-outputs).

---

## Expressions

### Block Expressions

A block expression contains one or more expressions, with layout determining structure:

```
block_expr          = BEGIN expr_sequence END
                    | "begin" expr_sequence "end"
                    | expression

expr_sequence       = expression { SEP expression }
                    | expression { ";" expression }
```

### Expression Precedence (lowest to highest)

1. `let`, `use`, `if`, `match`, `for`, `while` (binding/control)
2. `||` (logical or)
3. `&&` (logical and)
4. binary `~` (tuple extension; left-associative)
5. `==`, `!=`, `<`, `<=`, `>`, `>=` (comparison)
6. `|` (bitwise or)
7. `^` (bitwise xor)
8. `&` (bitwise and)
9. `<<`, `>>` (shift)
10. `+`, `-`, `++` (additive; `++` is concatenation)
11. `*`, `/`, `%` (multiplicative)
12. unary `!`, `-`, `~` (prefix)
13. postfix (call, field, index, method call, `orReturn`, coroutine operators)
14. primary (literals, identifiers, parenthesized)

### Expression Grammar

```
expression          = let_expr
                    | use_expr
                    | if_expr
                    | match_expr
                    | for_expr
                    | while_expr
                    | assignment_expr
                    | panic_expr
                    | assert_expr
                    | or_expr

/* Bindings */
let_expr            = "let" pattern [ ":" type ] "=" block_expr

use_expr            = "use" IDENT "=" expression

/* Control Flow */
if_expr             = "if" expression "then" block_expr [ "else" block_expr ]

match_expr          = "match" expression "with" match_body

match_body          = BEGIN { match_arm SEP } match_arm [ SEP ] END

match_arm           = "case" pattern [ "if" expression ] "=>" block_expr

for_expr            = "for" pattern "in" expression "do" block_expr

while_expr          = "while" expression "do" block_expr

/* Assignment */
assignment_expr     = postfix_expr "=" expression
                    | or_expr

/* Error Handling */
panic_expr          = "panic" expression

assert_expr         = "assert" expression [ "," expression ]

/* Binary Operators */
or_expr             = and_expr { "||" and_expr }

and_expr            = tuple_extension_expr { "&&" tuple_extension_expr }

tuple_extension_expr = equality_expr { "~" equality_expr }

equality_expr       = comparison_expr { ( "==" | "!=" ) comparison_expr }

comparison_expr     = bitor_expr { ( "<" | "<=" | ">" | ">=" ) bitor_expr }

bitor_expr          = bitxor_expr { "|" bitxor_expr }

bitxor_expr         = bitand_expr { "^" bitand_expr }

bitand_expr         = shift_expr { "&" shift_expr }

shift_expr          = additive_expr { ( "<<" | ">>" ) additive_expr }

additive_expr       = multiplicative_expr { ( "+" | "-" | "++" ) multiplicative_expr }

multiplicative_expr = unary_expr { ( "*" | "/" | "%" ) unary_expr }

/* Unary Operators */
unary_expr          = ( "!" | "-" | "~" | "try" ) unary_expr
                    | postfix_expr

/* Postfix Expressions */
postfix_expr        = primary_expr { postfix_op }

postfix_op          = "." IDENT                          /* field access */
                    | "." IDENT "(" [ arg_list ] ")"     /* method call */
                    | "(" [ arg_list ] ")"               /* function call */
                    | "[" expression "]"                 /* index */
                    | "[|" [ expression ] ".." [ expression ] "|]" /* slice */
                    | "[|" [ expression ] "..=" expression "|]"   /* inclusive slice */
                    | "." "orReturn"                     /* early return */
                    /* coroutine postfix operators like .andWait, .checkpoint */

/* Primary Expressions */
primary_expr        = INT_LIT
                    | BIGINT_LIT
                    | DECIMAL_LIT
                    | FLOAT_LIT
                    | STRING_LIT
                    | PREFIXED_STRING_LIT
                    | BOOL_LIT
                    | IDENT
                    | "(" ")"                            /* unit */
                    | "(" expression ")"                 /* parenthesized */
                    | tuple_expr
                    | array_expr
                    | list_expr
                    | record_construct_expr
                    | with_expr
                    | lambda_expr
                    | closure_expr
                    | async_do_expr

/* Deferred Awaitable Expressions */
async_do_expr       = "async" "do" block_expr

/* Closure Expressions */
closure_expr        = closure_params "=>" block_expr
closure_params      = IDENT
                    | "(" [ closure_param_list ] ")"
closure_param_list  = closure_param { "," closure_param }
closure_param       = IDENT [ ":" type ]

/* Composite Literals */
tuple_expr          = "(" expression "," expression { "," expression } ")"

array_expr          = "[|" [ expression { "," expression } [ "," ] ] "|]"

list_expr           = "[" [ expression { "," expression } [ "," ] ] "]"

/* `::` is right-associative; `a :: b :: []` is the same expression as `[a, b]`. */
cons_expr           = expression "::" expression

record_construct_expr = type_name "{" [ field_init_list ] "}"

field_init_list     = field_init { SEP field_init } [ SEP ]

field_init          = IDENT "=" block_expr

with_expr           = expression "with" with_body

with_body           = BEGIN { field_init SEP } field_init [ SEP ] END

/* Lambda */
lambda_expr         = lambda_params "->" block_expr

lambda_params       = IDENT                              /* single param, no parens */
                    | "(" [ lambda_param_list ] ")"

lambda_param_list   = lambda_param { "," lambda_param }

lambda_param        = IDENT [ ":" type ]

/* Arguments */
arg_list            = arg { "," arg }

arg                 = [ IDENT "=" ] expression           /* optional named arg */

/* Positional arguments precede named arguments. Each declared parameter is
   supplied exactly once; names bind against the statically visible declaration.
   Named arguments may be reordered, but eager evaluation follows source order.
   Function values and positional enum/newtype payloads have no argument labels. */

/* Type Name (for constructors) */
type_name           = [ package_path "." ] IDENT [ type_args ]
```

---

## Patterns

```
/* `cons_pattern` is the only infix pattern form. */
pattern             = pattern_atom [ "::" pattern ]

pattern_atom        = wildcard_pattern
                    | literal_pattern
                    | type_annotated_pattern
                    | ident_pattern
                    | constructor_pattern
                    | tuple_pattern
                    | record_pattern
                    | list_pattern

wildcard_pattern    = "_"

literal_pattern     = INT_LIT
                    | FLOAT_LIT
                    | STRING_LIT
                    | BOOL_LIT

type_annotated_pattern = IDENT ":" pattern_type

pattern_type        = named_type
                    | tuple_type
                    | "(" function_type ")"

ident_pattern       = IDENT

constructor_pattern = type_name "(" [ pattern { "," pattern } ] ")"

tuple_pattern       = "(" pattern "," pattern { "," pattern } ")"

record_pattern      = type_name [ type_args ] "{" [ field_pattern { "," field_pattern } ] "}"

/* `[]` and `[a, b]` match a `List`, desugaring to `Nil` / nested `Cons`. */
list_pattern        = "[" [ pattern { "," pattern } [ "," ] ] "]"

/* `h :: t` — right-associative, so `a :: b :: rest` nests to the right. */
cons_pattern        = pattern_atom "::" pattern

field_pattern       = IDENT [ "=" pattern ]
```

---

## Visibility

```
visibility          = "public"
                    | "private"
                    | "internal"
```

---

## Documentation Comments

```
doc_comment         = { DOC_COMMENT }
```

---

## Layout Rules Summary

Dovetail is a layout-sensitive language. The layout filter transforms raw tokens by inserting virtual tokens:

### Virtual Tokens

| Token | Meaning |
|-------|---------|
| `BEGIN` | Opens an indentation block |
| `END` | Closes an indentation block |
| `SEP` | Separates expressions in a seq-block |

### Layout Openers

The following tokens trigger block opening:

| Token | Context |
|-------|---------|
| `=` | After function/let/record field definitions |
| `then` | After `if` condition |
| `else` | After `then` block |
| `with` | After record in `with` expression, after `match` |
| `->` | After lambda parameters |
| `=>` | After closure parameters, match arm patterns |

### Block Types

**Seq-block (sequence block):**
- Created when layout opener is followed by a newline, then indented content
- Tokens at **same column** receive `SEP` (allows multiple expressions)
- Tokens at **lesser column** close the block with `END`

**Non-seq-block (single-expression block):**
- Created when layout opener is followed by content on the **same line**
- Tokens at **same or lesser column** close the block with `END`
- No `SEP` tokens are inserted

### Examples

**Seq-block:**
```dovetail
function foo() =
    let x = 5    -- BEGIN, block at column 5
    x + 1        -- SEP (same column)
-- END (outdentation)
```

Token stream: `function foo ( ) = BEGIN let x = 5 SEP x + 1 END`

**Non-seq-block:**
```dovetail
function foo() = 42    -- BEGIN, non-seq-block
function bar() = 7     -- END (closes foo), BEGIN (new for bar)
```

Token stream: `function foo ( ) = BEGIN 42 END function bar ( ) = BEGIN 7 END`

### Special Handling

**If-Then-Else:**
```dovetail
if x > 0 then
    1
else
    -1
```

Token stream: `if x > 0 then BEGIN 1 END else BEGIN -1 END`

**Explicit begin/end:**
```dovetail
function foo() = begin
    let x = 5
    x + 1
end
```

Token stream: `function foo ( ) = begin let x = 5 SEP x + 1 end`

---

## Operator Precedence Table

| Precedence | Operator | Associativity | Description |
|------------|----------|---------------|-------------|
| 1 (lowest) | `\|\|` | Left | Logical OR |
| 2 | `&&` | Left | Logical AND |
| 3 | `==` `!=` | Left | Equality |
| 4 | `<` `<=` `>` `>=` | Left | Comparison |
| 5 | `::` | **Right** | Cons (prepend to a list) |
| 6 | `\|` | Left | Bitwise OR |
| 7 | `^` | Left | Bitwise XOR |
| 8 | `&` | Left | Bitwise AND |
| 9 | `<<` `>>` | Left | Bit shift |
| 10 | `+` `-` `++` | Left | Additive / concatenation |
| 11 | `*` `/` `%` | Left | Multiplicative |
| 12 (highest) | `!` `-` `~` | Right (prefix) | Unary |

`::` sits below `+`/`++` and above comparison, so `x :: xs ++ ys` is
`x :: (xs ++ ys)` and `x :: xs == ys` is `(x :: xs) == ys`.

---

## Complete Example

```dovetail
package example

import http.Request
import http.Response

/// A user record
record User =
    name: String
    age: Int32

/// Extension for User
extension for User =
    function greet(self): String =
        "Hello, ${self.name}!"
    
    static function create(name: String): User =
        User { name = name; age = 0 }

/// A printable trait
trait Printable =
    function print(self): String

/// A base class
class Person(name: String) =
    function greet(self): String = "Hello, I'm ${self.name}"

/// A class with inheritance and trait implementation
class Employee(name: String, title: String) extends Person(name) implements Printable =
    function print(self): String = "${self.name} - ${self.title}"

/// Main entry point
function main(): Unit =
    let user = User.create("Alice")
    let greeting = user.greet()
    print(greeting)

test "user greeting" =
    let user = User { name = "Bob"; age = 30 }
    assert user.greet() == "Hello, Bob!"
```

**Token stream for `main` function:**
```
function main ( ) : Unit = BEGIN
  let user = User . create ( "Alice" ) SEP
  let greeting = user . greet ( ) SEP
  print ( greeting )
END
```

---

## Grammar Ambiguities and Resolutions

### 1. Less-than vs Type Parameter

`<` can be either comparison or start of type arguments:

**Resolution:** In type positions, `<` starts type arguments. In expression positions, use context:
- After type name in constructor: type arguments
- Otherwise: comparison operator

### 2. Arrow Operators

- `->` (arrow) is for lambdas and is a layout opener
- `=>` (fat arrow) is for match arms, closure bodies, and function types; also a layout opener

### 3. Semicolon vs Layout

In record construction, `;` can be used instead of layout-based SEP:
- `Point { x = 1; y = 2 }` (single line with `;`)
- Layout SEP for multi-line

### 4. Bracket after an Expression

A `[` immediately following an expression is always the index operator, never a
list literal: `xs[0]` indexes, `xs [0]` still indexes. A list literal is only
recognised where an expression may *start*.

`[|` following an expression starts a slice: `xs[|i..j|]`, `xs[|i..|]`,
`xs[|..j|]`, `xs[|..|]`, `xs[|i..=j|]`, or `xs[|..=j|]`. The receiver must
be an Array or Slice. `[|` is a single token and a range separator is required,
so `xs[|0|]` is an error. Element indexing remains `xs[0]`; `xs[0..2]` is an
error. Ranges are not first-class expressions. Array literals remain valid
where an expression may start.

### 5. `|]` vs `|` `]`

`|]` requires adjacency, exactly like `++` or `==`. An array literal whose last
element ends in a bitwise-or is written with a space — `[| a | b |]` is a
one-element array holding `a | b`. `[||]` is the empty array, not `[` `||` `]`,
because `[|` is consumed as one token.

---

## Reserved for Future

The following are reserved for potential future use:

- `do` (do-notation)
- `yield` (generators)
- `defer` (deferred execution)
- `macro` (metaprogramming)
- `module` (module system)
