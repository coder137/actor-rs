use std::{
    marker::PhantomData,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread::{self, JoinHandle},
};

pub trait ActorHandler<Req, Res> {
    fn handle(&mut self, request: Req) -> Res;
}

enum ActorRefState<Res> {
    Start,
    RequestSent(Receiver<Res>),
}

impl<Res> Clone for ActorRefState<Res> {
    fn clone(&self) -> Self {
        Self::Start
    }
}

#[derive(Clone)]
pub struct ActorRef<Req, Res> {
    tx: SyncSender<(Req, SyncSender<Res>)>,
    state: ActorRefState<Res>,
}

impl<Req, Res> ActorRef<Req, Res> {
    fn new(tx: SyncSender<(Req, SyncSender<Res>)>) -> Self {
        Self {
            tx,
            state: ActorRefState::Start,
        }
    }

    /// Blocking call till response is received
    pub fn call_blocking(&self, req: Req) -> Res {
        let (tx, rx) = mpsc::sync_channel(1);
        // TODO, Handle this unwrap gracefully
        // * Ideally the Actor service SHOULD NOT stop before ActorRef
        self.tx.send((req, tx)).unwrap();
        // TODO, Handle this unwrap gracefully
        // * Ideally the Actor service SHOULD NOT drop the tx channel
        let res = rx.recv().unwrap();
        res
    }

    /// Periodic polling for actions to be performed
    /// Should be polled as frequently as possible
    pub fn call_poll(&mut self, on_req: impl Fn() -> Req) -> Result<Option<Res>, String> {
        match &self.state {
            ActorRefState::Start => {
                let (tx, rx) = mpsc::sync_channel(1);
                let req = on_req();
                match self.tx.try_send((req, tx)) {
                    Ok(_) => {
                        self.state = ActorRefState::RequestSent(rx);
                        Ok(None)
                    }
                    Err(TrySendError::Full(_)) => Ok(None),
                    Err(TrySendError::Disconnected(_)) => Err("Error in Actor Thread".into()),
                }
            }
            ActorRefState::RequestSent(rx) => match rx.try_recv() {
                Ok(data) => {
                    self.state = ActorRefState::Start;
                    Ok(Some(data))
                }
                Err(TryRecvError::Empty) => Ok(None),
                Err(TryRecvError::Disconnected) => Err("Error in Actor Thread".into()),
            },
        }
    }
}

pub struct Actor<Req, Res> {
    handle: JoinHandle<()>,
    request: PhantomData<Req>,
    response: PhantomData<Res>,
}

impl<Req, Res> Actor<Req, Res>
where
    Req: Send + 'static,
    Res: Send + 'static,
{
    pub fn new(
        bound: usize,
        mut handler: impl ActorHandler<Req, Res> + Send + 'static,
    ) -> (Self, ActorRef<Req, Res>) {
        let (tx, rx) = mpsc::sync_channel::<(Req, SyncSender<Res>)>(bound);
        let handle = thread::spawn(move || loop {
            match rx.recv() {
                Ok((request, tx)) => {
                    let response = handler.handle(request);
                    let _ = tx.send(response); // Don't care if the response was received or no
                }
                Err(_) => {
                    break;
                }
            }
        });
        (
            Self {
                handle,
                request: PhantomData,
                response: PhantomData,
            },
            ActorRef::new(tx),
        )
    }

    pub fn join(self) {
        let _ = self.handle.join().unwrap();
    }

    // TODO, Actor shutdown
}

#[cfg(test)]
mod tests {

    use std::time::Instant;

    use super::*;

    struct Ping;

    impl ActorHandler<(), ()> for Ping {
        fn handle(&mut self, _request: ()) -> () {
            ()
        }
    }

    #[test]
    fn test_ping() {
        let (actor, actor_ref) = Actor::new(2, Ping);
        let prev = Instant::now();
        let _ = actor_ref.call_blocking(());
        let current = Instant::now();

        println!(
            "Current: {:?} Prev: {:?} Diff: {:?}, Elapsed: {:?}",
            current,
            prev,
            current.duration_since(prev),
            prev.elapsed()
        );

        drop(actor_ref);
        actor.join();
    }

    #[test]
    fn test_ping_poll() {
        let (actor, mut actor_ref) = Actor::new(2, Ping);
        let prev = Instant::now();
        loop {
            let res = actor_ref.call_poll(|| ());
            match res {
                Ok(data) => {
                    if data.is_some() {
                        break;
                    }
                }
                Err(_) => todo!(),
            }
        }
        let current = Instant::now();

        println!(
            "Current: {:?} Prev: {:?} Diff: {:?}, Elapsed: {:?}",
            current,
            prev,
            current.duration_since(prev),
            prev.elapsed()
        );

        drop(actor_ref);
        actor.join();
    }

    #[test]
    fn test_actor_multiple_messages() {
        let (actor, actor_ref) = Actor::new(1, Ping);

        let actor_ref1 = actor_ref.clone();
        let actor_ref2 = actor_ref.clone();

        let _pong1 = actor_ref1.call_blocking(());
        let _pong2 = actor_ref2.call_blocking(());

        drop(actor_ref);
        drop(actor_ref1);
        drop(actor_ref2);
        actor.join();
    }
}
