#[derive(Debug)]
pub enum Error {
    I2CBusReadFailed,
    I2CBusWriteFailed,
}

impl From<embassy_rp::i2c::Error> for Error {
    fn from(value: embassy_rp::i2c::Error) -> Self {
        match value {
            embassy_rp::i2c::Error::Abort(_abort_reason) => Error::I2CBusReadFailed,
            embassy_rp::i2c::Error::InvalidReadBufferLength => Error::I2CBusReadFailed,
            embassy_rp::i2c::Error::InvalidWriteBufferLength => Error::I2CBusWriteFailed,
            embassy_rp::i2c::Error::AddressOutOfRange(_) => Error::I2CBusReadFailed,
            _ => Error::I2CBusWriteFailed,
        }
    }
}
