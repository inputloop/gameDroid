// SPDX-License-Identifier: GPL-3.0-or-later
//! Webcams on Linux, through the kernel's V4L2 interface.
//!
//! Written from `linux/videodev2.h`. Structures are handled as bytes at
//! their offsets on a 64-bit system, which is the only kind gameDroid
//! runs on.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;

use crate::Webcam;

const fn ioc(dir: u64, nr: u64, size: u64) -> u64 {
    dir << 30 | size << 16 | (b'V' as u64) << 8 | nr
}
const WRITE: u64 = 1;
const READ: u64 = 2;
const QUERYCAP: u64 = ioc(READ, 0, 104);
const ENUM_FMT: u64 = ioc(READ | WRITE, 2, 64);
const S_FMT: u64 = ioc(READ | WRITE, 5, 208);
const REQBUFS: u64 = ioc(READ | WRITE, 8, 20);
const QUERYBUF: u64 = ioc(READ | WRITE, 9, 88);
const QBUF: u64 = ioc(READ | WRITE, 15, 88);
const DQBUF: u64 = ioc(READ | WRITE, 17, 88);
const STREAMON: u64 = ioc(WRITE, 18, 4);
const STREAMOFF: u64 = ioc(WRITE, 19, 4);
const S_PARM: u64 = ioc(READ | WRITE, 22, 204);

const CAPTURE: u32 = 1; // V4L2_BUF_TYPE_VIDEO_CAPTURE, and V4L2_CAP_VIDEO_CAPTURE
const MMAP: u32 = 1;
const MJPEG: u32 = u32::from_le_bytes(*b"MJPG");
const YUYV: u32 = u32::from_le_bytes(*b"YUYV");
const BUFFERS: u32 = 4;

fn get32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap_or([0; 4]))
}

fn put32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn ioctl(file: &File, request: u64, arg: &mut [u8]) -> bool {
    // SAFETY: `arg` is as large as the request's structure (the callers
    // size it from the same table as the request numbers).
    unsafe { libc::ioctl(file.as_raw_fd(), request as _, arg.as_mut_ptr()) == 0 }
}

/// The pixel formats a camera offers.
fn formats(file: &File) -> Vec<u32> {
    let mut found = Vec::new();
    for index in 0..32 {
        let mut desc = [0u8; 64];
        put32(&mut desc, 0, index);
        put32(&mut desc, 4, CAPTURE);
        if !ioctl(file, ENUM_FMT, &mut desc) {
            break;
        }
        found.push(get32(&desc, 44));
    }
    found
}

pub fn list() -> Vec<Webcam> {
    let mut cameras = Vec::new();
    for n in 0..16 {
        let path = format!("/dev/video{n}");
        let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
            continue;
        };
        let mut cap = [0u8; 104];
        // device_caps@88: this node captures video (a camera has other
        // nodes too, for its metadata), and in a format we can use.
        if !ioctl(&file, QUERYCAP, &mut cap) || get32(&cap, 88) & CAPTURE == 0 {
            continue;
        }
        let offered = formats(&file);
        if !offered.contains(&MJPEG) && !offered.contains(&YUYV) {
            continue;
        }
        let card = &cap[16..48];
        let end = card.iter().position(|b| *b == 0).unwrap_or(card.len());
        cameras.push(Webcam {
            name: String::from_utf8_lossy(&card[..end]).into_owned(),
            id: path,
        });
    }
    cameras
}

struct Mapping {
    at: *mut libc::c_void,
    len: usize,
}

pub struct Capture {
    file: File,
    buffers: Vec<Mapping>,
    format: u32,
    width: u32,
    height: u32,
}

// SAFETY: the mappings belong to this capture alone.
unsafe impl Send for Capture {}

impl Capture {
    pub fn open(path: &str, width: u32, height: u32, fps: u32) -> Result<Capture, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| format!("opening {path}: {e}"))?;
        let offered = formats(&file);
        let format = if offered.contains(&MJPEG) {
            MJPEG
        } else if offered.contains(&YUYV) {
            YUYV
        } else {
            return Err("the camera offers neither JPEG nor YUYV video".into());
        };
        // struct v4l2_format: type@0, then v4l2_pix_format at 8.
        let mut fmt = [0u8; 208];
        put32(&mut fmt, 0, CAPTURE);
        put32(&mut fmt, 8, width);
        put32(&mut fmt, 12, height);
        put32(&mut fmt, 16, format);
        put32(&mut fmt, 20, 1); // no interlacing
        if !ioctl(&file, S_FMT, &mut fmt) {
            return Err("the camera refused the picture format".into());
        }
        let (got_w, got_h, got_f) = (get32(&fmt, 8), get32(&fmt, 12), get32(&fmt, 16));
        if (got_w, got_h, got_f) != (width, height, format) {
            return Err(format!(
                "the camera gives {got_w}x{got_h}, not the {width}x{height} Android asked for"
            ));
        }
        // struct v4l2_streamparm: type@0, timeperframe {numerator@12, denominator@16}.
        // A camera that keeps its own rate is still usable.
        let mut parm = [0u8; 204];
        put32(&mut parm, 0, CAPTURE);
        put32(&mut parm, 12, 1);
        put32(&mut parm, 16, fps.max(1));
        let _ = ioctl(&file, S_PARM, &mut parm);

        let mut req = [0u8; 20];
        put32(&mut req, 0, BUFFERS);
        put32(&mut req, 4, CAPTURE);
        put32(&mut req, 8, MMAP);
        if !ioctl(&file, REQBUFS, &mut req) || get32(&req, 0) == 0 {
            return Err("the camera gave no buffers".into());
        }
        let mut capture = Capture {
            file,
            buffers: Vec::new(),
            format,
            width,
            height,
        };
        for index in 0..get32(&req, 0) {
            // struct v4l2_buffer: index@0, type@4, memory@60, m.offset@64, length@72.
            let mut buf = [0u8; 88];
            put32(&mut buf, 0, index);
            put32(&mut buf, 4, CAPTURE);
            put32(&mut buf, 60, MMAP);
            if !ioctl(&capture.file, QUERYBUF, &mut buf) {
                return Err("the camera would not describe its buffers".into());
            }
            let (offset, len) = (get32(&buf, 64), get32(&buf, 72) as usize);
            // SAFETY: mapping the camera's own buffer, as V4L2 prescribes.
            let at = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    capture.file.as_raw_fd(),
                    offset as libc::off_t,
                )
            };
            if at == libc::MAP_FAILED {
                return Err("the camera's buffers could not be mapped".into());
            }
            capture.buffers.push(Mapping { at, len });
            if !ioctl(&capture.file, QBUF, &mut buf) {
                return Err("the camera would not take its buffers back".into());
            }
        }
        let mut kind = CAPTURE.to_le_bytes();
        if !ioctl(&capture.file, STREAMON, &mut kind) {
            return Err("the camera would not start".into());
        }
        Ok(capture)
    }

    pub fn frame(&mut self) -> Option<Vec<u8>> {
        let mut fds = [libc::pollfd {
            fd: self.file.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        }];
        // SAFETY: one valid pollfd.
        if unsafe { libc::poll(fds.as_mut_ptr(), 1, 1000) } <= 0 {
            return None;
        }
        let mut buf = [0u8; 88];
        put32(&mut buf, 4, CAPTURE);
        put32(&mut buf, 60, MMAP);
        if !ioctl(&self.file, DQBUF, &mut buf) {
            return None;
        }
        // index@0, bytesused@8.
        let (index, used) = (get32(&buf, 0) as usize, get32(&buf, 8) as usize);
        let picture = self.buffers.get(index).map(|m| {
            // SAFETY: the mapping is `len` bytes; the kernel filled `used` of them.
            unsafe { std::slice::from_raw_parts(m.at as *const u8, used.min(m.len)) }.to_vec()
        });
        // Hand the buffer back for the next picture.
        let _ = ioctl(&self.file, QBUF, &mut buf);
        let picture = picture?;
        match self.format {
            MJPEG => (picture.len() > 4).then_some(picture),
            _ => crate::yuyv_to_jpeg(&picture, self.width, self.height),
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let mut kind = CAPTURE.to_le_bytes();
        let _ = ioctl(&self.file, STREAMOFF, &mut kind);
        for m in &self.buffers {
            // SAFETY: mapped in `open`, unmapped once.
            unsafe { libc::munmap(m.at, m.len) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_numbers_are_the_kernels() {
        // From videodev2.h on x86-64, as `strace` prints them.
        assert_eq!(QUERYCAP, 0x8068_5600);
        assert_eq!(S_FMT, 0xc0d0_5605);
        assert_eq!(REQBUFS, 0xc014_5608);
        assert_eq!(QUERYBUF, 0xc058_5609);
        assert_eq!(QBUF, 0xc058_560f);
        assert_eq!(DQBUF, 0xc058_5611);
        assert_eq!(STREAMON, 0x4004_5612);
        assert_eq!(STREAMOFF, 0x4004_5613);
        assert_eq!(S_PARM, 0xc0cc_5616);
        assert_eq!(ENUM_FMT, 0xc040_5602);
    }

    #[test]
    fn a_camera_that_is_not_there_is_an_error() {
        let e = Capture::open("/dev/video-none", 640, 480, 30)
            .err()
            .unwrap();
        assert!(e.contains("opening"), "{e}");
    }
}
