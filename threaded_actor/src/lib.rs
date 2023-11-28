use std::thread::{self, JoinHandle};

use crossbeam::channel::{self, Select, Sender};

mod common;
pub use common::*;

mod poll;
pub use poll::*;

pub struct ActorRef<Req, Res> {
    tx: Sender<(Req, Sender<Res>)>,
}

impl<Req, Res> Clone for ActorRef<Req, Res> {
    fn clone(&self) -> Self {
        Self::new(self.tx.clone())
    }
}

impl<Req, Res> ActorRef<Req, Res> {
    fn new(tx: Sender<(Req, Sender<Res>)>) -> Self {
        Self { tx }
    }

    /// Blocking call till response is received
    pub fn block(&self, req: Req) -> Result<Res, ActorError> {
        let (tx, rx) = channel::bounded(1);
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
    use super::*;
    use std::time::{Duration, Instant};

    struct Ping {
        delay: Option<Duration>,
    }

    impl ActorHandler<(), ()> for Ping {
        fn handle(&mut self, _request: ()) -> () {
            if let Some(d) = self.delay {
                thread::sleep(d);
            }
            ()
        }
    }

    struct SimulateThreadCrash;

    impl ActorHandler<(), ()> for SimulateThreadCrash {
        fn handle(&mut self, _request: ()) -> () {
            panic!("Simulate thread crash");
        }
    }

    #[test]
    fn test_ping() {
        let actor = Actor::new(2, Ping { delay: None });
        let actor_ref = actor.get_user_actor_ref();

        let prev = Instant::now();
        let _ = actor_ref.block(());
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
            .block(ActorCommandReq::Shutdown);
        assert!(matches!(res.unwrap(), ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());
    }

    #[test]
    fn test_ping_poll() {
        let actor = Actor::new(2, Ping { delay: None });
        let actor_ref = actor.get_user_actor_ref();

        let prev: Instant = Instant::now();
        let mut promise = actor_ref.as_poll(());
        loop {
            let res = promise.poll_once();
            if matches!(res.unwrap(), ActorRefPollInfo::Complete) {
                break;
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
            .block(ActorCommandReq::Shutdown);
        assert!(matches!(res.unwrap(), ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());
    }

    #[test]
    fn test_actor_multiple_messages() {
        let actor = Actor::new(1, Ping { delay: None });
        let actor_ref = actor.get_user_actor_ref();

        let actor_ref1 = actor_ref.clone();
        let actor_ref2 = actor_ref.clone();

        let _pong1 = actor_ref1.block(());
        let _pong2 = actor_ref2.block(());

        let res = actor
            .get_command_actor_ref()
            .block(ActorCommandReq::Shutdown);
        assert!(matches!(res.unwrap(), ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());
    }

    #[test]
    fn test_actor_queue_full() {
        let actor = Actor::new(
            1,
            Ping {
                delay: Some(Duration::from_secs(5)),
            },
        );
        let actor_ref1 = actor.get_user_actor_ref();
        let actor_ref2 = actor.get_user_actor_ref();

        // Sends in queue
        let mut promise1 = actor_ref1.as_poll(());
        let res1 = promise1.poll_once();
        assert!(res1.is_ok());
        assert!(matches!(res1.unwrap(), ActorRefPollInfo::RequestSent));

        // Queue is full
        let mut promise2 = actor_ref2.as_poll(());
        let res2 = promise2.poll_once();
        assert!(res2.is_ok());
        assert!(matches!(res2.unwrap(), ActorRefPollInfo::RequestQueueFull));

        // Shutdown
        let res = actor
            .get_command_actor_ref()
            .block(ActorCommandReq::Shutdown);
        assert!(matches!(res.unwrap(), ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());
    }

    #[test]
    fn test_actor_send_after_shutdown_with_blocking() {
        let actor = Actor::new(1, Ping { delay: None });
        let actor_ref = actor.get_user_actor_ref();

        let res = actor
            .get_command_actor_ref()
            .block(ActorCommandReq::Shutdown);
        assert!(matches!(res.unwrap(), ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());

        let result = actor_ref.block(());
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), ActorError::ActorShutdown));
    }

    #[test]
    fn test_actor_send_after_shutdown_with_poll() {
        let actor = Actor::new(1, Ping { delay: None });
        let actor_ref = actor.get_user_actor_ref();

        let res = actor
            .get_command_actor_ref()
            .block(ActorCommandReq::Shutdown);
        assert!(matches!(res.unwrap(), ActorCommandRes::Shutdown));
        assert!(actor.handle.join().unwrap().is_ok());

        let mut promise = actor_ref.as_poll(());
        let result = promise.poll_once();
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), ActorError::ActorShutdown));
    }

    #[test]
    fn test_actor_bad_behavior_with_blocking() {
        let actor = Actor::new(1, SimulateThreadCrash);
        let actor_ref = actor.get_user_actor_ref();
        let res = actor_ref.block(());
        assert!(res.is_err());
        assert!(matches!(res.err().unwrap(), ActorError::ActorInternalError));

        let result = actor.handle.join();
        assert!(result.is_err());
    }

    #[test]
    fn test_actor_bad_behavior_with_poll() {
        let actor = Actor::new(1, SimulateThreadCrash);
        let actor_ref = actor.get_user_actor_ref();

        let mut promise = actor_ref.as_poll(());
        loop {
            let result = promise.poll_once();
            if let Err(e) = result {
                assert!(matches!(e, ActorError::ActorInternalError));
                break;
            }
        }
        let result = actor.handle.join();
        assert!(result.is_err());
    }

    #[test]
    fn test_actor_bad_drop() {
        let actor = Actor::new(1, Ping { delay: None });
        drop(actor);
    }

    #[test]
    fn test_actor_bad_user_actor_drop() {
        let actor = Actor::new(1, Ping { delay: None });
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
        let actor = Actor::new(1, Ping { delay: None });
        let Actor {
            handle,
            user_actor_ref: _,
            command_actor_ref,
        } = actor;

        drop(command_actor_ref);
        assert!(handle.join().unwrap().is_err());
    }
}
