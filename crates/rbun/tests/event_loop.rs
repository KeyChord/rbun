//! How the async driver shares a thread between Bun's event loop and the host
//! executor (tokio), for code that waits on both.

mod common;

use common::block_on;
use rbun::{AsyncContext, AsyncRuntime, Function, async_with};
use std::time::{Duration, Instant};

/// While Bun's event loop is alive, the driver keeps polling the host future
/// inside one poll of the tokio task. Tokio allows a task a fixed number of
/// channel and socket operations per poll, so without an exemption the host
/// future stopped receiving after that many, until the loop died down.
#[test]
fn a_host_channel_keeps_working_while_bun_holds_the_thread() {
    let (elapsed, received) = block_on(|| async {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u32>();
        const MESSAGES: u32 = 1_000;
        std::thread::spawn(move || {
            for message in 0..MESSAGES {
                if tx.send(message).is_err() {
                    return;
                }
            }
        });
        async_with!(ctx => |ctx| {
            // Alive for 3 s, then free to end: a starved receiver resumes only
            // after that, so a hang shows up as a slow test, not a stuck one.
            // Tests share one thread and VM, so the timers are cleared at the end.
            let release: Function = ctx
                .eval("(() => { const timer = setInterval(() => {}, 1000); const limit = setTimeout(() => clearInterval(timer), 3000); return () => { clearInterval(timer); clearTimeout(limit); }; })()")
                .unwrap();
            let started = Instant::now();
            let mut received = 0;
            while received < MESSAGES {
                if rx.recv().await.is_none() {
                    break;
                }
                received += 1;
            }
            let elapsed = started.elapsed();
            release.call::<_, ()>(()).unwrap();
            (elapsed, received)
        })
        .await
    });
    assert_eq!(received, 1_000);
    assert!(
        elapsed < Duration::from_secs(2),
        "the host channel stalled while Bun held the thread: {elapsed:?}"
    );
}

/// A timer that does not hold the loop open (`unref`) still runs while the host
/// future waits on something else, as long as a keep-alive guard is held. Without
/// one, the driver gives the thread to tokio and the timer waits for the next poll.
#[test]
fn a_keep_alive_guard_runs_bun_timers_while_the_host_waits() {
    let (with_guard, without_guard) = block_on(|| async {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        let mut ticks = Vec::new();
        for keep_alive in [true, false] {
            let count = async_with!(ctx => |ctx| {
                let guard = keep_alive.then(|| ctx.keep_event_loop_alive().unwrap());
                let read: Function = ctx
                    .eval("(() => { let count = 0; const timer = setInterval(() => { count++; }, 10); timer.unref(); return () => { clearInterval(timer); return count; }; })()")
                    .unwrap();
                let (tx, rx) = tokio::sync::oneshot::channel::<()>();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(300));
                    let _ = tx.send(());
                });
                rx.await.unwrap();
                drop(guard);
                read.call::<_, u32>(()).unwrap()
            })
            .await;
            ticks.push(count);
        }
        (ticks[0], ticks[1])
    });
    assert!(
        with_guard >= 10,
        "an unref'd 10 ms timer ran {with_guard} times in 300 ms with the loop kept alive"
    );
    assert!(
        without_guard < with_guard / 2,
        "without a guard the timer is expected to wait for the next poll ({without_guard} vs {with_guard})"
    );
}

/// Dropping the guard releases the loop, so an idle runtime can finish.
#[test]
fn dropping_the_keep_alive_guard_lets_the_loop_go_idle() {
    let finished = block_on(|| async {
        let rt = AsyncRuntime::new().unwrap();
        let ctx = AsyncContext::full(&rt).await.unwrap();
        ctx.with(|ctx| drop(ctx.keep_event_loop_alive().unwrap())).await;
        tokio::time::timeout(Duration::from_secs(2), rt.idle()).await.is_ok()
    });
    assert!(finished, "the event loop stayed alive after the guard was dropped");
}
