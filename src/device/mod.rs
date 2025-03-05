use crate::error::Error;

pub trait Device<C> {
    async fn build(r: C) -> Result<Self, Error>;
    async fn verify(&mut self) -> Result<(), Error>;
}
