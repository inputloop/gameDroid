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

/// "How to use this browser and OS": the page kept on the phone.
pub const HELP_PAGE: &str = include_str!("../assets/help.html");
/// The browser the stock image ships: Chromium's WebView test shell. It is
/// the only one, has no start-page setting, and opens a blank page unless
/// it is started with an address.
pub const BROWSER: &str = "org.chromium.webview_shell";
const BROWSER_ACTIVITY: &str = "org.chromium.webview_shell/.WebViewBrowserActivity";
/// Where the page is kept: in the browser's own files, the one place it may
/// always read, whatever Android's storage rules say.
const HELP_FILE: &str = "/data/data/org.chromium.webview_shell/files/gamedroid-help.html";
/// The page's address in the browser.
pub const HELP_URL: &str = "file:///data/data/org.chromium.webview_shell/files/gamedroid-help.html";

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            let c = T[(n >> (18 - 6 * i)) as usize & 63] as char;
            out.push(if i <= chunk.len() { c } else { '=' });
        }
    }
    out
}

/// Shell command lines, to be typed in order at Android's root shell, that
/// store the help page on the phone where the browser can read it.
///
/// The page crosses the serial line as base64 text in pieces short enough
/// for a terminal line, is put back together on the other side, and is
/// given to the browser's user and security label.
pub fn help_page_commands() -> Vec<String> {
    let tmp = "/data/local/tmp/gd-help.b64";
    let mut lines = vec![format!("su 0 rm -f {tmp}")];
    for piece in base64(HELP_PAGE.as_bytes()).as_bytes().chunks(900) {
        let piece = std::str::from_utf8(piece).unwrap_or_default();
        lines.push(format!("su 0 sh -c 'echo {piece} >> {tmp}'"));
    }
    let dir = "/data/data/org.chromium.webview_shell";
    lines.push(format!(
        "su 0 sh -c 'mkdir -p {dir}/files && base64 -d {tmp} > {HELP_FILE} && \
         chown -R $(stat -c %u:%g {dir}) {dir}/files && restorecon -R {dir}/files; rm -f {tmp}'"
    ));
    lines
}

/// A shell command line that opens the help page in the browser.
pub fn open_help_command() -> String {
    format!("am start -n {BROWSER_ACTIVITY} -a android.intent.action.VIEW -d {HELP_URL}")
}

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
    fn the_help_page_crosses_the_serial_line_whole() {
        // RFC 4648's examples.
        for (plain, coded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), coded);
        }
        let lines = help_page_commands();
        // Every line fits a terminal line, and nothing in the page can end
        // the quoting it travels in.
        assert!(lines.iter().all(|l| l.len() < 1200 && !l.contains('\n')));
        let coded: String = lines
            .iter()
            .filter_map(|l| l.strip_prefix("su 0 sh -c 'echo ")?.split(' ').next())
            .collect();
        assert_eq!(coded, base64(HELP_PAGE.as_bytes()));
        assert!(coded
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c)));
        let last = lines.last().unwrap();
        assert!(
            last.contains("base64 -d") && last.contains("restorecon") && last.contains("chown")
        );
        // The page itself: complete, and it reaches nothing outside itself.
        assert!(
            HELP_PAGE.starts_with("<!doctype html>") && HELP_PAGE.trim_end().ends_with("</html>")
        );
        assert!(HELP_PAGE.contains("<title>How to use this browser and OS</title>"));
        assert!(
            !HELP_PAGE.contains("http://")
                && !HELP_PAGE.contains("https://")
                && !HELP_PAGE.contains("src=")
        );
        assert!(open_help_command().ends_with(HELP_URL));
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
