# Layout Rules

Dovetail is a layout-sensitive language, using indentation to delimit blocks rather than explicit braces or keywords. This design follows in the tradition of Haskell, F#, and Python, based on Peter Landin's "offside rule" from his 1966 paper "The Next 700 Programming Languages."

## Terminology

- **Offside line**: A vertical line at the column where a block begins. Tokens appearing to the left of this line are "offside" and close the block.
- **Layout opener**: A token that starts a new indentation context (`=`, `then`, `else`, `with`, `->`, `{`).
- **Block**: A region of code delimited by `Begin` and `End` tokens. Blocks come in two varieties:
  - **Seq-block** (sequence block): Allows multiple expressions at the same indentation level, separated by `Sep` tokens.
  - **Non-seq-block** (single-expression block): Contains exactly one expression; no `Sep` tokens are inserted.
- **Continuation**: A line that continues the previous expression (not starting a new statement).

## Implementation Approach

Dovetail uses a **lexer filter** to transform raw tokens into layout-aware tokens. The filter inserts three virtual tokens:

- **`Begin`**: Opens an indentation block (emitted after a layout opener when indentation follows)
- **`End`**: Closes an indentation block (emitted on outdentation)
- **`Sep`**: Separates expressions within a seq-block (emitted when a new line starts at the block's column)

This approach follows Haskell's model of inserting virtual `{`, `}`, and `;` tokens, making the grammar cleaner and the parser simpler since it can treat layout-inferred blocks identically to explicit blocks.

---

## Implementation Guide

This section provides detailed instructions for implementing the layout filter.

### Architecture Overview

```
┌─────────┐     ┌───────────────┐     ┌────────┐
│  Lexer  │ ──> │ Layout Filter │ ──> │ Parser │
└─────────┘     └───────────────┘     └────────┘
     │                  │
     │                  │
  Raw tokens      Tokens with
  (includes       Begin/End/Sep
   Newline)       (no Newline)
```

The layout filter:
1. Consumes raw tokens from the lexer (including `Newline` tokens)
2. Tracks indentation context using a stack
3. Emits tokens with virtual `Begin`, `End`, `Sep` inserted
4. Strips `Newline` tokens (parser never sees them)

### Data Structures

```rust
/// Represents an indentation context on the stack
struct OffsideContext {
    /// Column number where this block starts (1-indexed)
    column: u32,
    
    /// True if this is a sequence block (allows Sep tokens)
    /// Note: All implicit blocks are seq-blocks; same-line expressions don't create blocks
    is_seq_block: bool,
    
    /// True if this context was opened by explicit `begin` keyword
    is_explicit: bool,
}

/// Pending context info - set when a layout opener is seen
struct PendingContext {
    /// Line number where the opener appeared
    line: u32,
    /// Whether the opener was an explicit `begin` keyword
    is_explicit: bool,
}

/// The layout filter state
struct LayoutFilter {
    /// Input tokens from lexer
    tokens: Vec<Token>,
    
    /// Current position in token stream
    pos: usize,
    
    /// Stack of active indentation contexts
    /// Top of stack = innermost/current context
    context_stack: Vec<OffsideContext>,
    
    /// Queue of tokens to emit before the next raw token
    /// Used when we need to emit multiple tokens (End, Sep, etc.)
    pending_tokens: VecDeque<Token>,
    
    /// When Some, the next non-newline token establishes a new block
    /// Contains the opener line and whether it's explicit
    pending_context: Option<PendingContext>,
    
    /// Last token's span (for generating virtual token spans)
    last_span: Span,
}
```

### State Tracking

The filter must track:

| State | Purpose |
|-------|---------|
| `context_stack` | Stack of active blocks with their offside columns |
| `pending_context` | Set when a layout opener is seen; cleared when next token establishes the block |
| `pending_tokens` | Queue of tokens to emit (for multi-token insertions like `End End Sep`) |
| `saw_explicit_begin` | Prevents double Begin when user writes explicit `begin` |
| Current line/column | From token spans, used to detect newlines and alignment |

### Algorithm: Main Loop

```
function next_token():
    # 1. Return pending tokens first
    if pending_tokens is not empty:
        return pending_tokens.pop_front()
    
    loop:
        token = read_next_raw_token()
        
        # 2. Skip newlines (they're only used for line tracking)
        if token is Newline:
            continue
        
        # 3. Handle EOF - close all open contexts
        if token is EOF:
            emit_all_end_tokens()
            pending_tokens.push(token)
            return pending_tokens.pop_front()
        
        col = token.column
        line = token.line
        
        # 4. Establish pending context if any
        if pending_context is Some(opener_line):
            pending_context = None
            establish_new_context(token, opener_line)
        
        # 5. Handle 'else' specially (closes then-block, opens else-block)
        if token is Else and context_stack is not empty:
            handle_else(token)
            return token
        
        # 6. Check offside rules against current context
        if context_stack is not empty:
            ctx = context_stack.top()
            
            if should_close_block(col, ctx):
                close_offside_contexts(col)
                # May have queued End tokens and possibly Sep
            elif should_insert_sep(col, ctx):
                pending_tokens.push(token)
                return make_sep_token()
        
        # 7. Check if this token is a layout opener
        if is_layout_opener(token) and not followed_by_explicit_begin():
            pending_context = Some(line)
        
        return token
```

### Algorithm: Establishing a New Context

```
function establish_new_context(token, opener_line, is_explicit):
    token_line = token.line
    token_col = token.column
    
    # Check if this is a multi-line block (newline after opener)
    is_multi_line = (token_line > opener_line)
    
    # For same-line expressions (no newline after opener), don't create a block
    # Just pass through the token - the parser handles single expressions
    if not is_multi_line and not is_explicit:
        pending_tokens.push_back(token)
        return
    
    # Multi-line block: create a seq-block context
    offside_col = token_col
    
    # Push the new context
    context_stack.push(OffsideContext {
        column: offside_col,
        is_seq_block: true,
        is_explicit: is_explicit
    })
    
    # Emit Begin token before the current token (only for implicit blocks)
    if not is_explicit:
        pending_tokens.push_back(make_begin_token())
    pending_tokens.push_back(token)
    
    # Check if this token is also a layout opener
    if is_layout_opener(token):
        pending_context = Some(token_line)
```

### Algorithm: Closing Contexts on Outdentation

```
function should_close_block(col, ctx) -> bool:
    # All implicit blocks are seq-blocks (same-line expressions don't create blocks)
    return col < ctx.column  # Close when column is strictly less

function close_offside_contexts(col):
    while context_stack is not empty:
        ctx = context_stack.top()
        if should_close_block(col, ctx) and not ctx.is_explicit:
            context_stack.pop()
            pending_tokens.push(make_end_token())
        else:
            break
    
    # After closing, check if we need Sep at the remaining context
    if context_stack is not empty:
        ctx = context_stack.top()
        if col == ctx.column and not ctx.is_explicit:
            pending_tokens.push(make_sep_token())
```

### Algorithm: Separator Insertion

```
function should_insert_sep(col, ctx) -> bool:
    return col == ctx.column and ctx.is_seq_block
```

When a new line starts at exactly the block's column in a seq-block, emit `Sep` before that line's first token.

### Algorithm: Handling 'else'

The `else` keyword requires special treatment:

```
function handle_else(else_token):
    # Close the then-block with End
    if context_stack is not empty:
        context_stack.pop()
        pending_tokens.push_front(make_end_token())
    
    # Open a new context for the else-block
    pending_context = Some(else_token.line)
```

### Algorithm: Handling Explicit 'begin'

When the user writes explicit `begin` after a layout opener:

```
function followed_by_explicit_begin() -> bool:
    # Peek at next non-newline token
    next = peek_next_non_newline()
    return next is Begin keyword
```

If explicit `begin` follows a layout opener, don't set `pending_context`. The explicit `begin` will be passed through as a regular token.

### Algorithm: Handling Explicit 'end'

When the user writes an explicit `end` keyword, it must be handled specially because:
1. It may appear at a column that would close implicit blocks
2. It needs to pass through to the parser to match an explicit `begin`
3. The layout filter does NOT track explicit begin/end pairs - that's the parser's job

**Rule:** Explicit `end` triggers normal outdentation, then passes through as a token.

```
# In main loop, handle explicit 'end' before normal offside processing:
if token is End keyword (explicit):
    col = token.column
    
    # Close any implicit blocks at columns > col
    close_offside_contexts(col)
    
    # Pass through the explicit 'end' for the parser
    pending_tokens.push(token)
    return pending_tokens.pop_front()
```

**Example:**

```
function foo() =
    let x = begin
        5
    end          # Explicit end at column 5
    x + 1
```

Processing `end` at column 5:
1. Function body block is at column 5 (seq-block)
2. Column 5 == 5, seq-block → no close (would be Sep for normal tokens)
3. But explicit `end` doesn't get Sep treatment - just pass through
4. `x + 1` at column 5 gets `Sep`

**Token stream:** `function foo ( ) = Begin let x = begin 5 end Sep x + 1 End Eof`

**Example with outdentation:**

```
function foo() =
    if true then
        let x = begin
            5
        end      # Explicit end at column 9
    7            # Column 5 closes then-block
```

Processing `end` at column 9:
1. Then-block is at column 9 (seq-block)
2. Column 9 == 9, seq-block → would be Sep, but explicit `end` just passes through

**Token stream:** `function foo ( ) = Begin if true then Begin let x = begin 5 end End Sep 7 End Eof`

**Key insight:** Explicit `end`:
- Does NOT pop any context from the stack (it's not closing an implicit block)
- Triggers outdentation for implicit blocks at greater columns
- Passes through for parser to match with explicit `begin`

### Layout Opener Detection

```
function is_layout_opener(token) -> bool:
    match token.kind:
        Assign (=)  -> true   # After 'let x =', 'function f() ='
        Then        -> true   # After 'if cond then'
        Else        -> true   # After 'else' (handled specially)
        With        -> true   # After 'record with', 'match with'
        Arrow (->)  -> true   # After lambda '(x) ->'
        LBrace ({)  -> true   # After 'Record {' for multi-line field init
        _           -> false
```

### Brace-Delimited Contexts

The `{` token is a special layout opener: it creates a seq-block context for `Sep` insertion between fields, but **does not emit `Begin`/`End` tokens**. The braces themselves serve as delimiters.

```
Point {
    x = 1       # Brace context starts at column 5, no Begin emitted
    y = 2       # Sep inserted (same column)
}               # No End emitted - '}' closes the context
```

**Token stream:** `Point { x = 1 Sep y = 2 }`

This is cleaner than emitting redundant `Begin`/`End` inside braces.

**Key behaviors:**
- `{` opens a brace-delimited seq-block (for `Sep` insertion)
- `}` closes the brace-delimited context (no `End` emitted)
- `Sep` is still inserted between fields at the same column
- Non-brace blocks inside braces still emit `Begin`/`End` normally

```
Point {
    x =         # '=' opens a value block (not brace-delimited)
        1 + 2   # Begin emitted here for the value block
    y = 3       # End closes value block, Sep inserted for next field
}
```

**Token stream:** `Point { x = Begin 1 + 2 End Sep y = 3 }`

### Closing Delimiters

Closing delimiters (`}`, `)`, `]`) do not trigger `Sep` insertion, even if they appear at the same column as a seq-block. This is because closing delimiters are not the start of new statements - they're part of the expression structure.

```
function foo() =
    let p = Point {
        x = 1
        y = 2
    }       # '}' at column 5 does NOT get Sep
    p.x     # This gets Sep (same column as function body)
```

### Error Detection

The filter should detect and report these errors:

1. **Invalid outdentation**: Token at a column that doesn't match any context

```
function validate_outdent(col):
    # Check if col matches any context in the stack
    for ctx in context_stack (bottom to top):
        if col == ctx.column:
            return  # Valid - matches a context
        if col > ctx.column:
            return  # Valid - inside this context
    
    # col doesn't match any context
    emit_error("invalid outdentation at column {col}")
```

2. **Unexpected indentation**: Token indented more than expected without continuation

```
# This is harder to detect - may need parser cooperation
# For now, let the parser handle unexpected token errors
```

3. **Tab characters**: If tabs are disallowed

```
function check_indentation(token):
    if token.leading_whitespace contains TAB:
        emit_error("tab character in indentation")
```

### Token Span Handling

Virtual tokens (`Begin`, `End`, `Sep`) need spans for error reporting:

- **Begin**: Use the span of the first token in the block
- **End**: Use the span of the last token in the block (before outdentation)
- **Sep**: Use the span of the token that follows the separator

### Edge Cases

| Case | Handling |
|------|----------|
| EOF with open contexts | Emit `End` for each, then `EOF` |
| Empty block (immediate outdent after opener) | Emit `Begin End` (empty block) |
| Nested layout openers (`= if then`) | Each creates its own context |
| `else` without `then` context | Parser error, not filter error |
| Explicit `begin`/`end` mixing | Explicit tokens pass through; virtual tokens work around them |
| Multiple `End` on single outdent | Emit all `End` tokens, then check for `Sep` |

### Example Trace

Input:
```
function foo() =
    let x = 5
    x + 1
bar()
```

| Step | Raw Token | Line:Col | Stack | Action | Emit |
|------|-----------|----------|-------|--------|------|
| 1 | `function` | 1:1 | [] | - | `function` |
| 2 | `foo` | 1:10 | [] | - | `foo` |
| 3 | `(` | 1:13 | [] | - | `(` |
| 4 | `)` | 1:14 | [] | - | `)` |
| 5 | `=` | 1:16 | [] | Set pending_context=1 | `=` |
| 6 | `Newline` | 1:17 | [] | Skip | - |
| 7 | `let` | 2:5 | [] | Establish ctx(5,seq), emit Begin | `Begin`, `let` |
| 8 | `x` | 2:9 | [(5,seq)] | - | `x` |
| 9 | `=` | 2:11 | [(5,seq)] | Set pending_context=2 | `=` |
| 10 | `5` | 2:13 | [(5,seq)] | Same line as opener, no block | `5` |
| 11 | `Newline` | 2:14 | [(5,seq)] | Skip | - |
| 12 | `x` | 3:5 | [(5,seq)] | col=5, seq-block → Sep | `Sep`, `x` |
| 13 | `+` | 3:7 | [(5,seq)] | - | `+` |
| 14 | `1` | 3:9 | [(5,seq)] | - | `1` |
| 15 | `Newline` | 3:10 | [(5,seq)] | Skip | - |
| 16 | `bar` | 4:1 | [(5,seq)] | col=1 < 5 → close | `End`, `bar` |
| 17 | `(` | 4:4 | [] | - | `(` |
| 18 | `)` | 4:5 | [] | - | `)` |
| 19 | `EOF` | 4:6 | [] | - | `EOF` |

**Output:** `function foo ( ) = Begin let x = 5 Sep x + 1 End bar ( ) EOF`

---

## Offside Rules

### Opening a Block

When a **layout opener** token (`=`, `then`, `else`, `with`, `->`, `{`) is followed by a new line, a `Begin` token is emitted and a seq-block is established at the column of the first token on that new line. This column becomes the block's offside line.

```
function foo() =
    let x = 5    # Begin emitted before 'let', block starts at column 5
    x + 1        # Sep inserted before this line (same column)
```

Token stream: `function foo ( ) = Begin let x = 5 Sep x + 1 End`

If the expression continues on the same line as the opener, **no block context is created** and no `Begin`/`End` tokens are emitted. The expression is simply passed through:

```
function foo() = 42    # No Begin/End - just the expression
```

Token stream: `function foo ( ) = 42 Eof`

### Multi-line Blocks vs Same-line Expressions

The critical distinction is **whether the first token appears on a new line after the layout opener**.

**Multi-line block (seq-block):**
- Created when layout opener is followed by a **newline**, then indented content
- A `Begin` token is emitted and a block context is pushed
- The offside column is set to the first token's column on the new line
- Tokens at the **same column** receive `Sep` tokens between them (allowing multiple statements)
- Tokens at a **lesser column** close the block with `End`

```
function foo() =
    let x = 5    # Begin emitted, seq-block starts at column 5
    let y = 10   # Sep inserted (same column 5)
    x + y        # Sep inserted (same column 5)
```

**Same-line expression (no block):**
- When layout opener is followed by content **on the same line**
- **No block context is created** and no `Begin`/`End` tokens are emitted
- The expression is simply passed through to the parser
- The parser handles this by accepting either a block or a single expression

```
function foo() = 42    # No block context, just the expression
function bar() = 1 + 2 # No block context, just the expression
```

**Why no block for same-line expressions?**

Consider what happens at the end of a function:

```
function foo() = 42
function bar() = 7
```

Without creating a block context for `= 42`, the token stream is simply:
`function foo ( ) = 42 function bar ( ) = 7 Eof`

The parser sees `function` and knows a new declaration is starting. No block closure logic needed.

Compare with multi-line:

```
function foo() =
    42
bar()                  # Column 1, less than block's column 5
```

Here:
- `=` followed by newline creates a seq-block at column 5 with `Begin`
- `bar()` at column 1 < 5, so `End` is emitted before `bar`

**Summary table:**

| Condition | Block Created? | Behavior |
|-----------|---------------|----------|
| Newline after opener | Yes (seq-block) | `Begin` emitted, `Sep` at same column, `End` on outdent |
| Same line as opener | No | Expression passed through, parser handles it |

### Closing a Block (Outdentation)

A block is closed (with `End` emitted) when a token appears at a column **less than** the block's offside column.

When a token appears at the **same column** as the block, a `Sep` token is emitted (starting a new statement in the block).

Multiple `End` tokens may be emitted when outdentation closes several nested blocks at once.

```
function foo() =
    let x = 5
    x + 1
bar()    # Column 1 < Column 5: End emitted, block closes
```

Token stream: `function foo ( ) = Begin let x = 5 Sep x + 1 End bar ( )`

Multiple `End` tokens may be emitted for nested blocks:

```
function foo() =
    if true then
        let x = 5
        x
bar()    # Two End tokens: one for if-block, one for function body
```

Token stream: `function foo ( ) = Begin if true then Begin let x = 5 Sep x End End bar ( )`

### Separator Insertion

Within a seq-block, when a new line starts at **exactly** the block's offside column, a `Sep` token is inserted before that line's first token:

```
function main() =
    print("hello")     # Begin emitted, block starts at column 5
    print("world")     # Sep inserted here (same column)
    print("!")         # Sep inserted here (same column)
```

Token stream: `function main ( ) = Begin print ( "hello" ) Sep print ( "world" ) Sep print ( "!" ) End`

## Line Continuation

A line may continue the previous expression without triggering a separator. Continuation occurs when:

1. The line does **not** follow a layout opener, AND
2. The line is indented **more** than the block's offside column

The "beginning of the line" for separator purposes is the first line in a continuation sequence:

```
function foo() =
    let x = very_long_function_call(
        arg1,
        arg2
    )                  # Continuation of let expression
    x + 1              # Sep inserted here (column 5 matches block)
```

## Balancing Rules

**Question:** Do we need special handling for balanced delimiters (`()`, `[]`, `{}`)?

### What F# Does

In F#, balanced delimiters don't "suspend" layout - they **create new offside contexts**:

- `(`, `[`, `{` establish a new offside line at the column of the first token after them
- Layout openers like `->` still work normally (they create their own contexts)
- `)`, `]`, `}` close their respective contexts
- The offside lines stack up as normal

This means inside parentheses, you get a fresh offside context, which naturally allows flexible formatting.

### Dovetail's Approach

For Dovetail, we adopt a simpler model: **balanced delimiters don't affect layout at all**.

Layout processing continues normally inside balanced contexts. The opening and closing delimiters are just regular tokens. Layout openers like `->` create blocks as usual, and outdentation closes blocks as usual.

The array delimiters `[|` and `|]` follow the same rule. `|]` is registered in
`TokenKind::is_closing_delimiter()` so that no `SEP` is emitted before it — the
same treatment `)`, `}` and `]` get — which is what lets an array literal put its
closing bracket on its own line:

```
let xs = [|
    1,
    2
|]
```

```
map(items, (x) ->
    let y = x * 2
    y + 1
)
```

**Token stream:** `map ( items , ( x ) -> Begin let y = x * 2 Sep y + 1 End ) Eof`

How this works:
- `->` is a layout opener, creates seq-block at column 5
- `let y` and `y + 1` are in the seq-block
- `)` at column 1 triggers outdentation (column 1 < column 5), closing the block
- `)` is just a regular token

### Consequence: Weird Indentation in Arrays

Without special balancing rules, this would be an error:

```
function foo() =
    let items = [
1,        # Error: column 1 doesn't match any block
  2,
    3
    ]
    items
```

**Options:**
1. **Require consistent indentation in data structures** - Users must indent properly
2. **Add balancing rules that suppress errors** - More permissive but more complex
3. **Treat balanced contexts as not affecting column tracking** - Tokens inside don't trigger outdent

**Recommendation:** Start with option 1 (require proper indentation). This is simpler and encourages readable code:

```
function foo() =
    let items = [
        1,
        2,
        3
    ]
    items
```

If users find this too restrictive, we can add balancing rules later.

## If-Then-Else Handling

The `else` keyword receives special treatment to produce balanced `Begin`/`End` pairs:

1. `then` emits `Begin` and opens a block
2. When `else` is encountered, the `then` block is closed with an `End` token
3. `else` emits `Begin` and opens a new block
4. An `End` is emitted when the else-block ends

This ensures each branch has its own `Begin`/`End` pair:

```
function foo(x: Int32) =
    if x > 0 then
        1
    else
        -1
```

Token stream: `function foo ( x : Int32 ) = Begin if x > 0 then Begin 1 End else Begin -1 End End`

## Explicit `begin`/`end` Keywords

Users may write explicit `begin` and `end` keywords instead of relying on layout:

```
function foo() = begin
    let x = 5
    x + 1
end
```

When explicit `begin` is used after a layout opener, no virtual `Begin` is inserted. The explicit `end` must appear at the same column as the **enclosing** block (not the block being closed):

```
function foo() =
    let x = 5
    x + 1
end                    # Explicit end at column 1 (matches function's context)
```

One `end` keyword can close multiple nested blocks, but it syntactically belongs to the outermost block closed.

## Invalid Indentation

Indentation errors occur in two cases:

### 1. Unexpected Indentation

Starting indentation where none is expected:

```
function foo() =
    let x = 5
      7                # Error: unexpected indentation (column 7 > column 5)
```

### 2. Misaligned Outdentation

Outdenting to a column that doesn't match any open block:

```
function foo(x: Int32) =
    if x > 7 then
        5
      3                # Error: column 7 doesn't match any block (5 or 1)
```

## Grammar

### Functions

```
function =
    | "function" ident "(" params ")" [":" type] "=" block_expression
    | "function" ident "(" ")" [":" type] "=" block_expression

params = param *("," param)
param = ident ":" type
```

### Expressions

```
expression = let_expression | block_expression | if_expression | ...

let_expression = "let" ident [":" type] ["=" expression]

block_expression = Begin [*(expression Sep)] expression End

if_expression = "if" expression "then" block_expression ["else" block_expression]
```

Note: `let` expressions evaluate to `Unit`. They do not have a trailing "body" expression.

Note: `Begin` and `End` in the grammar represent both:
- Explicit `begin`/`end` keywords written by the user
- Virtual tokens inserted by the layout filter

### Records

```
record_declaration = "record" ident "=" Begin *(record_field Sep) record_field [Sep] End

record_field = ident ":" type

record_field_init = ident "=" expression

record_field_init_list = record_field_init *(Sep record_field_init) [Sep]

record_expression = ident "{" [record_field_init_list] "}"

record_with_expression = expression "with" "{" record_field_init_list "}"
```

---

# Examples and Test Cases

This section provides comprehensive examples for testing the layout filter implementation. Each example shows the input code, expected token stream, and whether it's valid or invalid.

## Notation

- `Begin` / `End` / `Sep` = virtual tokens inserted by layout filter
- `begin` / `end` = explicit keywords written by user
- Token streams use spaces to separate tokens; parentheses and brackets are literal tokens
- Comments in examples explain what's happening

---

## Basic Valid Examples

### E1: Simple function with seq-block

```
function foo() =
    let x = 5
    x + 1
```

**Token stream:** `function foo ( ) = Begin let x = 5 Sep x + 1 End Eof`

**Explanation:** `=` followed by newline creates seq-block at column 5. Second line at same column gets `Sep`.

---

### E2: Single-line function (no block)

```
function foo() = 42
```

**Token stream:** `function foo ( ) = 42 Eof`

**Explanation:** `=` followed by expression on same line - no block context created, expression passed through directly.

---

### E3: Two functions in sequence

```
function foo() = 42
function bar() = 7
```

**Token stream:** `function foo ( ) = 42 function bar ( ) = 7 Eof`

**Explanation:** Same-line expressions don't create blocks. Each function is parsed independently.

---

### E4: Nested blocks

```
function foo() =
    if true then
        let x = 5
        x
    else
        0
```

**Token stream:** `function foo ( ) = Begin if true then Begin let x = 5 Sep x End else Begin 0 End End Eof`

**Explanation:** 
- Function body is seq-block at column 5
- `then` creates seq-block at column 9
- `else` closes then-block, creates new block at column 9
- Outdentation to column 1 (or EOF) closes all remaining blocks

---

### E5: Multiple statements in function

```
function main() =
    print("hello")
    print("world")
    print("!")
```

**Token stream:** `function main ( ) = Begin print ( "hello" ) Sep print ( "world" ) Sep print ( "!" ) End Eof`

---

### E6: Deeply nested blocks

```
function foo() =
    if a then
        if b then
            1
        else
            2
    else
        3
```

**Token stream:** `function foo ( ) = Begin if a then Begin if b then Begin 1 End else Begin 2 End End else Begin 3 End End Eof`

---

## Explicit begin/end Examples

### E7: Explicit begin/end after layout opener

```
function foo() = begin
    let x = 5
    x + 1
end
```

**Token stream:** `function foo ( ) = begin let x = 5 Sep x + 1 end Eof`

**Explanation:** Explicit `begin` after `=` prevents virtual `Begin`. The `begin`/`end` keywords pass through as regular tokens. Layout rules still apply inside for `Sep`.

---

### E8: Fully explicit (no layout)

```
function foo() = begin let x = 5; x + 1 end
```

**Token stream:** `function foo ( ) = begin let x = 5 ; x + 1 end Eof`

**Explanation:** Explicit `;` separator, no virtual tokens inserted.

---

### E9: Mixing explicit and implicit

```
function foo() =
    let x = begin
        5
    end
    x + 1
```

**Token stream:** `function foo ( ) = Begin let x = begin 5 end Sep x + 1 End Eof`

**Explanation:** Function body uses implicit layout; `let` value uses explicit `begin`/`end`.

---

### E10: Explicit end closing multiple implicit blocks

```
function foo() =
    if true then
        let x = 5
        x
end
```

**Token stream:** `function foo ( ) = Begin if true then Begin let x = 5 Sep x End End end Eof`

**Explanation:** The explicit `end` at column 1 closes the blocks via outdentation (emitting `End` tokens), then appears as a regular token. This may cause a parse error (unmatched `end`), but the layout filter handles it correctly.

**Note:** This is likely a user error. The explicit `end` should match an explicit `begin`.

---

### E10a: Explicit begin/end inside implicit block

```
function foo() =
    let x = begin
        let a = 1
        let b = 2
        a + b
    end
    let y = 10
    x + y
```

**Token stream:** `function foo ( ) = Begin let x = begin let a = 1 Sep let b = 2 Sep a + b end Sep let y = 10 Sep x + y End Eof`

**Explanation:** 
- Function body is implicit (virtual `Begin`/`End`)
- `let x` value is explicit (`begin`/`end` keywords)
- Inside explicit block, layout still works (`Sep` between statements)
- After `end`, `let y` at column 5 gets `Sep` (same column as function body)

---

### E10b: Explicit begin on same line as opener

```
function foo() = begin
    let x = 5
    x + 1
end
```

**Token stream:** `function foo ( ) = begin let x = 5 Sep x + 1 end Eof`

**Explanation:** Explicit `begin` immediately after `=` prevents virtual `Begin`. No virtual `Begin` or `End` needed.

---

### E10c: Explicit begin on next line

```
function foo() =
    begin
        let x = 5
        x + 1
    end
```

**Token stream:** `function foo ( ) = Begin begin let x = 5 Sep x + 1 end End Eof`

**Explanation:** 
- `=` followed by newline creates implicit seq-block at column 5 (`Begin`)
- `begin` at column 5 is just a token in that block
- `end` at column 5 matches `begin`
- Implicit block closes at EOF with `End`

**Note:** This creates redundant nesting - the implicit block around the explicit block. Not an error, just unnecessary.

---

### E10d: Nested explicit blocks

```
function foo() = begin
    let x = begin
        1
    end
    let y = begin
        2
    end
    x + y
end
```

**Token stream:** `function foo ( ) = begin let x = begin 1 end Sep let y = begin 2 end Sep x + y end Eof`

**Explanation:** All blocks are explicit. Layout filter only inserts `Sep` between statements at same column.

---

### E10e: Explicit end at outdented position

```
function foo() =
    if true then
        begin
            let x = 5
            x
        end
    else
        0
```

**Token stream:** `function foo ( ) = Begin if true then Begin begin let x = 5 Sep x end End else Begin 0 End End Eof`

**Explanation:**
- `then` creates implicit block at column 9
- `begin` at column 9 is a token in that block
- `end` at column 9 matches `begin` (no outdentation)
- `else` at column 5 closes then-block with implicit `End`

---

### E10f: Explicit semicolon with explicit begin/end

```
function foo() = begin let a = 1; let b = 2; a + b end
```

**Token stream:** `function foo ( ) = begin let a = 1 ; let b = 2 ; a + b end Eof`

**Explanation:** Explicit `;` separators pass through. No virtual `Sep` inserted.

---

### E10g: Mixed explicit semicolons and layout

```
function foo() = begin
    let a = 1; let b = 2
    let c = 3
    a + b + c
end
```

**Token stream:** `function foo ( ) = begin let a = 1 ; let b = 2 Sep let c = 3 Sep a + b + c end Eof`

**Explanation:** First line has explicit `;`. Second and third lines get `Sep` from layout.

---

## Continuation Examples

### E11: Line continuation with parentheses

```
function foo() =
    let x = add(
        1,
        2
    )
    x + 1
```

**Token stream:** `function foo ( ) = Begin let x = add ( 1 , 2 ) Sep x + 1 End Eof`

**Explanation:** Inside `()`, indentation is ignored (balancing rule). The `)` on column 5 is still within the parentheses context.

---

### E12: Line continuation without parentheses

```
function foo() =
    let x = 1 +
        2 +
        3
    x
```

**Token stream:** `function foo ( ) = Begin let x = 1 + 2 + 3 Sep x End Eof`

**Explanation:** Lines at column 9 (greater than block column 5) are continuations, no `Sep` inserted.

---

### E13: Continuation then new statement

```
function foo() =
    let x = 1 +
        2
    let y = 3
    x + y
```

**Token stream:** `function foo ( ) = Begin let x = 1 + 2 Sep let y = 3 Sep x + y End Eof`

**Explanation:** `2` at column 9 is continuation; `let y` at column 5 is new statement.

---

## Balanced Delimiter Examples

### E14: Array with proper indentation

```
function foo() =
    let items = [
        1,
        2,
        3
    ]
    items
```

**Token stream:** `function foo ( ) = Begin let items = [ 1 , 2 , 3 ] Sep items End Eof`

**Explanation:** Balanced delimiters don't affect layout. The `]` at column 5 doesn't trigger outdent because array contents aren't in a layout block - they're just comma-separated values.

---

### E15: Function call with multi-line arguments

```
function foo() =
    let x = f(
        a,
        b,
        c
    )
    x
```

**Token stream:** `function foo ( ) = Begin let x = f ( a , b , c ) Sep x End Eof`

**Explanation:** The parentheses contain comma-separated arguments, not a layout block. No `Begin`/`End`/`Sep` inside.

---

### E16: Record literal with braces

```
function foo() =
    let p = Point {
        x = 1,
        y = 2
    }
    p
```

**Token stream:** `function foo ( ) = Begin let p = Point { x = 1 ; y = 2 } Sep p End Eof`

**Explanation:** Record literals use semicolons for separation, not layout. The braces are balanced delimiters.

---

### E17: Lambda inside function call (multi-line)

```
function foo() =
    map(items, (x) ->
        let y = x * 2
        y + 1
    )
    result
```

**Token stream:** `function foo ( ) = Begin map ( items , ( x ) -> Begin let y = x * 2 Sep y + 1 End ) Sep result End Eof`

**Explanation:** 
- `->` is a layout opener, creates seq-block at column 9 (where `let` starts)
- `let y` and `y + 1` are in the lambda's seq-block
- `)` at column 5 triggers outdentation, closing lambda block with `End`
- `result` at column 5 gets `Sep` (same column as function body)

---

### E18: Lambda inside function call (single-line)

```
function foo() =
    map(items, (x) -> x + 1)
```

**Token stream:** `function foo ( ) = Begin map ( items , ( x ) -> x + 1 ) End Eof`

**Explanation:** `->` followed by expression on same line - no block created, expression passed through.

---

### E19: Nested lambdas

```
function foo() =
    fold(items, 0, (acc, x) ->
        if x > 0 then
            acc + x
        else
            acc
    )
```

**Token stream:** `function foo ( ) = Begin fold ( items , 0 , ( acc , x ) -> Begin if x > 0 then Begin acc + x End else Begin acc End End ) End Eof`

**Explanation:** Full layout rules work inside the balanced context. The `if-then-else` creates its own nested blocks.

---

### E20: Lambda with explicit begin/end

```
function foo() =
    map(items, (x) -> begin
        let y = x * 2
        y + 1
    end)
```

**Token stream:** `function foo ( ) = Begin map ( items , ( x ) -> begin let y = x * 2 Sep y + 1 end ) End Eof`

**Explanation:** Explicit `begin` after `->` prevents virtual `Begin`. Layout still works inside for `Sep`.

---

## If-Then-Else Examples

### E17: If without else

```
function foo(x: Int32) =
    if x > 0 then
        print("positive")
    print("done")
```

**Token stream:** `function foo ( x : Int32 ) = Begin if x > 0 then Begin print ( "positive" ) End Sep print ( "done" ) End Eof`

**Explanation:** `then` block closes when `print("done")` appears at column 5 (same as function body, but less than then-block's column 9).

---

### E18: If-else on single lines

```
function foo(x: Int32) = if x > 0 then 1 else -1
```

**Token stream:** `function foo ( x : Int32 ) = if x > 0 then 1 else -1 Eof`

**Note:** When all branches are on the same line as their openers, no blocks are created.

**Explanation:** All blocks are non-seq-blocks (same-line content).

---

### E19: Chained if-else-if

```
function foo(x: Int32) =
    if x > 0 then
        1
    else if x < 0 then
        -1
    else
        0
```

**Token stream:** `function foo ( x : Int32 ) = Begin if x > 0 then Begin 1 End else Begin if x < 0 then Begin -1 End else Begin 0 End End End Eof`

---

## Edge Cases

### E20: Empty lines don't affect layout

```
function foo() =
    let x = 5

    let y = 10

    x + y
```

**Token stream:** `function foo ( ) = Begin let x = 5 Sep let y = 10 Sep x + y End Eof`

**Explanation:** Blank lines are skipped; layout is based on non-blank lines.

---

### E21: Comments don't affect layout

```
function foo() =
    let x = 5  # comment here
    # full line comment
    x + 1
```

**Token stream:** `function foo ( ) = Begin let x = 5 Sep x + 1 End Eof`

**Explanation:** Comments are stripped before layout processing.

---

### E22: Single expression with continuation looks like seq-block

```
function foo() =
    1
    + 2
    + 3
```

**Valid or Invalid?** This depends on whether `+ 2` at column 5 is a new statement or continuation.

**Current design (no operator continuation):** This is a **seq-block with three expressions**: `1`, `+ 2` (unary plus), `+ 3` (unary plus).

**Token stream:** `function foo ( ) = Begin 1 Sep + 2 Sep + 3 End Eof`

If `+` is not a valid unary operator, this would be a parse error, not a layout error.

---

### E23: Immediate outdentation after opener

```
function foo() =
bar()
```

**Token stream:** `function foo ( ) = Begin End bar ( ) Eof`

**Explanation:** After `=`, newline creates pending context. But `bar()` at column 1 is less than any reasonable block column, so the block is immediately empty and closed.

**Note:** This produces an empty block, which may be a parse error depending on grammar.

---

### E24: Non-seq-block followed by same-column token

```
function foo() = 1 + 2
function bar() = 3 + 4
```

**Token stream:** `function foo ( ) = Begin 1 + 2 End function bar ( ) = Begin 3 + 4 End Eof`

**Explanation:** Each non-seq-block inherits column 1. When next `function` at column 1 appears, it closes the non-seq-block.

---

### E25: Seq-block with only one statement

```
function foo() =
    42
```

**Token stream:** `function foo ( ) = Begin 42 End Eof`

**Explanation:** It's still a seq-block (newline after opener), but happens to have one expression. No `Sep` tokens needed.

---

### E26: Record declaration with layout

```
record Point =
    x: Float64
    y: Float64
```

**Token stream:** `record Point = Begin x : Float64 Sep y : Float64 End Eof`

---

### E27: Record with expression (brace-delimited fields)

```
function move(p: Point) =
    p with {
        x = p.x + 1
        y = p.y + 1
    }
```

**Token stream:** `function move ( p : Point ) = Begin p with { x = p.x + 1 Sep y = p.y + 1 } End Eof`

---

## Invalid Examples

### I1: Unexpected indentation increase

```
function foo() =
    let x = 5
      7
```

**Error:** Line 3, column 7: unexpected indentation. Expected column 5 (block) or continuation.

**Explanation:** `7` at column 7 is neither at block column (5) nor a valid continuation (would need column > 5 AND to be syntactically part of previous expression).

---

### I2: Misaligned outdentation

```
function foo() =
    if x > 0 then
        1
      2
```

**Error:** Line 4, column 7: invalid outdentation. Column 7 doesn't match any enclosing block (9, 5, or 1).

---

### I3: Outdent to non-existent column

```
function foo() =
    let x = 5
   y
```

**Error:** Line 3, column 4: invalid outdentation. Column 4 doesn't match any enclosing block (5 or 1).

---

### I4: Tab in indentation (if tabs disallowed)

```
function foo() =
→   let x = 5
```

**Error:** Line 2, column 1: tab character in indentation is not allowed.

---

### I5: Mixing tabs and spaces

```
function foo() =
    let x = 5    # 4 spaces
→   let y = 10   # 1 tab
```

**Error:** Line 3: inconsistent indentation (tab mixed with spaces).

---

### I6: else without matching then block

```
function foo() =
    else
        1
```

**Error:** Parse error - `else` without `if`/`then`.

**Note:** This is a parse error, not a layout error. The layout filter processes it correctly.

---

### I7: Explicit end without matching begin

```
function foo() =
    let x = 5
    x + 1
end
```

**Layout processing:** `function foo ( ) = Begin let x = 5 Sep x + 1 End end Eof`

**Parse result:** Error - unmatched `end` keyword.

**Note:** Layout filter emits `End` on outdentation, then passes through explicit `end`. Parser rejects unmatched `end`.

---

### I8: Expression at column 0

```
function foo() =
let x = 5
```

**Token stream:** `function foo ( ) = Begin End let x = 5 Eof`

**Explanation:** `let` at column 1 (less than any block would establish) immediately closes the empty block. The `let` then appears as a top-level statement (likely invalid without being in a function).

---

## Complex Examples

### C1: Multiple functions with different styles

```
function add(a: Int32, b: Int32) = a + b

function complex(x: Int32) =
    let doubled = x * 2
    let result = if doubled > 10 then
        doubled - 10
    else
        doubled
    result

function simple() = begin
    let x = 1
    x
end
```

**Token streams:**
- `add`: `function add ( a : Int32 , b : Int32 ) = Begin a + b End`
- `complex`: `function complex ( x : Int32 ) = Begin let doubled = x * 2 Sep let result = if doubled > 10 then Begin doubled - 10 End else Begin doubled End Sep result End`
- `simple`: `function simple ( ) = begin let x = 1 Sep x end`

---

### C2: Deeply nested with multiple block types

```
function process(items: List) =
    let result = if isEmpty(items) then
        []
    else
        let first = head(items)
        let rest = tail(items)
        if first > 0 then
            cons(first, process(rest))
        else
            process(rest)
    result
```

**Analysis:**
- Function body: seq-block at column 5
- `if` then branch: seq-block at column 9 (contains `[]`)
- `else` branch: seq-block at column 9 (contains multiple `let` and nested `if`)
- Inner `if` then branch: seq-block at column 13
- Inner `else` branch: seq-block at column 13

---

### C3: Record operations

```
record Person =
    name: String
    age: Int32

function birthday(p: Person) =
    p with {
        age = p.age + 1
    }

function create() =
    Person {
        name = "Alice"
        age = 30
    }
```

---

## Summary of Key Rules

| Scenario | Block Created? | Sep Behavior | End Behavior |
|----------|---------------|--------------|--------------|
| `= <newline> <indent>` | Yes (seq-block) | Same column = Sep | Lesser column = End |
| `= <expr>` (same line) | No | N/A | N/A (parser handles it) |
| `then <newline> <indent>` | Yes (seq-block) | Same column = Sep | `else` or lesser column = End |
| `then <expr>` (same line) | No | N/A | N/A (parser handles it) |
| Inside `()`, `[]`, `{}` | N/A | Layout continues normally | Layout continues normally |
| Explicit `begin` after opener | Yes (explicit) | `;` for separation | Explicit `end` required |

---

# Design Decisions and Rationale

This section documents key design decisions, alternatives considered, and lessons from other layout-sensitive languages.

## Decision: No parse-error(t) Rule

**Background:** Haskell's layout specification includes a `parse-error(t)` rule that closes implicit blocks when the next token would cause a parse error but inserting `}` would fix it. This creates a lexer-parser feedback loop.

**Problem:** No major Haskell compiler fully implements this rule. GHC fails on legal Haskell like `let x = 42 in x == 42 == True`. The rule requires the lexer to simulate the entire parser, which is impractical.

**Dovetail's approach:** We avoid parse-error(t) entirely. Block closure is determined **purely by indentation**, never by parse state. This means:

- The lexer filter operates independently of the parser
- Layout rules are fully deterministic from column positions alone
- Trade-off: Some single-line constructs may require explicit delimiters

```
# Haskell allows (via parse-error(t)):
let x = 1 in x + 1

# Dovetail requires either explicit begin/end or newline:
let x = begin 1 end
x + 1

# Or use layout:
let x =
    1
x + 1
```

## Decision: Tabs Policy

**Background:** Mixing tabs and spaces causes ambiguity in layout-sensitive languages because a TAB displays as 2-8 columns depending on editor settings.

**Options:**
1. **Spaces only** (Python PEP 8): Reject tabs entirely
2. **Tabs only** (Go): Standardize on tabs, use formatter
3. **Tab = N spaces** (Haskell): Define fixed tab width (Haskell uses 8)

**Recommendation:** Dovetail should use **spaces only** for indentation:

- Tabs in indentation produce a compile error
- Tabs in string literals and comments are allowed
- Rationale: Eliminates display ambiguity; modern editors can insert spaces on TAB key

```
function foo() =
→   let x = 5    # Error: tab character in indentation
    x + 1        # OK: spaces
```

## Decision: Operator Continuation

**Background:** F# allows infix operators (`+`, `|>`, `>>`) to appear past the offside line by their length plus one space. This enables:

```fsharp
let x = 1
      + 2    # Valid in F#: '+' is 1 char, allowed 2 columns right
```

**Options:**
1. **No special rules**: Operators follow normal offside rules
2. **F#-style**: Allow operators to extend past offside line
3. **Trailing operators only**: Allow operators at end of line to continue

**Recommendation:** Keep it simple with **no special operator rules**. Use balancing (parentheses) or explicit continuation for complex expressions:

```
# Use parentheses for complex expressions:
let result = (
    value1
    + value2
    + value3
)

# Or keep operators at end of line (within balanced context):
let result = value1 +
    value2 +
    value3
```

This is simpler to implement and reason about.

## Decision: Handling Explicit Delimiters After Layout Openers

**Background:** What happens when the user writes explicit `begin` immediately after `=`?

```
function foo() = begin
    let x = 5
    x + 1
end
```

**Rule:** When an explicit `begin` immediately follows a layout opener:
1. The layout opener does NOT create a pending context
2. The explicit `begin` is passed through as a regular token
3. No virtual `Begin` is emitted (the explicit one suffices)

**Detection:** The lexer filter checks if the token following a layout opener is `begin`. If so, skip virtual token insertion.

## Design: Error Messages for Layout Errors

**Background:** Layout errors are notoriously confusing. Haskell's GHC often reports errors far from the actual layout problem.

**Principles:**
1. **Report at the exact location**: Error should point to the misaligned token
2. **Show expected vs actual**: "Expected column 5, found column 7"
3. **Show context**: "In block starting at line 3"
4. **Suggest fixes**: "Did you mean to continue the previous expression? Indent to column 9 or greater."

**Example error messages:**

```
error: misaligned indentation
  --> src/main.dove:5:7
   |
 3 | function foo() =
 4 |     let x = 5
 5 |       7
   |       ^ expected column 5 (matching block) or column 9+ (continuation)
   |
   = help: this line starts a new statement but isn't aligned with the block
```

```
error: invalid outdentation
  --> src/main.dove:6:3
   |
 3 | function foo() =
 4 |     if x > 0 then
 5 |         1
 6 |   2
   |   ^ column 3 doesn't match any enclosing block
   |
   = note: valid outdentation columns are: 5 (if body), 1 (function body)
```

## Design: Empty Blocks

**Question:** What happens with an empty block?

```
function foo() =
    # empty?

bar()
```

**Options:**
1. **Disallow**: Empty blocks are a syntax error
2. **Unit value**: Empty block has value `()`
3. **Require explicit**: Must use `begin end` for empty blocks

**Recommendation:** Disallow truly empty blocks. A block must contain at least one expression. Empty blocks should use explicit `()`:

```
function foo() =
    ()    # Explicit unit value
```

## Design: Multi-line Strings and Layout

**Question:** Do multi-line strings interact with layout?

**Rule:** Multi-line strings (if supported) should **not** affect layout tracking:

```
function foo() =
    let msg = """
This is a
    multi-line string
        with varying indentation
"""
    print(msg)    # Sep inserted here, layout unaffected by string contents
```

The string's internal indentation is literal content, not layout structure.

## Future Consideration: Match/Case Expressions

When adding pattern matching, consider the layout implications:

```
function describe(x: Int32) =
    match x with
        0 -> "zero"
        1 -> "one"
        _ -> "many"
```

**Question:** Does `with` establish a block, and do pattern arms need alignment?

**Recommendation:** `with` opens a block. Each pattern arm at the same column gets `Sep` between them. The `->` acts like `=` in establishing the arm's body block:

Token stream: `match x with Begin 0 -> Begin "zero" End Sep 1 -> Begin "one" End Sep _ -> Begin "many" End End`

## Comparison with Other Languages

| Feature | Dovetail | Haskell | F# | Python |
|---------|--------|---------|-----|--------|
| Virtual tokens | Begin/End/Sep | {/}/; | Similar to Haskell | INDENT/DEDENT |
| parse-error(t) | No | Yes (problematic) | No | N/A |
| Tabs | Spaces only | 8-space tabs | Spaces only | Spaces preferred |
| Operator continuation | No | No | Yes | N/A |
| Balancing | Yes | Yes | Yes | N/A |
| Explicit delimiters | begin/end | {/}/; | N/A | N/A (required : and pass) |

---

# Open Questions

1. **Should `do` be a layout opener?** For monadic/imperative sequences, `do` blocks are common in Haskell/F#. If Dovetail adds effects, consider whether `do` should open layout.

2. ~~**Lambda bodies:** Should `->` or `=>` in lambdas open a block?~~
   **RESOLVED:** Yes, `->` is a layout opener. See examples E17-E20 in the test cases section.

3. **Where clauses:** If Dovetail adds `where` for local definitions, it should be a layout opener like in Haskell.

4. **Trailing separator:** Should a trailing `Sep` before `End` be allowed?
   ```
   function foo() =
       let x = 5
       x + 1
       # <- is there a Sep here before End?
   ```
   Current grammar suggests no trailing Sep is required/allowed.

5. **Should `=>` also be a layout opener?** If Dovetail uses `=>` for something (e.g., different lambda syntax, match arms), should it also open blocks?

---

## References

1. P. J. Landin, "The Next 700 Programming Languages," Communications of the ACM, 1966.
2. M. D. Adams, "Principled Parsing for Indentation-Sensitive Languages: Revisiting Landin's Offside Rule," POPL 2013.
3. M. D. Adams & Ö. S. Ağacan, "Indentation-Sensitive Parsing for Parsec," Haskell Symposium 2014.
4. F# Language Specification, Chapter 15: Lexical Filtering.
5. Python PEP 8 - Style Guide for Python Code.
6. GHC GitLab Issue #22173 - Layout/indentation error messages.
