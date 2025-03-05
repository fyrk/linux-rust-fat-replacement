// SPDX-License-Identifier: GPL-2.0-or-later

//! FAT file system.

use defs::*;
use kernel::fs::{
    self, address_space, dentry, dentry::DEntry, file, inode, inode::INode, iomap, mode, sb,
    sb::SuperBlock, Offset,
};
use kernel::time::Timespec;
use kernel::types::{ARef, Either, FromBytes, Locked, LE};
use kernel::{c_str, prelude::*, uapi};

pub mod defs;

kernel::module_fs! {
    type: FatFs,
    name: "fat",
    author: "Flora Rädiker <flora.raediker@student.hpi.uni-potsdam.de>",
    description: "FAT file system",
    license: "GPL",
}

#[allow(dead_code)]
struct INodeData {
    /// first data cluster
    first_cluster: u32,
}

struct FatFs {
    mapper: inode::Mapper,
    fat_type: FatType,
    /// Size of one sector in Bytes.
    sector_size: u16,
    /// Number of sectors per cluster.
    sectors_per_cluster: u8,
    /// Size of one cluster in Bytes.
    cluster_size: u32,
    /// Count of data clusters.
    cluster_count: u32,
    first_fat_sector: Offset,
    first_data_sector: u32,
    rootdir_entries: u16,
}

/*/// Convert FAT attribute bits and a mode mask to UNIX mode
fn fat_attr_to_mode(s: &FatFs, attrs: u8, mask: u16) {
    if attrs & fat_dentry_attr::READ_ONLY && (!(attrs & fat_dentry_attr::DIRECTORY)) || s.options.rodir) {
        mode &= ~S_IWUGO;
    }
}*/

impl FatFs {
    fn iget(sb: &SuperBlock<Self>, ino: u32) -> Result<ARef<INode<Self>>> {
        let s = sb.data();

        // Create an inode or find an existing (cached) one.
        let mut inode: inode::New<Self> = match sb.get_or_create_inode(ino.into())? {
            Either::Left(existing) => return Ok(existing),
            Either::Right(new) => new,
        };

        // TODO: this is for the root inode for now
        //if ino == FAT_ROOT_INO {

        // size in bytes
        let size: i64;
        // size in clusters
        let blocks: u64;
        match s.fat_type {
            FatType::FAT32 { root_cluster } => {
                blocks = Self::get_cluster_chain_length(s, root_cluster)?.try_into()?;
                size = (blocks as i64) * (s.cluster_size as i64);
            }
            _ => {
                size = (s.rootdir_entries as i64) * (FAT_DENTRY_SIZE as i64);
                // this is what C does: how many 512 byte blocks do we need given we store `size` in clusters of size `cluster_size`?
                blocks = (size as u64).next_multiple_of(s.cluster_size as u64) / 512;
            }
        };

        let first_cluster = match s.fat_type {
            FatType::FAT32 { root_cluster } => root_cluster,
            _ => 0, // TODO
        };

        const DIR_FOPS: file::Ops<FatFs> = file::Ops::new::<FatFs>();
        const DIR_IOPS: inode::Ops<FatFs> = inode::Ops::new::<FatFs>();
        const FILE_AOPS: address_space::Ops<FatFs> = iomap::ro_aops::<FatFs>();

        inode
            .set_iops(DIR_IOPS)
            .set_fops(DIR_FOPS)
            .set_aops(FILE_AOPS);

        // root directory has no entry itself and thus no time
        let time = Timespec::new(0, 0)?;

        inode.init(inode::Params {
            typ: inode::Type::Dir,
            mode: mode::S_IRUGO,
            size,
            blocks,
            nlink: 2,
            uid: 1000, // TODO: get from options
            gid: 1000,
            ctime: time,
            mtime: time,
            atime: time,
            value: INodeData { first_cluster },
        })
        //}
    }

    /// Iterates over the cluster numbers within the cluster chain starting at `first_cluster`.
    /// If it is not a bad cluster, the first cluster is `first_cluster`.
    ///
    /// The iterator ends when the end of the cluster chain is encountered.
    /// If a bad cluster is encountered, this gives `Err(EIO)` and ends the iteration.
    fn iter_cluster_chain(&self, first_cluster: u32) -> ClusterIter<'_> {
        ClusterIter {
            fs: &self,
            first_cluster,
            next_cluster: Some(first_cluster),
        }
    }

    /// Calculate the count of clusters in a cluster chain which starts at `first_cluster`.
    fn get_cluster_chain_length(&self, first_cluster: u32) -> Result<usize, Error> {
        if let Some((last_index, last)) = self.iter_cluster_chain(first_cluster).enumerate().last()
        {
            last?;
            return Ok(last_index + 1);
        }
        unreachable!("ChainIterator always returns at least one element")
    }
}

pub(crate) struct Cluster<'a> {
    fs: &'a FatFs,
    cluster_number: u32,
}

impl Cluster<'_> {
    fn for_each_page<U>(
        &self,
        first: Offset,
        mut cb: impl FnMut(&[u8]) -> Result<Option<U>>,
    ) -> Result<Option<U>> {
        // TODO: currently only takes first page into account
        let first_cluster_sector = ((self.cluster_number - 2)
            * (self.fs.sectors_per_cluster as u32))
            + self.fs.first_data_sector;
        let data = self
            .fs
            .mapper
            .mapped_folio(Offset::from(first_cluster_sector) * Offset::from(self.fs.sector_size))?;
        let avail = data.len();
        if first <= Offset::try_from(avail).unwrap() {
            // TODO: modelled after INode::for_each_page; this makes sense once this iterates over all pages
            let ret = cb(&data[first as usize..avail])?;
            if ret.is_some() {
                return Ok(ret);
            }
        }
        Ok(None)
    }
}

/// An iterator over a cluster chain, iterating over all cluster numbers.
///
/// Only the last element can be an [`Err`] and at least one element is always returned.
pub(crate) struct ClusterIter<'a> {
    fs: &'a FatFs,
    first_cluster: u32,
    next_cluster: Option<u32>,
}

impl ClusterIter<'_> {
    /// Reads the FAT entry for `cluster`. `cluster` is at least 2 and at most `cluster_count + 1`.
    fn read_fat_entry(&self, cluster: u32) -> Result<FatEntry, Error> {
        let s = self.fs;

        if !(2 <= cluster && cluster <= s.cluster_count + 1) {
            pr_err!(
                "cluster {} out of range for {} data clusters\n",
                cluster,
                s.cluster_count
            );
            return Err(EINVAL);
        }

        let sector_size = Offset::from(s.sector_size);

        let fat_offset = match s.fat_type {
            FatType::FAT32 { root_cluster: _ } => cluster * 4,
            FatType::FAT16 => cluster * 2,
            FatType::FAT12 => todo!(),
        };
        let fat_sector = s.first_fat_sector + (Offset::from(fat_offset) / sector_size);
        let entry_offset = fat_offset % u32::from(s.sector_size);

        let data = s.mapper.mapped_folio(fat_sector * sector_size)?;

        match s.fat_type {
            FatType::FAT32 { root_cluster: _ } => {
                let entry = LE::<u32>::from_bytes(&data, entry_offset as usize).ok_or(EIO)?;
                // ignore high four bits
                let entry = entry.value() & 0x0FFFFFFF;
                Ok(FatEntry::from_entry(&s.fat_type, entry))
            }
            _ => todo!(),
        }
    }
}

impl Iterator for ClusterIter<'_> {
    type Item = Result<u32, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(cluster) = self.next_cluster {
            match self.read_fat_entry(cluster) {
                Ok(fat_entry) => match fat_entry {
                    FatEntry::Next(next) => {
                        let previous = cluster;
                        self.next_cluster = Some(next);
                        Some(Ok(previous))
                    }
                    FatEntry::End => {
                        let previous = cluster;
                        self.next_cluster = None;
                        Some(Ok(previous))
                    }
                    FatEntry::Bad => {
                        pr_err!(
                            "bad cluster chain starting at {} (contains bad cluster {})\n",
                            self.first_cluster,
                            cluster
                        );
                        self.next_cluster = None;
                        Some(Err(EIO))
                    }
                },
                Err(e) => Some(Err(e)),
            }
        } else {
            None
        }
    }
}

impl fs::FileSystem for FatFs {
    type Data = KBox<Self>;
    type INodeData = INodeData;
    const NAME: &'static CStr = c_str!("rust-fat");
    const SUPER_TYPE: sb::Type = sb::Type::BlockDev;

    fn fill_super(
        sb: &mut SuperBlock<Self, sb::New>,
        mapper: Option<inode::Mapper>,
    ) -> Result<Self::Data> {
        macro_rules! validate {
            ($name:expr, $value:expr, $valid:expr) => {
                if !($valid) {
                    pr_err!("invalid {} = {}\n", $name, $value);
                    return Err(EINVAL);
                }
            };
        }

        let Some(mapper) = mapper else {
            pr_err!("mapper is missing\n");
            return Err(EINVAL);
        };

        if sb.min_blocksize(512) == 0 {
            pr_err!("unable to set block size\n");
            return Err(EIO);
        }

        let mapped = mapper.mapped_folio(0)?;

        let Some(signature) = FatBootSectorSignature::from_bytes(&mapped, 510) else {
            pr_err!("failed to read FAT signature\n");
            return Err(EIO);
        };
        signature.validate()?;

        let Some(bs) = BootSectorStart::from_bytes(&mapped, 0) else {
            pr_err!("failed to read boot sector\n");
            return Err(EIO);
        };
        let bpb: &BiosParamBlock = &bs.bpb;

        pr_info!("got {:?}\n", bpb);

        let sector_size = unwrap_packed!(bpb.sector_size);
        validate!(
            "sector size",
            sector_size,
            sector_size >= 512 && sector_size <= 4096 && sector_size.is_power_of_two()
        );

        if sb.min_blocksize(sector_size as i32) != sector_size as i32 {
            pr_err!("sector size {sector_size} not supported\n");
            return Err(EIO);
        }

        let rootdir_entries = unwrap_packed!(bpb.rootdir_entries);
        let first_fat_sector = unwrap_packed!(bpb.reserved_sectors);
        validate!("reserved sectors", first_fat_sector, first_fat_sector > 0);

        let sectors_per_cluster = unwrap_packed!(bpb.sectors_per_cluster);
        validate!(
            "sectors per cluster",
            sectors_per_cluster,
            sectors_per_cluster > 0 && sectors_per_cluster.is_power_of_two()
        );

        let num_fats = unwrap_packed!(bpb.num_fats);
        validate!("num fats", num_fats, num_fats > 0);

        let total_sectors: u32 = {
            let total_sectors_16 = unwrap_packed!(bpb.total_sectors_16);
            if total_sectors_16 != 0 {
                total_sectors_16 as u32
            } else {
                unwrap_packed!(bpb.total_sectors_32)
            }
        };
        validate!(
            "total sectors",
            total_sectors,
            0 < total_sectors && u64::from(total_sectors) <= sb.sector_count()
        );

        // TODO: only do this when it's certainly FAT32
        let Some(bpb32): Option<&BiosParamBlockFat32> =
            BiosParamBlockFat32::from_bytes(&mapped, 36)
        else {
            pr_err!("failed to read FAT32 Bios Parameter Block\n");
            return Err(EIO);
        };

        let fat_sectors: u32 = {
            let fat_sectors_16 = unwrap_packed!(bpb.fat_sectors_16);
            if fat_sectors_16 != 0 {
                fat_sectors_16 as u32
            } else {
                unwrap_packed!(bpb32.fat_sectors_32)
            }
        };
        validate!("fat size", fat_sectors, fat_sectors > 0);

        // layout:
        // - reserved sectors (length `reserved_sectors`)
        //   - Boot Sector
        //   - Bios Parameter Block
        //   - (Extended BPB for FAT32)
        // - FATs (`num_fats` times of size `fat_size`)
        // - rootdir sectors (length `rootdir_sectors`, 0 for FAT32)
        // - data sectors (length `data_sectors`)

        let first_rootdir_sector = (first_fat_sector as u32) + ((num_fats as u32) * fat_sectors);
        let rootdir_sectors: u32 = ((rootdir_entries as u32) * 32).div_ceil(sector_size as u32);
        let first_data_sector = first_rootdir_sector + rootdir_sectors;
        let data_sectors = total_sectors - first_data_sector;

        let cluster_count = data_sectors / (sectors_per_cluster as u32);

        let fat_type = FatType::from_cluster_count(cluster_count, first_rootdir_sector, bpb32);

        sb.set_magic(uapi::MSDOS_SUPER_MAGIC as usize);

        drop(mapped);

        Ok(KBox::new(
            FatFs {
                mapper,
                fat_type,
                sector_size,
                sectors_per_cluster,
                cluster_size: (sector_size as u32) * (sectors_per_cluster as u32),
                cluster_count,
                first_fat_sector: Offset::from(first_fat_sector),
                first_data_sector,
                rootdir_entries,
            },
            GFP_KERNEL,
        )?)
    }

    #[allow(unused_variables)]
    fn init_root(sb: &SuperBlock<Self>) -> Result<dentry::Root<Self>> {
        let inode = Self::iget(sb, FAT_ROOT_INO)?;
        dentry::Root::try_new(inode)
    }
}

#[vtable]
impl file::Operations for FatFs {
    type FileSystem = Self;

    fn read_dir(
        _file: &file::File<Self::FileSystem>,
        inode: &kernel::types::Locked<&INode<Self::FileSystem>, inode::ReadSem>,
        emitter: &mut file::DirEmitter,
    ) -> Result {
        let s = inode.super_block().data();
        match s.fat_type {
            FatType::FAT32 { root_cluster } => {
                let root_cluster = Cluster {
                    fs: s,
                    cluster_number: root_cluster,
                };
                // TODO: only takes first page of first cluster into account for now
                root_cluster.for_each_page(emitter.pos(), |data| {
                    let mut offset = 0usize;
                    let mut acc: Offset = 0;
                    let limit = data.len().saturating_sub(FAT_DENTRY_SIZE);
                    while offset < limit {
                        let entry = FatDirEntry::from_bytes(data, offset).ok_or(EIO)?;
                        offset += FAT_DENTRY_SIZE;
                        acc += Offset::try_from(FAT_DENTRY_SIZE)?;

                        let (name, name_len) = match entry.name() {
                            FatDirEntryName::Free => continue,
                            FatDirEntryName::FreeConsecutive => return Ok(Some(())),
                            FatDirEntryName::Name { short_name, len } => (short_name, len),
                        };

                        let t = match entry.get_type() {
                            None => continue,
                            Some(t) => t,
                        };

                        if !emitter.emit(acc, &name[..name_len], entry.first_cluster().into(), t) {
                            return Ok(Some(()));
                        }
                        acc = 0;
                    }
                    Ok(None)
                })?;
                Ok(())
            }
            _ => todo!(),
        }
    }
}

#[vtable]
impl inode::Operations for FatFs {
    type FileSystem = Self;

    fn lookup(
        _parent: &Locked<&INode<Self::FileSystem>, inode::ReadSem>,
        _dentry: dentry::Unhashed<'_, Self::FileSystem>,
    ) -> Result<Option<ARef<DEntry<Self::FileSystem>>>> {
        todo!()
    }
}

impl iomap::Operations for FatFs {
    type FileSystem = Self;

    #[allow(unused)]
    fn begin<'a>(
        inode: &'a INode<Self::FileSystem>,
        pos: Offset,
        length: Offset,
        flags: u32,
        map: &mut iomap::Map<'a>,
        srcmap: &mut iomap::Map<'a>,
    ) -> Result {
        todo!()
    }
}
