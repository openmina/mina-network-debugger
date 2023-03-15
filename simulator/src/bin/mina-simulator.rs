use std::{env, net::IpAddr, fs::File, path::PathBuf, process};

use structopt::StructOpt;

use simulator::{
    registry::{tests, server},
    peer,
};

#[derive(StructOpt)]
enum Command {
    Registry,
    Peer {
        #[structopt(short, long)]
        blocks: u32,
        #[structopt(short, long)]
        delay: u32,
    },
    Test {
        #[structopt(short, long)]
        summary_json: PathBuf,
        #[structopt(short, long)]
        name: String,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .format(|buf, record| {
            use std::{io::Write, time::SystemTime};
            use time::OffsetDateTime;

            let (hour, minute, second, micro) = OffsetDateTime::from(SystemTime::now())
                .time()
                .as_hms_micro();
            writeln!(
                buf,
                "{hour:02}:{minute:02}:{second:02}.{micro:06} [{}] {}",
                record.level(),
                record.args()
            )
        })
        .filter(None, log::LevelFilter::Info)
        .init();

    match Command::from_args() {
        Command::Registry => server::run(),
        Command::Peer { blocks, delay } => {
            let registry = env::var("REGISTRY")?;
            let build_number = env::var("BUILD_NUMBER")?.parse::<u32>()?;
            let this_ip = env::var("MY_POD_IP")?.parse::<IpAddr>()?;

            if let Err(err) = peer::behavior::run(blocks, delay, &registry, build_number, this_ip) {
                log::error!("{err}");
            }
            Ok(())
        }
        Command::Test { summary_json, name } => {
            let summary = serde_json::from_reader(File::open(summary_json)?)?;
            match name.as_str() {
                "ipc" => tests::test_ipc(&summary),
                "all-messages-number" => tests::test_all_messages_number(&summary),
                "all-messages-order" => tests::test_all_messages_order(&summary),
                "all-messages-time-filter" => tests::test_all_messages_time_filter(&summary),
                "ipc-events-number" => tests::test_ipc_events_number(&summary),
                "ipc-events-match" => tests::test_ipc_events_match(&summary),
                "ipc-events-consistent" => tests::test_ipc_events_consistent(&summary),
                "stream-messages-number" => tests::test_stream_messages_number(&summary),
                "stream-messages-order" => tests::test_stream_messages_order(&summary),
                "stream-messages-order-time" => tests::test_stream_messages_order_time(&summary),
                "network-checksum" => {
                    let mut ok = true;

                    let test_result = tests::test_network_checksum(&summary);
                    if let Some((ip, _)) = test_result
                        .iter()
                        .find(|(_, report)| !report.checksum_mismatch.is_empty())
                    {
                        log::error!("checksum test failed {ip}, checksum mismatch");
                        ok = false;
                    }
                    if let Some((ip, _)) = test_result
                        .iter()
                        .find(|(_, report)| !report.local_debugger_missing.is_empty())
                    {
                        log::error!("checksum test failed {ip}, local debugger missing");
                        ok = false;
                    }
                    if let Some((ip, _)) = test_result
                        .iter()
                        .find(|(_, report)| !report.remote_debugger_missing.is_empty())
                    {
                        log::error!("checksum test failed {ip}, remote debugger missing");
                        ok = false;
                    }
                    if let Some((ip, _)) = test_result
                        .iter()
                        .find(|(_, report)| !report.both_debuggers_missing.is_empty())
                    {
                        log::error!("checksum test failed {ip}, both debuggers missing");
                        ok = false;
                    }

                    if !ok {
                        log::error!("test failed");
                        let mut test_result = test_result;
                        for (_, report) in &mut test_result {
                            report.matches.clear();
                        }
                        serde_json::to_writer_pretty(std::io::stdout(), &test_result)?;
                        process::exit(1);
                    }

                    Ok(())
                }
                name => {
                    log::warn!("unknown test name: {name}");
                    Ok(())
                }
            }
        }
    }
}
