use std::{
    time::{SystemTime, Duration},
    thread, fs,
    net::SocketAddr,
    path::PathBuf,
};

use crate::database::DbFacade;

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

fn generic<const ID: usize, F>(f: F)
where
    F: Fn(SystemTime, &DbFacade, &mut StatsState),
{
    let path = PathBuf::from("/tmp/stats_test_db").join(ID.to_string());
    if path.exists() {
        fs::remove_dir_all(&path).unwrap();
    }
    let db = DbFacade::open(&path).unwrap();
    let mut state = StatsState::default();

    f(SystemTime::now(), &db, &mut state);
    fs::remove_dir_all(path).unwrap();
    thread::sleep(Duration::from_secs(1));
}

#[test]
fn check_latency_simple() {
    generic::<0, _>(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);
        let addr = "0.0.0.0:0".parse().unwrap();

        state.observe(0, FILES[0], true, time(1), &db, peer(100), &None, addr);
        state.observe(0, FILES[0], false, time(2), &db, peer(100), &None, addr);

        let (_, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 2);
        assert_eq!(stat.events[0].latency, None);
        assert_eq!(stat.events[1].latency, Some(Duration::from_secs(1)));
    })
}

#[test]
fn check_latency_outgoing_simple() {
    generic::<1, _>(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);
        let addr = "0.0.0.0:0".parse().unwrap();

        state.observe(0, FILES[0], false, time(1), &db, peer(100), &None, addr);
        state.observe(0, FILES[0], false, time(2), &db, peer(100), &None, addr);

        let (_, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 2);
        assert_eq!(stat.events[0].latency, None);
        assert_eq!(stat.events[1].latency, Some(Duration::from_secs(1)));
    })
}

#[test]
fn check_mixed_hashes() {
    generic::<2, _>(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);
        let addr = "0.0.0.0:0".parse().unwrap();

        state.observe(0, FILES[2], true, time(0), &db, peer(1000), &None, addr);
        state.observe(0, FILES[2], false, time(1), &db, peer(1001), &None, addr);
        state.observe(0, FILES[3], true, time(2), &db, peer(1002), &None, addr);
        state.observe(0, FILES[2], false, time(3), &db, peer(1003), &None, addr);
        state.observe(0, FILES[3], false, time(4), &db, peer(1004), &None, addr);

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
    generic::<3, _>(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);
        let addr = "0.0.0.0:0".parse().unwrap();

        for i in 0..7 {
            state.observe(
                0,
                FILES[i],
                true,
                time(i as _),
                &db,
                peer(1000 + i as u16),
                &None,
                addr,
            );
        }
        for i in 0..100 {
            state.observe(
                0,
                FILES[7],
                i % 7 > 3,
                time(100 + i as u64),
                &db,
                peer(2000 + i as u16),
                &None,
                addr,
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
    generic::<4, _>(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);
        let addr = "0.0.0.0:0".parse().unwrap();

        state.observe(0, FILES[2], false, time(0), &db, peer(1000), &None, addr);
        state.observe(0, FILES[2], false, time(1), &db, peer(1001), &None, addr);
        state.observe(0, FILES[3], false, time(2), &db, peer(1002), &None, addr);
        state.observe(0, FILES[2], false, time(3), &db, peer(1003), &None, addr);
        state.observe(0, FILES[3], false, time(4), &db, peer(1004), &None, addr);

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
    generic::<5, _>(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);
        let addr = "0.0.0.0:0".parse().unwrap();

        state.observe(0, FILES[2], true, time(0), &db, peer(1000), &None, addr);
        state.observe(0, FILES[2], false, time(1), &db, peer(1001), &None, addr);
        state.observe(0, FILES[3], true, time(2), &db, peer(1002), &None, addr);
        state.observe(0, FILES[2], false, time(3), &db, peer(1003), &None, addr);
        state.observe(0, FILES[3], false, time(4), &db, peer(1004), &None, addr);
        state.observe(0, FILES[4], true, time(10), &db, peer(1010), &None, addr);

        let (id, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 1);
        assert_eq!(stat.events[0].latency, None);

        let (_, stat) = db.core().fetch_stats(id - 1).unwrap().unwrap();
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
    generic::<6, _>(|now, db, state| {
        let time = |d| now + Duration::from_secs(d);
        let addr = "0.0.0.0:0".parse().unwrap();

        state.observe(0, FILES[2], true, time(0), &db, peer(1000), &None, addr);
        state.observe(0, FILES[2], false, time(1), &db, peer(1001), &None, addr);
        state.observe(0, FILES[3], true, time(2), &db, peer(1002), &None, addr);
        state.observe(0, FILES[2], false, time(3), &db, peer(1003), &None, addr);
        state.observe(0, FILES[3], false, time(4), &db, peer(1004), &None, addr);
        state.observe(0, FILES[4], true, time(10), &db, peer(1010), &None, addr);
        // obsolete events come after cleanup
        state.observe(0, FILES[3], true, time(11), &db, peer(1011), &None, addr);
        state.observe(0, FILES[3], true, time(12), &db, peer(1012), &None, addr);
        state.observe(0, FILES[3], true, time(13), &db, peer(1013), &None, addr);
        state.observe(0, FILES[3], true, time(14), &db, peer(1014), &None, addr);

        let (_, stat) = db.core().fetch_last_stat().unwrap();
        assert_eq!(stat.events.len(), 1);
        assert_eq!(stat.events[0].latency, None);
    })
}
