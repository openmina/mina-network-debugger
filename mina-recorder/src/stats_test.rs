use std::{
    time::{SystemTime, Duration},
    net::SocketAddr,
    thread::scope,
};

use temp_dir::TempDir;

use crate::{database::DbFacade, stats::update_block_stats};

use super::stats::StatsState;

const FILES: [&[u8]; 8] = [
    include_bytes!("test_data/_0.bin").as_slice(),
    include_bytes!("test_data/_1.bin").as_slice(),
    include_bytes!("test_data/_2.bin").as_slice(),
    include_bytes!("test_data/_3.bin").as_slice(),
    include_bytes!("test_data/_4.bin").as_slice(),
    include_bytes!("test_data/_5.bin").as_slice(),
    include_bytes!("test_data/_6.bin").as_slice(),
    include_bytes!("test_data/_7.bin").as_slice(),
];

fn peer(port: u16) -> SocketAddr {
    ([1, 1, 1, 1], port).into()
}

fn generic<F>(f: F)
where
    F: Fn(SystemTime, &DbFacade, &mut StatsState),
{
    let d = TempDir::new().expect("cannot create temporary directory");
    let path = d.path();
    let db = DbFacade::open(&path).unwrap();
    let mut state = StatsState::default();

    f(SystemTime::now(), &db, &mut state);
}

#[test]
fn check_latency_simple() {
    generic(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);

        state.observe_w(FILES[0], true, time(1), &db, peer(100));
        state.observe_w(FILES[0], false, time(2), &db, peer(100));

        let (_, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 2);
        assert_eq!(stat.events[0].latency, None);
        assert_eq!(stat.events[1].latency, Some(Duration::from_secs(1)));
    })
}

#[test]
fn check_latency_outgoing_simple() {
    generic(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);

        state.observe_w(FILES[0], false, time(1), &db, peer(100));
        state.observe_w(FILES[0], false, time(2), &db, peer(100));

        let (_, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 2);
        assert_eq!(stat.events[0].latency, None);
        assert_eq!(stat.events[1].latency, Some(Duration::from_secs(1)));
    })
}

#[test]
fn check_mixed_hashes() {
    generic(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);

        state.observe_w(FILES[2], true, time(0), &db, peer(1000));
        state.observe_w(FILES[2], false, time(1), &db, peer(1001));
        state.observe_w(FILES[3], true, time(2), &db, peer(1002));
        state.observe_w(FILES[2], false, time(3), &db, peer(1003));
        state.observe_w(FILES[3], false, time(4), &db, peer(1004));

        let (_, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 5);
        assert_eq!(stat.events[0].latency, None);
        assert_eq!(stat.events[1].latency, Some(Duration::from_secs(1)));
        assert_eq!(stat.events[2].latency, None);
        assert_eq!(stat.events[3].latency, Some(Duration::from_secs(3)));
        assert_eq!(stat.events[4].latency, Some(Duration::from_secs(2)));
    })
}

#[test]
fn check_multiple() {
    generic(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);

        for i in 0..7 {
            state.observe_w(FILES[i], true, time(i as _), &db, peer(1000 + i as u16));
        }
        for i in 0..100 {
            state.observe_w(
                FILES[7],
                i % 7 > 3,
                time(100 + i as u64),
                &db,
                peer(2000 + i as u16),
            );
        }

        let (_, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 100);
        assert_eq!(stat.events[0].latency, None);
        for i in 1..100 {
            assert_eq!(stat.events[i].latency, Some(Duration::from_secs(i as u64)));
        }
    })
}

#[test]
fn check_mixed_hashes_outgoing() {
    generic(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);

        state.observe_w(FILES[2], false, time(0), &db, peer(1000));
        state.observe_w(FILES[2], false, time(1), &db, peer(1001));
        state.observe_w(FILES[3], false, time(2), &db, peer(1002));
        state.observe_w(FILES[2], false, time(3), &db, peer(1003));
        state.observe_w(FILES[3], false, time(4), &db, peer(1004));

        let (_, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 5);
        assert_eq!(stat.events[0].latency, None);
        assert_eq!(stat.events[1].latency, Some(Duration::from_secs(1)));
        assert_eq!(stat.events[2].latency, None);
        assert_eq!(stat.events[3].latency, Some(Duration::from_secs(3)));
        assert_eq!(stat.events[4].latency, Some(Duration::from_secs(2)));
    })
}

#[test]
fn check_cleanup() {
    generic(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);

        state.observe_w(FILES[2], true, time(0), &db, peer(1000));
        state.observe_w(FILES[2], false, time(1), &db, peer(1001));
        state.observe_w(FILES[3], true, time(2), &db, peer(1002));
        state.observe_w(FILES[2], false, time(3), &db, peer(1003));
        state.observe_w(FILES[3], false, time(4), &db, peer(1004));
        state.observe_w(FILES[4], true, time(10), &db, peer(1010));

        let (id, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 1);
        assert_eq!(stat.events[0].latency, None);

        let (_, stat) = db.core().fetch_stats(id.height - 1).unwrap();
        assert_eq!(stat.events.len(), 5);
        assert_eq!(stat.events[0].latency, None);
        assert_eq!(stat.events[1].latency, Some(Duration::from_secs(1)));
        assert_eq!(stat.events[2].latency, None);
        assert_eq!(stat.events[3].latency, Some(Duration::from_secs(3)));
        assert_eq!(stat.events[4].latency, Some(Duration::from_secs(2)));
    })
}

#[test]
fn check_cleanup_obsolete() {
    generic(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);

        state.observe_w(FILES[2], true, time(0), &db, peer(1000));
        state.observe_w(FILES[2], false, time(1), &db, peer(1001));
        state.observe_w(FILES[3], true, time(2), &db, peer(1002));
        state.observe_w(FILES[2], false, time(3), &db, peer(1003));
        state.observe_w(FILES[3], false, time(4), &db, peer(1004));
        state.observe_w(FILES[4], true, time(10), &db, peer(1010));
        // obsolete events come after cleanup
        state.observe_w(FILES[3], true, time(11), &db, peer(1011));
        state.observe_w(FILES[3], true, time(12), &db, peer(1012));
        state.observe_w(FILES[3], true, time(13), &db, peer(1013));
        state.observe_w(FILES[3], true, time(14), &db, peer(1014));

        let (_, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 1);
        assert_eq!(stat.events[0].latency, None);
    })
}

#[test]
fn check_block_v2_latest() {
    generic(|now, db, _state| {
        update_block_stats(FILES[0], true, now, now, peer(1), peer(2), db).unwrap();
        update_block_stats(FILES[1], true, now, now, peer(1), peer(2), db).unwrap();
        update_block_stats(FILES[0], true, now, now, peer(1), peer(2), db).unwrap();

        let (height, events) = db.core().fetch_last_stat_block_v2().unwrap();
        assert_eq!(height, 638);
        assert_eq!(events.len(), 1);
    })
}

#[test]
fn check_order_block_v2() {
    generic(|now, db, _state| {
        for (_, d) in [0_i8, -1, 2, -3].into_iter().enumerate() {
            let t = if d < 0 {
                now - Duration::from_nanos((-d) as u64)
            } else {
                now + Duration::from_nanos(d as u64)
            };

            update_block_stats(FILES[0], true, t, t, peer(1), peer(2), db).unwrap();
        }

        let (_, events) = db.core().fetch_last_stat_block_v2().unwrap();

        events
            .into_iter()
            .scan(None, |prev_time, event| {
                if let Some(prev_time) = prev_time.replace(event.better_time) {
                    assert!(prev_time <= event.better_time);
                }
                Some(())
            })
            .for_each(|_| {});
    })
}

#[test]
fn check_mt_order_block_v2() {
    generic(|now, db, _state| {
        scope(|s| {
            for (_, d) in [0_i8, -1, 2, -3].into_iter().enumerate() {
                let t = if d < 0 {
                    now - Duration::from_nanos((-d) as u64)
                } else {
                    now + Duration::from_nanos(d as u64)
                };

                s.spawn(move || {
                    update_block_stats(FILES[0], true, t, t, peer(1), peer(2), db).unwrap();
                });
            }
        });

        let (_, events) = db.core().fetch_last_stat_block_v2().unwrap();

        events
            .into_iter()
            .scan(None, |prev_time, event| {
                if let Some(prev_time) = prev_time.replace(event.better_time) {
                    assert!(prev_time <= event.better_time);
                }
                Some(())
            })
            .for_each(|_| {});
    })
}
