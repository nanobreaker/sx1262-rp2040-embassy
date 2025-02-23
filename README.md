# SX1262 LoRaWAN Sensor Node with RP2040 and Embassy Framework

This project implements a LoRaWAN sensor node using the SX1262 transceiver and RP2040 microcontroller, leveraging the Embassy framework for efficient, asynchronous embedded programming. The node collects data from air (temperature, humidity, CO2) and soil (temperature, moisture) sensors, encodes it using Cayenne LPP, and transmits it over LoRaWAN. It's designed for low-power, long-range IoT applications.

## Features
- **LoRaWAN Communication**: Long-range, low-power wireless communication using the SX1262 transceiver.
- **RP2040 Microcontroller**: Efficient, dual-core processing for embedded applications.
- **Embassy Framework**: Asynchronous, non-blocking task management for optimal performance.
- **Sensor Integration**: Collects data from air and soil sensors.
- **Cayenne LPP Encoding**: Standardized payload format for easy integration with IoT platforms.
- **Low-Power Design**: Optimized for battery-powered operation (battery level reading planned).

## Getting Started

### Prerequisites
- Raspberry Pi Pico (RP2040-based board)
- SX1262 LoRa module
- I2C sensors for air and soil data
- Rust toolchain with `thumbv6m-none-eabi` target
- `probe-rs` for flashing and debugging
- LoRaWAN network server (e.g., ChirpStack) for gateway and device management

### Installation
1. Clone the repository:
  ```bash
  git clone https://github.com/nanobreaker/sx1262-rp2040-embassy.git
  cd sx1262-rp2040-embassy
  ```
2. Set up the Rust environment:
  ```bash
  rustup target add thumbv6m-none-eabi
  ```
3. Build the project:
  ```bash
  cargo build
  ```
4. Flash the firmware using `probe-rs`:
  ```bash
  probe-rs run --chip RP2040 target/thumbv6m-none-eabi/debug/rp2040
  ```

## Usage
1. **Configure Sensors**: Update `config.rs` with your I2C addresses and LoRaWAN credentials.
2. **Run the Application**: After flashing, the device will automatically start collecting sensor data and sending uplinks over LoRaWAN.
   ```bash
   cargo run
   ```
3. **Monitor Logs**: Use `defmt` with `probe-rs` to view logs:
4. **Decode Payloads**: Use a Cayenne LPP decoder on your LoRaWAN network server to interpret the sensor data.

## Contributing
- Fork the repository and create a new branch for your feature or bugfix.
- Ensure your code follows Rust's embedded best practices and Embassy's guidelines.
- Submit a pull request with a clear description of your changes.

## License
This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
