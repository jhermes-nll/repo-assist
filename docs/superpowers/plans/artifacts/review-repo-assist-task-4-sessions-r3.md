# Adversarial plan review — round 3 (delta only)

Rules: mark every finding below CLOSED or OPEN; in your reply do the same for new findings. Raise a new MUST ONLY against text changed in this delta; do not reopen a closed finding (F1–F8 are closed and were accepted; F10 was NICE and is fixed anyway below). First line: `SIGNOFF: APPROVE` (zero open MUST) or `SIGNOFF: CHANGES`. Same finding format. Reply with text only.

## Dispositions

**F9 [MUST] — CLOSED: FIXED.** Accepted — dropping the `\n` welded the tail onto `body`'s last complete line (the call_a result), which `rfind('\n')` would then discard, breaking `is_error == Some(true)` and gutting the torn-tail check. Changed text in the plan:

```rust
    // No trailing newline on the LAST line: a real assistant message (with usage input=10)
    // is the torn tail — if it were wrongly parsed, tokens_in below would be 21, not 11,
    // and parse_errors would grow by however it failed to decode. It must be ignored.
    // (The newline AFTER the body matters [review F9]: without it, `body`'s last complete
    // line — the call_a result — is welded onto the tail and lost.)
    std::fs::write(&f, format!("{body}\n{}", assistant_ok("2026-10-19T00:00:05.000Z"))).unwrap();
```
The intervening newline restores the result line to the complete region, so the assertions (`parse_errors == 3`, `tokens_in == 11`, `tokens_out == 6`, one call, `is_error == Some(true)`, `result_hash` present, cost 0.02) hold, and only the final assistant line is torn.

**F10 [NICE] — CLOSED: FIXED.** Accepted — `tool_calls.id` is `INTEGER PRIMARY KEY` without AUTOINCREMENT [E23], so on a single-session table a delete+reinsert reissues identical rowids and my rowid-equality proof was vacuous. Replaced the `call_ids` helper and its block entirely (helper deleted from the plan) with a marker-row proof, which distinguishes skip from re-parse WITHOUT relying on rowid allocation:

```rust
    // Unchanged -> skip. Proof the skip cursor fired [review F8 + F10]: a manually inserted
    // marker tool_calls row for this session can ONLY survive if the sync did no store write
    // at all — a re-parse would DELETE+reinsert and wipe it. (`tool_calls.id` has no
    // AUTOINCREMENT [E23], so raw rowid equality cannot prove the skip; the marker can.)
    store
        .write(|conn| {
            conn.execute(
                "INSERT INTO tool_calls(session_id, ts_ms, tool, args_hash, is_error, result_hash) \
                 VALUES ('01a1249c-ec21-7703-8773-813f926a6357', 0, 'marker', 'marker', NULL, NULL)",
                [],
            )
        })
        .unwrap();
    repo_assist::sources::sessions::sync_sessions(&store, repo.path(), &root);
    assert_eq!(read_agg(&store, "01a1249c-ec21-7703-8773-813f926a6357", &size), (4999, 2));
    let marker: i64 = store
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM tool_calls WHERE session_id = '01a1249c-ec21-7703-8773-813f926a6357' AND tool = 'marker'",
                [],
                |r| r.get(0),
            )
        })
        .unwrap();
    assert_eq!(marker, 1, "the byte-offset skip cursor prevented a re-parse (marker survived)");
```
Notes kept consistent by construction: the marker is inserted after the first sync (the session row exists, so the `tool_calls.session_id REFERENCES sessions(id)` FK holds [E23, FK pragma ON]); the later append/shrink steps in the same test DO re-parse and so legitimately wipe the marker, which is fine because no later step asserts on it; `read_agg` reads the `sessions.tool_calls` COLUMN (parse-derived), so the marker row does not perturb the (4999, 2) assertion.

Round-1/2 state: F1–F8 CLOSED (accepted). F9 CLOSED (FIXED). F10 CLOSED (FIXED). No open findings remain from any round.
