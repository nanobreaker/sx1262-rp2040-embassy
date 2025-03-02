use crate::error::Error;

pub trait Device<ID, INFO> {
    async fn init(&mut self) -> Result<ID, Error>;
    async fn info(&mut self) -> Result<INFO, Error>;
}
