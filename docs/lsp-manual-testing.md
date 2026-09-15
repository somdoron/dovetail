# Dovetail LSP — Manual Testing Guide

This document is a test template for manually verifying every LSP feature in VS Code.
After each test, record the result (PASS/FAIL) and paste any relevant log output.

---

## Table of Contents

1. [Setup & Log Capture](#1-setup--log-capture)
2. [Server Lifecycle](#2-server-lifecycle)
3. [Diagnostics](#3-diagnostics)
4. [Document Symbols (Outline)](#4-document-symbols-outline)
5. [Workspace Symbols](#5-workspace-symbols)
6. [Go to Definition](#6-go-to-definition)
7. [Go to Type Definition](#7-go-to-type-definition)
8. [Hover](#8-hover)
9. [Completion — Scope](#9-completion--scope)
10. [Completion — Dot](#10-completion--dot)
11. [Completion — Auto-Import](#11-completion--auto-import)
12. [Signature Help](#12-signature-help)
13. [Inlay Hints](#13-inlay-hints)
14. [Find References](#14-find-references)
15. [Go to Implementation](#15-go-to-implementation)
16. [Call Hierarchy](#16-call-hierarchy)
17. [Code Actions](#17-code-actions)
18. [Semantic Tokens](#18-semantic-tokens)
19. [Code Lens — Test Runner](#19-code-lens--test-runner)
20. [Formatting (Stub)](#20-formatting-stub)
21. [Auto-Indent](#21-auto-indent)
22. [TextMate Grammar](#22-textmate-grammar)
23. [Extension Settings](#23-extension-settings)
24. [Error Resilience](#24-error-resilience)

---

## 1. Setup & Log Capture

### 1.1 Enable Debug Logging

Set the `DOVETAIL_LSP_LOG` environment variable **before** launching VS Code so the
server emits detailed logs for every handler:

```bash
# Option A — launch VS Code from terminal with env var
DOVETAIL_LSP_LOG=debug code /path/to/your/dovetail/workspace

# Option B — set in VS Code settings.json (applies to next server restart)
# In settings.json, set "dovetail.verbose": true  (gives Debug level)
```

### 1.2 VS Code Output Channels

Open the **Output** panel (`Cmd+Shift+U` / `Ctrl+Shift+U`) and select these channels:

| Channel | What it shows |
|---------|--------------|
| **Dovetail Language Server** | Server stdout messages (startup, connection status) |
| **Dovetail Language Server Trace** | Full JSON-RPC trace (when `dovetail.trace.server` is `"messages"` or `"verbose"`) |

### 1.3 Server stderr (Debug Logs)

The `[dovetail-lsp ...]` log messages are written to **stderr**, which VS Code does
not show in the Output panel by default. To capture them:

**Method A — TCP mode (recommended for debugging):**

```bash
# Terminal 1: Start the LSP server manually with debug logging
DOVETAIL_LSP_LOG=debug dovetail lsp-server --tcp --port 9257 --verbose 2> /tmp/dovetail-lsp.log

# Terminal 2: Watch the log
tail -f /tmp/dovetail-lsp.log
```

Then in VS Code settings:
```json
{
  "dovetail.serverConnection": "tcp",
  "dovetail.serverPort": 9257
}
```

Reload the VS Code window (`Cmd+Shift+P` → "Developer: Reload Window").

**Method B — stdio mode with trace:**

Set `"dovetail.trace.server": "verbose"` in VS Code settings. The JSON-RPC messages
appear in the **Dovetail Language Server Trace** channel. Note: this does NOT show
the `[dovetail-lsp DEBUG]` lines — those require Method A.

### 1.4 What to Capture When Reporting Bugs

When a feature doesn't work, capture **all three** of the following:

1. **Server log excerpt** — the `[dovetail-lsp ...]` stderr lines from when you
   triggered the feature. Include 5 lines before and after. Example:
   ```
   [dovetail-lsp DEBUG] did_save: file:///path/to/file.dove, triggering check
   [dovetail-lsp DEBUG] check_and_publish: workspace check succeeded, 2 files with diagnostics
   [dovetail-lsp INFO] check_and_publish: 2 files with diagnostics in 45.3ms
   [dovetail-lsp DEBUG] goto_definition: file:///path/to/file.dove 5:10
   [dovetail-lsp DEBUG] goto_definition → None (typed_module/registry not available)
   ```

2. **The exact cursor position** — file path, line number, column number, and the
   word/symbol under the cursor.

3. **The source code** — the relevant `.dove` file content (or at minimum the
   5 lines around the cursor).

### 1.5 Test Workspace Setup

Create (or use an existing) Dovetail workspace with at least two packages:

```
test-workspace/
├── Dovetail.toml
└── src/
    ├── utils/
    │   └── math.dove
    └── app/
        └── main.dove
```

**Dovetail.toml:**
```toml
[workspace]
name = "test-workspace"

[[project]]
name = "test-app"

[[project.package]]
name = "utils"
path = "src/utils"

[[project.package]]
name = "app"
path = "src/app"
depends-on = ["utils"]
```

**src/utils/math.dove:**
```dovetail
package utils

/// Adds two integers.
public function add(x: Int32, y: Int32): Int32 = x + y

/// Subtracts b from a.
public function subtract(a: Int32, b: Int32): Int32 = a - b

public record Point
  public x: Int32
  public y: Int32

public function origin(): Point = Point(0, 0)

public trait Describable
  function describe(): String

public record NamedPoint
  public name: String
  public x: Int32
  public y: Int32

implement Describable for NamedPoint
  function describe(): String = "point"

test addTest = assert add(1, 2) == 3

test subtractTest = assert subtract(5, 3) == 2
```

**src/app/main.dove:**
```dovetail
package app

import utils.add
import utils.Point
import utils.origin

function main(): Unit =
  let result = add(1, 2)
  let p = origin()
  assert result == 3
  assert p.x == 0
```

Save both files and wait for the initial workspace check to complete.
Confirm the server log shows:

```
[dovetail-lsp DEBUG] initialized: starting workspace check
[dovetail-lsp DEBUG] load_and_check_workspace: root=/path/to/test-workspace
[dovetail-lsp DEBUG] manifest loaded: 1 projects
[dovetail-lsp DEBUG] merging project "test-app": N functions, has_errors=false
[dovetail-lsp DEBUG] workspace check complete: N total functions, M total diagnostics
[dovetail-lsp DEBUG] check_and_publish: workspace check succeeded, ...
[dovetail-lsp INFO] check_and_publish: ... files with diagnostics in ...
```

If you see `has_errors=true` or `workspace check failed`, fix the Dovetail source
before proceeding — most features depend on a successful workspace check.

---

## 2. Server Lifecycle

### Test 2.1 — Server Starts on .dove File Open

| Step | Action |
|------|--------|
| 1 | Close all VS Code windows |
| 2 | Open the test workspace folder in VS Code |
| 3 | Open any `.dove` file |

**Expected:**
- Output channel "Dovetail Language Server" shows:
  `Starting Dovetail Language Server (connection=stdio, server=dovetail, ...)`
  then `Dovetail Language Server started successfully`
- Server log shows `initialize: root_uri=Some(...)` followed by `initialized: starting workspace check`

**Log to capture:** `initialize` and `initialized` lines.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 2.2 — Server Starts on Dovetail.toml Workspace

| Step | Action |
|------|--------|
| 1 | Open a folder containing `Dovetail.toml` but no `.dove` files open yet |

**Expected:** The extension activates due to `workspaceContains:Dovetail.toml`.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 2.3 — Server Shutdown

| Step | Action |
|------|--------|
| 1 | Close the VS Code window |

**Expected:** Server log shows `[dovetail-lsp DEBUG] shutdown`.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 3. Diagnostics

### Test 3.1 — Error Diagnostics on Save

| Step | Action |
|------|--------|
| 1 | Open `main.dove` |
| 2 | Add a line: `let bad = unknownFunction()` |
| 3 | Save the file |

**Expected:**
- Red squiggly underline appears on `unknownFunction`
- Problems panel shows an error with file, line, column
- Server log shows:
  ```
  did_save: file:///.../main.dove, triggering check
  check_and_publish: workspace check succeeded, N files with diagnostics
  ```

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 3.2 — Diagnostics Clear When Error is Fixed

| Step | Action |
|------|--------|
| 1 | Remove the `let bad = unknownFunction()` line |
| 2 | Save the file |

**Expected:**
- Red underline disappears
- Problems panel shows no errors for this file

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 3.3 — Cross-file Diagnostics

| Step | Action |
|------|--------|
| 1 | In `math.dove`, rename `add` to `addNumbers` |
| 2 | Save `math.dove` |

**Expected:**
- Error appears in `main.dove` (import `utils.add` fails or `add(1,2)` is unknown)
- Both files may show diagnostics

| Step | Action |
|------|--------|
| 3 | Undo the rename and save |

**Expected:** All diagnostics clear.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 3.4 — Manifest Error

| Step | Action |
|------|--------|
| 1 | Temporarily break `Dovetail.toml` (e.g., remove a required field) |
| 2 | Save any `.dove` file to trigger a check |

**Expected:**
- Server log shows `[dovetail-lsp ERROR] manifest errors: ...`
- VS Code shows a notification: "Dovetail workspace check failed..."
- Features that depend on typed_module return None (e.g., go-to-definition stops working)

| Step | Action |
|------|--------|
| 3 | Fix `Dovetail.toml` and save a `.dove` file |

**Expected:** Server recovers — log shows successful workspace check, features work again.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 4. Document Symbols (Outline)

### Test 4.1 — Outline View

| Step | Action |
|------|--------|
| 1 | Open `math.dove` |
| 2 | Open the **Outline** view in the Explorer sidebar (or `Cmd+Shift+O`) |

**Expected:**
- Outline shows: functions (`add`, `subtract`, `origin`), record (`Point`),
  trait (`Describable`), record (`NamedPoint`), implement block, tests
- Each symbol has the correct icon (function, struct, interface)
- Server log: `document_symbol: file:///.../math.dove → N symbols`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 4.2 — Go to Symbol in File

| Step | Action |
|------|--------|
| 1 | Press `Cmd+Shift+O` (Go to Symbol in File) |
| 2 | Type `Point` |

**Expected:** Filters to `Point` and `NamedPoint`; selecting one jumps to its declaration.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 5. Workspace Symbols

### Test 5.1 — Workspace Symbol Search

| Step | Action |
|------|--------|
| 1 | Press `Cmd+T` (Go to Symbol in Workspace) |
| 2 | Type `add` |

**Expected:**
- Results include `add` from `utils` package
- Selecting it opens `math.dove` at the `add` function
- Server log: `symbol: query="add" → N results`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 5.2 — Empty Query

| Step | Action |
|------|--------|
| 1 | Press `Cmd+T` and leave the query empty |

**Expected:** Shows all symbols from the workspace.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 6. Go to Definition

### Test 6.1 — Go to Imported Function

| Step | Action |
|------|--------|
| 1 | Open `main.dove` |
| 2 | Place cursor on `add` in `let result = add(1, 2)` |
| 3 | Press `F12` (Go to Definition) or `Cmd+Click` |

**Expected:**
- Jumps to `function add(...)` in `math.dove`
- Server log: `goto_definition: file:///.../main.dove L:C → file:///.../math.dove:L:C`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 6.2 — Go to Record Definition

| Step | Action |
|------|--------|
| 1 | Place cursor on `Point` in `import utils.Point` |
| 2 | Press `F12` |

**Expected:** Jumps to `public record Point` in `math.dove`.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 6.3 — Go to Local Variable Definition

| Step | Action |
|------|--------|
| 1 | Place cursor on `result` in `assert result == 3` |
| 2 | Press `F12` |

**Expected:** Jumps to `let result = add(1, 2)` in the same file.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 6.4 — Go to Field Definition

| Step | Action |
|------|--------|
| 1 | Place cursor on `x` in `p.x` |
| 2 | Press `F12` |

**Expected:** Jumps to `public x: Int32` in the `Point` record in `math.dove`.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 6.5 — Definition on Whitespace/Keyword

| Step | Action |
|------|--------|
| 1 | Place cursor on the keyword `let` or on whitespace |
| 2 | Press `F12` |

**Expected:**
- Nothing happens (no jump)
- Server log: `goto_definition → None (no node at position)`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 7. Go to Type Definition

### Test 7.1 — Variable to its Type

| Step | Action |
|------|--------|
| 1 | Place cursor on `p` in `let p = origin()` |
| 2 | Right-click → "Go to Type Definition" (or keybinding) |

**Expected:**
- Jumps to `public record Point` in `math.dove`
- Server log: `goto_type_definition: ... → file:///.../math.dove:L:C`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 7.2 — Primitive Type

| Step | Action |
|------|--------|
| 1 | Place cursor on `result` (Int32 type) |
| 2 | Go to Type Definition |

**Expected:**
- Either jumps to the Int32 primitive definition (prelude) or returns None
- Server log shows the response

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 8. Hover

### Test 8.1 — Hover on Function Call

| Step | Action |
|------|--------|
| 1 | Hover the mouse over `add` in `add(1, 2)` |

**Expected:**
- Tooltip shows the function signature: `function add(x: Int32, y: Int32): Int32`
- If `add` has a doc comment (`/// Adds two integers.`), it appears below the signature
- Server log: `hover: ... → Some(hover info)`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 8.2 — Hover on Variable

| Step | Action |
|------|--------|
| 1 | Hover over `result` in `assert result == 3` |

**Expected:** Tooltip shows the inferred type, e.g., `let result: Int32`.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 8.3 — Hover on Record Field

| Step | Action |
|------|--------|
| 1 | Hover over `x` in `p.x` |

**Expected:** Tooltip shows field type info, e.g., `x: Int32` (from `Point`).

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 8.4 — Hover on Type Name

| Step | Action |
|------|--------|
| 1 | Hover over `Point` in `import utils.Point` |

**Expected:** Tooltip shows the record definition or type summary.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 8.5 — Hover on Whitespace

| Step | Action |
|------|--------|
| 1 | Hover over empty space or a keyword like `let` |

**Expected:**
- No tooltip
- Server log: `hover → None (no node at position)`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 9. Completion — Scope

### Test 9.1 — Local Variables and Parameters

| Step | Action |
|------|--------|
| 1 | In `main.dove`, place cursor after `assert result == 3` on a new line |
| 2 | Start typing `res` |
| 3 | Trigger completion (`Ctrl+Space` if it doesn't auto-trigger) |

**Expected:**
- Completion list includes `result` (local variable)
- Server log: `completion: ... (scope/auto-import)`, `scope_completion: prefix="res"`, `scope_completion → N items`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 9.2 — Imported Symbols

| Step | Action |
|------|--------|
| 1 | Start typing `ad` on a new line |
| 2 | Trigger completion |

**Expected:**
- `add` appears (imported from `utils`)
- Other functions matching `ad` from prelude or same package may appear

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 9.3 — Same-Package Symbols

| Step | Action |
|------|--------|
| 1 | Add a helper function `function helper(): Int32 = 42` in `main.dove` |
| 2 | Start typing `hel` in `main()` |

**Expected:** `helper` appears in completion list.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 10. Completion — Dot

### Test 10.1 — Record Field Completion

| Step | Action |
|------|--------|
| 1 | In `main.dove`, after `let p = origin()`, type `p.` on a new line |

**Expected:**
- Completion list shows `x` and `y` (fields of `Point`)
- Server log: `completion: ... (dot trigger)`, `dot_completion: receiver type = ...`, `dot_completion → 2 items`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 10.2 — Method Completion on String/Built-in

| Step | Action |
|------|--------|
| 1 | Type `"hello".` |

**Expected:**
- Completion list shows String methods (if any are defined in prelude/registry)
- Or empty list if String has no methods yet
- Server log shows receiver type

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 10.3 — Dot Completion on Unknown Expression

| Step | Action |
|------|--------|
| 1 | Type `unknownVar.` |

**Expected:**
- No completions (or empty list)
- Server log: `dot_completion → None (no expression type at position)`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 11. Completion — Auto-Import

### Test 11.1 — Auto-Import Suggestion

| Step | Action |
|------|--------|
| 1 | In `main.dove`, remove the `import utils.subtract` if present |
| 2 | Start typing `sub` on a new line in `main()` |

**Expected:**
- Completion list includes `subtract` with a note like "(auto-import from utils)"
- Accepting it inserts `import utils.subtract` at the top of the file
- Server log: `scope_completion → N items` (includes auto-import items)

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 12. Signature Help

### Test 12.1 — Function Call Signature

| Step | Action |
|------|--------|
| 1 | In `main.dove`, type `add(` |

**Expected:**
- Signature help popup appears showing `add(x: Int32, y: Int32): Int32`
- First parameter `x` is highlighted/bold
- Doc comment "Adds two integers." appears if available
- Server log: `signature_help: ... → 1 signatures`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 12.2 — Active Parameter Advances

| Step | Action |
|------|--------|
| 1 | Continue typing: `add(1, ` |

**Expected:**
- Second parameter `y` is now highlighted
- Signature help remains visible

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 12.3 — Signature Help Outside Call

| Step | Action |
|------|--------|
| 1 | Place cursor on a line outside any function call |
| 2 | Trigger signature help manually (`Cmd+Shift+Space`) |

**Expected:**
- No signature help shown
- Server log: `signature_help → None`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 13. Inlay Hints

### Test 13.1 — Type Hint on Let Binding

| Step | Action |
|------|--------|
| 1 | Open `main.dove` |
| 2 | Look at `let result = add(1, 2)` |

**Expected:**
- An inlay hint appears after `result` showing `: Int32` (grayed out, inline)
- Server log: `inlay_hint: ... → N hints`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 13.2 — No Hint When Type Annotation Present

| Step | Action |
|------|--------|
| 1 | Change to `let result: Int32 = add(1, 2)` |
| 2 | Save the file |

**Expected:** No inlay hint on this line (explicit annotation suppresses it).

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 13.3 — Parameter Name Hints

| Step | Action |
|------|--------|
| 1 | Look at `add(1, 2)` in `main.dove` |

**Expected:**
- Parameter name hints appear: `x:` before `1` and `y:` before `2`
- (This depends on the inlay hints implementation including parameter hints)

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 14. Find References

### Test 14.1 — Find All References of a Function

| Step | Action |
|------|--------|
| 1 | Place cursor on `add` in its definition in `math.dove` |
| 2 | Press `Shift+F12` (Find All References) |

**Expected:**
- References panel shows all uses: definition in `math.dove`, call in `main.dove`,
  and any test uses
- Server log: `references: ... → N locations`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 14.2 — Find References of a Record

| Step | Action |
|------|--------|
| 1 | Place cursor on `Point` in its definition |
| 2 | Press `Shift+F12` |

**Expected:** Shows all uses of `Point` across files (imports, type annotations, constructor calls).

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 14.3 — Find References of a Local Variable

| Step | Action |
|------|--------|
| 1 | Place cursor on `result` in `main.dove` |
| 2 | Press `Shift+F12` |

**Expected:** Shows the `let` binding and all uses within the function.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 15. Go to Implementation

### Test 15.1 — Trait to Implementations

| Step | Action |
|------|--------|
| 1 | Place cursor on `Describable` in its trait definition in `math.dove` |
| 2 | Right-click → "Go to Implementations" (or `Cmd+F12`) |

**Expected:**
- Jumps to (or shows list of) `implement Describable for NamedPoint`
- Server log: `goto_implementation: ... → N locations`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 15.2 — No Implementations

| Step | Action |
|------|--------|
| 1 | Create a trait with no implementations |
| 2 | Go to Implementations on it |

**Expected:**
- No results
- Server log: `goto_implementation → None (no implementations found)`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 16. Call Hierarchy

### Test 16.1 — Prepare Call Hierarchy

| Step | Action |
|------|--------|
| 1 | Place cursor on `add` in its definition |
| 2 | Right-click → "Show Call Hierarchy" (or `Shift+Alt+H`) |

**Expected:**
- Call hierarchy view opens showing `add` as the root
- Server log: `prepare_call_hierarchy: ... → 1 items`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 16.2 — Incoming Calls

| Step | Action |
|------|--------|
| 1 | In the Call Hierarchy view, expand "Incoming Calls" for `add` |

**Expected:**
- Shows `main` (and the `addTest` test) as callers
- Server log: `incoming_calls: add → N callers`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 16.3 — Outgoing Calls

| Step | Action |
|------|--------|
| 1 | Prepare call hierarchy on `main` in `main.dove` |
| 2 | Expand "Outgoing Calls" |

**Expected:**
- Shows `add`, `origin`, and `assert` as callees
- Server log: `outgoing_calls: main → N callees`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 17. Code Actions

### Test 17.1 — Auto-Import Quick Fix

| Step | Action |
|------|--------|
| 1 | In `main.dove`, add `let s = subtract(5, 3)` without importing `subtract` |
| 2 | Save to trigger diagnostics |
| 3 | Click the lightbulb on the error or press `Cmd+.` |

**Expected:**
- Code action offered: "Import utils.subtract" (or similar)
- Applying it inserts the import statement
- Server log: `code_action: ... → N actions`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 17.2 — Organize Imports

| Step | Action |
|------|--------|
| 1 | Add several imports in random order |
| 2 | Place cursor anywhere in the import block |
| 3 | Press `Cmd+.` or right-click → "Source Action..." → "Organize Imports" |

**Expected:**
- Imports are sorted and grouped by package
- Server log: `code_action: ... → N actions`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 17.3 — Add Type Annotation

| Step | Action |
|------|--------|
| 1 | Place cursor on `let result = add(1, 2)` (no explicit type annotation) |
| 2 | Press `Cmd+.` |

**Expected:**
- Code action offered: "Add type annotation" (or similar)
- Applying it changes to `let result: Int32 = add(1, 2)`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 18. Semantic Tokens

### Test 18.1 — Semantic Highlighting Active

| Step | Action |
|------|--------|
| 1 | Open `math.dove` |
| 2 | Check that semantic highlighting is visually distinct from TextMate grammar only |

**Expected:**
- Function names, type names, parameters, variables, enum members get semantic colors
- Different from plain TextMate: e.g., type names colored differently from function names
- Server log: `semantic_tokens_full: ... → N tokens`

**Tip:** To verify semantic tokens are active, use `Cmd+Shift+P` →
"Developer: Inspect Editor Tokens and Scopes" and click on a symbol.
The popup should show a "semantic token type" entry (e.g., `function`, `type`, `variable`).

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 18.2 — Token Types

Verify each token type by inspecting tokens (Developer: Inspect Editor Tokens):

| Symbol | Expected Semantic Token Type |
|--------|------------------------------|
| `add` (function definition) | `function` + `declaration` modifier |
| `Point` (record name) | `type` or `class` + `declaration` modifier |
| `Describable` (trait name) | `interface` + `declaration` modifier |
| `x` (parameter) | `parameter` |
| `result` (local variable) | `variable` |
| `x` (record field) | `property` |

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 19. Code Lens — Test Runner

### Test 19.1 — Run Test Code Lens Appears

| Step | Action |
|------|--------|
| 1 | Open `math.dove` which contains `test addTest` and `test subtractTest` |

**Expected:**
- "Run Test" appears above each test declaration
- "Run All Tests (2)" appears above the first test
- Server log: `code_lens: ... → N lenses`

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 19.2 — Run Single Test

| Step | Action |
|------|--------|
| 1 | Click "Run Test" above `test addTest` |

**Expected:**
- Terminal opens and runs `dovetail test --filter "utils.addTest" --verbose`
- Test output appears with PASS/FAIL status
- Problem matcher captures any failures

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 19.3 — Run All Tests in File

| Step | Action |
|------|--------|
| 1 | Click "Run All Tests (2)" |

**Expected:**
- Terminal runs `dovetail test --file "src/utils/math.dove" --verbose`
- Both tests execute

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 19.4 — Run All Tests in Workspace (Command Palette)

| Step | Action |
|------|--------|
| 1 | `Cmd+Shift+P` → "Dovetail: Run All Tests in Workspace" |

**Expected:** Terminal runs `dovetail test --verbose`.

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 20. Formatting (Stub)

### Test 20.1 — Format Document

| Step | Action |
|------|--------|
| 1 | Open any `.dove` file |
| 2 | Press `Shift+Alt+F` (Format Document) |

**Expected:**
- Nothing changes (formatting is not implemented yet)
- Server log: `formatting: ... → None (not implemented)`
- No error dialog appears

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## 21. Auto-Indent

### Test 21.1 — Indent After `=`

| Step | Action |
|------|--------|
| 1 | Type `function foo(): Int32 =` and press Enter |

**Expected:** Next line is indented one level deeper.

| Result |
|--------|
| PASS / FAIL |

### Test 21.2 — Indent After `then`

| Step | Action |
|------|--------|
| 1 | Type `if x > 0 then` and press Enter |

**Expected:** Next line is indented.

| Result |
|--------|
| PASS / FAIL |

### Test 21.3 — Indent After `else`

| Step | Action |
|------|--------|
| 1 | Type `else` and press Enter |

**Expected:** Next line is indented.

| Result |
|--------|
| PASS / FAIL |

### Test 21.4 — Indent After `do`

| Step | Action |
|------|--------|
| 1 | Type `while x > 0 do` and press Enter |

**Expected:** Next line is indented.

| Result |
|--------|
| PASS / FAIL |

### Test 21.5 — Indent After `->`

| Step | Action |
|------|--------|
| 1 | Type `case x ->` and press Enter |

**Expected:** Next line is indented.

| Result |
|--------|
| PASS / FAIL |

### Test 21.6 — No Indent on Normal Line

| Step | Action |
|------|--------|
| 1 | Type `let x = 42` and press Enter |

**Expected:** Next line is at the same indent level.

| Result |
|--------|
| PASS / FAIL |

---

## 22. TextMate Grammar

### Test 22.1 — Keyword Highlighting

| Step | Action |
|------|--------|
| 1 | Open any `.dove` file |
| 2 | Verify that keywords have syntax coloring |

**Expected:** Keywords like `function`, `let`, `if`, `then`, `else`, `match`, `case`,
`record`, `enum`, `class`, `trait`, `implement`, `import`, `package`, `public`,
`test`, `assert` are highlighted in the keyword color.

| Result |
|--------|
| PASS / FAIL |

### Test 22.2 — String and Number Literals

**Expected:**
- `"hello"` is colored as a string
- `42`, `3.14` are colored as numbers
- `true`, `false` are colored as language constants

| Result |
|--------|
| PASS / FAIL |

### Test 22.3 — Comments

| Step | Action |
|------|--------|
| 1 | Add a line `// this is a comment` |

**Expected:** The comment is colored in the comment color.

| Result |
|--------|
| PASS / FAIL |

---

## 23. Extension Settings

### Test 23.1 — Custom Server Path

| Step | Action |
|------|--------|
| 1 | Set `"dovetail.serverPath": "/absolute/path/to/dovetail"` in settings |
| 2 | Reload VS Code |

**Expected:**
- Server starts using the specified binary path
- Output channel shows the custom path in startup message

| Result |
|--------|
| PASS / FAIL |

### Test 23.2 — TCP Connection Mode

| Step | Action |
|------|--------|
| 1 | Start LSP server manually: `dovetail lsp-server --tcp --port 9257` |
| 2 | Set `"dovetail.serverConnection": "tcp"`, `"dovetail.serverPort": 9257` |
| 3 | Reload VS Code |

**Expected:**
- Output channel shows TCP connection attempts
- Server connects and features work

| Result |
|--------|
| PASS / FAIL |

### Test 23.3 — Verbose Mode

| Step | Action |
|------|--------|
| 1 | Set `"dovetail.verbose": true` |
| 2 | Reload VS Code |

**Expected:**
- Server starts with `--verbose` flag
- More detailed log output in stderr

| Result |
|--------|
| PASS / FAIL |

### Test 23.4 — Trace Server

| Step | Action |
|------|--------|
| 1 | Set `"dovetail.trace.server": "verbose"` |
| 2 | Reload VS Code |
| 3 | Open the "Dovetail Language Server Trace" output channel |

**Expected:** Full JSON-RPC request/response messages are logged.

| Result |
|--------|
| PASS / FAIL |

---

## 24. Error Resilience

### Test 24.1 — Features Work After Parse Errors

| Step | Action |
|------|--------|
| 1 | Add a syntax error to `main.dove` (e.g., unclosed paren) |
| 2 | Save the file |
| 3 | Try go-to-definition on `add` in the valid part of the file |

**Expected:**
- Diagnostics show the parse error
- Navigation may still work on valid parts (depends on error recovery)
- Server does not crash

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 24.2 — Recovery After Fixing Errors

| Step | Action |
|------|--------|
| 1 | Fix the syntax error from Test 24.1 |
| 2 | Save the file |

**Expected:**
- Diagnostics clear
- All features resume working
- Server log shows successful workspace check

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 24.3 — Server Survives Workspace Panic

| Step | Action |
|------|--------|
| 1 | If possible, create a source that triggers a compiler panic |

**Expected:**
- Server log: `[dovetail-lsp ERROR] panic in build_workspace: ...`
- VS Code shows notification: "Dovetail workspace check panicked: ..."
- Server stays alive; next save retries the check
- Features that depend on typed_module return None gracefully

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

### Test 24.4 — Debounced Analysis on Keystrokes

| Step | Action |
|------|--------|
| 1 | Type several characters rapidly in `main.dove` without saving |
| 2 | Stop typing and wait ~500ms |

**Expected:**
- Server log shows `did_change` events for each keystroke
- Only one `check_workspace (debounced)` runs after the 300ms quiet period
- No completion lag or freezes during typing

| Result | Log |
|--------|-----|
| PASS / FAIL | _(paste log)_ |

---

## Results Summary

Fill in after completing all tests:

| Section | Tests | Passed | Failed | Notes |
|---------|-------|--------|--------|-------|
| 2. Server Lifecycle | 3 | | | |
| 3. Diagnostics | 4 | | | |
| 4. Document Symbols | 2 | | | |
| 5. Workspace Symbols | 2 | | | |
| 6. Go to Definition | 5 | | | |
| 7. Go to Type Definition | 2 | | | |
| 8. Hover | 5 | | | |
| 9. Completion — Scope | 3 | | | |
| 10. Completion — Dot | 3 | | | |
| 11. Completion — Auto-Import | 1 | | | |
| 12. Signature Help | 3 | | | |
| 13. Inlay Hints | 3 | | | |
| 14. Find References | 3 | | | |
| 15. Go to Implementation | 2 | | | |
| 16. Call Hierarchy | 3 | | | |
| 17. Code Actions | 3 | | | |
| 18. Semantic Tokens | 2 | | | |
| 19. Code Lens — Test Runner | 4 | | | |
| 20. Formatting (Stub) | 1 | | | |
| 21. Auto-Indent | 6 | | | |
| 22. TextMate Grammar | 3 | | | |
| 23. Extension Settings | 4 | | | |
| 24. Error Resilience | 4 | | | |
| **Total** | **71** | | | |

---

## Quick Reference: Log Message Patterns

### Successful Handler Response
```
[dovetail-lsp DEBUG] {handler}: {uri} {line}:{col}
[dovetail-lsp DEBUG] {handler} → {file}:{line}:{col}           # navigation
[dovetail-lsp DEBUG] {handler} → N items/results/locations      # lists
[dovetail-lsp DEBUG] {handler} → Some(hover info)               # hover
```

### Handler Returned Nothing (with reason)
```
[dovetail-lsp DEBUG] {handler} → None (no workspace root)
[dovetail-lsp DEBUG] {handler} → None (typed_module/registry not available)
[dovetail-lsp DEBUG] {handler} → None (typed_module not available)
[dovetail-lsp DEBUG] {handler} → None (no node at position)
[dovetail-lsp DEBUG] {handler} → None (no source file/import scope)
[dovetail-lsp DEBUG] {handler} → None (no expression type at position)
[dovetail-lsp DEBUG] {handler} → None (no definition found)
[dovetail-lsp DEBUG] {handler} → None (no implementations found)
[dovetail-lsp DEBUG] {handler} → None (no references found)
```

### Workspace Check
```
[dovetail-lsp DEBUG] load_and_check_workspace: root=/path/...
[dovetail-lsp DEBUG] manifest loaded: N projects
[dovetail-lsp DEBUG] content overlays: N files
[dovetail-lsp DEBUG] merging project "name": N functions, has_errors=false
[dovetail-lsp DEBUG] workspace check complete: N total functions, M total diagnostics
[dovetail-lsp DEBUG] check_and_publish: workspace check succeeded, N files with diagnostics
[dovetail-lsp INFO]  check_and_publish: N files with diagnostics in Xms
```

### Errors
```
[dovetail-lsp ERROR] manifest errors: ...
[dovetail-lsp ERROR] panic in build_workspace: ...
[dovetail-lsp ERROR] workspace check failed
[dovetail-lsp ERROR] workspace check panicked: ...
```
