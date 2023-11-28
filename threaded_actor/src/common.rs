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
