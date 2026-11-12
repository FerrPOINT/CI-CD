//! Test-only PostgreSQL proxy: lose the client connection while COMMIT is still executing.
//! Keep the server connection alive so the transaction can actually finish.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, UnixStream},
    task::JoinSet,
};

pub struct EarlyCommitLoss {
    pub port: u16,
    pub armed: Arc<AtomicBool>,
    pub lost: Arc<AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for EarlyCommitLoss {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl EarlyCommitLoss {
    pub async fn start(socket: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let armed = Arc::new(AtomicBool::new(false));
        let lost = Arc::new(AtomicBool::new(false));
        let arm = armed.clone();
        let state = lost.clone();
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let (client,_)=accepted.unwrap();
                        let socket=socket.clone();let armed=arm.clone();let lost=state.clone();
                        connections.spawn(async move {
                            let server=UnixStream::connect(socket).await.unwrap();
                            let (mut cr,mut cw)=client.into_split();let (mut sr,mut sw)=server.into_split();
                            let (cut,mut cuts)=tokio::sync::mpsc::channel(1);
                            let lost_reply=lost.clone();
                            let forward=async move {
                                // sslmode=disable gives a normal startup packet without a tag.
                                let mut length=[0;4];if cr.read_exact(&mut length).await.is_err(){return;}
                                let n=u32::from_be_bytes(length) as usize;assert!((8..=65536).contains(&n));
                                let mut startup=vec![0;n-4];cr.read_exact(&mut startup).await.unwrap();
                                sw.write_all(&length).await.unwrap();sw.write_all(&startup).await.unwrap();
                                loop {
                                    let mut tag=[0];if cr.read_exact(&mut tag).await.is_err(){return;}
                                    if cr.read_exact(&mut length).await.is_err(){return;}
                                    let n=u32::from_be_bytes(length) as usize;assert!((4..=16*1024*1024).contains(&n));
                                    let mut body=vec![0;n-4];if cr.read_exact(&mut body).await.is_err(){return;}
                                    if sw.write_all(&tag).await.is_err() || sw.write_all(&length).await.is_err() || sw.write_all(&body).await.is_err(){return;}
                                    if tag[0]==b'Q' && body==b"COMMIT\0" && armed.load(Ordering::SeqCst) && !lost.swap(true,Ordering::SeqCst) {
                                        cut.send(()).await.unwrap();
                                        // Do not drop the server socket when SQLx notices client EOF.
                                        std::future::pending::<()>().await;
                                    }
                                }
                            };
                            let reply=async move {
                                let mut disconnected=false;
                                loop {
                                    let mut tag=[0];
                                    tokio::select! {
                                        _=cuts.recv(),if !disconnected=>{
                                            disconnected=true;let _=cw.shutdown().await;continue;
                                        },
                                        result=sr.read_exact(&mut tag)=>if result.is_err(){return;}
                                    }
                                    let mut length=[0;4];if sr.read_exact(&mut length).await.is_err(){return;}
                                    let n=u32::from_be_bytes(length) as usize;assert!((4..=16*1024*1024).contains(&n));
                                    let mut body=vec![0;n-4];if sr.read_exact(&mut body).await.is_err(){return;}
                                    if disconnected {
                                        if tag[0]==b'C' && body==b"COMMIT\0" || tag[0]==b'E' {return;}
                                    } else if cw.write_all(&tag).await.is_err() || cw.write_all(&length).await.is_err() || cw.write_all(&body).await.is_err() {
                                        // A fresh readback may close before returning its transaction.
                                        if !lost_reply.load(Ordering::SeqCst){return;}
                                    }
                                }
                            };
                            tokio::select!{_=forward=>{},_=reply=>{}}
                        });
                    },
                    _=connections.join_next(),if !connections.is_empty()=>{}
                }
            }
        });
        Self {
            port,
            armed,
            lost,
            task,
        }
    }
}
