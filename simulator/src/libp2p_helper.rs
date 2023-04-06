use std::{
    io,
    os::unix::prelude::AsRawFd,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
};

use mina_ipc::message::{incoming, outgoing::{self, Peer}, ChecksumIo, ChecksumPair, Config};

pub struct Process {
    this: Child,
    stdin: Arc<Mutex<ChecksumIo<ChildStdin>>>,
    stdout_handler: thread::JoinHandle<ChecksumIo<ChildStdout>>,
    rpc_rx: RpcReceiver,
    ctx: mpsc::Sender<Option<outgoing::Msg>>,
}

pub struct StreamSender {
    stdin: Arc<Mutex<ChecksumIo<ChildStdin>>>,
    stream_id: u64,
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
        let stdin = Arc::new(Mutex::new(stdin));
        let stdout = ChecksumIo::new(this.stdout.take().expect("must be present"));

        let (push_tx, push_rx) = mpsc::channel();
        let (rpc_tx, rpc_rx) = mpsc::channel();
        let (ctx, crx) = mpsc::channel::<Option<outgoing::Msg>>();
        let c_ctx = ctx.clone();
        let stdout_handler = thread::spawn(move || {
            let mut stdout = stdout;
            let inner = thread::spawn(move || {
                loop {
                    match stdout.decode() {
                        Ok(m) => c_ctx.send(Some(m)).unwrap_or_default(),
                        // stdout is closed, no error
                        Err(err) if err.description == "Premature end of file" => {
                            c_ctx.send(None).unwrap_or_default();
                            break;
                        }
                        Err(err) => {
                            c_ctx.send(None).unwrap_or_default();
                            log::error!("error decoding message: {err}");
                            break;
                        }
                    }
                }
                stdout
            });
            loop {
                match crx.recv() {
                    Ok(Some(outgoing::Msg::PushMessage(msg))) => {
                        push_tx.send(msg).expect("must exist")
                    }
                    Ok(Some(outgoing::Msg::RpcResponse(msg))) => {
                        rpc_tx.send(msg).expect("must exist")
                    }
                    Ok(Some(outgoing::Msg::Unknown(msg))) => {
                        log::error!("unknown discriminant: {msg}");
                        break;
                    }
                    Ok(None) => {
                        log::info!("break receiving loop");

                        break;
                    }
                    Err(_) => break,
                };
            }
            drop((push_tx, rpc_tx));
            inner.join().unwrap()
        });

        (
            Process {
                this,
                stdin,
                stdout_handler,
                rpc_rx,
                ctx,
            },
            push_rx,
        )
    }

    pub fn generate_keypair(&mut self) -> mina_ipc::Result<Option<(String, Vec<u8>, Vec<u8>)>> {
        self.stdin
            .lock()
            .unwrap()
            .encode(&incoming::Msg::RpcRequest(
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
        self.stdin.lock().unwrap().encode(&value)?;
        let _ = self.rpc_rx.recv();
        Ok(())
    }

    pub fn list_peers(&mut self) -> mina_ipc::Result<Option<Vec<Peer>>> {
        let value = incoming::Msg::RpcRequest(incoming::RpcRequest::ListPeers);
        self.stdin.lock().unwrap().encode(&value)?;
        let Ok(outgoing::RpcResponse::ListPeers(peers)) = self.rpc_rx.recv() else {
            return Ok(None);
        };
        Ok(Some(peers))
    }

    pub fn publish(&mut self, topic: String, data: Vec<u8>) -> mina_ipc::Result<()> {
        let value = incoming::Msg::RpcRequest(incoming::RpcRequest::Publish { topic, data });
        self.stdin.lock().unwrap().encode(&value)?;
        let _ = self.rpc_rx.recv();
        Ok(())
    }

    pub fn subscribe(&mut self, id: u64, topic: &str) -> mina_ipc::Result<()> {
        let value = incoming::Msg::RpcRequest(incoming::RpcRequest::Subscribe {
            id,
            topic: topic.to_owned(),
        });
        self.stdin.lock().unwrap().encode(&value)?;
        let _ = self.rpc_rx.recv();
        Ok(())
    }

    pub fn open_stream(
        &mut self,
        peer_id: &str,
        protocol: &str,
    ) -> mina_ipc::Result<Result<StreamSender, String>> {
        let value = incoming::Msg::RpcRequest(incoming::RpcRequest::AddStreamHandler {
            protocol: protocol.to_owned(),
        });
        self.stdin.lock().unwrap().encode(&value)?;
        if let Ok(outgoing::RpcResponse::Error(err)) = self.rpc_rx.recv() {
            return Ok(Err(err));
        }

        let value = incoming::Msg::RpcRequest(incoming::RpcRequest::OpenStream {
            peer_id: peer_id.to_owned(),
            protocol: protocol.to_owned(),
        });
        self.stdin.lock().unwrap().encode(&value)?;
        match self.rpc_rx.recv() {
            Ok(outgoing::RpcResponse::OutgoingStream(v)) => Ok(Ok(StreamSender {
                stdin: self.stdin.clone(),
                stream_id: v.stream_id,
            })),
            Ok(outgoing::RpcResponse::Error(err)) => Ok(Err(err)),
            _ => Ok(Err(String::new())),
        }
    }

    pub fn stop_receiving(&mut self) {
        self.ctx.send(None).unwrap_or_default()
    }

    pub fn stop(mut self) -> io::Result<(ChecksumPair, Option<i32>)> {
        nix::unistd::close(self.stdin.lock().unwrap().inner.as_raw_fd()).unwrap();

        let status = self.this.wait().unwrap();

        // read remaining data in pipe
        let stdout = self.stdout_handler.join().unwrap();

        Ok((
            ChecksumPair(self.stdin.lock().unwrap().checksum(), stdout.checksum()),
            status.code(),
        ))
    }
}

impl StreamSender {
    pub fn send_stream(&mut self, data: &[u8]) -> mina_ipc::Result<()> {
        let value = incoming::Msg::RpcRequest(incoming::RpcRequest::SendStream {
            data: data.to_owned(),
            stream_id: self.stream_id,
        });
        self.stdin.lock().unwrap().encode(&value)?;
        // TODO:
        // let _ = self.rpc_rx.recv();
        Ok(())
    }
}
