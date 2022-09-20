use core::array;

use crate::{packet, VERSION};

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

pub trait ReadyCheck<const N: usize> {
    fn check(&self, idx: usize) -> bool;

    fn check_all(&self) -> [bool; N];
}

pub trait Select<const N: usize> {
    fn select(&mut self, index: usize);
}

struct CtrlExtension {
    id: u8,
    initialized: bool,
}

impl<const N: usize, Sel, Rc, Ser> Controller<N, Sel, Rc, Ser>
where
    Sel: Select<N>,
    Rc: ReadyCheck<N>,
    Ser: embedded_hal::serial::nb::Read + embedded_hal::serial::nb::Write,
{
    pub fn init(mut select: Sel, ready: Rc, mut serial: Ser) -> Result<Self, ()> {
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
            serial.flush();

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
