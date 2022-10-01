use core::array;

use crate::packet;

/// This should only be used by the Controller in the Rack
pub struct Controller<const N: usize, Sel, Rc, Ser>
where
    Sel: Select<N>,
    Rc: ReadyCheck<N>,
{
    selector: Sel,
    ready: Rc,
    serial: Ser,

    extensions: [CtrlExtension; N],
}

/// Defines an interface to check if a specific Extension is ready
pub trait ReadyCheck<const N: usize> {
    /// Check the ready state of the Extension with the given index
    fn check(&self, idx: usize) -> bool;

    /// Check the ready state of all the Extensions
    fn check_all(&self) -> [bool; N];
}

/// Defines an interface to select a specific Extension
pub trait Select<const N: usize> {
    /// Select the Extension corresponding to the index
    fn select(&mut self, index: usize);
}

struct CtrlExtension {
    id: u8,
    initialized: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InitError<SE> {
    NBSerialError(nb::Error<SE>),
    SerialError(SE),
}

impl<const N: usize, Sel, Rc, Ser> Controller<N, Sel, Rc, Ser>
where
    Sel: Select<N>,
    Rc: ReadyCheck<N>,
    Ser: embedded_hal::serial::nb::Read + embedded_hal::serial::nb::Write,
{
    pub fn init(
        mut select: Sel,
        ready: Rc,
        mut serial: Ser,
    ) -> Result<Self, InitError<Ser::Error>> {
        let extension = array::from_fn(|idx| {
            if !ready.check(idx) {
                return CtrlExtension {
                    id: idx as u8,
                    initialized: false,
                };
            }

            // Select the correct line
            select.select(idx);

            let probe_packet = packet::Packet::init_probe();
            for byte in probe_packet.serialize() {
                loop {
                    if let Err(e) = serial.write(byte) {
                        match e {
                            nb::Error::WouldBlock => continue,
                            _ => panic!(""),
                        };
                    }
                }
            }
            serial.flush().unwrap();

            let mut buffer = [0; 256];
            let response = packet::Packet::read_blocking(&mut serial, &mut buffer).expect("");

            let (status, id) = match response.data {
                packet::PacketData::InitProbeResponse { status, id } => (status, id),
                _ => panic!(""),
            };

            match id {
                Some(id) => CtrlExtension {
                    id,
                    initialized: status,
                },
                None => CtrlExtension {
                    id: idx as u8,
                    initialized: false,
                },
            }
        });

        Ok(Self {
            selector: select,
            ready,
            serial,
            extensions: extension,
        })
    }
}
