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
