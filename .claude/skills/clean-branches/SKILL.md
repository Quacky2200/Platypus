---
name: clean-branches
description: Review local git branches (and their worktrees) other than main and the web-ui branch, report which are unmerged, which are already on main and safe to tidy, and warn when removing a merged branch would lose work that main has not pushed yet. Run before archiving or closing a chat, when all tasks are finished, or when the main target is met.
---

# Clean branches

Platypus (and its `web-ui-split` branch) collects branches and worktrees from many sessions. Run this
before a chat is archived or closed, or once the main target is done, so nothing unmerged is lost and
nothing merged is left behind.

## Ground rules

- **Never touch remotes.** No `git fetch`, `pull`, `push`, `remote update`. Work from the local refs,
  including the `origin/main` ref as it is now (say so if it may be stale).
- **Skip, never delete or merge:** `main`, `web-ui-split` and its worktree (`.claude/worktrees/web-ui`),
  and the branch/worktree this chat is running in until the user says it is finished.
- **Read first, delete last.** Steps 1-4 change nothing. Only step 5 deletes, and only what the user
  confirmed. Use `git branch -d` for merged branches; `-D` only after the user confirms a
  patch-equivalent branch (step 3). Never delete an unmerged branch without an explicit, named yes.
- Do not mass-convert or edit files here; this skill is git bookkeeping only.

## 1. Gather

```bash
git status --short
git branch -vv --no-color
git worktree list
git rev-list --left-right --count origin/main...main
```

The last line is `behind ahead` for `main` against `origin/main`. Call the `ahead` number **A**.
Skip `main` and `web-ui-split`; everything else is a candidate.

## 2. Classify each candidate

Platypus work often comes in as **linear cherry-picked commits** (no merge commits, new SHAs), so
`--merged` alone misses it. Use both:

```bash
git branch --merged main --no-color           # tip is an ancestor of main
git cherry main <branch>                      # '-' = patch already on main, '+' = not on main
git log --oneline main..<branch>              # what main lacks
git log --oneline <branch>@{u}..<branch>      # unpushed commits on the branch itself (if it has an upstream)
```

Sort into:

| Class | Test | Action |
|---|---|---|
| **Merged** | in `--merged main` | tidy |
| **Applied** | every `git cherry` line is `-` | tidy after confirm (needs `-D`) |
| **Partly applied** | mix of `-` and `+` | keep, list the `+` commits |
| **Unmerged** | all `+` / commits ahead | keep, list the commits and ask what to do |
| **Backup** (`*-backup`) | name | same tests, but say it is a backup; ask |

Also note for each: its worktree (path) and whether that worktree is dirty
(`git -C <path> status --short`) or detached HEAD. A dirty or detached worktree is never removed.

## 3. Warn about merged-but-unpushed

A branch counted as merged/applied is "on main", but if `main` is **not pushed**, those commits exist
only on this machine and the branch is the only other copy.

- If **A > 0**, warn: "main is A commits ahead of origin/main; these merged branches' work is not on
  the remote yet: <list>". Removing the branch is still safe for git (the commits are on main), but
  point out the work is unpushed so the user can push main first. Do not push for them.
- Also warn for a merged branch that has its own unpushed commits relative to its upstream that are
  *not* on main (`git cherry main` shows `+`) - those were never merged.
- If `origin/main` does not exist locally, say the pushed state is unknown.

## 4. Report

Give one table: branch, class, commits not on main, worktree + dirty/detached, warnings. End with
the proposed removals, split into "safe (merged)" and "needs your OK (applied / backup)", plus the
worktrees that would go with them. Ask the user which to remove.

## 5. Tidy (only what was confirmed)

1. Remove a clean worktree first: `git worktree remove <path>` (no `--force`). If it refuses, report
   why and leave it.
2. Delete the branch: `git branch -d <branch>` (merged) or `git branch -D <branch>` (applied, after the
   explicit yes).
3. `git worktree prune`, then re-run `git branch -vv` and `git worktree list` and show the result.

Finish with a short summary: removed, kept (and why), warnings the user still has to act on
(unmerged work, unpushed main).
