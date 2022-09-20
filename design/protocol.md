# Protocol
Outlines the basics of the Protocol used between the Controller and the Extensions

## General
The Protocol is a simple Master/Slave(s) architecture build on top of RS485. This means that only
the Master/Controller can start communications and is basically responsible for managing who is
currently allowed to communicate over, so the Slaves/Extensions will only respond to messages from
the master, but not start their own.

## Controller
### Init
* Waits for the Ready-Line to be pulled to High by extension
* Pull the Selection-Line High for the extension
* Send a Registration Message with the new ID for the extension over serial/rs485
* Wait for an acknowledgement from the extension

## Extension
### Init
* Once the board is started up and ready to init communication, pull Ready-Line High
* Wait for a Registration Message and check if it is targeted (Selection Line High) (ignore otherwise)
* Store the new ID and change to initiliazed status
* Send acknowledgement to the Contoller over serial/rs485

## Packets
### General
1. Protocol-Version - 1 byte
2. Receiver ID - 1 byte (0x00 => Master, 0xff => everyone (init, etc.))
3. Packet Data - 253 bytes
4. CRC - 1 byte

### Packet Data
1. Packet Type - 1 byte
2. Type Specific Data - 252 bytes

#### Types
0. Init-Probe
1. Init-Probe Response
2. Init
3. Acknowledge
4. Error
5. Restart
6. Configure
7. Metrics
8. Metrics Response
9. Configure-Options
10. Configure-Options Response

#### Init-Probe Packet-Data
Empty

#### Init-Probe Response Packet-Data
1. Status (0 => false, everything else => true)
2. ID (Optional, only considered in case Status == true)

#### Init Packet-Data
1. The ID for the selected extension

#### Acknowledge Packet-Data
Empty