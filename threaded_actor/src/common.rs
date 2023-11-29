pub trait ActorHandler<Req, Res> {
    fn handle(&mut self, request: Req) -> Res;
}

#[derive(Debug, Clone, Copy)]
pub enum ActorError {
    ActorShutdown,
    ActorInternalError,
}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        format!("{:?}", self).fmt(f)
    }
}

impl std::error::Error for ActorError {}

// TODO, Make custom types for other channel types (crossbeam, flume etc)
// TODO, Add them as features

pub type ActorSender<T> = std::sync::mpsc::SyncSender<T>;
pub type ActorReceiver<T> = std::sync::mpsc::Receiver<T>;
pub type ActorMessage<Req, Res> = (Req, ActorSender<Res>);

pub fn create_channel<T>(bound: usize) -> (ActorSender<T>, ActorReceiver<T>) {
    std::sync::mpsc::sync_channel(bound)
}

#[cfg(test)]
pub mod common_test_actors {
    use super::*;
    use std::{thread, time::Duration};

    pub struct Ping {
        pub delay: Option<Duration>,
    }

    impl ActorHandler<(), ()> for Ping {
        fn handle(&mut self, _request: ()) -> () {
            if let Some(d) = self.delay {
                thread::sleep(d);
            }
            ()
        }
    }

    pub struct SimulateThreadCrash;

    impl ActorHandler<(), ()> for SimulateThreadCrash {
        fn handle(&mut self, _request: ()) -> () {
            panic!("Simulate thread crash");
        }
    }
}
