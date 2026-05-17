---
name: review-pr
description: Review a GitHub PR — pulls diff + metadata + checks via gh CLI, then asks you to assess
when_to_use: User pastes a PR url or says "review pr #N"
argument-hint: "<pr-number-or-url>"
---

You are reviewing GitHub PR `$ARGUMENTS`. The diff, metadata, and CI status below were resolved before you saw this message — use them as authoritative; don't re-fetch unless you need something specific.

## Title + body

!`gh pr view $ARGUMENTS --json title,body,author,headRefName,baseRefName,mergeable,isDraft 2>&1 | head -100`

## Changed files

!`gh pr view $ARGUMENTS --json files -q '.files[] | "\(.additions)+\(.deletions)- \(.path)"' 2>&1 | head -40`

## CI status

!`gh pr checks $ARGUMENTS 2>&1 | head -30`

## Diff

!`gh pr diff $ARGUMENTS 2>&1 | head -400`

---

Review the PR using this checklist:

1. **Correctness** — does the change actually do what the title claims? Spot logic bugs, off-by-ones, wrong nullability handling.
2. **Architecture** — does it fit the surrounding code, or shoehorn? Flag new dependencies, new abstractions that don't match the rest of the file.
3. **Tests** — coverage proportional to risk. Pure functions: minimal. Network / IO / migrations: substantial.
4. **Security** — input validation, secrets in code, shell injection, SQL injection.
5. **Performance** — anything obviously O(n²) or N+1 query? Comment if so.
6. **Style** — last priority. Don't nitpick; flag only patterns that diverge from the rest of the codebase.

Output:
- Top-line verdict: **approve** / **request changes** / **needs discussion**
- 3–5 prioritized comments (line + what to change + why)
- Skip "nit:" comments unless the PR is otherwise clean
