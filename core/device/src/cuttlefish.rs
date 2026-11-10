// SPDX-License-Identifier: GPL-3.0-or-later
//! The host's side of what the stock Android virtual-device ("Cuttlefish")
//! image expects from its host.
//!
//! The image's hardware services talk to host helpers over console ports,
//! one port per helper. gameDroid uses the same image, so it has to hold up
//! the host's end where a service will not start without an answer.
//!
//! The wire formats here follow Cuttlefish's host tools as published in
//! <https://github.com/google/android-cuttlefish> (read at e3b764a,
//! `common/libs/transport/channel.h` and
//! `host/commands/sensors_simulator/`). Nothing is copied from there; the
//! formats are re-implemented.

/// Which console port (`/dev/hvcN` in the guest) carries what, in the order
/// the image expects them. Taken from a stock boot under crosvm, recorded in
/// `docs/engine/reference/raw/crosvm-args.txt`.
pub const CONSOLE_PORTS: usize = 20;
/// The sensors service asks which sensors exist on this port.
pub const SENSORS_CONTROL_PORT: usize = 18;
/// Sensor readings are sent to the guest on this port.
pub const SENSORS_DATA_PORT: usize = 19;

/// Sensor numbers, shared with the guest's sensor service.
pub mod sensor {
    pub const ACCELERATION: u32 = 0;
    pub const GYROSCOPE: u32 = 1;
    pub const MAGNETIC: u32 = 2;
    pub const TEMPERATURE: u32 = 4;
    pub const PROXIMITY: u32 = 5;
    pub const LIGHT: u32 = 6;
    pub const PRESSURE: u32 = 7;
    pub const HUMIDITY: u32 = 8;
}

/// The sensors a phone-shaped virtual device has, as a bit per sensor number.
pub const PHONE_SENSORS: u32 = 1 << sensor::ACCELERATION
    | 1 << sensor::GYROSCOPE
    | 1 << sensor::MAGNETIC
    | 1 << sensor::TEMPERATURE
    | 1 << sensor::PROXIMITY
    | 1 << sensor::LIGHT
    | 1 << sensor::PRESSURE
    | 1 << sensor::HUMIDITY;

/// Command number for "here is something for the sensor service".
const CMD_UPDATE_HAL: u32 = 2;
/// Messages longer than this are not believed.
const MAX_PAYLOAD: usize = 64 << 10;

/// One framed message: a 31-bit command, a response flag, and a payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub command: u32,
    pub is_response: bool,
    pub payload: Vec<u8>,
}

impl Message {
    /// The bytes on the wire: command and flag in one little-endian word,
    /// the payload length in another, then the payload.
    pub fn encode(&self) -> Vec<u8> {
        let word = (self.command & 0x7fff_ffff) | (self.is_response as u32) << 31;
        let mut out = word.to_le_bytes().to_vec();
        out.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&self.payload);
        out
    }
}

/// Reassembles messages from a byte stream that arrives in arbitrary pieces.
#[derive(Debug, Default)]
pub struct Framer {
    buffer: Vec<u8>,
}

impl Framer {
    /// Add bytes; get back every message they complete.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Message> {
        self.buffer.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            if self.buffer.len() < 8 {
                return out;
            }
            let word = u32::from_le_bytes(self.buffer[0..4].try_into().unwrap());
            let len = u32::from_le_bytes(self.buffer[4..8].try_into().unwrap()) as usize;
            if len > MAX_PAYLOAD {
                // Not a message we understand: the stream is out of step and
                // there is no way to find the next boundary. Start over.
                self.buffer.clear();
                return out;
            }
            if self.buffer.len() < 8 + len {
                return out;
            }
            let payload = self.buffer[8..8 + len].to_vec();
            self.buffer.drain(..8 + len);
            out.push(Message {
                command: word & 0x7fff_ffff,
                is_response: word >> 31 != 0,
                payload,
            });
        }
    }
}

/// The host's end of the sensors control port: answers the service's
/// "list-sensors" request with the set of sensors, which is what lets the
/// service finish starting.
pub struct SensorsControl {
    framer: Framer,
    sensors: u32,
}

impl SensorsControl {
    pub fn new(sensors: u32) -> SensorsControl {
        SensorsControl {
            framer: Framer::default(),
            sensors,
        }
    }

    /// Feed what the guest wrote; get what to write back.
    pub fn handle(&mut self, from_guest: &[u8]) -> Vec<u8> {
        let mut reply = Vec::new();
        for message in self.framer.push(from_guest) {
            if message.payload.starts_with(b"list-sensors") {
                reply.extend(
                    Message {
                        command: CMD_UPDATE_HAL,
                        is_response: true,
                        payload: format!("{}\n", self.sensors).into_bytes(),
                    }
                    .encode(),
                );
            }
        }
        reply
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(payload: &[u8]) -> Vec<u8> {
        Message {
            command: 0,
            is_response: false,
            payload: payload.to_vec(),
        }
        .encode()
    }

    #[test]
    fn messages_have_the_documented_layout() {
        let bytes = Message {
            command: 2,
            is_response: true,
            payload: b"503\n".to_vec(),
        }
        .encode();
        assert_eq!(bytes, [2, 0, 0, 0x80, 4, 0, 0, 0, b'5', b'0', b'3', b'\n']);
    }

    #[test]
    fn the_framer_handles_any_split_and_several_messages() {
        let mut stream = request(b"list-sensors");
        stream.extend(request(b""));
        stream.extend(request(b"x"));
        for cut in 0..stream.len() {
            let mut f = Framer::default();
            let mut got = f.push(&stream[..cut]);
            got.extend(f.push(&stream[cut..]));
            assert_eq!(got.len(), 3, "cut at {cut}");
            assert_eq!(got[0].payload, b"list-sensors");
            assert!(got[1].payload.is_empty());
            assert_eq!(got[2].payload, b"x");
        }
        // One byte at a time.
        let mut f = Framer::default();
        let n: usize = stream.iter().map(|b| f.push(&[*b]).len()).sum();
        assert_eq!(n, 3);
    }

    #[test]
    fn nonsense_does_not_grow_or_wedge_the_framer() {
        let mut f = Framer::default();
        assert!(f.push(&[0xff; 64]).is_empty(), "an absurd length");
        assert!(f.buffer.is_empty(), "dropped, not kept");
        assert_eq!(f.push(&request(b"ok")).len(), 1, "and it recovers");
    }

    #[test]
    fn list_sensors_is_answered_with_the_sensor_set() {
        let mut s = SensorsControl::new(PHONE_SENSORS);
        assert_eq!(PHONE_SENSORS, 503);
        let reply = s.handle(&request(b"list-sensors"));
        let mut f = Framer::default();
        let msgs = f.push(&reply);
        assert_eq!(
            msgs,
            [Message {
                command: 2,
                is_response: true,
                payload: b"503\n".to_vec()
            }]
        );
        assert!(s.handle(&request(b"something-else")).is_empty());
        // A request split across two writes is still answered, once.
        let r = request(b"list-sensors");
        assert!(s.handle(&r[..5]).is_empty());
        assert_eq!(s.handle(&r[5..]), reply);
    }
}
