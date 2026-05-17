---
name: explore-repo
description: Quick repo orientation — top-level layout, key files, recent activity
when_to_use: User is new to this project or just switched cwd
---

Repository snapshot (resolved before this message):

## Layout (top 2 levels)

!`(find . -maxdepth 2 -type d -not -path '*/node_modules/*' -not -path '*/.git/*' -not -path '*/target/*' -not -path '*/.svelte-kit/*' -not -path '*/dist/*' 2>/dev/null | head -40)`

## Key files at root

!`ls -la 2>&1 | head -25`

## README excerpt

!`(test -f README.md && head -40 README.md) || echo '(no README.md at repo root)'`

## Languages by file count

!`(find . -type f -not -path '*/.git/*' -not -path '*/node_modules/*' -not -path '*/target/*' -not -path '*/.svelte-kit/*' -not -path '*/dist/*' 2>/dev/null | sed -n 's/.*\.//p' | sort | uniq -c | sort -rn | head -15)`

## Recent commits

!`git log --oneline -10 2>&1 || echo '(not a git repo)'`

---

Give the user a one-paragraph briefing of this repo: what it does, the primary stack, the main entry points / directories worth knowing, and where the action's been recently. Be concrete — cite specific paths. End with 2–3 suggested first questions / tasks for someone diving in.
