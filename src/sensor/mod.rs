use embassy_rp::adc::{self, Async};

pub mod air_sensor;
pub mod soil_sensor;
pub mod system_sensor;

/// Trait to describe generic functionality of a sensor.
/// In general we want to be able to gather environmental data in form of probing
/// and also have a simple way to manage power of the sensor by turning it on/off.
///
/// For example a soil sensor should be turned off after probing otherwise
/// constant power will accelerate oxidation process and hence limit the lifetime of the sensor.
pub trait Sensor<const PAYLOAD_SIZE: usize> {
    /// Error type representation, left up to the implementor
    type Error;

    /// Blocking method to turn on the device
    async fn on(&mut self) -> Result<(), Self::Error>;

    /// Blocking method to turn off the device
    async fn off(&mut self) -> Result<(), Self::Error>;

    /// Async method to verify device
    async fn verify(&mut self) -> Result<(), Self::Error>;

    /// Async method to probe the environment and gather data, response must be encoded thru Cayenne LPP codec
    async fn probe(&mut self, adc: &mut adc::Adc<'static, Async>) -> Result<[u8; PAYLOAD_SIZE], Self::Error>;
}
