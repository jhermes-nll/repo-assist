# Agent integration snippet

Copy the block below into your `AGENTS.md` / `CLAUDE.md`. The daemon **never** installs this file itself — keep ownership with the repo.

```markdown
## Repo coordination (repo-assist)

- Status: run `ra status` (check `generated_at` and daemon version; read-only, may be stale).
- Before editing shared files, run `ra who-touches <path>`.
- Before merging, run `ra conflicts`.
- Views under the state dir (`views/STATUS.md`, `views/PLANS.md`, `views/FINDINGS.md`) are generated read-only and may be stale; prefer live `ra` commands.

Repo coordination status: run `ra status`, `ra who-touches <path>` before editing shared files, `ra conflicts` before merging. Read-only. Check `generated_at`.
```

Caveats: generated views/daemon output is read-only and may be stale. The daemon never installs this snippet.
