use crate::{create_channel, ActorError, ActorMessage, ActorRefPollPromise, ActorSender};

#[derive(Debug)]
pub struct ActorRef<Req, Res> {
    tx: ActorSender<ActorMessage<Req, Res>>,
}

impl<Req, Res> Clone for ActorRef<Req, Res> {
    fn clone(&self) -> Self {
        Self::new(self.tx.clone())
    }
}

impl<Req, Res> ActorRef<Req, Res> {
    pub fn new(tx: ActorSender<ActorMessage<Req, Res>>) -> Self {
        Self { tx }
    }

    /// Blocking call till response is received
    pub fn block(&self, req: Req) -> Result<Res, ActorError> {
        let (tx, rx) = create_channel(1);
        self.tx
            .send(ActorMessage::User((req, tx)))
            .map_err(|_e| ActorError::ActorShutdown)?;
        let response = rx.recv().map_err(|_e| ActorError::ActorInternalError)?;
        Ok(response)
    }

    pub fn as_poll(&self, req: Req) -> ActorRefPollPromise<Req, Res> {
        ActorRefPollPromise::new(req, self.tx.clone())
    }
}
