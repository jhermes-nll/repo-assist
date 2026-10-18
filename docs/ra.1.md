---
name: ra
about: repo-assist binary
tags: [cli, daemon]
---
# ra — repo-assist

Advisory git repository coordination daemon.

```
ra daemon [--foreground]
ra status [--json]
ra worktrees [--json]
ra plans [--json]
ra plan <id> [--json]
ra drift [--json]
ra conflicts [--json]
ra who-touches <path> [--json]
ra findings [--open|--all] [--kind K] [--json]
ra ack <id>
ra sessions [--json]
ra tui
ra doctor
ra render
```

Read-only: the daemon never writes inside any work tree.
