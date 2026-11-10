// SPDX-License-Identifier: GPL-3.0-or-later
//! A GUID partition table (UEFI 2.10, section 5.3), built around partitions
//! that each start on a 1 MiB boundary from 1 MiB onwards.

pub const MIB: u64 = 1 << 20;
const SECTOR: u64 = 512;
const ENTRIES: usize = 128;
const ENTRY: usize = 128;
/// "Linux filesystem data", as bytes on disk.
const LINUX_FS: [u8; 16] = [
    0xaf, 0x3d, 0xc6, 0x0f, 0x83, 0x84, 0x72, 0x47, 0x8e, 0x79, 0x3d, 0x69, 0xd8, 0x47, 0x7d, 0xe4,
];

pub fn align(n: u64, to: u64) -> u64 {
    n.div_ceil(to) * to
}

/// CRC-32 as GPT uses it (the zlib polynomial).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & (!(crc & 1)).wrapping_add(1));
        }
    }
    !crc
}

/// A GUID, held the way it is written on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guid(pub [u8; 16]);

impl Guid {
    /// A GUID that is always the same for the same name, so a prepared disk
    /// does not change identity when it is prepared again.
    pub fn from_name(name: &str) -> Guid {
        let mut bytes = [0u8; 16];
        for (half, seed) in [0xcbf2_9ce4_8422_2325u64, 0x8422_2325_cbf2_9ce4]
            .iter()
            .enumerate()
        {
            let mut h = *seed;
            for b in b"gamedroid:".iter().chain(name.as_bytes()) {
                h = (h ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
            bytes[half * 8..half * 8 + 8].copy_from_slice(&h.to_le_bytes());
        }
        // Mark it as a name-based, standard-variant GUID. On disk the
        // version sits in the high bits of byte 7.
        bytes[7] = (bytes[7] & 0x0f) | 0x50;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Guid(bytes)
    }
}

impl std::fmt::Display for Guid {
    /// The usual text form. The first three groups are little-endian on disk.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            b[3], b[2], b[1], b[0], b[5], b[4], b[7], b[6], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
        )
    }
}

#[derive(Debug, Clone)]
pub struct Partition {
    pub name: String,
    pub bytes: u64,
    pub guid: Guid,
}

pub struct Table {
    /// The first 1 MiB of the disk: protective MBR, header, entries, zeros.
    pub head: Vec<u8>,
    /// The last 1 MiB: zeros, then the backup entries and header.
    pub tail: Vec<u8>,
    pub disk_bytes: u64,
}

pub fn build(partitions: &[Partition], disk: Guid) -> Result<Table, String> {
    if partitions.len() > ENTRIES {
        return Err(format!(
            "{} partitions; a table holds {ENTRIES}",
            partitions.len()
        ));
    }
    let mut entries = vec![0u8; ENTRIES * ENTRY];
    let mut lba = MIB / SECTOR;
    for (i, p) in partitions.iter().enumerate() {
        let name: Vec<u16> = p.name.encode_utf16().collect();
        if name.len() > 36 || p.bytes == 0 {
            return Err(format!("partition {}: bad name or size", p.name));
        }
        let sectors = align(p.bytes, MIB) / SECTOR;
        let e = &mut entries[i * ENTRY..(i + 1) * ENTRY];
        e[0..16].copy_from_slice(&LINUX_FS);
        e[16..32].copy_from_slice(&p.guid.0);
        e[32..40].copy_from_slice(&lba.to_le_bytes());
        e[40..48].copy_from_slice(&(lba + sectors - 1).to_le_bytes());
        for (j, unit) in name.iter().enumerate() {
            e[56 + j * 2..58 + j * 2].copy_from_slice(&unit.to_le_bytes());
        }
        lba += sectors;
    }
    let disk_bytes = lba * SECTOR + MIB;
    let last = disk_bytes / SECTOR - 1;
    let entries_crc = crc32(&entries);
    let header = |this: u64, other: u64, entries_lba: u64| -> Vec<u8> {
        let mut h = vec![0u8; SECTOR as usize];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        h[12..16].copy_from_slice(&92u32.to_le_bytes());
        h[24..32].copy_from_slice(&this.to_le_bytes());
        h[32..40].copy_from_slice(&other.to_le_bytes());
        h[40..48].copy_from_slice(&(MIB / SECTOR).to_le_bytes());
        h[48..56].copy_from_slice(&(last - 33).to_le_bytes());
        h[56..72].copy_from_slice(&disk.0);
        h[72..80].copy_from_slice(&entries_lba.to_le_bytes());
        h[80..84].copy_from_slice(&(ENTRIES as u32).to_le_bytes());
        h[84..88].copy_from_slice(&(ENTRY as u32).to_le_bytes());
        h[88..92].copy_from_slice(&entries_crc.to_le_bytes());
        let crc = crc32(&h[..92]);
        h[16..20].copy_from_slice(&crc.to_le_bytes());
        h
    };
    let mut head = vec![0u8; SECTOR as usize];
    // One protective partition of type 0xEE covering the disk.
    head[446 + 1..446 + 4].copy_from_slice(&[0x00, 0x02, 0x00]);
    head[446 + 4] = 0xee;
    head[446 + 5..446 + 8].copy_from_slice(&[0xff, 0xff, 0xff]);
    head[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    head[446 + 12..446 + 16].copy_from_slice(&(last.min(u32::MAX as u64) as u32).to_le_bytes());
    head[510..512].copy_from_slice(&[0x55, 0xaa]);
    head.extend(header(1, last, 2));
    head.extend_from_slice(&entries);
    head.resize(MIB as usize, 0);
    let mut tail = vec![0u8; (MIB - 33 * SECTOR) as usize];
    tail.extend_from_slice(&entries);
    tail.extend(header(last, 1, last - 32));
    Ok(Table {
        head,
        tail,
        disk_bytes,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Check a whole disk image's partition table the way a reader would.
    /// Returns each partition's (name, first byte, last byte).
    pub fn check(disk: &[u8]) -> Vec<(String, u64, u64)> {
        assert_eq!(&disk[510..512], &[0x55, 0xaa]);
        assert_eq!(disk[450], 0xee, "protective MBR");
        let header = &disk[512..512 + 92];
        assert_eq!(&header[..8], b"EFI PART");
        let mut zeroed = header.to_vec();
        zeroed[16..20].fill(0);
        assert_eq!(
            crc32(&zeroed).to_le_bytes(),
            header[16..20],
            "header checksum"
        );
        let entries = &disk[1024..1024 + ENTRIES * ENTRY];
        assert_eq!(
            crc32(entries).to_le_bytes(),
            header[88..92],
            "entries checksum"
        );
        let last = disk.len() as u64 / 512 - 1;
        let backup = &disk[disk.len() - 512..][..92];
        assert_eq!(&backup[..8], b"EFI PART");
        assert_eq!(u64::from_le_bytes(backup[24..32].try_into().unwrap()), last);
        assert_eq!(u64::from_le_bytes(backup[32..40].try_into().unwrap()), 1);
        assert_eq!(
            &disk[disk.len() - 33 * 512..disk.len() - 512],
            entries,
            "backup entries match"
        );
        let mut out = Vec::new();
        let mut next = MIB / 512;
        for e in entries.chunks(ENTRY) {
            let units: Vec<u16> = e[56..]
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|u| *u != 0)
                .collect();
            if units.is_empty() {
                continue;
            }
            let first = u64::from_le_bytes(e[32..40].try_into().unwrap());
            let end = u64::from_le_bytes(e[40..48].try_into().unwrap());
            assert_eq!(first, next, "partitions are back to back");
            assert_eq!(first * 512 % MIB, 0);
            assert!(end < last - 33);
            next = end + 1;
            out.push((
                String::from_utf16(&units).unwrap(),
                first * 512,
                end * 512 + 511,
            ));
        }
        out
    }

    #[test]
    fn crc_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn guids_are_stable_distinct_and_well_formed() {
        let a = Guid::from_name("vbmeta_a");
        assert_eq!(a, Guid::from_name("vbmeta_a"));
        assert_ne!(a, Guid::from_name("vbmeta_b"));
        let text = a.to_string();
        assert_eq!(text.len(), 36);
        assert_eq!(text.as_bytes()[14], b'5', "version nibble: {text}");
        assert!(
            matches!(text.as_bytes()[19], b'8' | b'9' | b'a' | b'b'),
            "variant: {text}"
        );
        // Text and disk order differ in the first three groups.
        let g = Guid([
            0x33, 0x22, 0x11, 0x00, 0x55, 0x44, 0x77, 0x66, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd,
            0xee, 0xff,
        ]);
        assert_eq!(g.to_string(), "00112233-4455-6677-8899-aabbccddeeff");
    }

    #[test]
    fn a_built_table_reads_back() {
        let parts = vec![
            Partition {
                name: "boot_a".into(),
                bytes: 64 * MIB,
                guid: Guid::from_name("boot_a"),
            },
            Partition {
                name: "odd".into(),
                bytes: MIB + 1,
                guid: Guid::from_name("odd"),
            },
        ];
        let t = build(&parts, Guid::from_name("disk")).unwrap();
        assert_eq!(t.disk_bytes, MIB + 64 * MIB + 2 * MIB + MIB);
        let mut disk = t.head.clone();
        disk.resize((t.disk_bytes - MIB) as usize, 0);
        disk.extend_from_slice(&t.tail);
        let found = check(&disk);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0], ("boot_a".to_string(), MIB, 65 * MIB - 1));
        assert_eq!(found[1], ("odd".to_string(), 65 * MIB, 67 * MIB - 1));
        assert!(build(
            &[Partition {
                name: "x".repeat(40),
                bytes: 1,
                guid: Guid([0; 16])
            }],
            Guid([0; 16])
        )
        .is_err());
    }
}
