self-reviewed: PI_MODEL=claude-sonnet-5-5 is already sonnet/opus; external review skipped

Check run: `case "$PI_MODEL" in *sonnet*|*opus*) echo SELF;; *) echo EXTERNAL;; esac` -> SELF.
No `claude` command was run; zero open MUST findings by construction (self-satisfied, NOT an external adversarial pass).

Self-review (writing-plans checklist) findings, all fixed inline before saving:
- F1 (fixed): parse_plan warns on every zero-task parse -> SpecAdapter uses the silent parsers (E37).
- F2 (fixed): claim-set exclusion would leak artifacts/ -> path-prefix exclusion (E9).
- F3 (fixed): config keys are inert at runtime (E19) -> surfaced as Decision D1, README wording made honest.
- F4 (fixed): `{:<6}` column misaligns "unknown" -> T3.
Carried, not resolved (need caller decision): D1 (config wiring), D2 (specs feed stalled/overlap/abandoned checks), D3 (docs/plans/ double-index), D4 (parser choice).
Unverified: A1-A3 (nothing was executed during planning).
