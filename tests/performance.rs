mod common;

use repo_assist::paths::{git_common_dir, repo_key_from_git_common_dir, Env};

use assert_cmd::Command as AssertCommand;
use std::process::Command as StdCommand;
use std::time::{Duration, Instant};

#[test]
#[ignore]
fn performance_idle_cpu_rss_and_status_p95() {
    let mut repo = common::GitRepo::new();
    for i in 1..=5 {
        repo.add_worktree(&format!("wt{i}"), "dirty.txt");
    }

    let state = tempfile::TempDir::new().unwrap();
    let runtime = tempfile::TempDir::new().unwrap();
    let env = Env::with(
        "/tmp",
        Some(state.path().display().to_string()),
        Some(runtime.path().display().to_string()),
    );
    let key = git_common_dir(repo.path())
        .map(|c| repo_key_from_git_common_dir(&c))
        .unwrap_or_else(|| "manual".to_string());
    let sf = env.state_files(&key);

    let mut cmd = AssertCommand::cargo_bin("ra").unwrap();
    cmd.env("HOME", "/tmp")
        .env("XDG_STATE_HOME", state.path())
        .env("XDG_RUNTIME_DIR", runtime.path())
        .current_dir(repo.path())
        .arg("status");
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "initial status failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    for _ in 0..100 {
        if env.socket_path(&key).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        env.socket_path(&key).exists(),
        "daemon socket did not appear"
    );

    let pid: u32 = std::fs::read_to_string(&sf.pid)
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    // Sample idle RSS/CPU for ~10s.
    let samples = 10;
    let mut cpu_sum = 0.0f64;
    let mut rss_sum = 0.0f64;
    for _ in 0..samples {
        let ps = StdCommand::new("ps")
            .args(["-p", &pid.to_string(), "-o", "%cpu=,rss="])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&ps.stdout);
        let mut parts = text.split_whitespace();
        let cpu: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
        let rss_kb: f64 = parts.next().unwrap_or("0").parse().unwrap_or(0.0);
        cpu_sum += cpu;
        rss_sum += rss_kb / 1024.0;
        std::thread::sleep(Duration::from_secs(1));
    }
    let cpu_avg = cpu_sum / samples as f64;
    let rss_avg = rss_sum / samples as f64;

    // Warm status x20.
    let mut durations = Vec::new();
    for _ in 0..20 {
        let start = Instant::now();
        let mut c = AssertCommand::cargo_bin("ra").unwrap();
        c.env("HOME", "/tmp")
            .env("XDG_STATE_HOME", state.path())
            .env("XDG_RUNTIME_DIR", runtime.path())
            .current_dir(repo.path())
            .arg("status");
        let o = c.output().unwrap();
        assert!(
            o.status.success(),
            "status failed: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        durations.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    durations.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((durations.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(durations.len() - 1);
    let p95 = durations[idx];

    // Exercise retention caps.
    let store = repo_assist::store::Store::open(&sf.db).unwrap();
    let cap = repo_assist::store::EVENT_CAP;
    for i in 0..cap + 10 {
        store
            .append_event(1_000_000 + i as i64, "perf", "cap", None, "{}")
            .unwrap();
    }
    let count: i64 = store
        .read(|conn| conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)))
        .unwrap();
    assert!(count <= cap as i64, "events not capped: {count}");

    let _ = StdCommand::new("kill").arg(pid.to_string()).status();

    eprintln!("PERF idle_cpu_pct={cpu_avg:.3} rss_mb={rss_avg:.2} status_p95_ms={p95:.2}");

    assert!(cpu_avg < 1.0, "idle cpu {cpu_avg} >= 1.0");
    assert!(rss_avg < 50.0, "idle rss {rss_avg} >= 50.0");
    assert!(p95 < 100.0, "status p95 {p95} >= 100.0");
}
