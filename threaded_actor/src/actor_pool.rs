use std::thread::{self, JoinHandle};

use crate::{create_channel, ActorDropGuard, ActorHandler, ActorMessage, ActorRef};

pub struct ActorPool {
    actors: Vec<JoinHandle<()>>,
}

impl ActorPool {
    pub fn new() -> Self {
        Self { actors: Vec::new() }
    }

    pub fn new_actor<Req, Res>(
        &mut self,
        bound: usize,
        handler: impl ActorHandler<Req, Res> + Send + 'static,
    ) -> (ActorRef<Req, Res>, ActorDropGuard<Req, Res>)
    where
        Req: Send + 'static,
        Res: Send + 'static,
    {
        Actor::create(bound, handler)
    }

    pub fn shutdown(&self) {
        // self.shutdown.store(true, Ordering::Relaxed);
        todo!()
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
    ) -> (ActorRef<Req, Res>, ActorDropGuard<Req, Res>)
    where
        Req: Send + 'static,
        Res: Send + 'static,
    {
        // let (user_tx, user_rx) = create_channel::<(Req, ActorSender<Res>)>(bound);
        let (user_tx, user_rx) = create_channel::<ActorMessage<Req, Res>>(bound);
        let handle = thread::spawn(move || {
            loop {
                let message = match user_rx.recv() {
                    Ok(message) => message,
                    Err(_) => {
                        // TODO, Notify about closed channel
                        break;
                    }
                };

                match message {
                    ActorMessage::User((req, res_tx)) => {
                        let res = handler.handle(req);
                        let _ = res_tx.send(res);
                    }
                    ActorMessage::Shutdown => {
                        // TODO, Notify about shutting down
                        break;
                    }
                }
            }
        });
        (
            ActorRef::new(user_tx.clone()),
            ActorDropGuard::new(user_tx, handle),
        )
    }
}
