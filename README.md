# ESP Radar

ESP32-S3 flight radar for the LilyGO T-Embed.

## Requirements

- Rust toolchain from [espup](https://github.com/esp-rs/espup)
- `espflash`
- ESP32-S3 board connected by USB

Install the tools if needed:

```sh
cargo install espup espflash
espup install
```

Load the Espressif toolchain environment in every new shell:

```sh
source ~/export-esp.sh
```

## Build

Debug build:

```sh
cargo build
```

Release build and firmware image:

```sh
cargo build --release
mkdir -p firmware
espflash save-image --chip esp32s3 --merge \
  target/xtensa-esp32s3-none-elf/release/esp-radar \
  firmware/esp-radar.bin
```

The resulting image is `firmware/esp-radar.bin` and can be shared or flashed later.

## Flash and monitor

Flash the board and view serial logs:

```sh
espflash flash --chip esp32s3 --monitor firmware/esp-radar.bin
```

Alternatively, build and flash directly through Cargo:

```sh
cargo run --release
```

On first boot, configure Wi-Fi through the device setup access point shown on the display.
