---
name: iterate-to-target
description: Default working mode for every conversation. Keep iterating (plan, do, check, next task) until the conversation's plan or target is met, without stopping to ask between steps, unless the user says to stop or pause. Also governs keeping token use low between tasks (handoffs; it never asks the user to compact).
---

# Iterate to target

This is the default mode unless the user says stop, pause, "just answer" or "ask me first".

A copy of this skill lives in web-ui's `.claude/skills/` (`..\web-ui`); change both together.

## 1. Fix the target

At the start (or when it changes), state the target in a line or two and break it into tasks on the
task list (`update_plan`). A target is the user's request, or the agreed plan.

**Prefer showing a TODO list.** Keep the task list current the whole time so the user can see progress:
one task `in_progress`, finished ones advanced as you go, new tasks added when you discover them,
tasks waiting on a decision marked as such in their wording ("Blocked: pick X"). Don't rely on prose
updates for progress.

**Don't stop for questions you can answer with common practice.** If the request is unclear in a way
that changes what you build, ask once up front; after that, choose the recommended, conventional
option (what the surrounding code, CLAUDE.md and the usual practice for the language or library
suggest) and carry on.

## 2. Loop

For each task: do it, run the per-step checks CLAUDE.md names for the area touched, commit per its
working agreements, tick `TODO.md` / `TESTS.md`, then **start the next task straight away**. Do not
end the turn to report between tasks or ask "shall I continue?".

### Decisions while iterating

When a choice comes up, don't stop; sort it:
- **Minor / easily reversed** (naming, small layout, which of two equivalent APIs): take the
  recommended common-practice option and add a one-line note of the action taken to the commit message
  or the task's completion note.
- **Important** (architecture, public API or file format, dependency, behaviour users will notice,
  anything costly to undo): still take the recommended option provisionally if work can safely
  continue on it, and record it in `decisions.md` at the repo root (git-ignored, so never committed; create it if missing; newest
  first, appended, not rewritten). Each entry: date, task, the question, options considered, what
  was chosen provisionally and why, and what changes if the user picks differently. Then continue
  with that task or move to the next one.
- **Cannot continue without an answer** (the work would be wasted or harmful on a wrong guess, or
  the action is the user's by rule: remotes, deleting, downloads, outward-facing actions): record it
  in `decisions.md` as "Blocked", mark the task blocked on the list, and move to a task that doesn't
  depend on it.

### When to stop and hand back

Stop looping when:
- the user told you to stop or pause;
- you have done everything possible without a decision (every remaining task is blocked or waiting
  on one);
- you are blocked after a real attempt to fix the cause (same failure twice: change approach, then
  report);
- the target is met.

When you stop for decisions, **offer them as prompts**: read `decisions.md`, then list each open
decision as a short question with your recommended answer first (use the question tool or the
wrap-up buttons, up to four at once, the rest in the next round). Say which tasks each answer
unblocks and what was already done provisionally, so a "yes, as recommended" is enough to confirm
the lot. After the answers, update `decisions.md` (mark entries Decided, with the answer), fix any
provisional work that went the other way, and resume the loop.

When the target is met: run the full sweep CLAUDE.md describes, then `merge-on-target`, then
`clean-branches`, and only then wrap up.

## Progress updates while iterating

Send the user a short update at least:
- **every 10 minutes** when in a multi-agent session (you started sessions or subagents, or others
  are running beside you), or after **10 minutes without any message** to the user;
- **every 15 minutes while still on the same task**, even if you messaged recently, so a stuck or
  slow task is visible.

Whichever comes first. Note the time with `date` when you start and after each update (you have no
clock otherwise), and check it between tool calls; a long-running command is no excuse, so run it in
the background and report while it runs.

**Format:** one or two lines, starting with a unique prefix that names you, then the target, the
current task and why:

`[<unique-prefix>] target: Renaming all UI models, current task: fixing problematic rename when editing a property, then retesting`

- **Prefix:** unique to this agent so several can be told apart: the session's short name, or a
  role plus a short tag (`[ui-rename]`, `[docs-b2]`). Use the same prefix for every update from this
  agent for the whole conversation. Subagents and sessions you start get their own, passed in their
  brief.
- **Target:** the conversation's target in a few words.
- **Current task + reasoning:** what you are doing right now and the reason (the failure being
  fixed, what the retest will show, what is blocked), not just the task title.
- Add a short count of tasks done out of total, and any new entry in `decisions.md`, when they
  exist. No filler, no repeating earlier updates.

## 3. Handoffs to keep tokens low

You cannot run `/compact` yourself, so **never ask the user to compact, suggest it, or stop for it**
(the user decided this, 2026-11-07). The app compacts on its own when context runs out, and a
compaction drops detail, so leave a handoff at the right moments and keep working.

**Write a handoff after a task when either holds:**
- **The next task is unrelated.** Example: "add a button and style it" is followed by "add a tab
  control". The button's code and discussion won't help the tab control.
- **Context is about 60-90% used and the task just finished.** Use the real figure if the session
  shows one. If you have no figure, estimate: long transcripts, many large file reads or build/test
  logs, or several finished tasks mean you are probably in that range.

Not mid-task, and not inside a related group (a shared spec several tasks follow, a feature split
into steps that rely on earlier decisions, a half-finished migration): write it once the group is
done.

**The handoff** goes in `TODO.md` "Resume point" (about 30 lines, replaced, not appended) and holds
only the target, what is done and committed, what is next, decisions that aren't in the code, and
the files or commands the next task needs. History stays in git. Then start the next task at once;
before an unrelated one, read only what it needs rather than leaning on earlier context.

## 4. Keep it cheap

- Read big files in parts (CLAUDE.md lists them); use Grep before reading.
- Two levels of checks: touched-area checks per step, the full sweep once at the end.
- Send independent tool calls together; delegate wide searches to a subagent so their output stays
  out of this context.
