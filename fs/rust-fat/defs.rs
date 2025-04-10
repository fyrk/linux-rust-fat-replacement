// SPDX-License-Identifier: GPL-2.0-or-later

//! Definitions of FAT structures.

use core::mem::size_of;
use kernel::static_assert;
use kernel::types::LE;

pub(crate) const FAT_ROOT_INO: u64 = 0;

pub(crate) const FAT_BOOT_SECTOR_SIGNATURE: u16 = 0xAA55;

pub(crate) const MIN_FAT16_CLUSTERS: u32 = 4085;
pub(crate) const MIN_FAT32_CLUSTERS: u32 = 65525;

pub(crate) const FAT_DENTRY_SIZE: usize = size_of::<RawFatDirEntry>();
static_assert!(FAT_DENTRY_SIZE == 32);
pub(crate) const FAT_DENTRY_FREE: u8 = 0xE5;
pub(crate) const FAT_DENTRY_FREE_CONSECUTIVE: u8 = 0x00;

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
    pub(crate) const LONG_NAME_MASK: u8 = LONG_NAME | DIRECTORY | ARCHIVE;
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

    /// directly follows [`BiosParamBlock`] at byte 36 on FAT32 volume
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

    #[derive(Debug)]
    #[repr(C, packed)]
    pub(crate) struct RawFatDirEntry {
        /// name and extension
        /// `short_name[0] == 0xE5` => free
        /// `short_name[0] == 0x00` => free, and following entries also free
        /// `short_name[0] == 0x05` => actually 0xE5
        pub(crate) short_name: [LE<u8>; 11],
        pub(crate) attributes: LE<u8>,
        _reserved: u8,
        /// in centiseconds (0-199)
        pub(crate) creation_time_cs: LE<u8>,
        pub(crate) creation_time: LE<u16>,
        pub(crate) creation_date: LE<u16>,
        pub(crate) access_date: LE<u16>,
        /// high 16 bits of cluster, 0 for FAT12/16
        pub(crate) first_cluster_hi: LE<u16>,
        pub(crate) write_time: LE<u16>,
        pub(crate) write_date: LE<u16>,
        /// low 16 bits of cluster
        pub(crate) first_cluster_lo: LE<u16>,
        /// in bytes
        pub(crate) file_size: LE<u32>,
    }
}
