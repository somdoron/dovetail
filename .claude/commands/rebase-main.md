Rebase the current branch on top of the latest origin/main.

Steps:
1. Run `git status` to check for uncommitted changes.
2. If there are modified or staged files, run `git stash` to save them.
3. Run `git pull --rebase origin main` to rebase on the latest main.
4. If there are merge conflicts, resolve them and continue the rebase with `git rebase --continue`.
5. If files were stashed in step 2, run `git stash pop` to restore them.
6. If there are conflicts from the stash pop, resolve those as well.
7. Report the result.
