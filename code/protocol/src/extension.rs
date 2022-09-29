use general::AsyncSerial;

use crate::{packet, ConfigOption, DataPoint, VERSION};

/// This should be used by every Extension Board
pub struct Extension<R, Sel, Ser> {
    ready_pin: R,
    selection_pin: Sel,
    serial: Ser,
    /// The ID of the Extension
    id: u8,
}

#[derive(Debug)]
pub enum ExtensionInitError<RE> {
    ReadyError(RE),
    ReadingSerial,
    WritingSerial,
}

impl<R, Sel, Ser> Extension<R, Sel, Ser>
where
    R: embedded_hal::digital::blocking::OutputPin,
    Sel: embedded_hal::digital::blocking::InputPin,
    Ser: embedded_hal::serial::nb::Read<u8> + embedded_hal::serial::nb::Write<u8>,
{
    pub fn init(
        mut ready: R,
        selection: Sel,
        mut serial: Ser,
    ) -> Result<Self, ExtensionInitError<R::Error>> {
        ready.set_high().map_err(ExtensionInitError::ReadyError)?;

        let id = loop {
            let mut buffer = [0; 256];
            let packet = packet::Packet::read_blocking(&mut serial, &mut buffer)
                .map_err(|err| ExtensionInitError::ReadingSerial)?;

            // If we are not selected, we will not react to the packet
            if !selection.is_high().unwrap_or(false)
                || packet.receiver != packet::ReceiverID::Everyone
            {
                continue;
            }

            match &packet.data {
                packet::PacketData::InitProbe => {
                    // We are still initialising, so in case the Controller asks about our init status
                    // we respond that we are not initialized and have no ID

                    let response = packet::Packet {
                        protocol_version: VERSION,
                        receiver: packet::ReceiverID::Controller,
                        data: packet::PacketData::InitProbeResponse {
                            status: false,
                            id: None,
                        },
                    };
                    let response_data = response.serialize();

                    for byte in response_data {
                        loop {
                            if let Err(e) = serial.write(byte) {
                                match e {
                                    nb::Error::WouldBlock => continue,
                                    err => return Err(ExtensionInitError::WritingSerial),
                                };
                            }
                        }
                    }
                    serial.flush();

                    continue;
                }
                packet::PacketData::Init { id } => {
                    // We just got initialised, so we will accept the provided ID and send an
                    // acknowledgement

                    let response = packet::Packet::ack(packet::ReceiverID::Controller);

                    for byte in response.serialize() {
                        serial
                            .write(byte)
                            .map_err(|err| ExtensionInitError::WritingSerial)?;
                    }
                    serial.flush();

                    break *id;
                }
                _ => {
                    panic!("");
                }
            };
        };

        Ok(Self {
            ready_pin: ready,
            selection_pin: selection,
            serial,
            id,
        })
    }

    pub async fn run<const MC: usize, M, C, ASer>(
        mut self,
        mut metrics: M,
        mut configure: C,
        config_options: &'static [ConfigOption<'static>],
        to_async_serial: impl FnOnce(Ser) -> ASer,
    ) where
        M: FnMut() -> [DataPoint<'static>; MC],
        C: FnMut(DataPoint<'_>),
        ASer: AsyncSerial<256>,
    {
        let mut async_serial = to_async_serial(self.serial);

        loop {
            let buffer = async_serial.read().await;
            let recv_packet = packet::Packet::deserialize(&buffer).unwrap();

            match recv_packet.receiver {
                packet::ReceiverID::Everyone if self.selection_pin.is_high().unwrap_or(false) => {}
                packet::ReceiverID::ID(id) if id == self.id => {}
                _ => continue,
            };

            match recv_packet.data {
                // All the Packets that we will just ignore and return an error for
                packet::PacketData::Init { .. }
                | packet::PacketData::InitProbeResponse { .. }
                | packet::PacketData::Acknowledge
                | packet::PacketData::Error {}
                | packet::PacketData::MetricsResponse { .. }
                | packet::PacketData::ConfigureOptionsResponse { .. } => {
                    todo!("Send Error Response")
                }
                packet::PacketData::InitProbe => {
                    let probe_response = packet::Packet {
                        protocol_version: VERSION,
                        receiver: packet::ReceiverID::Controller,
                        data: packet::PacketData::InitProbeResponse {
                            status: true,
                            id: Some(self.id),
                        },
                    };
                    let buffer = probe_response.serialize();

                    async_serial.write(buffer).await;
                }
                packet::PacketData::Restart => {
                    self.ready_pin.set_low();
                    return;
                }
                packet::PacketData::Configure { option } => {
                    configure(option);

                    let ack_packet = packet::Packet::ack(packet::ReceiverID::Controller);
                    async_serial.write(ack_packet.serialize()).await;
                }
                packet::PacketData::Metrics => {
                    let data = metrics();

                    let metrics_packet = packet::Packet {
                        protocol_version: VERSION,
                        receiver: packet::ReceiverID::Controller,
                        data: packet::PacketData::MetricsResponse {
                            metrics: (&data).into(),
                        },
                    };

                    async_serial.write(metrics_packet.serialize()).await;
                }
                packet::PacketData::ConfigureOptions => {
                    let opts_packet = packet::Packet {
                        protocol_version: VERSION,
                        receiver: packet::ReceiverID::Controller,
                        data: packet::PacketData::ConfigureOptionsResponse {
                            options: config_options.into(),
                        },
                    };

                    async_serial.write(opts_packet.serialize()).await;
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        packet::{Packet, PacketData, ReceiverID},
        ConfigOption, OptionsIter, Value, ValueType,
    };

    use super::*;

    use embedded_hal_mock::{
        pin::{
            Mock as PinMock, State as PinState, Transaction as PinTransaction,
            TransactionKind as PinTransactionKind,
        },
        serial::{Mock as SerialMock, Transaction as SerialTransaction},
    };

    fn init_extension<'r, 'sel, 'ser>(
        id: u8,
        ready: &'r mut PinMock,
        selection: &'sel mut PinMock,
        serial: &'ser mut SerialMock<u8>,
    ) -> Extension<&'r mut PinMock, &'sel PinMock, &'ser mut SerialMock<u8>> {
        ready.expect(&[PinTransaction::new(PinTransactionKind::Set(PinState::High))]);
        selection.expect(&[PinTransaction::new(PinTransactionKind::Get(PinState::High))]);

        let mut expectations = vec![];

        let init_packet = Packet {
            protocol_version: VERSION,
            receiver: ReceiverID::Everyone,
            data: PacketData::Init { id },
        };
        expectations.extend(
            init_packet
                .serialize()
                .into_iter()
                .map(SerialTransaction::read),
        );

        let ack_packet = Packet {
            protocol_version: VERSION,
            receiver: ReceiverID::Controller,
            data: PacketData::Acknowledge,
        };
        expectations.extend(
            ack_packet
                .serialize()
                .into_iter()
                .map(SerialTransaction::write),
        );
        expectations.push(SerialTransaction::flush());

        serial.expect(&expectations);

        let ext = Extension::init(ready, selection as &'sel PinMock, serial).expect("Should work");

        assert_eq!(id, ext.id);

        ext
    }

    #[test]
    fn init_extension_selected() {
        let mut ready =
            PinMock::new(&[PinTransaction::new(PinTransactionKind::Set(PinState::High))]);
        let mut selection =
            PinMock::new(&[PinTransaction::new(PinTransactionKind::Get(PinState::High))]);

        let mut serial = {
            let mut expectations = vec![];

            let init_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::Everyone,
                data: PacketData::Init { id: 13 },
            };
            expectations.extend(
                init_packet
                    .serialize()
                    .into_iter()
                    .map(SerialTransaction::read),
            );

            let ack_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::Controller,
                data: PacketData::Acknowledge,
            };
            expectations.extend(
                ack_packet
                    .serialize()
                    .into_iter()
                    .map(SerialTransaction::write),
            );
            expectations.push(SerialTransaction::flush());

            SerialMock::new(&expectations)
        };

        let ext = Extension::init(&mut ready, &selection, &mut serial).expect("Should work");

        assert_eq!(13, ext.id);

        ready.done();
        selection.done();
        serial.done();
    }

    #[test]
    fn init_extension_unselected_selected() {
        let mut ready =
            PinMock::new(&[PinTransaction::new(PinTransactionKind::Set(PinState::High))]);
        let mut selection = PinMock::new(&[
            PinTransaction::new(PinTransactionKind::Get(PinState::Low)),
            PinTransaction::new(PinTransactionKind::Get(PinState::High)),
        ]);

        let mut serial = {
            let mut expectations = vec![];

            let init_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::Everyone,
                data: PacketData::Init { id: 12 },
            };
            expectations.extend(
                init_packet
                    .serialize()
                    .into_iter()
                    .map(SerialTransaction::read),
            );

            let init_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::Everyone,
                data: PacketData::Init { id: 13 },
            };
            expectations.extend(
                init_packet
                    .serialize()
                    .into_iter()
                    .map(SerialTransaction::read),
            );

            let ack_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::Controller,
                data: PacketData::Acknowledge,
            };
            expectations.extend(
                ack_packet
                    .serialize()
                    .into_iter()
                    .map(SerialTransaction::write),
            );
            expectations.push(SerialTransaction::flush());

            SerialMock::new(&expectations)
        };

        let ext = Extension::init(&mut ready, &selection, &mut serial).expect("Should work");

        assert_eq!(13, ext.id);

        ready.done();
        selection.done();
        serial.done();
    }

    #[test]
    fn run_restart() {
        let mut ready = PinMock::new(&[]);
        let mut selection = PinMock::new(&[]);
        let mut serial = SerialMock::new(&[]);

        let extension = init_extension(13, &mut ready, &mut selection, &mut serial);

        extension
            .ready_pin
            .expect(&[PinTransaction::new(PinTransactionKind::Set(PinState::Low))]);

        let mut async_serial = general::mocks::MockSerial::new();
        {
            let restart_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::ID(13),
                data: PacketData::Restart,
            };
            async_serial.read(restart_packet.serialize());
        }

        let run_fut = extension.run(|| [], |_| {}, &[], |_| &mut async_serial);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(run_fut);

        async_serial.assert_outstanding();
    }

    #[test]
    fn run_configure() {
        let mut ready = PinMock::new(&[]);
        let mut selection = PinMock::new(&[]);
        let mut serial = SerialMock::new(&[]);

        let extension = init_extension(13, &mut ready, &mut selection, &mut serial);

        extension
            .ready_pin
            .expect(&[PinTransaction::new(PinTransactionKind::Set(PinState::Low))]);

        let mut async_serial = general::mocks::MockSerial::new();
        {
            let config_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::ID(13),
                data: PacketData::Configure {
                    option: DataPoint {
                        name: "testing",
                        value: Value::Switch { state: true },
                    },
                },
            };
            async_serial.read(config_packet.serialize());

            let ack_packet = Packet::ack(ReceiverID::Controller);
            async_serial.write(ack_packet.serialize());

            let restart_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::ID(13),
                data: PacketData::Restart,
            };
            async_serial.read(restart_packet.serialize());
        }

        let run_fut = extension.run(|| [], |_| {}, &[], |_| &mut async_serial);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(run_fut);

        async_serial.assert_outstanding();
    }

    #[test]
    fn run_configure_options() {
        let mut ready = PinMock::new(&[]);
        let mut selection = PinMock::new(&[]);
        let mut serial = SerialMock::new(&[]);

        let extension = init_extension(13, &mut ready, &mut selection, &mut serial);

        extension
            .ready_pin
            .expect(&[PinTransaction::new(PinTransactionKind::Set(PinState::Low))]);

        let mut async_serial = general::mocks::MockSerial::new();
        {
            let opts_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::ID(13),
                data: PacketData::ConfigureOptions,
            };
            async_serial.read(opts_packet.serialize());

            let opts_response_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::Controller,
                data: PacketData::ConfigureOptionsResponse {
                    options: OptionsIter::from(&[ConfigOption {
                        name: "testing",
                        ty: ValueType::Switch,
                    }]),
                },
            };
            async_serial.write(opts_response_packet.serialize());

            let restart_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::ID(13),
                data: PacketData::Restart,
            };
            async_serial.read(restart_packet.serialize());
        }

        let run_fut = extension.run(
            || [],
            |_| {},
            &[ConfigOption {
                name: "testing",
                ty: ValueType::Switch,
            }],
            |_| &mut async_serial,
        );

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(run_fut);

        async_serial.assert_outstanding();
    }

    #[test]
    fn run_metrics() {
        let mut ready = PinMock::new(&[]);
        let mut selection = PinMock::new(&[]);
        let mut serial = SerialMock::new(&[]);

        let extension = init_extension(13, &mut ready, &mut selection, &mut serial);

        extension
            .ready_pin
            .expect(&[PinTransaction::new(PinTransactionKind::Set(PinState::Low))]);

        let mut async_serial = general::mocks::MockSerial::new();
        {
            let metrics_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::ID(13),
                data: PacketData::Metrics,
            };
            async_serial.read(metrics_packet.serialize());

            let metrics_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::Controller,
                data: PacketData::MetricsResponse {
                    metrics: OptionsIter::from(&[DataPoint {
                        name: "testing",
                        value: Value::Pwm { percent: 10 },
                    }]),
                },
            };
            async_serial.write(metrics_packet.serialize());

            let restart_packet = Packet {
                protocol_version: VERSION,
                receiver: ReceiverID::ID(13),
                data: PacketData::Restart,
            };
            async_serial.read(restart_packet.serialize());
        }

        let run_fut = extension.run(
            || {
                [DataPoint {
                    name: "testing",
                    value: Value::Pwm { percent: 10 },
                }]
            },
            |_| {},
            &[],
            |_| &mut async_serial,
        );

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(run_fut);

        async_serial.assert_outstanding();
    }
}
