#![no_std]
#![no_main]

mod air_sensor;
mod board;
mod config;
mod device;
mod error;
mod sensor;
mod soil_sensor;
mod transceiver;

use air_sensor::AirSensor;
use assign_resources::assign_resources;
use board::{Board, BoardBuilder};
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

static RADIO_CELL: StaticCell<Mutex<ThreadModeRawMutex, Option<crate::transceiver::RadioDevice>>> = StaticCell::new();
static RADIO_GUARD: OnceLock<Mutex<ThreadModeRawMutex, crate::transceiver::RadioDevice>> = OnceLock::new();
static AIR_SENSOR_I2C_BUS: StaticCell<I2c<'static, I2C0, i2c::Async>> = StaticCell::new();
static AIR_SENSOR_GUARD: OnceLock<Mutex<ThreadModeRawMutex, AirSensor>> = OnceLock::new();
static SOIL_SENSOR_I2C_BUS: StaticCell<I2c<'static, I2C1, i2c::Async>> = StaticCell::new();
static SOIL_SENSOR_GUARD: OnceLock<Mutex<ThreadModeRawMutex, SoilSensor>> = OnceLock::new();
static BOARD_GUARD: OnceLock<Mutex<ThreadModeRawMutex, Board>> = OnceLock::new();

bind_interrupts!(struct Irqs {
    ADC_IRQ_FIFO => embassy_rp::adc::InterruptHandler;
    I2C0_IRQ => embassy_rp::i2c::InterruptHandler<I2C0>;
    I2C1_IRQ => embassy_rp::i2c::InterruptHandler<I2C1>;
});

assign_resources! {
    vbus: Vbus {
        pin_24: PIN_24,
    },
    vsys: Vsys {
        adc: ADC,
        adc_temp_sensor: ADC_TEMP_SENSOR,
        pin_26: PIN_26,
        pin_29: PIN_29,
    },
    sen_1: Sen1 {
        pin_16: PIN_16,
        pin_17: PIN_17,
        i2c0: I2C0,
    },
    sen_2: Sen2 {
        i2c1: I2C1,
        pin_19: PIN_19,
        pin_18: PIN_18,
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
        spi_1: SPI1,
    }
}

enum State {
    Register, // join lorawan network
    Run,      // collect environment data and send over uplink
    Sleep,    // do nothing
}

#[embassy_executor::main]
async fn main(s: Spawner) {
    let p = embassy_rp::init(Default::default());
    let r = split_resources! {p};

    let board_builder = BoardBuilder::new();

    if let Ok(radio) = init_lora(r.xcvr).await {
        board_builder.with_radio(radio);
    };

    s.spawn(init_board(p.ADC.into_ref(), p.ADC_TEMP_SENSOR.into_ref(), p.PIN_26.into_ref()))
        .expect("must");
    s.spawn(init_air_sensor(p.I2C0, p.PIN_17, p.PIN_16)).expect("must");
    s.spawn(init_soil_sensor(p.I2C1, p.PIN_19, p.PIN_18)).expect("must");

    let mut ticker = Ticker::every(Duration::from_secs(60 * 5));
    let mut state = State::Register;
    let mut join_counter: u8 = 0;
    loop {
        state = match state {
            State::Register => {
                let radio_mutex = RADIO_GUARD.get().await;
                let mut radio = radio_mutex.lock().await;
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
                            State::Register
                        }
                    }
                }
            }
            State::Run => {
                let board_mtx = BOARD_GUARD.get().await;
                let mut board = board_mtx.lock().await;

                let air_sensor_mtx = AIR_SENSOR_GUARD.get().await;
                let mut air_sensor = air_sensor_mtx.lock().await;

                let soil_sensor_mtx = SOIL_SENSOR_GUARD.get().await;
                let mut soil_sensor = soil_sensor_mtx.lock().await;

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
                State::Register
            }
        };
        ticker.next().await;
    }
}

#[embassy_executor::task]
async fn init_board(
    adc: PeripheralRef<'static, ADC>,
    adc_temp_sensor: PeripheralRef<'static, ADC_TEMP_SENSOR>,
    pin_26: PeripheralRef<'static, PIN_26>,
) {
    let adc = Adc::new(adc, Irqs, Default::default());
    let tmp_ctrl = adc::Channel::new_temp_sensor(adc_temp_sensor);
    let btr_ctrl = adc::Channel::new_pin(pin_26, Pull::None);
    let mut board = Board { adc, tmp_ctrl, btr_ctrl };

    if let Err(_) = board.init().await {
        error!("rp2040: failed to initialize");
        return;
    } else {
        info!("rp2040: successfully initialized");
    };

    if let Err(_) = board.info().await {
        error!("rp2040: failed to read board information");
        return;
    };

    if let Err(_) = BOARD_GUARD.init(Mutex::new(board)) {
        error!("rp2040: already initialized!");
    };
}

async fn init_lora(c: Xcvr) -> Result<&'static mut Mutex<ThreadModeRawMutex, Option<RadioDevice>>, error::Error> {
    let nss = Output::new(c.pin_3.degrade(), Level::High);
    let reset = Output::new(c.pin_15.degrade(), Level::High);
    let dio1 = Input::new(c.pin_20.degrade(), Pull::None);
    let busy = Input::new(c.pin_2.degrade(), Pull::None);
    let spi = Spi::new(c.spi_1, c.pin_10, c.pin_11, c.pin_12, c.dma_ch0, c.dma_ch1, Config::default());
    let spi_bus = ExclusiveDevice::new(spi, nss, Delay).unwrap();
    let sx1262_config = sx126x::Config {
        chip: Sx1262,
        tcxo_ctrl: Some(TcxoCtrlVoltage::Ctrl1V7),
        use_dcdc: true,
        rx_boost: false,
    };
    let iv = GenericSx126xInterfaceVariant::new(reset, dio1, busy, None, None).unwrap();
    let lora = LoRa::new(Sx126x::new(spi_bus, iv, sx1262_config), true, Delay).await.unwrap();
    let mut radio: LorawanRadio<_, _, 14> = lora.into();
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

    RADIO_CELL.try_init(Mutex::new(Some(lora_radio))).ok_or(error::Error::LoraRadio)
}

#[embassy_executor::task]
async fn init_air_sensor(i2c0: I2C0, pin_17: PIN_17, pin_16: PIN_16) {
    let i2c_0_bus = I2c::new_async(i2c0, pin_17, pin_16, Irqs, i2c::Config::default());
    let i2c_0_bus_ref = AIR_SENSOR_I2C_BUS.init(i2c_0_bus);
    let mut air_sensor = air_sensor::AirSensor::new(config::Config::I2C_ADDR_AIR_SENSOR, i2c_0_bus_ref);

    match air_sensor.init().await {
        Ok(id) => {
            info!("air sensor: successfully initialized");
            info!("air sensor: serial number {=u64}", id);
        }
        Err(_) => {
            error!("air sensor: failed to initialize");
            return;
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
            return;
        }
    };

    if let Err(_) = AIR_SENSOR_GUARD.init(Mutex::new(air_sensor)) {
        error!("air sensor: already initialized!");
    };
}

#[embassy_executor::task]
async fn init_soil_sensor(i2c1: I2C1, pin_19: PIN_19, pin_18: PIN_18) {
    let i2c_1_bus = I2c::new_async(i2c1, pin_19, pin_18, Irqs, i2c::Config::default());
    let i2c_1_bus_ref = SOIL_SENSOR_I2C_BUS.init(i2c_1_bus);
    let mut soil_sensor = soil_sensor::SoilSensor::new(config::Config::I2C_ADDR_SOIL_SENSOR, i2c_1_bus_ref);

    match soil_sensor.init().await {
        Ok(id) => {
            info!("soil sensor: successfully initialized");
            info!("soil sensor: serial number {=u16}", id);
        }
        Err(_) => {
            error!("soil sensor: failed to initialize");
            return;
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
            return;
        }
    };

    if let Err(_) = SOIL_SENSOR_GUARD.init(Mutex::new(soil_sensor)) {
        error!("soil sensor: already initialized!");
    };
}
