---
name: iterate-to-target
description: Default working mode for every conversation. Keep iterating (plan, do, check, next task) until the conversation's plan or target is met, without stopping to ask between steps, unless the user says to stop or pause. Also governs when to compact context between tasks to keep token use low.
---

# Iterate to target

This is the default mode unless the user says stop, pause, "just answer" or "ask me first".

## 1. Fix the target

At the start (or when it changes), state the target in a line or two and break it into tasks on the
task list (`update_plan`). A target is the user's request, or the agreed plan. If it is unclear in a way
that changes what you build, ask once; otherwise pick the sensible default and say so.

## 2. Loop

For each task: do it, run the per-step checks CLAUDE.md names for the area touched, commit per its
working agreements, tick `TODO.md` / `TESTS.md`, then **start the next task straight away**. Do not
end the turn to report between tasks or ask "shall I continue?".

Stop looping and ask only when:
- the user told you to stop or pause;
- a decision is genuinely theirs (CLAUDE.md and the harness list the cases: remotes, deleting,
  downloads, outward-facing actions, ambiguous requirements);
- you are blocked after a real attempt to fix the cause (same failure twice: change approach, then
  report);
- the target is met.

When the target is met: run the full sweep CLAUDE.md describes, then `merge-on-target`, then
`clean-branches`, and only then wrap up.

## 3. Compaction to keep tokens low

Compaction drops detail, so choose the moment. A task finishing is the natural point.

**Compact after a task when either holds:**
- **The next task is unrelated.** Example: "add a button and style it" is followed by "add a tab
  control". The button's code and discussion won't help the tab control.
- **Context is about 60-90% used and the task just finished.** Use the real figure if the session
  shows one. If you have no figure, estimate: long transcripts, many large file reads or build/test
  logs, or several finished tasks mean you are probably in that range. Don't guess below 60%; do
  not compact mid-task.

**Do not compact (carry the context) when it is related and needed for the target.** Examples: a
shared spec several tasks follow (the button spec for every button), a feature or refactor split
into steps that each rely on the earlier decisions, or a half-finished migration. Compact once the
whole related group is done instead.

**How:** you cannot run `/compact` yourself. At the compaction point, finish with a short handoff and
tell the user "good point to compact" (name why: unrelated next task, or context at roughly N%). The
handoff, saved in the recap or `TODO.md` "Resume point" (kept to about 30 lines, replaced not
appended), holds only: the target, what is done and committed, what is next, decisions that
aren't in the code, files or commands the next task needs. History stays in git. If the app
compacts automatically, write that handoff before it happens so the summary keeps it.

If the user declines to compact, carry on in the same mode.

## 4. Keep it cheap

- Read big files in parts (CLAUDE.md lists them); use Grep before reading.
- Two levels of checks: touched-area checks per step, the full sweep once at the end.
- Send independent tool calls together; delegate wide searches to a subagent so their output stays
  out of this context.
