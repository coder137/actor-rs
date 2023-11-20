use std::thread::{self, JoinHandle};

use crossbeam::{
    channel::{self, Receiver, Sender, TryRecvError, TrySendError},
    select,
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
    handle: JoinHandle<()>,
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

        let handle = thread::spawn(move || loop {
            select! {
                recv(user_rx) -> user_msg => {
                    match user_msg {
                        Ok((request, tx)) => {
                            let response = handler.handle(request);
                            let _ = tx.send(response);
                        },
                        Err(e) => {
                            println!("e : {e:?}");
                            break;
                        },
                    }
                }
                recv(command_rx) -> command_msg => {
                    match command_msg {
                        Ok((request, tx)) => {
                            let response = match request {
                                ActorCommandReq::Shutdown => ActorCommandRes::Shutdown,
                            };
                            let _ = tx.send(response);
                        }
                        Err(e) => {
                            println!("e: {e:?}");
                            break;
                        }
                    }
                }
            }
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

    pub fn join(self) {
        self.handle.join().unwrap();
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
    }
}
