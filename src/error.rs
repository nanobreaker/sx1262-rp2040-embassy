#[derive(Debug, defmt::Format)]
pub enum Error {
    Init,

    Duty,

    JoinFailed,
    JoinLimitReached,
    JoinAcceptMissing,
    JoinSessionExpired,

    LoRaWAN(lorawan_device::async_device::Error<lora_phy::lorawan_radio::Error>),
    LoRaPHY(lora_phy::mod_params::RadioError),

    Flash(#[defmt(Debug2Format)] ekv::FormatError<embassy_rp::flash::Error>),

    Spi(embassy_rp::spi::Error),
    SPIErrorKind(#[defmt(Debug2Format)] embedded_hal_1::spi::ErrorKind),
    I2C(embassy_rp::i2c::Error),
    Adc(embassy_rp::adc::Error),

    Infallible,
}

impl From<core::convert::Infallible> for Error {
    fn from(_: core::convert::Infallible) -> Self {
        Error::Infallible
    }
}

impl From<embassy_rp::i2c::Error> for Error {
    fn from(value: embassy_rp::i2c::Error) -> Self {
        Error::I2C(value)
    }
}

impl From<embassy_rp::spi::Error> for Error {
    fn from(value: embassy_rp::spi::Error) -> Self {
        Error::Spi(value)
    }
}

impl From<embassy_rp::adc::Error> for Error {
    fn from(value: embassy_rp::adc::Error) -> Self {
        Error::Adc(value)
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
        Error::SPIErrorKind(value)
    }
}

impl From<lora_phy::mod_params::RadioError> for Error {
    fn from(value: lora_phy::mod_params::RadioError) -> Self {
        Error::LoRaPHY(value)
    }
}

impl From<lorawan_device::async_device::Error<lora_phy::lorawan_radio::Error>> for Error {
    fn from(value: lorawan_device::async_device::Error<lora_phy::lorawan_radio::Error>) -> Self {
        Error::LoRaWAN(value)
    }
}
