use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::RecvTimeoutError,
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{create_channel, ActorHandler, ActorRef, ActorSender};

pub struct ActorPool {
    actors: Vec<JoinHandle<()>>,
    shutdown: Arc<AtomicBool>,
}

impl ActorPool {
    pub fn new() -> Self {
        Self {
            actors: Vec::new(),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn new_actor<Req, Res>(
        &mut self,
        bound: usize,
        handler: impl ActorHandler<Req, Res> + Send + 'static,
    ) -> ActorRef<Req, Res>
    where
        Req: Send + 'static,
        Res: Send + 'static,
    {
        let (handle, actor_ref) = Actor::create(bound, handler, self.shutdown.clone());
        self.actors.push(handle);
        actor_ref
    }

    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    pub fn is_shutdown(&self) -> bool {
        self.actors.iter().filter(|h| !h.is_finished()).count() == 0
    }
}

struct Actor;

impl Actor {
    fn create<Req, Res>(
        bound: usize,
        mut handler: impl ActorHandler<Req, Res> + Send + 'static,
        shutdown_ind: Arc<AtomicBool>,
    ) -> (JoinHandle<()>, ActorRef<Req, Res>)
    where
        Req: Send + 'static,
        Res: Send + 'static,
    {
        let (user_tx, user_rx) = create_channel::<(Req, ActorSender<Res>)>(bound);
        let handle = thread::spawn(move || {
            //
            println!("Current {:?}", thread::current().id());
            loop {
                // * The recv channel times out to check if this actor needs to shutdown
                match user_rx.recv_timeout(Duration::from_secs(1)) {
                    Ok((req, res_tx)) => {
                        let res = handler.handle(req);
                        let _ = res_tx.send(res);
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        if shutdown_ind.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        break;
                    }
                }
            }
        });
        (handle, ActorRef::new(user_tx))
    }
}
