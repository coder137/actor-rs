use std::thread::{self, JoinHandle};

use crossbeam::channel::{self, Receiver, Select, Sender, TryRecvError, TrySendError};

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

pub struct ActorRef<Req, Res> {
    tx: Sender<(Req, Sender<Res>)>,
    state: ActorRefState<Res>,
}

impl<Req, Res> Clone for ActorRef<Req, Res> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            state: self.state.clone(),
        }
    }
}

impl<Req, Res> ActorRef<Req, Res> {
    fn new(tx: Sender<(Req, Sender<Res>)>) -> Self {
        Self {
            tx,
            state: ActorRefState::Start,
        }
    }

    /// Blocking call till response is received
    pub fn call_blocking(&self, req: Req) -> Res {
        let (tx, rx) = channel::bounded(1);
        // TODO, Handle this unwrap gracefully
        // * Ideally the Actor service SHOULD NOT stop before ActorRef
        self.tx.send((req, tx)).unwrap();
        // TODO, Handle this unwrap gracefully
        // * Ideally the Actor service SHOULD NOT drop the tx channel
        rx.recv().unwrap()
    }

    /// Periodic polling for actions to be performed
    /// Should be polled as frequently as possible
    pub fn call_poll(&mut self, on_req: impl Fn() -> Req) -> Result<Option<Res>, String> {
        match &self.state {
            ActorRefState::Start => {
                let (tx, rx) = channel::bounded(1);
                // !FIXME, on_req should only be called when value can be sent
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

pub enum ActorCommandReq {
    Shutdown,
}

pub enum ActorCommandRes {
    Shutdown,
}

pub struct Actor<Req, Res> {
    handle: JoinHandle<Result<(), ()>>,
    user_actor_ref: ActorRef<Req, Res>,
    command_actor_ref: ActorRef<ActorCommandReq, ActorCommandRes>,
}

impl<Req, Res> Actor<Req, Res>
where
    Req: Send + 'static,
    Res: Send + 'static,
{
    pub fn new(bound: usize, mut handler: impl ActorHandler<Req, Res> + Send + 'static) -> Self {
        let (user_tx, user_rx) = channel::bounded::<(Req, Sender<Res>)>(bound);

        let (command_tx, command_rx) =
            channel::bounded::<(ActorCommandReq, Sender<ActorCommandRes>)>(1);

        let handle = thread::spawn(move || {
            let mut shutdown = false;
            let mut exit_result = Ok(()); // TODO, Specialize the error return type latyer
            let mut sel = Select::new();
            // TODO, Use indexes later instead of match
            let _user_rx_index = sel.recv(&user_rx);
            let _command_rx_index = sel.recv(&command_rx);
            loop {
                match sel.ready() {
                    0 => {
                        if let Ok((user_request, tx)) = user_rx.recv() {
                            let response = handler.handle(user_request);
                            let _ = tx.send(response);
                        } else {
                            shutdown = true;
                            exit_result = Err(());
                        }
                    }
                    1 => {
                        if let Ok((user_command, tx)) = command_rx.recv() {
                            let response = match user_command {
                                ActorCommandReq::Shutdown => {
                                    shutdown = true;
                                    ActorCommandRes::Shutdown
                                }
                            };
                            let _ = tx.send(response);
                        } else {
                            shutdown = true;
                            exit_result = Err(());
                        }
                    }
                    _ => unreachable!(),
                };

                if shutdown {
                    break;
                }
            }
            exit_result
        });
        Self {
            handle,
            user_actor_ref: ActorRef::new(user_tx),
            command_actor_ref: ActorRef::new(command_tx),
        }
    }

    pub fn get_user_actor_ref(&self) -> ActorRef<Req, Res> {
        self.user_actor_ref.clone()
    }

    pub fn get_command_actor_ref(&self) -> ActorRef<ActorCommandReq, ActorCommandRes> {
        self.command_actor_ref.clone()
    }
}

#[cfg(test)]
mod tests {

    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::Instant,
    };

    use super::*;

    struct Ping;

    impl ActorHandler<(), ()> for Ping {
        fn handle(&mut self, _request: ()) -> () {
            ()
        }
    }

    #[test]
    fn test_ping() {
        let actor = Actor::new(2, Ping);
        let actor_ref = actor.get_user_actor_ref();
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

        let res = actor
            .get_command_actor_ref()
            .call_blocking(ActorCommandReq::Shutdown);
        assert!(matches!(res, ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());
    }

    #[test]
    fn test_ping_poll() {
        let actor = Actor::new(2, Ping);
        let mut actor_ref = actor.get_user_actor_ref();
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

        let res = actor
            .get_command_actor_ref()
            .call_blocking(ActorCommandReq::Shutdown);
        assert!(matches!(res, ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());
    }

    #[test]
    fn test_actor_multiple_messages() {
        let actor = Actor::new(1, Ping);
        let actor_ref = actor.get_user_actor_ref();

        let actor_ref1 = actor_ref.clone();
        let actor_ref2 = actor_ref.clone();

        let _pong1 = actor_ref1.call_blocking(());
        let _pong2 = actor_ref2.call_blocking(());

        let res = actor
            .get_command_actor_ref()
            .call_blocking(ActorCommandReq::Shutdown);
        assert!(matches!(res, ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());
    }

    #[ignore = "Update call_poll with states for each subsequent call"]
    #[test]
    fn test_actor_queue_full() {
        let actor = Actor::new(1, Ping);
        let mut actor_ref1 = actor.get_user_actor_ref();
        let mut actor_ref2 = actor.get_user_actor_ref();

        // Sends in queue
        let is_taken1 = AtomicBool::new(false);
        let res1 = actor_ref1.call_poll(|| {
            println!("Called A");
            is_taken1.store(true, Ordering::SeqCst);
            ()
        });

        // Queue is full
        let is_taken2 = AtomicBool::new(false);
        let res2 = actor_ref2.call_poll(|| {
            println!("Called B");
            is_taken2.store(true, Ordering::SeqCst);
            ()
        });

        // Checks
        assert!(res1.is_ok());
        assert!(res1.unwrap().is_none());
        assert!(is_taken1.load(Ordering::SeqCst));

        assert!(res2.is_ok());
        assert!(res2.unwrap().is_none());
        assert_eq!(false, is_taken2.load(Ordering::SeqCst));

        // Shutdown
        let res = actor
            .get_command_actor_ref()
            .call_blocking(ActorCommandReq::Shutdown);
        assert!(matches!(res, ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());
    }

    #[test]
    fn test_actor_bad_drop() {
        let actor = Actor::new(1, Ping);
        drop(actor);
    }

    #[test]
    fn test_actor_bad_user_actor_drop() {
        let actor = Actor::new(1, Ping);
        let Actor {
            handle,
            user_actor_ref,
            command_actor_ref: _,
        } = actor;

        drop(user_actor_ref);
        assert!(handle.join().unwrap().is_err());
    }

    #[test]
    fn test_actor_bad_command_actor_drop() {
        let actor = Actor::new(1, Ping);
        let Actor {
            handle,
            user_actor_ref: _,
            command_actor_ref,
        } = actor;

        drop(command_actor_ref);
        assert!(handle.join().unwrap().is_err());
    }
}
