Commit all changes, push to remote, and open a pull request.

Steps:
1. Run `git status` and `git diff` to review all current changes.
2. Run `git log` to understand the commit message style of the repository.
3. Run `cargo clippy` and fix any warnings or errors.
4. Stage all relevant changed files (avoid staging secrets or generated files).
5. Create a commit with a concise, descriptive message that follows the repository's style.
5. Push the branch to the remote (create the remote branch if needed with `-u`).
6. Use `gh pr create` to open a pull request with:
   - A short, descriptive title (under 70 characters)
   - A body with a `## Summary` section (1-3 bullet points) and a `## Test plan` section
7. Return the PR URL.
