#![allow(dead_code)]
#![allow(non_camel_case_types)]

use actor_macros::threaded_actor;
use threaded_actor::Actor;

struct ActorRef<Req, Res> {
    tx: std::sync::mpsc::Sender<(Req, std::sync::mpsc::Sender<Res>)>,
}

impl<Req, Res> ActorRef<Req, Res> {
    pub fn block(&self, req: Req) -> Res {
        let (tx, rx) = std::sync::mpsc::channel();
        self.tx.send((req, tx)).unwrap();
        let res = rx.recv().unwrap();
        res
    }
}

// User implemented

struct MyActor {
    data: usize,
}

// TODO, Macro needs to handle situations where user does not specify
// self, &mut self, &self in functions
// TODO, self should be disallowed
// &self and &mut self is allowed
#[threaded_actor]
impl MyActor {
    pub fn ping(&self) {}

    pub fn get_data(&self) -> usize {
        self.data
    }

    pub fn simple_add(&mut self, data1: usize) -> usize {
        self.data += data1;
        self.data
    }

    pub fn complex_add(&mut self, data1: usize, data2: usize) -> (usize, f64, String) {
        (self.data + data1 + data2, 0.0, "".to_string())
    }

    // // NO self parameter is present
    // pub fn impure_function_add(data: usize) {
    //     // This add is technically safe since it is ordered
    //     unsafe { DATA += data };
    // }

    // pub fn get_impure_internal_data() -> usize {
    //     unsafe { DATA }
    // }
}

#[test]
fn test_my_actor() {
    // let mut actor_pool = ActorPool::new();
    let (actor_ref, _actor_drop_guard) = Actor::create(1, MyActor { data: 1 });

    let my_actor_ref = MyActorRef::from(actor_ref);

    // 1
    let response = my_actor_ref.ping();
    assert!(response.is_ok());

    // 2
    let response = my_actor_ref.get_data();
    assert!(response.is_ok());
    assert_eq!(response.unwrap(), 1);

    // 3
    let response = my_actor_ref.simple_add(11);
    assert!(response.is_ok());
    assert_eq!(response.unwrap(), 12);

    // 4
    let response = my_actor_ref.get_data();
    assert!(response.is_ok());
    assert_eq!(response.unwrap(), 12);
}
