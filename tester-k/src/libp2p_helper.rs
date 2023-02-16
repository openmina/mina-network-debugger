use std::{
    io,
    os::unix::prelude::AsRawFd,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::mpsc,
    thread,
};

use mina_ipc::message::{incoming, outgoing, ChecksumIo, ChecksumPair, Config};

pub struct Process {
    this: Child,
    stdin: ChecksumIo<ChildStdin>,
    stdout_handler: thread::JoinHandle<ChecksumIo<ChildStdout>>,
    rpc_rx: RpcReceiver,
}

pub type PushReceiver = mpsc::Receiver<outgoing::PushMessage>;

type RpcReceiver = mpsc::Receiver<outgoing::RpcResponse>;

impl Process {
    pub fn spawn() -> (Self, PushReceiver) {
        let mut this = Command::new("coda-libp2p_helper")
            .envs(std::env::vars())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("launcher executable");
        let stdin = ChecksumIo::new(this.stdin.take().expect("must be present"));
        let stdout = ChecksumIo::new(this.stdout.take().expect("must be present"));

        let (push_tx, push_rx) = mpsc::channel();
        let (rpc_tx, rpc_rx) = mpsc::channel();
        let stdout_handler = thread::spawn(move || {
            let mut stdout = stdout;
            loop {
                match stdout.decode() {
                    Ok(outgoing::Msg::PushMessage(msg)) => push_tx.send(msg).expect("must exist"),
                    Ok(outgoing::Msg::RpcResponse(msg)) => rpc_tx.send(msg).expect("must exist"),
                    Ok(outgoing::Msg::Unknown(msg)) => {
                        log::error!("unknown discriminant: {msg}");
                        break;
                    }
                    // stdout is closed, no error
                    Err(err) if err.description == "Premature end of file" => {
                        break;
                    }
                    Err(err) => {
                        log::error!("error decoding message: {err}");
                        break;
                    }
                };
            }
            stdout
        });

        (
            Process {
                this,
                stdin,
                stdout_handler,
                rpc_rx,
            },
            push_rx,
        )
    }

    pub fn generate_keypair(&mut self) -> mina_ipc::Result<Option<(String, Vec<u8>, Vec<u8>)>> {
        self.stdin.encode(&incoming::Msg::RpcRequest(
            incoming::RpcRequest::GenerateKeypair,
        ))?;
        let r = self.rpc_rx.recv();
        let r = match r {
            Err(_) => return Ok(None),
            Ok(v) => v,
        };
        match r {
            outgoing::RpcResponse::GenerateKeypair {
                peer_id,
                public_key,
                secret_key,
            } => Ok(Some((peer_id, public_key, secret_key))),
            _ => Ok(None),
        }
    }

    pub fn configure(&mut self, config: Config) -> mina_ipc::Result<()> {
        let value = incoming::Msg::RpcRequest(incoming::RpcRequest::Configure(config));
        self.stdin.encode(&value)?;
        let _ = self.rpc_rx.recv();
        Ok(())
    }

    pub fn publish(&mut self, topic: String, data: Vec<u8>) -> mina_ipc::Result<()> {
        let value = incoming::Msg::RpcRequest(incoming::RpcRequest::Publish { topic, data });
        self.stdin.encode(&value)?;
        let _ = self.rpc_rx.recv();
        Ok(())
    }

    pub fn subscribe(&mut self, id: u64, topic: &str) -> mina_ipc::Result<()> {
        let value = incoming::Msg::RpcRequest(incoming::RpcRequest::Subscribe {
            id,
            topic: topic.to_owned(),
        });
        self.stdin.encode(&value)
    }

    pub fn stop(mut self) -> io::Result<(ChecksumPair, Option<i32>)> {
        nix::unistd::close(self.stdin.inner.as_raw_fd()).unwrap();

        let status = self.this.wait().unwrap();

        // read remaining data in pipe
        let stdout = self.stdout_handler.join().unwrap();

        Ok((
            ChecksumPair(self.stdin.checksum(), stdout.checksum()),
            status.code(),
        ))
    }
}
