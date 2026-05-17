---
name: summarize-changes
description: Summarize what changed in the working tree since the last commit (or N commits back)
argument-hint: "[commits-back, default 1]"
---

Git context (resolved before you see this):

## Status

!`git status --short 2>&1`

## Stats since HEAD~$ARGUMENTS

!`git diff --stat HEAD~${ARGUMENTS:-1} 2>&1 | head -40`

## Recent commits

!`git log --oneline -15 2>&1`

## Diff

!`git diff HEAD~${ARGUMENTS:-1} 2>&1 | head -300`

---

Summarize the changes in 5–10 bullet points. Group by intent (feature / fix / refactor / test / doc). Mention files only when they clarify the bullet. Skip noise (whitespace-only, generated files, lockfile bumps). End with "**Next:**" suggesting the most natural follow-up if any.
