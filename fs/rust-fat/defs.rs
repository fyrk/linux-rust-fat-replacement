// SPDX-License-Identifier: GPL-2.0-or-later

//! Definitions of FAT structures.

use core::mem::size_of;
use kernel::prelude::*;
use kernel::types::LE;

pub(crate) const FAT_ROOT_INO: u32 = 0;

pub(crate) const MIN_FAT16_CLUSTERS: u32 = 4085;
pub(crate) const MIN_FAT32_CLUSTERS: u32 = 65525;

pub(crate) const FAT_DENTRY_SIZE: usize = size_of::<FatDirEntry>();

pub(crate) struct Fat32Info {
    pub(crate) root_cluster: u32,
}

#[allow(dead_code)]
pub(crate) enum FatType {
    FAT12,
    FAT16,
    FAT32(Fat32Info),
}

impl FatType {
    pub(crate) fn from_cluster_count(cluster_count: u32, bpb32: &BiosParamBlockFat32) -> FatType {
        if cluster_count < MIN_FAT16_CLUSTERS {
            FatType::FAT12
        } else if cluster_count < MIN_FAT32_CLUSTERS {
            FatType::FAT16
        } else {
            let root_cluster = bpb32.root_cluster;
            FatType::FAT32(Fat32Info {
                root_cluster: root_cluster.value(),
            })
        }
    }
}

pub(crate) enum FatEntry {
    Next(u32),
    End,
    Bad,
}

impl FatEntry {
    pub(crate) fn from_entry(fat_type: &FatType, entry: u32) -> FatEntry {
        match fat_type {
            FatType::FAT12 => {
                if entry >= 0x0FF8 {
                    FatEntry::End
                } else if entry == 0x0FF7 {
                    FatEntry::Bad
                } else {
                    FatEntry::Next(entry)
                }
            }
            FatType::FAT16 => {
                if entry >= 0xFFF8 {
                    FatEntry::End
                } else if entry == 0xFFF7 {
                    FatEntry::Bad
                } else {
                    FatEntry::Next(entry)
                }
            }
            FatType::FAT32(_) => {
                if entry >= 0x0FFFFFF8 {
                    FatEntry::End
                } else if entry == 0x0FFFFFF7 {
                    FatEntry::Bad
                } else {
                    FatEntry::Next(entry)
                }
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) mod fat_dentry_attr {
    pub(crate) const NONE: u8 = 0x00;
    pub(crate) const READ_ONLY: u8 = 0x01;
    pub(crate) const HIDDEN: u8 = 0x02;
    pub(crate) const SYSTEM: u8 = 0x04;
    pub(crate) const VOLUME_ID: u8 = 0x08;
    pub(crate) const DIRECTORY: u8 = 0x10;
    pub(crate) const ARCHIVE: u8 = 0x20;
    pub(crate) const LONG_NAME: u8 = READ_ONLY | HIDDEN | SYSTEM | VOLUME_ID;
}

#[macro_export]
/// Unwrap [`LE`] value from a packed struct.
///
/// # Examples
/// ```
/// kernel::derive_readable_from_bytes! {
///     #[repr(C, packed)]
///     struct SuperBlock {
///         a: LE<u16>,
///         b: LE<u64>,
///     }
/// }
///
/// let a = unwrap_packed!(sb.a);
/// ```
macro_rules! unwrap_packed {
    ($attr:expr) => {{
        let wrapped = $attr;
        wrapped.value()
    }};
}

kernel::derive_readable_from_bytes! {
    #[repr(packed)]
    pub(crate) struct BootSectorStart {
        pub(crate) boot_jump: [u8; 3],
        pub(crate) system_id: [u8; 8],
        pub(crate) bpb: BiosParamBlock,
    }

    /// starts at byte 11
    #[derive(Debug)]
    #[repr(packed)]
    pub(crate) struct BiosParamBlock {
        /// in bytes
        pub(crate) sector_size: LE<u16>,
        pub(crate) sectors_per_cluster: LE<u8>,
        /// from start of volume
        pub(crate) reserved_sectors: LE<u16>,
        /// count of FATs, usually 2
        pub(crate) num_fats: LE<u8>,
        /// FAT12/16: count of dentries for root directory; FAT32: `0`
        pub(crate) rootdir_entries: LE<u16>,
        /// FAT32: `0`, superseded by [`BiosParamBlock::total_sectors_32`]
        pub(crate) total_sectors_16: LE<u16>,
        pub(crate) media: LE<u8>,
        /// size of one FAT; FAT32: `0`, superseded by [`BiosParamBlockFat32::fat_sectors_32`]
        pub(crate) fat_sectors_16: LE<u16>,
        pub(crate) _ignored: [u8; 8],
        pub(crate) total_sectors_32: LE<u32>,
    }

    /// directly follows [`BiosParamBlock`] at byte 36 on FAT 32 volume
    #[repr(C, packed)]
    pub(crate) struct BiosParamBlockFat32 {
        pub(crate) fat_sectors_32: LE<u32>,
        pub(crate) flags: LE<u16>,
        pub(crate) version: [u8; 2],
        pub(crate) root_cluster: LE<u32>,
        pub(crate) info_sector: LE<u16>,
        pub(crate) backup_boot_sector: LE<u16>,
        pub(crate) _reserved: [u8; 12],
    }

    /// second boot sector portion starting at byte 36 for FAT12/16 and byte 64 for FAT32
    #[repr(C, packed)]
    pub(crate) struct BootSectorEnd {
        pub(crate) drive_number: LE<u8>,
        pub(crate) _reserved: u8,
        pub(crate) boot_signature: LE<u8>,
        pub(crate) volume_id: LE<u32>,
        pub(crate) volume_label: [u8; 11],
        pub(crate) fs_type: [u8; 8],
    }

    /// starts at byte 510
    #[repr(C, packed)]
    pub(crate) struct FatBootSectorSignature {
        pub(crate) signature: LE<u16>,
    }

    #[repr(C, packed)]
    pub(crate) struct FatDirEntry {
        /// name and extension
        /// `short_name[0] == 0xE5` => free
        /// `short_name[0] == 0x00` => free, and following entries also free
        /// `short_name[0] == 0x05` => actually 0xE5
        short_name: [u8; 11],
        attributes: LE<u8>,
        _reserved: u8,
        /// in centiseconds (0-199)
        creation_time_cs: LE<u8>,
        creation_time: LE<u16>,
        creation_date: LE<u16>,
        access_date: LE<u16>,
        /// high 16 bits of cluster, 0 for FAT12/16
        first_cluster_hi: LE<u16>,
        write_time: LE<u16>,
        write_date: LE<u16>,
        /// low 16 bits of cluster
        first_cluster_lo: LE<u16>,
        /// in bytes
        file_size: LE<u32>,
    }
}

impl FatBootSectorSignature {
    pub(crate) fn validate(&self) -> Result<()> {
        match unwrap_packed!(self.signature) {
            0xAA55 => Ok(()),
            _ => {
                pr_err!("not a FAT volume, signature mismatch\n");
                return Err(EINVAL);
            }
        }
    }
}
