## Building
```cargo build --release```
```arm-none-eabi-objcopy -O binary ../target/thumbv7em-none-eabihf/release/dev-board ../target/thumbv7em-none-eabihf/release/dev-board.bin```
```arm-none-eabi-objcopy -O ihex ../target/thumbv7em-none-eabihf/release/dev-board ../target/thumbv7em-none-eabihf/release/dev-board.hex```
```sudo st-flash write ../target/thumbv7em-none-eabihf/release/dev-board.bin 0x8000000```