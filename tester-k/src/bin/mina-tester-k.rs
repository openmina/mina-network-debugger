use structopt::StructOpt;

use tester_k::{center, peer};

#[derive(StructOpt)]
enum Command {
    Registry,
    Peer {
        #[structopt(short, long)]
        blocks: u32,
        #[structopt(short, long)]
        delay: u32,
    },
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    match Command::from_args() {
        Command::Registry => center::run(),
        Command::Peer { blocks, delay } => peer::run(blocks, delay),
    }
}
