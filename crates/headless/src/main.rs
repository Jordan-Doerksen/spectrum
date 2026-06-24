//! Spectrum runner — the always-on engine, now a thin loop over the `Engine` service
//! (the same service the Tauri panel drives).
//!
//! Flags: `--once` (one cycle then exit) · `--dry` (never post) · `--feedcheck`
//! (fetch every feed, report ok/err, exit).

use std::time::{Duration, Instant};

use spectrum_engine::{engine::Engine, feeds, rss, ua_client};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let dry = std::env::args().any(|a| a == "--dry");
    let once = std::env::args().any(|a| a == "--once");

    // `--feedcheck`: fetch every feed once, report ok/err + counts, exit. No config needed.
    if std::env::args().any(|a| a == "--feedcheck") {
        let client = ua_client();
        let (mut ok, mut total) = (0usize, 0usize);
        for feed in feeds::feeds() {
            match rss::fetch(&client, &feed).await {
                Ok(list) => {
                    ok += 1;
                    total += list.len();
                    println!("[ok ] {:<16} {:>3}  ({:?})", feed.source, list.len(), feed.hint);
                }
                Err(e) => println!("[ERR] {:<16} {e}", feed.source),
            }
        }
        println!("\n{ok} feeds ok · {total} items pulled");
        return Ok(());
    }

    let cfg_path = std::env::var("SPECTRUM_CONFIG").unwrap_or_else(|_| "config.local.json".into());
    let mut engine = Engine::new(&cfg_path).map_err(|e| anyhow::anyhow!("config '{cfg_path}': {e}"))?;

    let st = engine.status();
    println!(
        "Spectrum engine{}{} · poll {}m · drip {}s · floor {} · {} feeds · {} seen",
        if dry { " [DRY]" } else { "" },
        if once { " [ONCE]" } else { "" },
        st.poll_minutes,
        st.drip_seconds,
        st.min_severity,
        st.feeds,
        st.seen
    );

    if engine.seen_empty() {
        let n = engine.seed().await;
        println!("first run: seeded {n} headlines (posted nothing); watching for new.");
        if once {
            return Ok(());
        }
    }

    let mut last_poll = Instant::now();
    let mut first = true;
    loop {
        if first || last_poll.elapsed().as_secs() >= engine.cfg.poll_minutes * 60 {
            engine.reload_config();
            let n = engine.poll().await;
            last_poll = Instant::now();
            first = false;
            let st = engine.status();
            println!("[poll] {n} new · {} queued · {} seen", st.queued, st.seen);
        }

        for line in engine.drip(dry).await {
            println!("  {line}");
        }

        if once {
            println!("[once] done · {} pending", engine.status().queued);
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(engine.cfg.drip_seconds)).await;
    }
}
