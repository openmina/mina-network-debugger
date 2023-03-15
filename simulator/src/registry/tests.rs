use std::{collections::BTreeMap, net::IpAddr};

use super::messages::{Summary, MockReport, NetReport};

pub fn test_ipc(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!("This test verifies that each node's interprocess communication checksum matches the checksum recorded by the debugger.");

    let mut fail = false;
    for (&ip, summary) in summary {
        let Some(mock_report) = &summary.mock_report else {
            continue;
        };
        let Some(debugger_report) = &summary.mock_report else {
            continue;
        };
        if !mock_report.ipc.matches_(&debugger_report.ipc) {
            log::error!("ipc checksum mismatch at {ip}");
            fail = true;
        }
    }

    if fail {
        Err(anyhow::anyhow!("test failed"))
    } else {
        Ok(())
    }
}

pub fn test_all_messages_number(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!("This test checks if each node has sent/received some messages.");
    test_local(summary, |mock_report| {
        let success = mock_report.test.timestamps.total_messages != 0;
        if success {
            log::info!("messages: {}", mock_report.test.timestamps.total_messages);
        } else {
            log::error!("no messages");
        }
        success
    })
}

pub fn test_all_messages_order(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!("This test checks whether recorded messages are sorted by timestamp.");
    test_local(summary, |mock_report| {
        let success = mock_report.test.timestamps.ordered;
        if success {
            log::info!("messages timestamps are ordered");
        } else {
            log::error!("messages timestamps are unordered");
            log::info!("{:?}", mock_report.test.timestamps.group_report);
        }
        success
    })
}

pub fn test_all_messages_time_filter(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!("This test verifies that the time filter is working.");
    test_local(summary, |mock_report| {
        let success = mock_report.test.timestamps.timestamps_filter_ok;
        if success {
            log::info!("messages timestamp filter is ok");
        } else {
            log::error!("messages timestamp filter doesn't work");
            log::info!("{:?}", mock_report.test.timestamps.group_report);
        }
        success
    })
}

pub fn test_ipc_events_number(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!("This test checks if the debugger's `/libp2p_ipc/block/<N>` returns some events for each block.");
    test_local(summary, |mock_report| {
        let len = !mock_report.test.events.events.len();
        if len != 0 {
            log::error!("have {len} ipc events");
        } else {
            log::error!("no ipc events");
        }
        !mock_report.test.events.events.is_empty()
    })
}

pub fn test_ipc_events_match(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!("This test checks if the debugger's `/libp2p_ipc/block/<N>` returns correct data for each block, according to the mock node.");
    test_local(summary, |mock_report| {
        let success = mock_report.test.events.matching;
        if success {
            log::info!("ipc events reported by mock and reported by debugger are match");
        } else {
            log::error!("ipc events reported by mock and reported by debugger are not match");
            log::info!("mock events: {:?}", mock_report.test.events.events);
            log::info!(
                "debugger events: {:?}",
                mock_report.test.events.debugger_events
            );
        }
        success
    })
}

pub fn test_ipc_events_consistent(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!("This test checks if the debugger's `/libp2p_ipc/block/<N>` consistent with `/block/<N>` for each block.");
    test_local(summary, |mock_report| {
        let success = mock_report.test.events.consistent;
        if success {
            log::error!("ipc events are consistent with network events");
        } else {
            log::error!("ipc events are inconsistent with network events");
            log::info!(
                "debugger ipc events: {:?}",
                mock_report.test.events.debugger_events
            );
            log::info!(
                "debugger network events: {:?}",
                mock_report.test.events.network_events
            );
        }
        success
    })
}

pub fn test_stream_messages_number(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!("This test checks if the test libp2p stream contains some messages.");
    test_local(summary, |mock_report| {
        let success = mock_report.test.order.messages != 0;
        if success {
            log::info!(
                "there are {} stream messages",
                mock_report.test.order.messages
            );
        } else {
            log::error!("no stream messages");
        }
        success
    })
}

pub fn test_stream_messages_order(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!("This test checks that the messages in the test libp2p stream are recorded in the same order as they were cast.");
    test_local(summary, |mock_report| {
        let success = mock_report.test.order.unordered_num.is_empty();
        if success {
            log::info!("stream messages are ordered");
        } else {
            log::error!("stream messages are unordered");
            log::info!("{:?}", mock_report.test.order.unordered_num);
        }
        success
    })
}

pub fn test_stream_messages_order_time(summary: &BTreeMap<IpAddr, Summary>) -> anyhow::Result<()> {
    println!(
        "This test checks that the messages in the test libp2p stream have ordered timestamps."
    );
    test_local(summary, |mock_report| {
        let success = mock_report.test.order.unordered_time.is_empty();
        if success {
            log::info!("stream messages are ordered by time");
        } else {
            log::error!("stream messages are unordered by time");
            log::info!("{:?}", mock_report.test.order.unordered_time);
        }
        success
    })
}

fn test_local(
    summary: &BTreeMap<IpAddr, Summary>,
    assertion: impl Fn(&MockReport) -> bool,
) -> anyhow::Result<()> {
    let mut fail = false;
    for (ip, summary) in summary {
        let Some(mock_report) = &summary.mock_report else {
            continue;
        };

        log::info!("checking {ip}");
        if !assertion(mock_report) {
            fail = true;
        }
    }

    if fail {
        Err(anyhow::anyhow!("test failed"))
    } else {
        Ok(())
    }
}

mod types {
    use std::{time::SystemTime, net::SocketAddr};

    use serde::{Serialize, Deserialize};

    use mina_ipc::message::ChecksumPair;

    #[derive(Default, Serialize, Deserialize)]
    pub struct NetworkVerbose {
        pub matches: Vec<NetworkMatches>,
        pub too_short_to_validate: Vec<NetworkMatches>,
        pub checksum_mismatch: Vec<NetworkMatches>,
        pub local_debugger_missing: Vec<NetworkMatches>,
        pub remote_debugger_missing: Vec<NetworkMatches>,
        pub both_debuggers_missing: Vec<NetworkMatches>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct NetworkMatches {
        pub bytes_number: u64,
        pub timestamp: SystemTime,
        pub local: SocketAddr,
        pub local_time: Option<SystemTime>,
        pub local_crc64: Option<ChecksumPair>,
        pub remote: SocketAddr,
        pub remote_time: Option<SystemTime>,
        pub remote_crc64: Option<ChecksumPair>,
    }
}
use self::types::{NetworkMatches, NetworkVerbose};

pub fn test_network_checksum(
    summary: &BTreeMap<IpAddr, Summary>,
) -> BTreeMap<IpAddr, NetworkVerbose> {
    println!("This test checks the CRC64 checksum of the traffic flowing over all connections for each node.");
    println!("\
        For each connection seen by tcpflow must exist only one debugger who seen this connection as incoming \
        and only one (distinct) debugger who seen this connection as outgoing.
    ");
    summary
        .iter()
        .filter_map(|(ip, this_summary)| {
            let debugger_report = this_summary.debugger_report.as_ref()?;
            let net_report = &this_summary.net_report;

            let mut network_verbose = NetworkVerbose::default();
            for r in net_report {
                let NetReport {
                    local,
                    remote,
                    counter,
                    timestamp,
                    bytes_number,
                } = *r;
                log::info!("at {ip} found connection {local} <-> {remote}");

                let remote_cn = summary
                    .get(&remote.ip())
                    .and_then(|s| s.debugger_report.as_ref())
                    .and_then(|dbg| {
                        dbg.network
                            .iter()
                            .find(|cn| cn.ip == local.ip() && cn.counter == counter)
                    });
                let local_cn = debugger_report
                    .network
                    .iter()
                    .find(|cn| cn.ip == remote.ip() && cn.counter == counter);

                match (local_cn, remote_cn) {
                    (Some(l), Some(r)) => {
                        log::info!("at {:?} seen by local debugger", l.timestamp);
                        log::info!("at {:?} seen by remote debugger", r.timestamp);
                        let item = NetworkMatches {
                            bytes_number,
                            timestamp,
                            local,
                            local_time: Some(l.timestamp),
                            local_crc64: Some(l.checksum.clone()),
                            remote,
                            remote_time: Some(r.timestamp),
                            remote_crc64: Some(r.checksum.clone()),
                        };
                        // TODO:
                        // let l_bigger = bytes_number <= l.checksum.bytes_number();
                        // let r_bigger = bytes_number <= r.checksum.bytes_number();
                        if let Some((x, y)) = l.checksum.matches_ext(&r.checksum) {
                            log::info!("matches: ({x:016x}, {y:016x}) == ({y:016x}, {x:016x})");
                            network_verbose.matches.push(item);
                        } else if l.checksum.too_short() || r.checksum.too_short() {
                            log::warn!("traffic is too short to verify checksum");
                            log::warn!("sometimes libp2p drop failed connection and not doing `flush`, so one debugger see some traffic, a=but another debugger see nothing, it is not the debugger's fault");
                            network_verbose.too_short_to_validate.push(item);
                        } else {
                            log::error!("checksum mismatches");
                            network_verbose.checksum_mismatch.push(item);
                        }
                    }
                    (Some(l), None) => {
                        log::info!("at {:?} seen by local debugger", l.timestamp);
                        log::error!("remote debugger doesn't see this connection");
                        network_verbose.local_debugger_missing.push(NetworkMatches {
                            bytes_number,
                            timestamp,
                            local,
                            local_time: Some(l.timestamp),
                            local_crc64: Some(l.checksum.clone()),
                            remote,
                            remote_time: None,
                            remote_crc64: None,
                        });
                        break;
                    }
                    (None, Some(r)) => {
                        log::error!("local debugger doesn't see this connection");
                        log::info!("at {:?} seen by remote debugger", r.timestamp);
                        network_verbose
                            .remote_debugger_missing
                            .push(NetworkMatches {
                                bytes_number,
                                timestamp,
                                local,
                                local_time: None,
                                local_crc64: None,
                                remote,
                                remote_time: Some(r.timestamp),
                                remote_crc64: Some(r.checksum.clone()),
                            });
                        break;
                    }
                    (None, None) => {
                        log::error!("local debugger doesn't see this connection");
                        log::error!("remote debugger doesn't see this connection");
                        network_verbose.both_debuggers_missing.push(NetworkMatches {
                            bytes_number,
                            timestamp,
                            local,
                            local_time: None,
                            local_crc64: None,
                            remote,
                            remote_time: None,
                            remote_crc64: None,
                        });
                        break;
                    }
                }
            }

            Some((*ip, network_verbose))
        })
        .collect()
}
