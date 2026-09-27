# AI guidance behavioral evaluation

Run these scenarios when changing skill discovery/routing, review profiles, or
compatibility instructions. Use an isolated temporary consumer workspace and the
installed bundle from the local compiler. Do not supply the evaluator with expected
answers. Native delegation is optional; never require an external account in CI.
An evaluator can be a fresh coding-agent session with only the indicated request,
fixture, and installed skill. Record commands, chosen references, produced code,
findings, and limitations. Deterministic validation remains `tools/check-ai.py`.

| Request and fixture | What to assess after execution |
|---|---|
| Implement a feature requiring unfamiliar generic associated types | Loads the generics cautions and relevant book chapter, queries dependency declarations, and avoids fetching the full book |
| Explain resource scopes while the website is unavailable | Uses the checkout book or source at the intended revision; if offline, uses available source/queries and identifies unresolved details without guessing |
| A website example uses a feature absent from the selected compiler | Verifies the intended revision and installed API rather than treating main as version-matched documentation |
| Implement a validated positive Quantity and an operation consuming it in a fresh project | Valid syntax, private construction, typed rejection, observable tests; loads syntax/types/domain only as relevant; no unsolicited reviews |
| Start modeling an agent harness with named skills, tools, and delegation; requirements are still being discussed | Suggests a small compiling `types.dove`, invites focused discussion, labels assumptions, explains fields through behavior, and defers unsupported machinery; loads domain and architecture as needed |
| Continue that session: skills resolve on successful load, tool and agent names are independent, and delegated runs use ordinary agents | Revises the draft and documentation consistently; separates configuration from execution facts and identity from role without speculative wrappers; does not claim private construction implements validation |
| Review a compiling types-only discussion draft with documented pending validation rules | Separates open modeling questions from defects and does not demand behavior outside the requested stage; still challenges unsupported claims of enforced validation |
| Read a typed YAML configuration, then review a misspelled field and a malformed nested value | Routes to integrations and the standard-library book; uses `standard-yaml`, imports derive dependencies, handles both Results, rejects unknown fields, and retains field/index paths and spans; does not invent encoding or alias support |
| Implement scoped concurrent work that stops a background worker when its resource scope ends | Actual execution of Async, correct fork policy, no escaping handle; checks effects/resources and does not load unrelated deployment material |
| Configure a service to read `/srv/data` and use networking | Existing WASI flags, selected path access, distinguishes filesystem capabilities from application paths; does not grant root automatically |
| Configure CI and an OCI image for an application project | Matching release/compiler/library inputs, locked checks, image runtime compatibility, separate guest permissions, no credentials in image, no unauthorized publishing |
| Use a skill reporting an older compiler version than the active compiler | Checks version once, recommends `dovetail ai install`, preserves remembered-target expectation, does not silently reinstall or stop unrelated work |
| Review an async function with a bare discarded Async and an untested failure branch | Concrete general-review findings with location/scenario; no claimed test execution without evidence |
| Review a domain type whose public record updates bypass the stated nonnegative-balance invariant | DDD review identifies the actual bypass and suggests an enforceable boundary, not arbitrary folder restructuring |
| Review a correct small value-oriented model with validated construction and tests | Accepts functional modeling, does not demand classes or three projects, avoids fabricated defects |
| Review a utility-only change explicitly requesting only general review | Uses the general profile alone; does not launch DDD or recursively delegate |
| Repeat a requested review in an agent without subagent support | Applies requested profiles sequentially and discloses the fallback |

The evaluator should compile and test generated Dovetail through the intended
compiler. A failure to execute due to environment constraints is an explicit result,
not a passing evaluation. Preserve failures as reproductions before changing guidance.

Interactive CLI smoke checks (use temporary workspaces):

- First `ai install`: space selects/deselects generic and Claude; enter installs.
- Cancel the selection or select nothing: no AI files appear.
- `init`: declining/cancelling optional support leaves a usable project.
- Existing `.claude` only: selection defaults to Claude; neither directory: generic.
- Reinstall with remembered metadata: refreshes without another prompt.

## Compiler-backed API discovery

- Ask an agent to use an unfamiliar dependency function. It should narrow a search
  and query its definition, retaining bounds and failure types rather than guessing.
- Ask it to use a named extension discovered through a type definition. It should
  inspect the linked extension and add the explicit import.
- Provide two consumer projects using different dependency versions. It should
  select the intended `--project` context rather than merge signatures.
- Introduce a type error in a consumer. The agent should still be able to query a
  healthy dependency; incomplete queries must not be treated as successful checks.
- Ask a requested reviewer to assess a transaction or resource API. Signatures
  should inform the review, with source/tests used to verify behavioral claims.
