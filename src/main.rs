#![no_std]
#![no_main]

mod air_sensor;
mod board;
mod board_sensor;
mod config;
mod device;
mod error;
mod sensor;
mod soil_sensor;
mod transceiver;

use air_sensor::AirSensor;
use assign_resources::assign_resources;
use board::{Board, BoardBuilder};
use board_sensor::BoardSensor;
use config::Config;
use core::ops::Deref;
use defmt::{error, info, warn};
use device::Device;
use embassy_executor::Spawner;
use embassy_rp::adc::{self, Adc};
use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
use embassy_rp::i2c::I2c;
use embassy_rp::peripherals::{
    self, ADC, ADC_TEMP_SENSOR, DMA_CH0, DMA_CH1, I2C0, I2C1, PIN_10, PIN_11, PIN_12, PIN_15, PIN_16, PIN_17, PIN_18, PIN_19, PIN_2, PIN_20, PIN_26,
    PIN_3, SPI1,
};
use embassy_rp::spi::{Config, Spi};
use embassy_rp::{bind_interrupts, i2c, Peripheral, PeripheralRef};
use embassy_sync::blocking_mutex::raw::ThreadModeRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_sync::once_lock::OnceLock;
use embassy_time::{Delay, Duration, Ticker, Timer};
use embedded_hal_bus::spi::ExclusiveDevice;
use error::Error;
use lora_phy::iv::GenericSx126xInterfaceVariant;
use lora_phy::lorawan_radio::LorawanRadio;
use lora_phy::sx126x::TcxoCtrlVoltage;
use lora_phy::sx126x::{self, Sx1262, Sx126x};
use lora_phy::LoRa;
use lorawan_device::async_device::{self, region, EmbassyTimer, JoinMode, SendResponse};
use lorawan_device::default_crypto::DefaultFactory as Crypto;
use lorawan_device::{AppEui, AppKey, DevEui};
use sensor::Sensor;
use soil_sensor::SoilSensor;
use static_cell::StaticCell;
use transceiver::{RadioDevice, Transceiver};
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    ADC_IRQ_FIFO => embassy_rp::adc::InterruptHandler;
    I2C0_IRQ => embassy_rp::i2c::InterruptHandler<I2C0>;
    I2C1_IRQ => embassy_rp::i2c::InterruptHandler<I2C1>;
});

assign_resources! {
    board: BoardSen {
        adc: ADC,
        adc_temp_sensor: ADC_TEMP_SENSOR,
        pin_24: PIN_24,
        pin_26: PIN_26,
        pin_29: PIN_29,
    },
    air: AirSen {
        pin_16: PIN_16,
        pin_17: PIN_17,
        i2c0: I2C0,
    },
    soil: SoilSen {
        pin_18: PIN_18,
        pin_19: PIN_19,
        i2c1: I2C1,
    },
    xcvr: Xcvr{
        pin_2: PIN_2,
        pin_3: PIN_3,
        pin_10: PIN_10,
        pin_11: PIN_11,
        pin_12: PIN_12,
        pin_15: PIN_15,
        pin_20: PIN_20,
        dma_ch0: DMA_CH0,
        dma_ch1: DMA_CH1,
        spi1: SPI1,
    }
}

enum State {
    Auth,
    Run,
    Sleep,
}

#[embassy_executor::main]
async fn main(s: Spawner) {
    let p = embassy_rp::init(Default::default());
    let r = split_resources! {p};

    let board_sensor = BoardSensor::build(r.board).await.expect("board sensors should be functional");
    let air_sensor = AirSensor::build(r.air).await.expect("air sensor should be connected");
    let soil_sensor = SoilSensor::build(r.soil).await.expect("soil sensor should be connected");
    let radio = RadioDevice::build(r.xcvr).await.expect("radio module should be connected");
    let board = BoardBuilder::new()
        .with_board_sensor(board_sensor)
        .with_radio(radio)
        .with_air_sensor(air_sensor)
        .with_soil_sensor(soil_sensor)
        .build()
        .expect("all devices should be connected");

    s.spawn(orchestator(board)).expect("executor should be initialized")
}

#[embassy_executor::task]
async fn orchestator(mut board: Board) {
    let mut ticker = Ticker::every(Duration::from_secs(60 * 5));
    let mut state = State::Auth;
    let mut join_counter: u8 = 0;
    loop {
        state = match state {
            State::Auth => {
                let mut radio = board.radio.lock().await;
                let response = radio
                    .auth(&JoinMode::OTAA {
                        deveui: DevEui::from(config::Config::DEV_EUI),
                        appeui: AppEui::from(config::Config::APP_EUI),
                        appkey: AppKey::from(config::Config::APP_KEY),
                    })
                    .await;

                match response {
                    Ok(_) => {
                        info!("radio: successfully joined lora network");
                        State::Run
                    }
                    Err(_) => {
                        warn!("radio: failed to join lora network, attempt {=u8}", join_counter);

                        if join_counter > 5 {
                            error!("failed to join network more than 5 times, going into sleep mode for 10 minutes");
                            State::Sleep
                        } else {
                            join_counter += 1;
                            State::Auth
                        }
                    }
                }
            }
            State::Run => {
                // let mut board = bo.lock().await;
                let mut air_sensor = board.air_sensor.lock().await;
                let mut soil_sensor = board.soil_sensor.lock().await;

                let (air, soil, board) =
                    embassy_futures::join::join3(air_sensor.collect_data(), soil_sensor.collect_data(), board.collect_data()).await;

                if let (Ok(air_data), Ok(soil_data), Ok(board_data)) = (air, soil, board) {
                    let a_payload: [u8; 11] = air_data.into();
                    let s_payload: [u8; 8] = soil_data.into();
                    let b_payload: [u8; 11] = board_data.into();
                    let payload: [u8; 30] = {
                        let mut buf = [0u8; 30];
                        buf[0..11].copy_from_slice(&a_payload);
                        buf[12..19].copy_from_slice(&s_payload);
                        buf[20..30].copy_from_slice(&b_payload);
                        buf
                    };

                    let radio_mutex = RADIO_GUARD.get().await;
                    let mut radio = radio_mutex.lock().await;
                    let send_response = radio.uplink(&payload).await;
                    match send_response {
                        Ok(response) => match response {
                            SendResponse::DownlinkReceived(fcount) => {
                                info!("radio: received downlink with fcount {=u32}", fcount);
                            }
                            SendResponse::SessionExpired => {
                                error!("radio: failed to send uplink, session expired");
                            }
                            SendResponse::NoAck => {
                                warn!("radio: uplink sent but no ack received");
                            }
                            SendResponse::RxComplete => {
                                info!("radio: uplink successfully sent and acknowledged by the gateway");
                            }
                        },
                        Err(_) => {
                            error!("failed to send uplink");
                        }
                    }
                }
                State::Run
            }
            State::Sleep => {
                Timer::after_secs(60 * 10).await;
                State::Auth
            }
        };
        ticker.next().await;
    }
}

async fn init_transceiver(c: Xcvr) -> Result<&'static mut Mutex<ThreadModeRawMutex, RadioDevice>, error::Error> {
    let nss = Output::new(c.pin_3.degrade(), Level::High);
    let reset = Output::new(c.pin_15.degrade(), Level::High);
    let dio1 = Input::new(c.pin_20.degrade(), Pull::None);
    let busy = Input::new(c.pin_2.degrade(), Pull::None);
    let spi = Spi::new(c.spi1, c.pin_10, c.pin_11, c.pin_12, c.dma_ch0, c.dma_ch1, Config::default());
    let spi_bus = ExclusiveDevice::new(spi, nss, Delay).unwrap();
    let sx1262_config = sx126x::Config {
        chip: Sx1262,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V7),
        use_dcdc: true,
        rx_boost: false,
    };
    let iv = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None).unwrap();
    let lora = LoRa::new(Sx126x::new(spi_bus, iv, sx1262_config), true, Delay).await.unwrap();
    let mut radio: LorawanRadio<_, _, config::Config::TEST::take> = lora.into();
    radio.set_rx_window_lead_time(config::Config::RX_WINDOW_LEAD_TIME);
    radio.set_rx_window_buffer(config::Config::RX_WINDOW_BUFFER);
    let region: region::Configuration = region::Configuration::new(config::Config::LORAWAN_REGION);
    let mut lora_radio: async_device::Device<_, Crypto, _, _> =
        async_device::Device::new(region, radio, EmbassyTimer::new(), embassy_rp::clocks::RoscRng);

    if let Err(_) = lora_radio.init().await {
        error!("radio: failed to initialize");
        return Err(error::Error::LoraRadio);
    } else {
        info!("radio: successfully initialized");
    };

    if let Err(_) = lora_radio.info().await {
        error!("radio: failed to read device information");
        return Err(error::Error::LoraRadio);
    };

    RADIO_CELL.try_init(Mutex::new(lora_radio)).ok_or(error::Error::LoraRadio)
}

async fn init_air_sensor(c: AirSen) -> Result<&'static mut Mutex<ThreadModeRawMutex, AirSensor<'static>>, error::Error> {
    let i2c_0_bus = I2c::new_async(c.i2c0, c.pin_17, c.pin_16, Irqs, i2c::Config::default());
    let i2c_0_bus_ref = AIR_SENSOR_I2C_BUS.init(i2c_0_bus);
    let mut air_sensor = air_sensor::AirSensor::new(config::Config::I2C_ADDR_AIR_SENSOR, i2c_0_bus_ref);

    match air_sensor.init().await {
        Ok(id) => {
            info!("air sensor: successfully initialized");
            info!("air sensor: serial number {=u64}", id);
        }
        Err(_) => {
            error!("air sensor: failed to initialize");
            return Err(error::Error::FailedToInitialize);
        }
    };

    match air_sensor.info().await {
        Ok(info) => {
            info!(
                "air sensor:
                serial_number: {=u64}",
                info.serial_number
            );
        }
        Err(_) => {
            error!("air sensor: failed to read device information");
            return Err(error::Error::FailedToInitialize);
        }
    };

    AIR_SENSOR_CELL.try_init(Mutex::new(air_sensor)).ok_or(error::Error::FailedToInitialize)
}

async fn init_soil_sensor(c: SoilSen) -> Result<&'static mut Mutex<ThreadModeRawMutex, SoilSensor<'static>>, error::Error> {
    let i2c_1_bus = I2c::new_async(c.i2c1, c.pin_19, c.pin_18, Irqs, i2c::Config::default());
    let i2c_1_bus_ref = SOIL_SENSOR_I2C_BUS.init(i2c_1_bus);
    let mut soil_sensor = soil_sensor::SoilSensor::new(config::Config::I2C_ADDR_SOIL_SENSOR, i2c_1_bus_ref);

    match soil_sensor.init().await {
        Ok(id) => {
            info!("soil sensor: successfully initialized");
            info!("soil sensor: serial number {=u16}", id);
        }
        Err(_) => {
            error!("soil sensor: failed to initialize");
            return Err(error::Error::FailedToInitialize);
        }
    };

    match soil_sensor.info().await {
        Ok(info) => {
            info!(
                "soil sensor:
                hardware id: {=u8}
                product code: {=u16}
                manufactoring date: {=u8}/{=u8}/{=u8}",
                info.hw_id, info.product_code, info.day, info.month, info.year
            );
        }
        Err(_) => {
            error!("soil sensor: failed to read device information");
            return Err(error::Error::FailedToInitialize);
        }
    };

    SOIL_SENSOR_CELL.try_init(Mutex::new(soil_sensor)).ok_or(error::Error::FailedToInitialize)
}
