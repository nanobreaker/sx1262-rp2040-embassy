#[derive(Debug, defmt::Format)]
pub enum Error {
    FailedToInitialize,
    FailedToCollectData,
    LoraRadio,
    LoraMac,
    I2CBusReadFailed,
    I2CBusWriteFailed,
    SpiBusFailed,
    AdcFailed,
    RadioError,
}

impl From<core::convert::Infallible> for Error {
    fn from(value: core::convert::Infallible) -> Self {
        match value {}
    }
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

impl From<embassy_rp::spi::Error> for Error {
    fn from(value: embassy_rp::spi::Error) -> Self {
        match value {
            _ => Error::SpiBusFailed,
        }
    }
}

impl From<embassy_rp::adc::Error> for Error {
    fn from(value: embassy_rp::adc::Error) -> Self {
        match value {
            embassy_rp::adc::Error::ConversionFailed => Error::AdcFailed,
        }
    }
}

impl From<embedded_hal_1::digital::ErrorKind> for Error {
    fn from(value: embedded_hal_1::digital::ErrorKind) -> Self {
        match value {
            embedded_hal_1::digital::ErrorKind::Other => todo!(),
            _ => todo!(),
        }
    }
}

impl From<embedded_hal_1::spi::ErrorKind> for Error {
    fn from(value: embedded_hal_1::spi::ErrorKind) -> Self {
        match value {
            embedded_hal_1::spi::ErrorKind::Overrun => todo!(),
            embedded_hal_1::spi::ErrorKind::ModeFault => todo!(),
            embedded_hal_1::spi::ErrorKind::FrameFormat => todo!(),
            embedded_hal_1::spi::ErrorKind::ChipSelectFault => todo!(),
            embedded_hal_1::spi::ErrorKind::Other => todo!(),
            _ => todo!(),
        }
    }
}

impl From<lora_phy::mod_params::RadioError> for Error {
    fn from(value: lora_phy::mod_params::RadioError) -> Self {
        match value {
            _ => Error::RadioError,
        }
    }
}

impl From<lorawan_device::async_device::Error<lora_phy::lorawan_radio::Error>> for Error {
    fn from(value: lorawan_device::async_device::Error<lora_phy::lorawan_radio::Error>) -> Self {
        match value {
            lorawan_device::async_device::Error::Radio(_) => Error::LoraRadio,
            lorawan_device::async_device::Error::Mac(_) => Error::LoraMac,
        }
    }
}
