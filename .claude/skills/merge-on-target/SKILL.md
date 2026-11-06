---
name: merge-on-target
description: When the chat's target is met or all tasks are finished, and before the chat is archived or closed, check whether the work is on main and ask the user whether to merge it. Use at the end of every task or session that ran on a branch or worktree other than main.
---

# Merge on target

Work left on a side branch is easy to lose when a chat is archived. Once the target is met, ask about
merging before anything is closed, then hand over to `clean-branches`.

## Ground rules

- **Ask, don't assume.** Merging changes `main`, so wait for a clear yes. A yes covers this branch
  only, not later ones.
- **Never touch remotes** (no fetch/pull/push). Merging here means local `main` only; pushing stays
  the user's call, and you remind them if `main` ends up ahead of `origin/main`.
- Follow CLAUDE.md: work comes in as **linear commits, no merge commits**, and the checks in
  "Working agreements" pass first.
- Do not merge `web-ui-split` unless the user names it; it is managed separately.

## 1. Is the target met?

Restate the target in one line and check it against what was done: tasks complete, tests/checks for
the touched crates run, `TODO.md` / `TESTS.md` / docs updated as CLAUDE.md requires. If something is
missing, say so and finish or list it; do not ask to merge unfinished work.

## 2. Where does the work live?

```bash
git branch --show-current
git status --short
git cherry main HEAD
git log --oneline main..HEAD
git rev-list --left-right --count origin/main...main
```

- On `main` already, nothing uncommitted: nothing to merge. Mention if `main` is ahead of
  `origin/main` (unpushed) and go to step 4.
- Uncommitted changes: offer to commit them first (CLAUDE.md: one commit per step, with the
  `Co-Authored-By` line).
- Commits with `+` in `git cherry main HEAD`: not on main. Go to step 3.
- All `-`: already on main (cherry-picked); go to step 4.

## 3. Ask to merge

Ask once, concisely, with the facts: branch name, the commits that would land
(`git log --oneline main..HEAD`), whether checks passed, and the method you'd use (rebase or
cherry-pick onto `main` so history stays linear; `main` is checked out in the base checkout, so do it
there or via the worktree-aware route the app provides). Offer: **merge now**, **leave on the branch**
(and say the branch/worktree is kept), or **something else**.

On a yes: do it, rerun the quick checks for the touched crates, report the resulting `main` SHA and that
nothing was pushed. On no or silence: leave everything as it is and state plainly that the work is
still only on `<branch>`.

## 4. Hand over to clean-branches

After the merge decision, and before the chat is archived or closed, run the `clean-branches` skill so
merged branches/worktrees are tidied and unmerged or unpushed work is warned about. Do not archive or
close the chat yourself until the user has seen that report.
