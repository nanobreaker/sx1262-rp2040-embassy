use crate::error::Error;

pub trait Sensor<D> {
    async fn collect_data(&mut self) -> Result<D, Error>;
}
