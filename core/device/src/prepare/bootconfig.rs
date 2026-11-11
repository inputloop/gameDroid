// SPDX-License-Identifier: GPL-3.0-or-later
//! The kernel's boot configuration: `key = value` lines appended to the
//! ramdisk, which Android reads its `androidboot.*` settings from (Linux
//! `Documentation/admin-guide/bootconfig.rst`).

const MAGIC: &[u8] = b"#BOOTCONFIG\n";

/// The key a line sets, or None for blank lines and comments.
pub fn key(line: &str) -> Option<&str> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = [":=", "+=", "="]
        .iter()
        .filter_map(|sep| line.find(sep))
        .min()
        .unwrap_or(line.len());
    Some(line[..end].trim())
}

/// The image's settings, then ours, with ours replacing theirs.
///
/// The kernel refuses a configuration that sets a key twice ("Value is
/// redefined") and then ignores all of it, which sends Android into recovery
/// mode. So the two cannot simply be joined.
pub fn merge(theirs: &str, ours: &str) -> Result<String, String> {
    let mine: Vec<&str> = ours.lines().filter_map(key).collect();
    let mut seen: Vec<&str> = Vec::new();
    let mut out = String::new();
    let kept = theirs
        .lines()
        .filter(|l| key(l).is_none_or(|k| !mine.contains(&k)));
    for line in kept.chain(ours.lines()) {
        if let Some(k) = key(line) {
            if seen.contains(&k) {
                return Err(format!("boot configuration sets {k} twice"));
            }
            seen.push(k);
        }
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

/// The bytes to append to a ramdisk of `initrd_len` bytes to carry `text`.
pub fn trailer(text: &str, initrd_len: usize) -> Vec<u8> {
    let mut body = text.as_bytes().to_vec();
    if !body.ends_with(b"\n") {
        body.push(b'\n');
    }
    body.push(0);
    // The whole file must end on a 4-byte boundary; the padding counts as data.
    let total = initrd_len + body.len() + 8 + MAGIC.len();
    body.resize(body.len() + (total.div_ceil(4) * 4 - total), 0);
    let checksum = body.iter().fold(0u32, |a, b| a.wrapping_add(*b as u32));
    let mut out = body.clone();
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&checksum.to_le_bytes());
    out.extend_from_slice(MAGIC);
    out
}

/// Read a trailer back the way the kernel does. For tests and diagnosis.
pub fn read_trailer(blob: &[u8]) -> Option<String> {
    let end = blob.len().checked_sub(MAGIC.len() + 8)?;
    if &blob[end + 8..] != MAGIC {
        return None;
    }
    let size = u32::from_le_bytes(blob[end..end + 4].try_into().ok()?) as usize;
    let checksum = u32::from_le_bytes(blob[end + 4..end + 8].try_into().ok()?);
    let body = blob.get(end.checked_sub(size)?..end)?;
    if body.iter().fold(0u32, |a, b| a.wrapping_add(*b as u32)) != checksum {
        return None;
    }
    let text = String::from_utf8_lossy(body);
    Some(text.trim_end_matches('\0').to_string())
}

/// What a bootloader would add: the virtual hardware Android is running on.
///
/// Taken from a stock boot under crosvm (`docs/engine/reference`), keeping
/// what describes the device and dropping what named host services that do
/// not exist here.
pub fn runtime(boot_device: &str, vbmeta_guid: &str, memory_mib: u64, density: u32) -> String {
    let fixed = [
        ("androidboot.hardware", "cutf_cvm"),
        ("androidboot.slot_suffix", "_a"),
        ("androidboot.force_normal_boot", "1"),
        ("androidboot.fstab_suffix", "cf.f2fs.hctr2"),
        ("androidboot.serialno", "GAMEDROID0001"),
        ("androidboot.setupwizard_mode", "DISABLED"),
        ("androidboot.enable_bootanimation", "0"),
        ("androidboot.hw_timeout_multiplier", "3"),
        // Graphics: the guest draws with the CPU until the host GPU is wired in.
        ("androidboot.hardware.hwcomposer", "ranchu"),
        ("androidboot.hardware.hwcomposer.display_finder_mode", "drm"),
        // The stock value. This image fills its buffer red-first whatever
        // this says; the host corrects for it (android::true_colours).
        (
            "androidboot.hardware.hwcomposer.display_framebuffer_format",
            "rgba",
        ),
        ("androidboot.hardware.gralloc", "minigbm"),
        ("androidboot.hardware.egl", "angle"),
        ("androidboot.hardware.vulkan", "pastel"),
        (
            "androidboot.hardware.angle_feature_overrides_enabled",
            "mapUnspecifiedColorSpaceToPassThrough",
        ),
        (
            "androidboot.hardware.angle_feature_overrides_disabled",
            "exposeES32ForTesting",
        ),
        ("androidboot.cpuvulkan.version", "4206592"),
        ("androidboot.opengles.version", "196609"),
        // Security services that run inside the guest and need no host peer.
        (
            "androidboot.vendor.apex.com.android.hardware.keymint",
            "com.android.hardware.keymint.rust_nonsecure",
        ),
        (
            "androidboot.vendor.apex.com.android.hardware.gatekeeper",
            "com.android.hardware.gatekeeper.nonsecure",
        ),
        (
            "androidboot.vendor.apex.com.android.hardware.graphics.composer",
            "com.android.hardware.graphics.composer.ranchu",
        ),
        (
            "androidboot.vendor.apex.com.android.hardware.secure_element",
            "com.android.hardware.secure_element",
        ),
        (
            "androidboot.vendor.apex.com.android.hardware.strongbox",
            "none",
        ),
        (
            "androidboot.vendor.apex.com.android.hardware.weaver",
            "none",
        ),
        (
            "androidboot.vendor.apex.com.google.emulated.camera.provider.hal",
            "com.google.emulated.camera.provider.hal",
        ),
        ("androidboot.hypervisor.vm.supported", "0"),
        ("androidboot.hypervisor.protected_vm.supported", "0"),
        ("androidboot.hypervisor.version", "gamedroid-bvmm"),
        // A root shell on the serial port (this is a userdebug image). Until
        // adb works it is how the host runs commands in Android.
        ("androidboot.serialconsole", "1"),
        ("androidboot.console", "ttyS0"),
        // Services that listen on vsock for a host peer. Nobody connects
        // yet, but they need the address to start.
        ("androidboot.vsock_lights_port", "6900"),
        ("androidboot.vsock_lights_cid", "3"),
        ("androidboot.vsock_tombstone_port", "6600"),
        ("androidboot.modem_simulator_ports", "9600"),
        // Wi-Fi made from a plain network card (the third one; see the
        // machine's network cards in android.rs). Without this setting the
        // image starts no Wi-Fi at all.
        ("androidboot.wifi_impl", "virt_wifi"),
        // Verified boot: unlocked, as on the stock virtual device.
        ("androidboot.vbmeta.avb_version", "1.2"),
        ("androidboot.vbmeta.device_state", "unlocked"),
        ("androidboot.vbmeta.hash_alg", "sha256"),
        ("androidboot.vbmeta.invalidate_on_error", "yes"),
        ("androidboot.veritymode", "enforcing"),
        ("androidboot.verifiedbootstate", "orange"),
    ];
    let mut out = String::new();
    for (k, v) in fixed {
        out.push_str(&format!("{k} = \"{v}\"\n"));
    }
    out.push_str(&format!("androidboot.boot_devices = \"{boot_device}\"\n"));
    out.push_str(&format!("androidboot.ddr_size = \"{memory_mib}MB\"\n"));
    out.push_str(&format!("androidboot.lcd_density = \"{density}\"\n"));
    out.push_str(&format!(
        "androidboot.vbmeta.device = \"PARTUUID={vbmeta_guid}\"\n"
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_found_whatever_the_operator() {
        assert_eq!(key("a.b = 1"), Some("a.b"));
        assert_eq!(key("  a.b:= 1"), Some("a.b"));
        assert_eq!(key("a.b += \"x=y\""), Some("a.b"));
        assert_eq!(key("flag"), Some("flag"));
        assert_eq!(key("# note = 1"), None);
        assert_eq!(key("   "), None);
    }

    #[test]
    fn ours_replace_theirs_and_no_key_is_left_twice() {
        let merged = merge("a = 1\n# note\nb = \"x\"\nc := 3\n", "b = \"y\"\nd = 4\n").unwrap();
        assert_eq!(merged, "a = 1\n# note\nc := 3\nb = \"y\"\nd = 4\n");
        assert_eq!(merge("", "a = 1\n").unwrap(), "a = 1\n");
        assert!(merge("", "a = 1\na = 2\n").unwrap_err().contains("a twice"));
        assert!(merge("z = 1\nz = 2\n", "a = 1\n").is_err());
    }

    #[test]
    fn the_runtime_settings_never_repeat_a_key() {
        let text = runtime("pci0000:00/0000:00:01.0", "1234", 4096, 280);
        let keys: Vec<&str> = text.lines().filter_map(key).collect();
        let mut unique = keys.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(keys.len(), unique.len());
        assert!(text.contains("androidboot.boot_devices = \"pci0000:00/0000:00:01.0\""));
        assert!(text.contains("androidboot.ddr_size = \"4096MB\""));
        assert!(text.contains("androidboot.vbmeta.device = \"PARTUUID=1234\""));
        assert!(merge("", &text).is_ok());
    }

    #[test]
    fn the_trailer_has_the_kernels_layout() {
        for initrd_len in [0usize, 1, 2, 3, 4097] {
            let t = trailer("a = 1", initrd_len);
            assert!(t.ends_with(MAGIC));
            assert_eq!((initrd_len + t.len()) % 4, 0);
            let size = u32::from_le_bytes(t[t.len() - 20..t.len() - 16].try_into().unwrap());
            assert_eq!(size as usize, t.len() - 20);
            assert!(t.starts_with(b"a = 1\n\0"));
            let mut blob = vec![0x55u8; initrd_len];
            blob.extend_from_slice(&t);
            assert_eq!(read_trailer(&blob).as_deref(), Some("a = 1\n"));
        }
        assert_eq!(read_trailer(b"no trailer here"), None);
        let mut bad = trailer("a = 1", 0);
        bad[0] ^= 1;
        assert_eq!(read_trailer(&bad), None, "checksum");
    }
}
