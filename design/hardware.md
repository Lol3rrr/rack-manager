# Hardware

## Controller-Extension Communication
Each extension is connected to the controller with 6 Wires.
1. 12V to supply the extension with power
2. GND
3. Ready-Line (5V), pulled to GND on the side of the Controller
4. Selection-Line (5V), pulled to GND on the side of the Controller
5. RS485 - 1
6. RS485 - 2

### Line 3.
* 10k Ohm Pull-Down resistor
* 100 Ohm Current limiting resistor

### Line 4.
* 10k Ohm Pull-Down resistor
* 100 Ohm Current limiting resistor