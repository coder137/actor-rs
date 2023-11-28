use crossbeam::channel::{self, Receiver, Sender, TryRecvError, TrySendError};

use crate::ActorError;

enum ActorRefPollState<Res> {
    Start,                      // Try to send the request to Actor
    RequestSent(Receiver<Res>), // Request sent successfully, waiting for response from Actor
    End(Result<ActorRefPollInfo, ActorError>),
}

#[derive(Debug, Clone, Copy)]
pub enum ActorRefPollInfo {
    RequestSent,
    RequestQueueFull,
    ResponseQueueEmpty,
    Complete,
}

pub struct ActorRefPollPromise<Req, Res> {
    req: Option<Req>,
    channel: Option<(Sender<Res>, Receiver<Res>)>,
    res: Option<Res>,
    tx: Sender<(Req, Sender<Res>)>,
    state: ActorRefPollState<Res>,
}

impl<Req, Res> ActorRefPollPromise<Req, Res> {
    pub fn new(req: Req, tx: Sender<(Req, Sender<Res>)>) -> Self {
        Self {
            req: Some(req),
            channel: Some(channel::bounded(1)),
            res: None,
            tx,
            state: ActorRefPollState::Start,
        }
    }

    pub fn is_ready() -> bool {
        todo!()
    }

    pub fn get(&self) -> Option<&Req> {
        todo!()
    }

    pub fn get_mut(&mut self) -> Option<&mut Req> {
        todo!()
    }

    pub fn take(self) -> Option<Req> {
        todo!()
    }

    /// Periodic polling for actions to be performed
    /// Should be polled as frequently as possible
    pub fn poll_once(&mut self) -> Result<ActorRefPollInfo, ActorError> {
        match &self.state {
            ActorRefPollState::Start => {
                let (tx, rx) = self.channel.take().unwrap();
                match self.tx.try_send((self.req.take().unwrap(), tx)) {
                    Ok(_) => {
                        self.state = ActorRefPollState::RequestSent(rx);
                        Ok(ActorRefPollInfo::RequestSent)
                    }
                    Err(TrySendError::Full((r, t))) => {
                        // Same state
                        self.req = Some(r);
                        self.channel = Some((t, rx));
                        Ok(ActorRefPollInfo::RequestQueueFull)
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        let result = Err(ActorError::ActorShutdown);
                        self.state = ActorRefPollState::End(result);
                        result
                    }
                }
            }
            ActorRefPollState::RequestSent(rx) => {
                //
                match rx.try_recv() {
                    Ok(data) => {
                        let result = Ok(ActorRefPollInfo::Complete);
                        self.res = Some(data);
                        self.state = ActorRefPollState::End(result);
                        result
                    }
                    Err(TryRecvError::Empty) => {
                        // Same state
                        Ok(ActorRefPollInfo::ResponseQueueEmpty)
                    }
                    Err(TryRecvError::Disconnected) => {
                        let result = Err(ActorError::ActorInternalError);
                        self.state = ActorRefPollState::End(result);
                        result
                    }
                }
            }
            ActorRefPollState::End(reason) => {
                // Do nothing
                *reason
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::{
        common_test_actors::{Ping, SimulateThreadCrash},
        Actor, ActorCommandReq, ActorCommandRes,
    };

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
}
