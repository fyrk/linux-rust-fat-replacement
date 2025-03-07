// SPDX-License-Identifier: GPL-2.0-or-later

//! Logic structures for FAT.

use crate::defs::*;
use crate::time::*;
use kernel::prelude::*;
use kernel::time::Timespec;
use kernel::types::FromBytes;

use crate::{fat_dentry_attr, unwrap_packed, RawFatDirEntry};

pub(crate) enum FatType {
    FAT12,
    FAT16,
    FAT32 { root_cluster: u32 },
}

impl FatType {
    pub(crate) fn from_cluster_count(
        cluster_count: u32,
        _first_rootdir_sector: u32,
        bpb32: &BiosParamBlockFat32,
    ) -> FatType {
        if cluster_count < MIN_FAT16_CLUSTERS {
            FatType::FAT12
        } else if cluster_count < MIN_FAT32_CLUSTERS {
            FatType::FAT16
        } else {
            let root_cluster = bpb32.root_cluster;
            FatType::FAT32 {
                root_cluster: root_cluster.value(),
            }
        }
    }
}

pub(crate) enum FatEntry {
    Next(u32),
    End,
    Bad,
}

impl FatEntry {
    pub(crate) fn from_bytes(fat_type: &FatType, entry: u32) -> FatEntry {
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
            FatType::FAT32 { root_cluster: _ } => {
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

pub(crate) enum FatDirEntry<'a> {
    /// The directory entry is free.
    Free,
    /// This directory entry and all following directory entries are free.
    FreeConsecutive,
    /// This is an occupied entry of either a file or directory.
    Entry(RegularFatDirEntry<'a>),
    /// TODO: This is some other entry (placeholder for long names).
    Other,
}

impl<'a> FatDirEntry<'a> {
    pub(crate) fn from_bytes(data: &'a [u8], offset: usize) -> Option<Self> {
        let entry = RawFatDirEntry::from_bytes(data, offset)?;
        match unwrap_packed!(entry.short_name[0]) {
            FAT_DENTRY_FREE => return Some(FatDirEntry::Free),
            FAT_DENTRY_FREE_CONSECUTIVE => return Some(FatDirEntry::FreeConsecutive),
            _ => (),
        }
        let attributes = entry.attributes.value();
        if attributes & (fat_dentry_attr::SYSTEM | fat_dentry_attr::VOLUME_ID) != 0 {
            return Some(FatDirEntry::Other);
        }
        Some(FatDirEntry::Entry(RegularFatDirEntry(entry)))
    }
}

pub(crate) struct RegularFatDirEntry<'a>(&'a RawFatDirEntry);

impl RegularFatDirEntry<'_> {
    pub(crate) fn name(&self) -> ([u8; 12], usize) {
        let mut name = self.0.short_name.map(|x| x.value());
        if name[0] == 0x05 {
            name[0] = 0xE5;
        }
        fn get_name_len(part: &[u8]) -> usize {
            part.iter().rposition(|x| *x != b' ').map_or(0, |l| l + 1)
        }
        let base_len = get_name_len(&name[0..8]);
        let ext_len = get_name_len(&name[8..11]);
        let mut short_name = [b' '; 12];
        short_name[..base_len].copy_from_slice(&name[..base_len]);
        let len = if ext_len > 0 {
            short_name[base_len] = b'.';
            short_name[base_len + 1..base_len + 1 + ext_len].copy_from_slice(&name[8..8 + ext_len]);
            base_len + 1 + ext_len
        } else {
            base_len
        };
        (short_name, len)
    }

    /// Returns whether the entry represents a file (and a directory otherwise).
    pub(crate) fn is_file(&self) -> bool {
        let attributes = self.0.attributes.value();
        (attributes) & fat_dentry_attr::DIRECTORY == 0
    }

    pub(crate) fn first_cluster(&self) -> u32 {
        ((unwrap_packed!(self.0.first_cluster_hi) as u32) << 16)
            + (unwrap_packed!(self.0.first_cluster_lo) as u32)
    }

    pub(crate) fn file_size(&self) -> u32 {
        unwrap_packed!(self.0.file_size)
    }

    pub(crate) fn ctime(&self) -> Result<Timespec> {
        timespec_from_fat(
            unwrap_packed!(self.0.creation_date),
            unwrap_packed!(self.0.creation_time),
            unwrap_packed!(self.0.creation_time_cs),
        )
    }

    pub(crate) fn mtime(&self) -> Result<Timespec> {
        timespec_from_fat(
            unwrap_packed!(self.0.write_date),
            unwrap_packed!(self.0.write_time),
            0,
        )
    }

    pub(crate) fn atime(&self) -> Result<Timespec> {
        timespec_from_fat(unwrap_packed!(self.0.access_date), 0, 0)
    }
}
