use structopt::StructOpt;

#[derive(StructOpt)]
enum Arg {
    Ledger {
        #[structopt(long)]
        hash: String,
    },
    State {
        #[structopt(long)]
        hash: String,
    },
}

fn main() {
    let arg = Arg::from_args();
    let (version, hash) = match arg {
        Arg::Ledger { hash } => (5, hash),
        Arg::State { hash } => (16, hash),
    };
    let x = if let Ok(bytes) = hex::decode(format!("01{hash}")) {
        bs58::encode(bytes)
            .with_check_version(version)
            .into_string()
    } else {
        hex::encode(
            &bs58::decode(hash)
                .with_check(Some(version))
                .into_vec()
                .unwrap()[2..],
        )
    };
    println!("{x}");
}
// jwrPvAMUNo3EKT2puUk5Fxz6B7apRAoKNTGpAA49t3TRSfzvdrL
// 636f5b2d67278e17bc4343c7c23fb4991f8cf0bbbfd8558615b124d5d62548
