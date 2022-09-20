use core::convert::TryInto;

use crate::{ConfigOption, DataPoint, OptionsIter, Sendable, Value, VERSION};

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiverID {
    Controller,
    Everyone,
    ID(u8),
}

impl From<u8> for ReceiverID {
    fn from(raw: u8) -> Self {
        match raw {
            0x00 => Self::Controller,
            0xff => Self::Everyone,
            id => Self::ID(id),
        }
    }
}
impl From<ReceiverID> for u8 {
    fn from(id: ReceiverID) -> Self {
        match id {
            ReceiverID::Controller => 0x00,
            ReceiverID::Everyone => 0xff,
            ReceiverID::ID(id) => id,
        }
    }
}
impl From<&ReceiverID> for u8 {
    fn from(id: &ReceiverID) -> Self {
        match id {
            ReceiverID::Controller => 0x00,
            ReceiverID::Everyone => 0xff,
            ReceiverID::ID(id) => *id,
        }
    }
}

pub struct Packet<'r> {
    pub(crate) protocol_version: u8,
    pub(crate) receiver: ReceiverID,
    pub(crate) data: PacketData<'r>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PacketData<'r> {
    InitProbe,
    InitProbeResponse {
        status: bool,
        id: Option<u8>,
    },
    Init {
        id: u8,
    },
    Acknowledge,
    Error {},
    Restart,
    Configure {
        option: DataPoint<'r>,
    },
    Metrics,
    MetricsResponse {
        metrics: OptionsIter<'r, DataPoint<'r>>,
    },
    ConfigureOptions,
    ConfigureOptionsResponse {
        options: OptionsIter<'r, ConfigOption<'r>>,
    },
}

#[derive(Debug)]
pub enum PacketDataParseError {
    UnknownID(u8),
}

impl<'r> PacketData<'r> {
    pub fn parse<'b>(prot_version: u8, value: &'b [u8; 253]) -> Result<Self, PacketDataParseError>
    where
        'b: 'r,
    {
        let ptype_id = value[0];

        match ptype_id {
            0 => Ok(Self::InitProbe),
            1 => {
                let status = value[1] != 0;

                let id = if status { Some(value[2]) } else { None };

                Ok(Self::InitProbeResponse { status, id })
            }
            2 => {
                let n_id = value[1];
                Ok(Self::Init { id: n_id })
            }
            3 => Ok(Self::Acknowledge),
            4 => {
                todo!("Parse Error Packet")
            }
            5 => Ok(Self::Restart),
            6 => {
                let (name, rest): (&str, _) = Sendable::deserialize(&value[1..]).unwrap();

                let value = Value::deserialize((&rest[..2]).try_into().unwrap()).unwrap();

                Ok(Self::Configure {
                    option: DataPoint { name, value },
                })
            }
            7 => Ok(Self::Metrics),
            8 => {
                let (metrics, rest) = Sendable::deserialize(&value[1..]).unwrap();

                Ok(Self::MetricsResponse { metrics })
            }
            9 => Ok(Self::ConfigureOptions),
            10 => {
                let (options, rest) = Sendable::deserialize(&value[1..]).unwrap();

                Ok(Self::ConfigureOptionsResponse { options })
            }
            id => Err(PacketDataParseError::UnknownID(id)),
        }
    }

    pub fn serialize(&self, data: &mut [u8; 253]) {
        match self {
            Self::InitProbe => {
                data[0] = 0;
            }
            Self::InitProbeResponse { status, id } => {
                data[0] = 1;
                data[1] = if *status { 1 } else { 0 };
                data[2] = id.unwrap_or(0);
            }
            Self::Init { id } => {
                data[0] = 2;
                data[1] = *id;
            }
            Self::Acknowledge => {
                data[0] = 3;
            }
            Self::Error {} => todo!("Serialize Error"),
            Self::Restart => {
                data[0] = 5;
            }
            Self::Configure { option } => {
                data[0] = 6;

                let rest = option.name.serialize(&mut data[1..]).unwrap();

                rest[0..2].copy_from_slice(&option.value.serialize());
            }
            Self::Metrics => {
                data[0] = 7;
            }
            Self::MetricsResponse { metrics } => {
                data[0] = 8;

                let buffer = metrics.serialize(&mut data[1..]).unwrap();
            }
            Self::ConfigureOptions => {
                data[0] = 9;
            }
            Self::ConfigureOptionsResponse { options } => {
                data[0] = 10;

                let buffer = options.serialize(&mut data[1..]).unwrap();
            }
        }
    }
}

pub enum PacketReadError {
    SerialRead,
    Checksum,
}

impl<'r> Packet<'r> {
    pub fn init_probe() -> Self {
        Self {
            protocol_version: VERSION,
            receiver: ReceiverID::Everyone,
            data: PacketData::InitProbe,
        }
    }

    pub fn ack(recv: ReceiverID) -> Self {
        Self {
            protocol_version: VERSION,
            receiver: recv,
            data: PacketData::Acknowledge,
        }
    }

    pub fn read_blocking<'b, S>(serial: &mut S, buffer: &'b mut [u8; 256]) -> Result<Self, ()>
    where
        'b: 'r,
        S: embedded_hal::serial::nb::Read,
    {
        for buffer_entry in buffer.iter_mut() {
            loop {
                match serial.read() {
                    Ok(d) => {
                        *buffer_entry = d;
                    }
                    Err(nb::Error::WouldBlock) => continue,
                    Err(err) => {
                        return Err(());
                    }
                };
                break;
            }
        }

        Self::deserialize(buffer)
    }

    pub fn deserialize<'b>(buffer: &'b [u8; 256]) -> Result<Self, ()>
    where
        'b: 'r,
    {
        let protocol_version = buffer[0];
        let raw_receiver_id = buffer[1];
        let raw_data: &'b [u8; 253] = (&buffer[2..255])
            .try_into()
            .expect("We always select a 253 byte sized area");
        let crc = buffer[255];

        // TODO
        // Validate the Packet with the CRC

        let receiver_id: ReceiverID = raw_receiver_id.into();
        let packet_data = PacketData::parse(protocol_version, &raw_data).map_err(|err| ())?;

        Ok(Self {
            protocol_version,
            receiver: receiver_id,
            data: packet_data,
        })
    }

    pub fn serialize(&self) -> [u8; 256] {
        let mut buffer = [0; 256];

        buffer[0] = VERSION;
        buffer[1] = (&self.receiver).into();

        self.data
            .serialize((&mut buffer[2..255]).try_into().unwrap());

        // TODO
        // Calculate CRC
        let crc = 0;
        buffer[255] = crc;

        buffer
    }

    pub fn receiver(&self) -> &ReceiverID {
        &self.receiver
    }
    pub fn data(&self) -> &PacketData {
        &self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_data_init_probe() {
        let data: [u8; 253] = {
            let mut raw = vec![0];
            raw.resize_with(253, || 0);
            raw.try_into().unwrap()
        };

        let result = PacketData::parse(0, &data).expect("Should work");

        assert_eq!(PacketData::InitProbe, result);
    }

    #[test]
    fn packet_data_init_probe_response_false() {
        let data: [u8; 253] = {
            let mut raw = vec![1, 0, 13];
            raw.resize_with(253, || 0);
            raw.try_into().unwrap()
        };

        let result = PacketData::parse(0, &data).expect("Should work");

        assert_eq!(
            PacketData::InitProbeResponse {
                status: false,
                id: None
            },
            result
        );
    }
    #[test]
    fn packet_data_init_probe_response_true() {
        let data: [u8; 253] = {
            let mut raw = vec![1, 1, 13];
            raw.resize_with(253, || 0);
            raw.try_into().unwrap()
        };

        let result = PacketData::parse(0, &data).expect("Should work");

        assert_eq!(
            PacketData::InitProbeResponse {
                status: true,
                id: Some(13)
            },
            result
        );
    }

    #[test]
    fn packet_data_init() {
        let data: [u8; 253] = {
            let mut raw = vec![2, 123];
            raw.resize_with(253, || 0);
            raw.try_into().unwrap()
        };

        let result = PacketData::parse(0, &data).expect("Should be parseable");

        assert_eq!(PacketData::Init { id: 123 }, result);
    }

    #[test]
    fn packet_data_acknowledge() {
        let data: [u8; 253] = {
            let mut raw = vec![3];
            raw.resize_with(253, || 0);
            raw.try_into().unwrap()
        };

        let result = PacketData::parse(0, &data).expect("Should work");

        assert_eq!(PacketData::Acknowledge, result);
    }

    #[test]
    fn packet_metrics_response() {}
}
