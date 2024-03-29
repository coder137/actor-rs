use std::sync::mpsc::{TryRecvError, TrySendError};

use crate::{create_channel, ActorError, ActorMessage, ActorReceiver, ActorSender};

enum ActorRefPollState<Res> {
    Start,                           // Try to send the request to Actor
    RequestSent(ActorReceiver<Res>), // Request sent successfully, waiting for response from Actor
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
    channel: Option<(ActorSender<Res>, ActorReceiver<Res>)>,
    res: Option<Res>,
    tx: ActorSender<ActorMessage<Req, Res>>,
    state: ActorRefPollState<Res>,
}

impl<Req, Res> ActorRefPollPromise<Req, Res> {
    pub fn new(req: Req, tx: ActorSender<ActorMessage<Req, Res>>) -> Self {
        Self {
            req: Some(req),
            channel: Some(create_channel(1)),
            res: None,
            tx,
            state: ActorRefPollState::Start,
        }
    }

    pub fn get(&self) -> Option<&Res> {
        self.res.as_ref().map(|d| d)
    }

    pub fn get_mut(&mut self) -> Option<&mut Res> {
        self.res.as_mut().map(|d| d)
    }

    pub fn take(self) -> Option<Res> {
        self.res
    }

    /// Periodic polling for actions to be performed
    /// Should be polled as frequently as possible
    pub fn poll_once(&mut self) -> Result<ActorRefPollInfo, ActorError> {
        match &self.state {
            ActorRefPollState::Start => {
                let (tx, rx) = self.channel.take().unwrap();
                match self
                    .tx
                    .try_send(ActorMessage::User((self.req.take().unwrap(), tx)))
                {
                    Ok(_) => {
                        self.state = ActorRefPollState::RequestSent(rx);
                        Ok(ActorRefPollInfo::RequestSent)
                    }
                    Err(TrySendError::Full(ActorMessage::User((r, t)))) => {
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
                    Err(TrySendError::Full(ActorMessage::Shutdown)) => {
                        unreachable!()
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
mod actor_ref_poll_tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::{
        common_test_actors::{Ping, SimulateThreadCrash},
        Actor,
    };

    #[test]
    fn test_ping_poll() {
        let (actor_ref, actor_drop_guard) = Actor::create(2, Ping { delay: None });

        let now: Instant = Instant::now();
        let mut promise = actor_ref.as_poll(());
        assert!(promise.get().is_none());
        loop {
            let res = promise.poll_once();
            if matches!(res.unwrap(), ActorRefPollInfo::Complete) {
                break;
            }
        }
        println!("Elapsed: {:?}", now.elapsed());
        assert!(promise.get().is_some());
        assert!(promise.get_mut().is_some());
        assert!(promise.take().is_some());

        drop(actor_drop_guard);
    }

    #[test]
    fn test_actor_queue_full() {
        let (actor_ref, actor_drop_guard) = Actor::create(
            1,
            Ping {
                delay: Some(Duration::from_secs(1)),
            },
        );
        let actor_ref1 = actor_ref.clone();
        let actor_ref2 = actor_ref.clone();

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
        drop(actor_drop_guard);
    }

    #[test]
    fn test_actor_poll_after_complete() {
        let (actor_ref, actor_drop_guard) = Actor::create(1, Ping { delay: None });

        let mut promise = actor_ref.as_poll(());
        loop {
            let res = promise.poll_once();
            if matches!(res.unwrap(), ActorRefPollInfo::Complete) {
                break;
            }
        }
        let res = promise.poll_once();
        assert!(res.is_ok());
        assert!(matches!(res.unwrap(), ActorRefPollInfo::Complete));
        assert!(matches!(promise.state, ActorRefPollState::End(..)));

        // Shutdown
        drop(actor_drop_guard);
    }

    #[test]
    fn test_actor_send_after_shutdown_with_poll() {
        let (actor_ref, actor_drop_guard) = Actor::create(1, Ping { delay: None });

        drop(actor_drop_guard);

        let mut promise = actor_ref.as_poll(());
        let result = promise.poll_once();
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), ActorError::ActorShutdown));
    }

    #[test]
    fn test_actor_bad_behavior_with_poll() {
        let (actor_ref, actor_drop_guard) = Actor::create(1, SimulateThreadCrash);

        let mut promise = actor_ref.as_poll(());
        loop {
            let result = promise.poll_once();
            if let Err(e) = result {
                assert!(matches!(e, ActorError::ActorInternalError));
                break;
            }
        }

        drop(actor_drop_guard);
    }
}
