use crate::error::Error;

pub trait Device<R>: Sized {
    type Info;

    async fn prepare(r: R) -> Result<Self, Error>;
    async fn init(&mut self) -> Result<Self::Info, Error>;
}
