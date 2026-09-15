# Known issues — async runtime

**Currently empty.** Every item opened by the scope-attached-resources review
(August 2026) has been closed — see the commits from `standard-io: run every
finalizer in a closing scope's unwind` onwards. Four were code changes; the
rest were decisions to keep the behaviour, and each of those was closed by
writing the reasoning where the code is, not by dropping it.

This file tracks **policy calls**, not defects: behaviours in the `standard-io`
async runtime that are consistent with a documented doctrine, where changing
them would trade one guarantee for another and the suite passes either way. A
defect goes in a commit and a test; an item goes here only when a reader could
reasonably conclude the runtime is wrong and the answer is "no, and here is
what it costs".

Closing one means either changing the code (with a test that pins the new
behaviour) or recording the decision at the site that implements it —
`Runtime.dove`'s comments, `async-runtime-design.md`,
`resource-management-design.md`, `component-model-design.md` — so the reasoning
outlives this file. Deleting an entry without doing one of those loses it.

Line references drift as files are edited; name the function, not the line.
