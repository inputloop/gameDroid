// SPDX-License-Identifier: GPL-3.0-or-later
//! Android's serial console: noticing that it has booted, and typing
//! commands at the root shell the stock image offers there.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// What Android's init logs when the system has finished booting.
pub const BOOTED: &[u8] = b"sys.boot_completed=1";

/// Settings a games machine wants and a phone does not: no lock screen, a
/// screen that stays on, and the (virtual) Wi-Fi joined without asking.
///
/// The `wlan0` step finishes what the image's own Wi-Fi helper sometimes
/// cannot: it runs once at boot, and when it runs before the `virt_wifi`
/// kernel module has loaded it fails to create the interface (seen in CI). Typed at the shell once Android has booted. They
/// persist in Android's own settings, so repeating them is harmless.
pub const GAMING_SETTINGS: &str = "su 0 sh -c 'locksettings set-disabled true; svc power stayon true; \
     wm dismiss-keyguard; [ -e /sys/class/net/wlan0 ] || { ip link add link buried_eth2 name wlan0 type virt_wifi; \
     ip link set buried_eth2 up; }; cmd wifi set-wifi-enabled enabled; sleep 3; \
     cmd wifi connect-network VirtWifi open'";

/// Passes the console through to `out` and sets `booted` when Android
/// reports that it has finished booting.
pub struct BootWatch<W: Write> {
    out: W,
    line: Vec<u8>,
    booted: Arc<AtomicBool>,
}

impl<W: Write> BootWatch<W> {
    pub fn new(out: W) -> (BootWatch<W>, Arc<AtomicBool>) {
        let booted = Arc::new(AtomicBool::new(false));
        let watch = BootWatch {
            out,
            line: Vec::new(),
            booted: booted.clone(),
        };
        (watch, booted)
    }
}

impl<W: Write> Write for BootWatch<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for b in bytes {
            if *b == b'\n' {
                if self.line.windows(BOOTED.len()).any(|w| w == BOOTED) {
                    self.booted.store(true, Ordering::SeqCst);
                }
                self.line.clear();
            } else if self.line.len() < 4096 {
                self.line.push(*b);
            }
        }
        self.out.write_all(bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// The guest's serial input: whatever is sent down a channel. Reading
/// blocks until there is something; the line closes when every sender has
/// gone.
pub struct SerialKeys {
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

/// A serial input for the guest and the handle that types into it.
pub fn serial() -> (Sender<Vec<u8>>, SerialKeys) {
    let (tx, rx) = std::sync::mpsc::channel();
    let keys = SerialKeys {
        rx,
        pending: Vec::new(),
    };
    (tx, keys)
}

impl Read for SerialKeys {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.rx.recv() {
                Ok(bytes) => self.pending = bytes,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_is_noticed_across_writes_and_text_passes_through() {
        let (mut watch, booted) = BootWatch::new(Vec::new());
        watch
            .write_all(b"[ 1.0] init: starting\n[ 37.5] init: processing action (sys.boot_")
            .unwrap();
        assert!(!booted.load(Ordering::SeqCst));
        watch.write_all(b"completed=1) from init.rc\n").unwrap();
        assert!(booted.load(Ordering::SeqCst));
        assert!(watch.out.ends_with(b"from init.rc\n") && watch.out.starts_with(b"[ 1.0]"));
    }

    #[test]
    fn typed_lines_arrive_in_order_and_the_line_closes() {
        let (tx, mut keys) = serial();
        tx.send(b"id\n".to_vec()).unwrap();
        tx.send(b"ls\n".to_vec()).unwrap();
        drop(tx);
        let mut all = Vec::new();
        keys.read_to_end(&mut all).unwrap();
        assert_eq!(all, b"id\nls\n");
    }
}
