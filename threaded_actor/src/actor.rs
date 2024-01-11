use std::thread::{self};

use crate::{create_channel, ActorDropGuard, ActorHandler, ActorMessage, ActorRef};

pub struct Actor;

impl Actor {
    pub fn create<Req, Res>(
        bound: usize,
        mut handler: impl ActorHandler<Req, Res> + Send + 'static,
    ) -> (ActorRef<Req, Res>, ActorDropGuard<Req, Res>)
    where
        Req: Send + 'static,
        Res: Send + 'static,
    {
        let (user_tx, user_rx) = create_channel::<ActorMessage<Req, Res>>(bound);
        let handle = thread::spawn(move || {
            loop {
                let message = user_rx.recv().expect("ActorDropGuard should ensure that tx channel is not dropped before shutting down Actor thread");
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

#[cfg(test)]
mod actor_tests {
    use super::*;
    use crate::{
        common_test_actors::{Ping, SimulateThreadCrash},
        ActorError,
    };
    use std::{thread, time::Instant};

    #[test]
    fn test_ping() {
        let (actor_ref, actor_drop_guard) = Actor::create(1, Ping { delay: None });

        let now = Instant::now();
        let _ = actor_ref.block(());
        println!("Elapsed: {:?}", now.elapsed());

        println!("Current {:?}", thread::current().id());

        // Shutdown
        drop(actor_drop_guard);
    }

    #[test]
    fn test_actor_multiple_messages() {
        let (actor_ref, actor_drop_guard) = Actor::create(1, Ping { delay: None });

        let actor_ref1 = actor_ref.clone();
        let actor_ref2 = actor_ref.clone();

        let _pong1 = actor_ref1.block(());
        let _pong2 = actor_ref2.block(());

        // Shutdown
        drop(actor_drop_guard);
    }

    #[test]
    fn test_actor_send_after_shutdown_with_blocking() {
        let (actor_ref, actor_drop_guard) = Actor::create(1, Ping { delay: None });

        // Shutdown
        drop(actor_drop_guard);

        let result = actor_ref.block(());
        assert!(result.is_err());
        assert!(matches!(result.err().unwrap(), ActorError::ActorShutdown));
    }

    #[test]
    fn test_actor_bad_behavior_with_blocking() {
        let (actor_ref, actor_drop_guard) = Actor::create(1, SimulateThreadCrash);

        let res = actor_ref.block(());
        assert!(res.is_err());
        assert!(matches!(res.err().unwrap(), ActorError::ActorInternalError));

        // Shutdown
        drop(actor_drop_guard);
    }

    #[test]
    fn test_actor_bad_user_actor_drop() {
        let (actor_ref, actor_drop_guard) = Actor::create(1, Ping { delay: None });

        drop(actor_ref);

        // Shutdown
        drop(actor_drop_guard);
    }
}
