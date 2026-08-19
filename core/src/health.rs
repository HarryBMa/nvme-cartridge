//! How well is this cartridge actually connected?
//!
//! Three things decide whether a cartridge performs like the drive inside it,
//! and all three are invisible until you go looking:
//!
//!   * **The negotiated link.** A front-panel port, a hub or a charge-only
//!     cable quietly gives you 5 Gbps, or USB 2.0's 480 Mbps, and nothing says
//!     so. People diagnose this as a slow drive for months.
//!   * **UASP or BOT.** Bulk-Only Transport allows one command in flight; UASP
//!     queues them. On the small random reads a game streams, that is the
//!     difference between smooth and stuttering, and which one you get depends
//!     on the enclosure's firmware and the port it is in.
//!   * **How full it is.** Almost every M.2 2230 drive is DRAM-less and leans
//!     on the Host Memory Buffer — host RAM, borrowed over PCIe, holding the
//!     flash translation table. A USB bridge does not provide HMB, so the
//!     translation table is paged from the flash itself, and the fuller the
//!     drive, the more that costs.
//!
//! On Linux all of this is read straight out of sysfs — no processes, no
//! libraries. On Windows the transport is asked for once, lazily, through
//! PowerShell; the link speed is not reported there, and this says so rather
//! than guessing.

use std::path::Path;

use serde::Serialize;

/// Above this, a DRAM-less drive's garbage collection has little room to work.
const CROWDED_PERCENT: u8 = 85;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    /// "10 Gbps", "5 Gbps", "480 Mbps", or empty when it cannot be read.
    pub link: String,
    /// Raw negotiated speed, for the caller that wants to compare rather than
    /// print.
    pub link_mbps: Option<u32>,
    /// "UASP" or "BOT", or empty when unknown.
    pub transport: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// 0-100. Saturates rather than dividing by zero on an unreadable volume.
    pub used_percent: u8,
    /// Things worth telling the user, in plain sentences.
    pub warnings: Vec<String>,
}

/// Look at the cartridge mounted at `mount`.
pub fn inspect(mount: &str) -> Health {
    let (total_bytes, free_bytes) = capacity(Path::new(mount));
    let used_percent = if total_bytes == 0 {
        0
    } else {
        let used = total_bytes.saturating_sub(free_bytes);
        ((used as f64 / total_bytes as f64) * 100.0)
            .round()
            .min(100.0) as u8
    };

    let link = probe(mount);
    let mut health = Health {
        link: link
            .as_ref()
            .and_then(|l| l.mbps)
            .map(speed_label)
            .unwrap_or_default(),
        link_mbps: link.as_ref().and_then(|l| l.mbps),
        transport: link.map(|l| l.transport).unwrap_or_default(),
        total_bytes,
        free_bytes,
        used_percent,
        warnings: Vec::new(),
    };
    health.warnings = advise(&health);
    health
}

/// What is worth saying about a connection, given what was measured.
///
/// Split out from the reading so the wording is testable without a drive.
fn advise(health: &Health) -> Vec<String> {
    let mut out = Vec::new();

    match health.link_mbps {
        Some(mbps) if mbps < 1000 => out.push(format!(
            "Connected at {} — this is a USB 2.0 port or cable. Games will stream badly. \
             Try a different port, and the cable the enclosure came with.",
            speed_label(mbps)
        )),
        Some(mbps) if mbps < 10_000 => out.push(format!(
            "Connected at {}, about half what the enclosure can do. That is usually a \
             front-panel port, a hub, or a cable that is not rated for 10 Gbps.",
            speed_label(mbps)
        )),
        _ => {}
    }

    if health.transport == "BOT" {
        out.push(
            "Running in BOT mode, which sends one command at a time. UASP queues them and \
             is worth roughly two to three times as much on the small random reads a game \
             streams. Usually a different port or enclosure firmware fixes it."
                .to_string(),
        );
    }

    if health.used_percent >= CROWDED_PERCENT {
        out.push(format!(
            "{}% full. These drives have no DRAM of their own and cannot borrow host memory \
             over USB, so the last 15% costs more than it looks like it should. Leaving some \
             room back keeps random reads quick.",
            health.used_percent
        ));
    }

    out
}

fn speed_label(mbps: u32) -> String {
    if mbps >= 1000 && mbps.is_multiple_of(1000) {
        format!("{} Gbps", mbps / 1000)
    } else if mbps >= 1000 {
        format!("{:.1} Gbps", mbps as f64 / 1000.0)
    } else {
        format!("{mbps} Mbps")
    }
}

/// What the bus says about this drive.
struct Link {
    mbps: Option<u32>,
    transport: String,
}

// --------------------------------------------------------------------------
// Linux: sysfs
// --------------------------------------------------------------------------

#[cfg(not(windows))]
fn capacity(mount: &Path) -> (u64, u64) {
    crate::drives::list_drives()
        .into_iter()
        .find(|d| Path::new(&d.path) == mount)
        .map(|d| (d.total_bytes, d.free_bytes))
        .unwrap_or((0, 0))
}

#[cfg(not(windows))]
fn probe(mount: &str) -> Option<Link> {
    let mounts = std::fs::read_to_string("/proc/mounts").ok()?;
    let device = device_for_mount(&mounts, Path::new(mount))?;
    let block = block_name(&device)?;
    usb_link(Path::new("/sys"), &block)
}

/// The device backing a mount point, from `/proc/mounts`.
#[cfg(not(windows))]
fn device_for_mount(mounts: &str, mount: &Path) -> Option<String> {
    crate::drives::parse_proc_mounts(mounts)
        .into_iter()
        .find(|entry| entry.mount == mount)
        .map(|entry| entry.device)
        .filter(|device| device.starts_with("/dev/"))
}

/// `/dev/sdb1` is a partition; sysfs holds the link details on the disk, `sdb`.
///
/// The two naming schemes have to be told apart: `nvme0n1` and `mmcblk0` end in
/// digits that are part of the disk's name, and only gain a `p1` when they are
/// partitioned, while `sdb` gains a bare `1`.
#[cfg(not(windows))]
fn block_name(device: &str) -> Option<String> {
    let name = device.strip_prefix("/dev/")?;
    if name.is_empty() {
        return None;
    }

    // nvme0n1p3 -> nvme0n1, mmcblk0p1 -> mmcblk0. The disk part ends in a digit,
    // which is what separates this from a name that merely contains a "p".
    if let Some((disk, partition)) = name.rsplit_once('p') {
        if !partition.is_empty()
            && partition.bytes().all(|b| b.is_ascii_digit())
            && disk.ends_with(|c: char| c.is_ascii_digit())
        {
            return Some(disk.to_string());
        }
    }

    // sdb1 -> sdb. Only for the schemes where trailing digits really are the
    // partition number.
    if ["sd", "vd", "hd"].iter().any(|p| name.starts_with(p)) {
        let disk = name.trim_end_matches(|c: char| c.is_ascii_digit());
        if !disk.is_empty() {
            return Some(disk.to_string());
        }
    }

    Some(name.to_string())
}

/// Walk from a block device up to the USB device that carries it.
///
/// `sys_root` is a parameter so this can be pointed at a fake tree in the
/// tests; in use it is `/sys`.
#[cfg(not(windows))]
fn usb_link(sys_root: &Path, block: &str) -> Option<Link> {
    // /sys/class/block/sdb -> ../../devices/…/usb2/2-4/2-4:1.0/host6/…/block/sdb
    let resolved = std::fs::read_link(sys_root.join("class/block").join(block)).ok()?;
    let full = sys_root.join("class/block").join(resolved);

    // The USB interface directory is the one named like "2-4:1.0"; its driver
    // is uas or usb-storage, and its parent is the device that knows the speed.
    let mut interface: Option<std::path::PathBuf> = None;
    let mut walked = std::path::PathBuf::new();
    for part in full.components() {
        walked.push(part);
        let name = match part.as_os_str().to_str() {
            Some(name) => name,
            None => continue,
        };
        if is_usb_interface(name) {
            interface = Some(walked.clone());
        }
    }
    let interface = interface?;

    let transport = std::fs::read_link(interface.join("driver"))
        .ok()
        .and_then(|p| p.file_name().and_then(|n| n.to_str()).map(transport_name))
        .unwrap_or_default();

    let mbps = interface
        .parent()
        .and_then(|device| std::fs::read_to_string(device.join("speed")).ok())
        .and_then(|text| text.trim().parse::<f64>().ok())
        .map(|speed| speed.round() as u32);

    Some(Link { mbps, transport })
}

/// USB interface directories are named `<bus>-<port>:<config>.<interface>`.
#[cfg(not(windows))]
fn is_usb_interface(name: &str) -> bool {
    let Some((device, config)) = name.split_once(':') else {
        return false;
    };
    device.contains('-') && device.starts_with(|c: char| c.is_ascii_digit()) && config.contains('.')
}

#[cfg(not(windows))]
fn transport_name(driver: &str) -> String {
    match driver {
        "uas" => "UASP".to_string(),
        "usb-storage" => "BOT".to_string(),
        other => other.to_string(),
    }
}

// --------------------------------------------------------------------------
// Windows
// --------------------------------------------------------------------------

#[cfg(windows)]
fn capacity(mount: &Path) -> (u64, u64) {
    crate::drives::list_drives()
        .into_iter()
        .find(|d| Path::new(&d.path) == mount)
        .map(|d| (d.total_bytes, d.free_bytes))
        .unwrap_or((0, 0))
}

/// Windows does not publish the negotiated USB speed anywhere cheap, so only
/// the transport is asked for — and only when someone opens the details.
#[cfg(windows)]
fn probe(mount: &str) -> Option<Link> {
    let letter = mount.trim_end_matches('\\').trim_end_matches(':');
    if letter.len() != 1 {
        return None;
    }

    // The service driving the disk is the tell: uaspstor is UASP, USBSTOR is
    // BOT. Anything unexpected is reported as-is rather than guessed at.
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         $p = Get-Partition -DriveLetter {letter}; \
         $d = Get-PnpDevice -InstanceId (Get-Disk -Number $p.DiskNumber).Path.Split('#')[1..2] \
              -ErrorAction SilentlyContinue; \
         (Get-PhysicalDisk | Where-Object DeviceId -eq $p.DiskNumber).BusType"
    );

    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    let bus = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if bus.is_empty() {
        return None;
    }

    Some(Link {
        mbps: None,
        transport: windows_transport(&bus),
    })
}

/// Windows reports a bus type rather than the transport; only USB is
/// interesting here, and it does not say which USB protocol is in use.
#[cfg(windows)]
fn windows_transport(bus: &str) -> String {
    match bus.trim() {
        "USB" => String::new(),
        other if other.is_empty() => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_read_the_way_people_say_them() {
        assert_eq!(speed_label(480), "480 Mbps");
        assert_eq!(speed_label(5000), "5 Gbps");
        assert_eq!(speed_label(10_000), "10 Gbps");
        assert_eq!(speed_label(20_000), "20 Gbps");
        // Not every bridge reports a round number.
        assert_eq!(speed_label(1500), "1.5 Gbps");
    }

    #[test]
    fn a_good_connection_has_nothing_to_say() {
        let health = Health {
            link: "10 Gbps".into(),
            link_mbps: Some(10_000),
            transport: "UASP".into(),
            total_bytes: 128_000_000_000,
            free_bytes: 64_000_000_000,
            used_percent: 50,
            warnings: Vec::new(),
        };
        assert!(advise(&health).is_empty());
    }

    #[test]
    fn a_slow_port_is_named_as_the_port_not_the_drive() {
        let health = Health {
            link_mbps: Some(5000),
            transport: "UASP".into(),
            ..Default::default()
        };
        let said = advise(&health).join(" ");
        assert!(said.contains("5 Gbps"), "{said}");
        assert!(said.contains("front-panel"), "{said}");

        let health = Health {
            link_mbps: Some(480),
            ..Default::default()
        };
        let said = advise(&health).join(" ");
        assert!(said.contains("USB 2.0"), "{said}");
    }

    #[test]
    fn bot_and_a_full_drive_are_both_worth_saying() {
        let health = Health {
            link_mbps: Some(10_000),
            transport: "BOT".into(),
            used_percent: 92,
            ..Default::default()
        };
        let said = advise(&health);
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].contains("UASP"), "{said:?}");
        assert!(said[1].contains("92%"), "{said:?}");
    }

    #[cfg(not(windows))]
    #[test]
    fn transports_are_named_the_way_the_docs_do() {
        assert_eq!(transport_name("uas"), "UASP");
        assert_eq!(transport_name("usb-storage"), "BOT");
        // Anything else is passed through rather than guessed at.
        assert_eq!(transport_name("ums-realtek"), "ums-realtek");
    }

    #[cfg(not(windows))]
    #[test]
    fn a_partition_resolves_to_the_disk_that_holds_it() {
        assert_eq!(block_name("/dev/sdb1").unwrap(), "sdb");
        assert_eq!(block_name("/dev/sdb").unwrap(), "sdb");
        assert_eq!(block_name("/dev/nvme0n1p3").unwrap(), "nvme0n1");
        assert_eq!(block_name("/dev/mmcblk0p1").unwrap(), "mmcblk0");
        assert_eq!(block_name("/dev/nvme0n1").unwrap(), "nvme0n1");
        assert!(block_name("tmpfs").is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn usb_interface_directories_are_recognised() {
        assert!(is_usb_interface("2-4:1.0"));
        assert!(is_usb_interface("1-1.2:1.0"));
        // The device itself, its bus, and PCI addresses are not interfaces.
        assert!(!is_usb_interface("2-4"));
        assert!(!is_usb_interface("usb2"));
        assert!(!is_usb_interface("0000:00:14.0"));
        assert!(!is_usb_interface("host6"));
    }

    #[cfg(not(windows))]
    #[test]
    fn finds_the_mount_in_proc_mounts() {
        let mounts = "\
/dev/sda2 / ext4 rw,relatime 0 0
/dev/sdb1 /run/media/harry/CINDER exfat rw,nosuid 0 0
tmpfs /run tmpfs rw 0 0
";
        assert_eq!(
            device_for_mount(mounts, Path::new("/run/media/harry/CINDER")).unwrap(),
            "/dev/sdb1"
        );
        assert!(device_for_mount(mounts, Path::new("/run")).is_none());
        assert!(device_for_mount(mounts, Path::new("/nowhere")).is_none());
    }

    #[cfg(not(windows))]
    #[test]
    fn reads_the_link_out_of_a_sysfs_tree() {
        // A miniature /sys, laid out the way the kernel does it.
        let scratch = crate::testutil::Scratch::new("sysfs");
        let sys = scratch.path();
        let device = sys.join("devices/pci0000:00/0000:00:14.0/usb2/2-4");
        let interface = device.join("2-4:1.0");
        let block = interface.join("host6/target6:0:0/6:0:0:0/block/sdb");
        std::fs::create_dir_all(&block).unwrap();
        std::fs::write(device.join("speed"), b"10000\n").unwrap();

        // The driver is a symlink into /sys/bus, and only its name matters.
        let driver = sys.join("bus/usb/drivers/uas");
        std::fs::create_dir_all(&driver).unwrap();
        std::os::unix::fs::symlink(&driver, interface.join("driver")).unwrap();

        std::fs::create_dir_all(sys.join("class/block")).unwrap();
        std::os::unix::fs::symlink(&block, sys.join("class/block/sdb")).unwrap();

        let link = usb_link(sys, "sdb").expect("the tree describes a USB disk");
        assert_eq!(link.mbps, Some(10_000));
        assert_eq!(link.transport, "UASP");
    }

    #[cfg(not(windows))]
    #[test]
    fn an_internal_disk_simply_has_no_usb_link() {
        let scratch = crate::testutil::Scratch::new("sysfs-internal");
        let sys = scratch.path();
        let block = sys.join("devices/pci0000:00/0000:00:1d.0/nvme/nvme0/nvme0n1");
        std::fs::create_dir_all(&block).unwrap();
        std::fs::create_dir_all(sys.join("class/block")).unwrap();
        std::os::unix::fs::symlink(&block, sys.join("class/block/nvme0n1")).unwrap();

        assert!(usb_link(sys, "nvme0n1").is_none());
    }
}
