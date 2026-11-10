// SPDX-License-Identifier: GPL-3.0-or-later
//! Android sparse images (AOSP `libsparse`, `sparse_format.h`): how the
//! stock image ships its large partitions. A sparse image lists chunks of
//! real data, repeated fill and "don't care" gaps; this expands one into the
//! plain image the engine uses as a disk.

use std::io::{Read, Seek, SeekFrom, Write};

const MAGIC: u32 = 0xed26_ff3a;
const CHUNK_RAW: u16 = 0xcac1;
const CHUNK_FILL: u16 = 0xcac2;
const CHUNK_DONT_CARE: u16 = 0xcac3;
const CHUNK_CRC32: u16 = 0xcac4;
/// The most a guest-independent, well-formed image would describe.
const MAX_RAW_BYTES: u64 = 1 << 40;

/// True if `head` (at least 4 bytes) starts a sparse image.
pub fn is_sparse(head: &[u8]) -> bool {
    head.len() >= 4 && u32::from_le_bytes([head[0], head[1], head[2], head[3]]) == MAGIC
}

fn read_exact(r: &mut impl Read, n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)
        .map_err(|e| format!("sparse image is truncated: {e}"))?;
    Ok(buf)
}

/// Expand the sparse image in `input` into `output`. Gaps are skipped, not
/// written, so they become holes where the filesystem supports them and an
/// 8 GiB image that is mostly empty stays small on disk. An image that ends
/// in a gap leaves `output` short; the caller sets the final length
/// ([`expand_file`] does). Returns the expanded size in bytes.
pub fn expand<R: Read, W: Write + Seek>(input: &mut R, output: &mut W) -> Result<u64, String> {
    let h = read_exact(input, 28)?;
    let u16_at = |at: usize| u16::from_le_bytes([h[at], h[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes([h[at], h[at + 1], h[at + 2], h[at + 3]]);
    if u32_at(0) != MAGIC {
        return Err("not an Android sparse image".into());
    }
    if u16_at(4) != 1 {
        return Err(format!(
            "sparse format version {} is not handled",
            u16_at(4)
        ));
    }
    let (file_header, chunk_header) = (u16_at(8) as usize, u16_at(10) as usize);
    let (block, total_blocks, chunks) = (u32_at(12) as u64, u32_at(16) as u64, u32_at(20));
    if file_header < 28 || chunk_header < 12 || block == 0 || block % 4 != 0 {
        return Err("sparse image header is malformed".into());
    }
    let total = block
        .checked_mul(total_blocks)
        .filter(|t| *t <= MAX_RAW_BYTES)
        .ok_or("sparse image describes an implausible size")?;
    read_exact(input, file_header - 28)?;

    let fail = |e: std::io::Error| format!("writing the expanded image: {e}");
    let mut written = 0u64;
    for _ in 0..chunks {
        let c = read_exact(input, chunk_header)?;
        let kind = u16::from_le_bytes([c[0], c[1]]);
        let blocks = u32::from_le_bytes([c[4], c[5], c[6], c[7]]) as u64;
        let total_size = u32::from_le_bytes([c[8], c[9], c[10], c[11]]) as u64;
        let data = total_size
            .checked_sub(chunk_header as u64)
            .ok_or("sparse chunk is smaller than its header")?;
        let bytes = blocks * block;
        if written + bytes > total && kind != CHUNK_CRC32 {
            return Err("sparse image has more blocks than it declares".into());
        }
        match kind {
            CHUNK_RAW => {
                if data != bytes {
                    return Err("sparse raw chunk has the wrong length".into());
                }
                let mut left = bytes;
                let mut buf = vec![0u8; 1 << 20];
                while left > 0 {
                    let n = left.min(buf.len() as u64) as usize;
                    input
                        .read_exact(&mut buf[..n])
                        .map_err(|e| format!("sparse image is truncated: {e}"))?;
                    output.write_all(&buf[..n]).map_err(fail)?;
                    left -= n as u64;
                }
                written += bytes;
            }
            CHUNK_FILL => {
                if data != 4 {
                    return Err("sparse fill chunk has the wrong length".into());
                }
                let fill = read_exact(input, 4)?;
                if fill == [0, 0, 0, 0] {
                    output.seek(SeekFrom::Current(bytes as i64)).map_err(fail)?;
                } else {
                    let pattern: Vec<u8> = fill.iter().cycle().take(1 << 16).copied().collect();
                    let mut left = bytes;
                    while left > 0 {
                        let n = left.min(pattern.len() as u64) as usize;
                        output.write_all(&pattern[..n]).map_err(fail)?;
                        left -= n as u64;
                    }
                }
                written += bytes;
            }
            CHUNK_DONT_CARE => {
                if data != 0 {
                    return Err("sparse gap chunk carries data".into());
                }
                output.seek(SeekFrom::Current(bytes as i64)).map_err(fail)?;
                written += bytes;
            }
            CHUNK_CRC32 => {
                read_exact(input, data as usize)?;
            }
            other => return Err(format!("unknown sparse chunk type {other:#x}")),
        }
    }
    if written != total {
        return Err(format!(
            "sparse image declares {total} bytes but its chunks cover {written}"
        ));
    }
    Ok(total)
}

/// Expand `input` to `output` as files. The output ends with the right
/// length even when the image ends in a gap.
pub fn expand_file(input: &std::path::Path, output: &std::path::Path) -> Result<u64, String> {
    let mut src = std::io::BufReader::new(
        std::fs::File::open(input).map_err(|e| format!("opening {}: {e}", input.display()))?,
    );
    let mut dst =
        std::fs::File::create(output).map_err(|e| format!("creating {}: {e}", output.display()))?;
    let total = expand(&mut src, &mut dst)?;
    dst.set_len(total)
        .map_err(|e| format!("sizing {}: {e}", output.display()))?;
    Ok(total)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a sparse image with 4096-byte blocks from (type, blocks, data).
    pub fn build(chunks: &[(u16, u32, Vec<u8>)], total_blocks: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&28u16.to_le_bytes());
        out.extend_from_slice(&12u16.to_le_bytes());
        out.extend_from_slice(&4096u32.to_le_bytes());
        out.extend_from_slice(&total_blocks.to_le_bytes());
        out.extend_from_slice(&(chunks.len() as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        for (kind, blocks, data) in chunks {
            out.extend_from_slice(&kind.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&blocks.to_le_bytes());
            out.extend_from_slice(&(12 + data.len() as u32).to_le_bytes());
            out.extend_from_slice(data);
        }
        out
    }

    fn expand_to_vec(image: &[u8]) -> Result<Vec<u8>, String> {
        let dir = crate::prepare::tests::temp(&format!("sparse-{}", image.len()));
        let (a, b) = (dir.join("in"), dir.join("out"));
        std::fs::write(&a, image).unwrap();
        let r = expand_file(&a, &b).map(|n| {
            let v = std::fs::read(&b).unwrap();
            assert_eq!(v.len() as u64, n);
            v
        });
        std::fs::remove_dir_all(dir).unwrap();
        r
    }

    #[test]
    fn every_chunk_type_expands() {
        let image = build(
            &[
                (CHUNK_RAW, 1, vec![0xaa; 4096]),
                (CHUNK_DONT_CARE, 2, vec![]),
                (CHUNK_FILL, 1, vec![1, 2, 3, 4]),
                (CHUNK_FILL, 1, vec![0, 0, 0, 0]),
                (CHUNK_CRC32, 0, vec![9, 9, 9, 9]),
                (CHUNK_RAW, 1, vec![0xbb; 4096]),
                (CHUNK_DONT_CARE, 1, vec![]),
            ],
            7,
        );
        assert!(is_sparse(&image));
        let raw = expand_to_vec(&image).unwrap();
        assert_eq!(raw.len(), 7 * 4096);
        assert!(raw[..4096].iter().all(|b| *b == 0xaa));
        assert!(
            raw[4096..3 * 4096].iter().all(|b| *b == 0),
            "the gap reads as zeros"
        );
        assert_eq!(&raw[3 * 4096..3 * 4096 + 8], &[1, 2, 3, 4, 1, 2, 3, 4]);
        assert!(raw[4 * 4096..5 * 4096].iter().all(|b| *b == 0));
        assert!(raw[5 * 4096..6 * 4096].iter().all(|b| *b == 0xbb));
        assert!(
            raw[6 * 4096..].iter().all(|b| *b == 0),
            "a trailing gap still counts"
        );
    }

    #[test]
    fn malformed_images_are_refused() {
        assert!(!is_sparse(b"ANDROID!"));
        assert!(expand_to_vec(b"ANDROID! and then some more bytes here")
            .unwrap_err()
            .contains("not an Android sparse"));
        let short = build(&[(CHUNK_RAW, 1, vec![0; 4096])], 2);
        assert!(expand_to_vec(&short).unwrap_err().contains("cover"));
        let long = build(&[(CHUNK_DONT_CARE, 3, vec![])], 2);
        assert!(expand_to_vec(&long).unwrap_err().contains("more blocks"));
        let wrong = build(&[(CHUNK_RAW, 2, vec![0; 4096])], 2);
        assert!(expand_to_vec(&wrong).unwrap_err().contains("wrong length"));
        let mut cut = build(&[(CHUNK_RAW, 1, vec![7; 4096])], 1);
        cut.truncate(cut.len() - 100);
        assert!(expand_to_vec(&cut).unwrap_err().contains("truncated"));
        let unknown = build(&[(0x1234, 1, vec![])], 1);
        assert!(expand_to_vec(&unknown)
            .unwrap_err()
            .contains("unknown sparse chunk"));
        // A header claiming an absurd size is refused before anything is written.
        let mut huge = build(&[], 0);
        huge[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        huge[12..16].copy_from_slice(&(1u32 << 30).to_le_bytes());
        assert!(expand_to_vec(&huge).unwrap_err().contains("implausible"));
    }
}
