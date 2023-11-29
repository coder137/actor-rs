use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

mod common;
pub use common::*;

mod poll;
pub use poll::*;

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

type ActorSender<T> = SyncSender<T>;
type ActorReceiver<T> = Receiver<T>;
type ActorMessage<Req, Res> = (Req, ActorSender<Res>);

fn create_channel<T>(bound: usize) -> (ActorSender<T>, ActorReceiver<T>) {
    mpsc::sync_channel(bound)
}

pub struct ActorRef<Req, Res> {
    tx: ActorSender<ActorMessage<Req, Res>>,
}

impl<Req, Res> Clone for ActorRef<Req, Res> {
    fn clone(&self) -> Self {
        Self::new(self.tx.clone())
    }
}

impl<Req, Res> ActorRef<Req, Res> {
    fn new(tx: ActorSender<ActorMessage<Req, Res>>) -> Self {
        Self { tx }
    }

    /// Blocking call till response is received
    pub fn block(&self, req: Req) -> Result<Res, ActorError> {
        let (tx, rx) = create_channel(1);
        self.tx
            .send((req, tx))
            .map_err(|_e| ActorError::ActorShutdown)?;
        let response = rx.recv().map_err(|_e| ActorError::ActorInternalError)?;
        Ok(response)
    }

    pub fn as_poll(&self, req: Req) -> ActorRefPollPromise<Req, Res> {
        ActorRefPollPromise::new(req, self.tx.clone())
    }
}

pub enum ActorCommandReq {
    Shutdown,
}

pub enum ActorCommandRes {
    Shutdown,
}

pub struct Actor {}

impl Actor {
    pub fn create<Req, Res>(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common_test_actors::{Ping, SimulateThreadCrash};
    use std::time::Instant;

    fn shutdown_actor_pool(actor_pool: ActorPool) {
        actor_pool.shutdown();
        loop {
            if actor_pool.is_shutdown() {
                break;
            }
        }
        assert!(actor_pool.is_shutdown());
    }

    #[test]
    fn test_ping() {
        let mut actor_pool = ActorPool::new();
        let actor_ref = actor_pool.new_actor(1, Ping { delay: None });

        let now = Instant::now();
        let _ = actor_ref.block(());
        println!("Elapsed: {:?}", now.elapsed());

        println!("Current {:?}", thread::current().id());

        // Shutdown the actor pool
        shutdown_actor_pool(actor_pool);
    }

    #[test]
    fn test_actor_multiple_messages() {
        let mut actor_pool = ActorPool::new();
        let actor_ref = actor_pool.new_actor(1, Ping { delay: None });

        let actor_ref1 = actor_ref.clone();
        let actor_ref2 = actor_ref.clone();

        let _pong1 = actor_ref1.block(());
        let _pong2 = actor_ref2.block(());

        shutdown_actor_pool(actor_pool);
    }

    #[test]
    fn test_actor_send_after_shutdown_with_blocking() {
        let mut actor_pool = ActorPool::new();
        let actor_ref = actor_pool.new_actor(1, Ping { delay: None });

        shutdown_actor_pool(actor_pool);

        let result = actor_ref.block(());
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), ActorError::ActorShutdown));
    }

    #[test]
    fn test_actor_bad_behavior_with_blocking() {
        let mut actor_pool = ActorPool::new();
        let actor_ref = actor_pool.new_actor(1, SimulateThreadCrash);

        let res = actor_ref.block(());
        assert!(res.is_err());
        assert!(matches!(res.err().unwrap(), ActorError::ActorInternalError));

        shutdown_actor_pool(actor_pool);
    }

    #[test]
    fn test_actor_bad_user_actor_drop() {
        let mut actor_pool = ActorPool::new();
        let actor_ref = actor_pool.new_actor(1, Ping { delay: None });

        drop(actor_ref);

        shutdown_actor_pool(actor_pool);
    }
}
