// SPDX-License-Identifier: GPL-3.0-or-later
//! Android boot image headers, versions 3 and 4 (AOSP `bootimg.h`).

const PAGE: usize = 4096;

fn align(n: usize, to: usize) -> usize {
    n.div_ceil(to) * to
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?) as usize)
}

fn text(b: &[u8]) -> String {
    let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// A `boot.img` or `init_boot.img`: a kernel and/or a ramdisk.
#[derive(Debug, Clone, PartialEq)]
pub struct Boot {
    pub kernel: Vec<u8>,
    pub ramdisk: Vec<u8>,
    pub cmdline: String,
}

pub fn parse_boot(data: &[u8]) -> Result<Boot, String> {
    if data.get(..8) != Some(b"ANDROID!") {
        return Err("not an Android boot image".into());
    }
    let short = || "truncated".to_string();
    let (kernel_size, ramdisk_size) = (
        u32_at(data, 8).ok_or_else(short)?,
        u32_at(data, 12).ok_or_else(short)?,
    );
    let version = u32_at(data, 40).ok_or_else(short)?;
    if !(3..=4).contains(&version) {
        return Err(format!("boot image header v{version} is not handled"));
    }
    let kernel = data.get(PAGE..PAGE + kernel_size).ok_or_else(short)?;
    let at = PAGE + align(kernel_size, PAGE);
    let ramdisk = data.get(at..at + ramdisk_size).ok_or_else(short)?;
    Ok(Boot {
        kernel: kernel.to_vec(),
        ramdisk: ramdisk.to_vec(),
        cmdline: text(data.get(44..44 + 1536).ok_or_else(short)?),
    })
}

/// A `vendor_boot.img`: the vendor ramdisk, a command line, and (v4) the
/// image's own boot configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct VendorBoot {
    pub ramdisk: Vec<u8>,
    pub cmdline: String,
    pub bootconfig: String,
}

pub fn parse_vendor_boot(data: &[u8]) -> Result<VendorBoot, String> {
    if data.get(..8) != Some(b"VNDRBOOT") {
        return Err("not a vendor boot image".into());
    }
    let short = || "truncated".to_string();
    let version = u32_at(data, 8).ok_or_else(short)?;
    let page = u32_at(data, 12).ok_or_else(short)?;
    if !(3..=4).contains(&version) {
        return Err(format!("vendor boot header v{version} is not handled"));
    }
    if page == 0 || page > 1 << 20 {
        return Err("implausible page size".into());
    }
    let ramdisk_size = u32_at(data, 24).ok_or_else(short)?;
    let cmdline = text(data.get(28..28 + 2048).ok_or_else(short)?);
    let header_size = u32_at(data, 2096).ok_or_else(short)?;
    let dtb_size = u32_at(data, 2100).ok_or_else(short)?;
    let mut at = align(header_size, page);
    let ramdisk = data.get(at..at + ramdisk_size).ok_or_else(short)?.to_vec();
    let mut bootconfig = String::new();
    if version == 4 {
        let table_size = u32_at(data, 2112).ok_or_else(short)?;
        let config_size = u32_at(data, 2124).ok_or_else(short)?;
        at += align(ramdisk_size, page) + align(dtb_size, page) + align(table_size, page);
        bootconfig = text(data.get(at..at + config_size).ok_or_else(short)?);
    }
    Ok(VendorBoot {
        ramdisk,
        cmdline,
        bootconfig,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn pad(mut b: Vec<u8>) -> Vec<u8> {
        b.resize(align(b.len(), PAGE), 0);
        b
    }

    pub fn boot_image(kernel: &[u8], ramdisk: &[u8]) -> Vec<u8> {
        let mut h = vec![0u8; PAGE];
        h[..8].copy_from_slice(b"ANDROID!");
        h[8..12].copy_from_slice(&(kernel.len() as u32).to_le_bytes());
        h[12..16].copy_from_slice(&(ramdisk.len() as u32).to_le_bytes());
        h[20..24].copy_from_slice(&1584u32.to_le_bytes());
        h[40..44].copy_from_slice(&4u32.to_le_bytes());
        h[44..48].copy_from_slice(b"k=v\0");
        h.extend(pad(kernel.to_vec()));
        h.extend(pad(ramdisk.to_vec()));
        h
    }

    pub fn vendor_boot_image(ramdisk: &[u8], cmdline: &str, bootconfig: &str) -> Vec<u8> {
        let mut h = vec![0u8; PAGE];
        h[..8].copy_from_slice(b"VNDRBOOT");
        h[8..12].copy_from_slice(&4u32.to_le_bytes());
        h[12..16].copy_from_slice(&(PAGE as u32).to_le_bytes());
        h[24..28].copy_from_slice(&(ramdisk.len() as u32).to_le_bytes());
        h[28..28 + cmdline.len()].copy_from_slice(cmdline.as_bytes());
        let (dtb, table) = (vec![b'D'; 100], vec![b'T'; 108]);
        h[2096..2100].copy_from_slice(&2128u32.to_le_bytes());
        h[2100..2104].copy_from_slice(&(dtb.len() as u32).to_le_bytes());
        h[2112..2116].copy_from_slice(&(table.len() as u32).to_le_bytes());
        h[2124..2128].copy_from_slice(&(bootconfig.len() as u32).to_le_bytes());
        h.extend(pad(ramdisk.to_vec()));
        h.extend(pad(dtb));
        h.extend(pad(table));
        h.extend(pad(bootconfig.as_bytes().to_vec()));
        h
    }

    #[test]
    fn reads_kernel_ramdisk_and_command_line() {
        let b = parse_boot(&boot_image(b"KERNEL", b"RAMDISK")).unwrap();
        assert_eq!(
            (b.kernel.as_slice(), b.ramdisk.as_slice()),
            (&b"KERNEL"[..], &b"RAMDISK"[..])
        );
        assert_eq!(b.cmdline, "k=v");
        let only_ramdisk = parse_boot(&boot_image(b"", b"R")).unwrap();
        assert!(only_ramdisk.kernel.is_empty());
        assert_eq!(only_ramdisk.ramdisk, b"R");
    }

    #[test]
    fn reads_the_vendor_image_sections_in_order() {
        let v = parse_vendor_boot(&vendor_boot_image(b"VENDOR", "a=1 b", "x = 1\n")).unwrap();
        assert_eq!(v.ramdisk, b"VENDOR");
        assert_eq!(v.cmdline, "a=1 b");
        assert_eq!(v.bootconfig, "x = 1\n");
    }

    #[test]
    fn refuses_what_it_does_not_understand() {
        assert!(parse_boot(b"nope").is_err());
        assert!(parse_vendor_boot(b"ANDROID!").is_err());
        let mut old = boot_image(b"K", b"");
        old[40] = 2;
        assert!(parse_boot(&old).unwrap_err().contains("v2"));
        let mut cut = boot_image(&[1; 9000], b"");
        cut.truncate(6000);
        assert_eq!(parse_boot(&cut).unwrap_err(), "truncated");
        let mut huge = vendor_boot_image(b"V", "", "");
        huge[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(parse_vendor_boot(&huge).unwrap_err(), "truncated");
    }
}
