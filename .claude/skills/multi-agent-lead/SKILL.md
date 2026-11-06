---
name: multi-agent-lead
description: When a plan has several tasks, analyse whether running them in parallel with other agents (this agent as project lead, merging their work into a branch) would be faster, or whether conflicts and shared resources mean it is better to work through the tasks in order here. Offer the user the choice, with the analysis and a recommendation. Use after the task list is made and before starting work on a plan of two or more tasks.
---

# Multi-agent lead

Parallel agents are faster only when the tasks really are independent. This skill decides, shows its
reasoning, and lets the user choose. It runs once per plan (again if the plan changes a lot).

## 1. Analyse the task list

For each task, note what it will touch and what it needs. Cheap tools first: `Grep`/`Glob` and
`git diff --stat`, no big reads.

- **Files and crates touched.** Two tasks that edit the same file, module or crate public API will
  conflict. Big shared files (CLAUDE.md lists them: `lower/expr.rs`, `tree.rs`, `platypus_app/src/main.rs`,
  `TESTS.md`, `TODO.md`) are conflict magnets; generated files (`types/platypus.d.ts`, `THIRD-PARTY-NOTICES.md`,
  `Cargo.lock`) too.
- **Order dependencies.** Does a task need another's result (a new API, a renamed type, a shared
  spec)? Those chain, they don't parallelise.
- **Shared build resources.** On this repo, builds are heavy and CLAUDE.md warns: never two cargo
  builds at once in the shared checkout, disk is tight (no extra target dirs), and many test exes at
  once filled 32 GB of RAM. A separate worktree normally means its own build output, so each extra
  agent costs a full build and disk. Count this against the speed-up.
- **Size.** Tasks of a few tool calls are not worth an agent; the brief and the merge cost more than
  they save. A task is worth its own agent when it is a self-contained piece of real work.
- **Docs and bookkeeping** (docs refresh, `TODO.md`/`TESTS.md` ticks) belong to the lead, at merge time.

Group the tasks into **independent lanes** (no shared files, no dependency, safe to build apart) and
**chains** (must run in order).

## 2. Recommend

Pick one and say why in a few lines, with the lanes listed:

- **Parallel**: two or more lanes of real work, no overlap, and the machine can take the extra builds.
  Say the expected gain ("about 2 lanes of 3: roughly a third faster") and the cost (extra builds,
  merge work). Cap at the number of lanes and at 3 agents unless the user asks for more.
- **Sequential, here**: tasks overlap, chain, are small, or the build cost would eat the gain. Name
  the specific conflict ("tasks 2 and 4 both rewrite `layout.rs`; running them in parallel means
  rebasing one onto the other"). Say that working in order here is the safer and usually the
  quicker-to-correct choice.
- **Mixed**: parallel lanes plus a chain here.

Never recommend parallel to look busy. If you are unsure, recommend sequential.

## 3. Offer the choice

Ask once (question tool or wrap-up buttons): **work in parallel with me as lead** / **work through
them here in order**, recommended first. If the user already said which, skip the question. Under
`iterate-to-target`, the sequential option is the default if the user doesn't answer and nothing
else can proceed; do not start sessions without a yes.

## 4. If parallel: act as lead

1. **Integration branch.** Name it after the target (`target/<slug>`) from current `main`. Agents
   work on their own branches; the lead merges into the integration branch, never straight into
   `main`.
2. **Starts.** One session per lane, in its own worktree, with a self-contained brief: the target
   in a line, its task(s) and done-criteria, the files/crates it owns and must not touch, the checks to
   run (touched-area only, `-j 4`, one test binary per crate), the CLAUDE.md rules (no heredoc edits,
   no remotes, commit per step with the `Co-Authored-By` line, release builds only), its unique
   update prefix (see `iterate-to-target`), and "commit nothing outside your lane; log decisions in
   `decisions.md` of your branch, don't wait for an answer". Start them with `start_session`.
3. **Stagger builds.** Tell agents that cargo builds in this repo are one at a time per shared
   checkout and heavy; the lead schedules heavy checks (stress GC, full sweep) for itself at the end.
4. **Lead's own work.** Take the chain or the shared-file tasks here, or integrate; don't idle.
5. **Merge as each lane finishes.** Read its note and its diff, don't trust the note alone.
   Bring commits in as **linear commits, no merge commits** (rebase or cherry-pick onto the
   integration branch), resolve conflicts, rebuild and run the touched-area tests after each, then
   tick `TODO.md`/`TESTS.md` and refresh docs once per group.
6. **Conflicts surfacing late.** If two lanes collide after all, stop the later one, rebase it,
   and add the lesson to the plan (those tasks now run in order). Say so in the next update.
7. **Finish.** Run the full sweep once on the integration branch, then use `merge-on-target` to ask
   about moving it onto `main`, then `clean-branches` for the lane branches and worktrees.
   Sessions are archived only after the user has seen that.

Never push, fetch or pull; never merge into `main` without the `merge-on-target` yes.

## 5. If sequential: carry on here

Say in one line which order and why (dependencies first, shared files together), put it in the task
list, and continue under `iterate-to-target`. Revisit the analysis only if the plan grows by
independent tasks.
