// SPDX-License-Identifier: GPL-2.0-or-later

//! FAT file system.

use defs::*;
use kernel::fs::{
    self, address_space, dentry, dentry::DEntry, file, file::File, inode, inode::INode, iomap, sb,
    sb::SuperBlock, Offset,
};
use kernel::time::Timespec;
use kernel::types::{ARef, Either, FromBytes, Locked, LE};
use kernel::{c_str, prelude::*, uaccess, uapi};
use types::*;

pub mod defs;
mod time;
mod types;
mod utils;

kernel::module_fs! {
    type: FatFs,
    name: "fat",
    author: "Flora Rädiker <flora.raediker@student.hpi.uni-potsdam.de>",
    description: "FAT file system",
    license: "GPL",
}

struct INodeData {
    /// Contains the file's cluster numbers; or [`None`] for empty files or if this is a pre-FAT32 root inode.
    clusters: Option<KVec<u32>>,
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
    num_fats: u8,
    fat_sectors: u32,
}

impl FatFs {
    /// Returns the inode associated with the given directory entry;
    /// if the entry is [`None`], returns the root inode.
    fn iget(
        sb: &SuperBlock<Self>,
        ino: u64,
        entry: Option<RegularFatDirEntry<'_>>,
    ) -> Result<ARef<INode<Self>>> {
        let s = sb.data();

        // Create an inode or find an existing (cached) one.
        let mut inode: inode::New<Self> = match sb.get_or_create_inode(ino)? {
            Either::Left(existing) => return Ok(existing),
            Either::Right(new) => new,
        };

        let size;
        let blocks: u64;
        let is_file;
        let (ctime, mtime, atime);
        let clusters;

        match entry {
            None => {
                match s.fat_type {
                    FatType::FAT32 { root_cluster } => {
                        clusters = Some({
                            let clusters = s.get_cluster_chain(root_cluster)?;
                            blocks = clusters.len().try_into()?;
                            clusters
                        });
                        size = blocks as i64 * s.cluster_size as i64;
                    }
                    _ => {
                        clusters = None;
                        size = s.rootdir_entries as i64 * FAT_DENTRY_SIZE as i64;
                        // This is what `fs/fat/inode.c` does: how many 512 byte blocks do we need given we store `size` in clusters of size `cluster_size`?
                        blocks = (size as u64).next_multiple_of(s.cluster_size as u64) / 512;
                    }
                };

                // Default values for the root inode, which has no directory entry itself.
                is_file = false;

                let t = Timespec::new(0, 0)?;
                (ctime, mtime, atime) = (t, t, t);
            }
            Some(entry) => {
                clusters = if entry.file_size() == 0 && entry.is_file() {
                    blocks = 0;
                    None
                } else {
                    Some({
                        let clusters = s.get_cluster_chain(entry.first_cluster())?;
                        blocks = clusters.len().try_into()?;
                        clusters
                    })
                };

                size = if entry.is_file() {
                    entry.file_size().into()
                } else {
                    let blocks: i64 = blocks.try_into()?;
                    let cluster_size: i64 = s.cluster_size.into();
                    blocks * cluster_size
                };

                is_file = entry.is_file();

                ctime = entry.ctime()?;
                mtime = entry.mtime()?;
                atime = entry.atime()?;
            }
        };

        const DIR_FOPS: file::Ops<FatFs> = file::Ops::new::<FatFs>();
        const DIR_IOPS: inode::Ops<FatFs> = inode::Ops::new::<FatFs>();
        // const FILE_AOPS: address_space::Ops<FatFs> = iomap::ro_aops::<FatFs>();
        const FILE_AOPS: address_space::Ops<FatFs> = iomap::rw_aops::<FatFs>();

        let mut mode = fs::mode::S_IRUGO; // TODO
        let typ = if is_file {
            mode |= fs::mode::S_IFREG;
            mode |= fs::mode::S_IWUSR;
            // let file_ops = file::Ops::generic_ro_file();

            inode.set_fops(DIR_FOPS).set_aops(FILE_AOPS);
            inode::Type::Reg
        } else {
            mode |= fs::mode::S_IFDIR;
            inode
                .set_iops(DIR_IOPS)
                .set_fops(DIR_FOPS)
                .set_aops(FILE_AOPS);
            inode::Type::Dir
        };

        inode.init(inode::Params {
            typ,
            mode,
            size,
            blocks,
            nlink: 1,
            uid: 1000, // TODO: get from options
            gid: 1000,
            ctime,
            mtime,
            atime,
            value: INodeData { clusters },
        })
    }

    /// Reads the FAT entry for `cluster`. `cluster` is at least 2 and at most `cluster_count + 1`.
    fn get_fat_entry(&self, cluster: u32) -> Result<FatEntry> {
        if !(2 <= cluster && cluster <= self.cluster_count + 1) {
            pr_err!(
                "cluster {} out of range for {} data clusters\n",
                cluster,
                self.cluster_count
            );
            return Err(EINVAL);
        }

        let sector_size = Offset::from(self.sector_size);

        let fat_offset = match self.fat_type {
            FatType::FAT32 { root_cluster: _ } => cluster * 4,
            FatType::FAT16 => cluster * 2,
            FatType::FAT12 => todo!(),
        };
        let fat_sector = self.first_fat_sector + (Offset::from(fat_offset) / sector_size);
        let entry_offset = fat_offset % u32::from(self.sector_size);

        let data = self.mapper.mapped_folio(fat_sector * sector_size)?;

        match self.fat_type {
            FatType::FAT32 { root_cluster: _ } => {
                let entry = LE::<u32>::from_bytes(&data, entry_offset as usize).ok_or(EIO)?;
                let entry = entry.value() & 0x0FFFFFFF; // ignore high four bits
                Ok(FatEntry::from_bytes(&self.fat_type, entry))
            }
            FatType::FAT16 => {
                let entry = LE::<u16>::from_bytes(&data, entry_offset as usize).ok_or(EIO)?;
                let entry = entry.value() as u32;
                Ok(FatEntry::from_bytes(&self.fat_type, entry))
            }
            _ => todo!(),
        }
    }

    fn get_cluster_chain(&self, first_cluster: u32) -> Result<KVec<u32>> {
        let mut clusters = KVec::new();
        let mut cluster = first_cluster;
        // TODO: Detect infinite cluster chain.
        loop {
            match self.get_fat_entry(cluster)? {
                FatEntry::Next(next) => {
                    clusters.push(cluster, GFP_KERNEL)?;
                    cluster = next;
                }
                FatEntry::End => {
                    clusters.push(cluster, GFP_KERNEL)?;
                    return Ok(clusters);
                }
                FatEntry::Bad => {
                    pr_err!(
                        "bad cluster chain starting at {} (contains bad cluster {})\n",
                        first_cluster,
                        cluster
                    );
                    return Err(EIO);
                }
            }
        }
    }

    fn parse_utf16_long_entries(
        entries: &[RegularFatLongDirEntry],
        dest: &mut KVec<u8>,
    ) -> Result<()> {
        // TODO: this assumes the entries are just in reverse order and it does
        // not check the sequence numbers
        let chars = char::decode_utf16(
            entries
                .iter()
                .rev()
                .map(|e| e.name_contents())
                .flatten()
                .take_while(|c| *c != 0),
        );

        for res in chars {
            let c = res.map_err(|_| EIO)?;

            let pos = dest.len();
            let empty = [0; 4];
            dest.extend_from_slice(&empty[0..c.len_utf8()], GFP_KERNEL)?;
            c.encode_utf8(&mut dest[pos..]);
        }

        Ok(())
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

        let Some(signature) = LE::<u16>::from_bytes(&mapped, 510) else {
            pr_err!("failed to read FAT signature\n");
            return Err(EIO);
        };
        if unwrap_packed!(signature) != FAT_BOOT_SECTOR_SIGNATURE {
            pr_err!("not a FAT volume, signature mismatch\n");
            return Err(EINVAL);
        }

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

        // TODO: set `sb->s_time_{min,max}`

        drop(mapped);

        Ok(KBox::new(
            FatFs {
                mapper,
                fat_type,
                sector_size,
                sectors_per_cluster,
                cluster_size: sector_size as u32 * sectors_per_cluster as u32,
                cluster_count,
                first_fat_sector: Offset::from(first_fat_sector),
                first_data_sector,
                rootdir_entries,
                num_fats,
                fat_sectors,
            },
            GFP_KERNEL,
        )?)
    }

    #[allow(unused_variables)]
    fn init_root(sb: &SuperBlock<Self>) -> Result<dentry::Root<Self>> {
        let inode = Self::iget(sb, FAT_ROOT_INO, None)?;
        dentry::Root::try_new(inode)
    }
}

struct ByteStr<'a>(pub &'a [u8]);

impl core::fmt::Display for ByteStr<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write;

        f.write_str("ByteStr(\"")?;
        for b in self.0 {
            let c = char::from_u32(*b as u32).unwrap_or('?');
            f.write_char(c)?;
            f.write_str(", ")?;
        }
        f.write_str(" // ")?;
        for b in self.0 {
            write!(f, "{b}")?;
            f.write_str(" ")?;
        }
        f.write_str("\")")?;
        Ok(())
    }
}

#[vtable]
impl file::Operations for FatFs {
    type FileSystem = Self;

    fn seek(file: &File<Self>, offset: Offset, whence: file::Whence) -> Result<Offset> {
        file::generic_seek(file, offset, whence)
    }

    fn read(
        file: &File<Self>,
        writer: &mut uaccess::UserSliceWriter,
        offset: &mut Offset,
    ) -> Result<usize> {
        let size = file.inode().size();
        if *offset > size {
            return Ok(0);
        }

        let mut rem = (size - *offset) as usize;

        let folio = unsafe { file.inode().mapped_folio(*offset)? };
        let len = folio.len();

        pr_info!("[read] folio size={size} offset={offset} len={len}, rem={rem}");

        rem = rem.min(len);

        pr_info!("Reading {}", ByteStr(&folio[..rem]));

        writer.write_slice(&folio[..rem])?;
        *offset += rem as i64;
        Ok(rem)
    }

    fn write(
        file: &File<Self>,
        reader: uaccess::UserSliceReader,
        offset: &mut Offset,
    ) -> Result<usize> {
        let size = file.inode().size();

        pr_info!("[write] offset={offset}, size={size}");

        if *offset != 0 {
            panic!("Tried to write at non zero offset {offset}");
        }

        // if reader.len() as i64 != size {
        //     panic!(
        //         "Tried to write diffrent length {} than current size {size}",
        //         reader.len()
        //     );
        // }

        // if *offset > size {
        //     return Ok(0);
        // }

        // let mut rem = (size - *offset) as usize;

        let mut buf = KVec::new();
        reader.read_all(&mut buf, GFP_KERNEL)?;
        pr_info!("Writing {} bytes", buf.len());
        pr_info!("Writing {}", ByteStr(&buf));

        let mut mapped = unsafe { file.inode().mapped_folio(*offset)? };

        // let mem = unsafe { slice::from_raw_parts_mut((*mapped).as_ptr().cast_mut(), buf.len()) };
        // mem.copy_from_slice(&buf);

        let mut folio = mapped.lock();
        folio.mark_uptodate();

        let err = folio.write(*offset as usize, &buf);
        pr_info!("folio write res {err:?}");
        err?;

        *offset += buf.len() as i64;
        pr_info!("offset={offset}");

        file.inode().set_size(*offset);

        // let folio: kernel::folio::Mapped<'_, kernel::folio::PageCache<FatFs>> =
        //     unsafe { file.inode().mapped_folio(*offset)? };
        // pr_info!(
        //     "Read after write {}, {}",
        //     ByteStr(&folio[..buf.len()]),
        //     buf.len()
        // );

        Ok(buf.len())
    }

    fn read_dir(
        _file: &file::File<Self::FileSystem>,
        inode: &kernel::types::Locked<&INode<Self::FileSystem>, inode::ReadSem>,
        emitter: &mut file::DirEmitter,
    ) -> Result {
        let s = inode.super_block().data();
        // TODO: this match might not be required
        match s.fat_type {
            FatType::FAT32 { root_cluster: _ } | FatType::FAT16 => {
                let mut found_long_entries = KVec::new();
                let mut long_name_scratch = KVec::new();

                inode.for_each_page(emitter.pos(), Offset::MAX, |data| {
                    let mut offset = 0usize;
                    let mut acc: Offset = 0;
                    let limit = data.len().saturating_sub(FAT_DENTRY_SIZE);

                    while offset < limit {
                        acc += Offset::try_from(FAT_DENTRY_SIZE)?;

                        'read: {
                            let entry = match FatDirEntry::from_bytes(data, offset).ok_or(EIO)? {
                                FatDirEntry::Free => break 'read,
                                FatDirEntry::FreeConsecutive => return Ok(None),
                                FatDirEntry::Entry(entry) => entry,
                                FatDirEntry::LongEntry(entry) => {
                                    found_long_entries.push(entry, GFP_KERNEL)?;
                                    break 'read;
                                }
                                FatDirEntry::Other => break 'read,
                            };

                            let t = if entry.is_file() {
                                file::DirEntryType::Reg
                            } else {
                                file::DirEntryType::Dir
                            };

                            let short_name = entry.name();

                            let name = if found_long_entries.is_empty() {
                                &short_name.0[..short_name.1]
                            } else {
                                Self::parse_utf16_long_entries(
                                    &found_long_entries,
                                    &mut long_name_scratch,
                                )?;
                                &*long_name_scratch
                            };

                            if !emitter.emit(acc, &name, entry.first_cluster().into(), t) {
                                return Ok(Some(()));
                            }
                            acc = 0;

                            // clear Vec
                            found_long_entries.clear();
                            long_name_scratch.clear();
                        }

                        offset += FAT_DENTRY_SIZE;
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
        parent: &Locked<&INode<Self::FileSystem>, inode::ReadSem>,
        dentry: dentry::Unhashed<'_, Self::FileSystem>,
    ) -> Result<Option<ARef<DEntry<Self::FileSystem>>>> {
        let mut found_long_entries = KVec::new();
        let mut long_name_scratch = KVec::new();

        let inode = parent.for_each_page(0, Offset::MAX, |data| {
            let mut offset = 0usize;
            while data.len() - offset > FAT_DENTRY_SIZE {
                'read: {
                    let entry = match FatDirEntry::from_bytes(data, offset).ok_or(EIO)? {
                        FatDirEntry::Free => break 'read,
                        FatDirEntry::FreeConsecutive => return Ok(None),
                        FatDirEntry::Entry(entry) => entry,
                        FatDirEntry::LongEntry(entry) => {
                            found_long_entries.push(entry, GFP_KERNEL)?;
                            break 'read;
                        }
                        FatDirEntry::Other => break 'read,
                    };

                    Self::parse_utf16_long_entries(&found_long_entries, &mut long_name_scratch)?;

                    if &long_name_scratch == dentry.name() {
                        // TODO: We currently use ino = offset (with ino = FAT_ROOT_INO being the root).
                        // This works for now, since this is read-only.
                        return Ok(Some(Self::iget(
                            parent.super_block(),
                            offset.try_into().unwrap(),
                            Some(entry),
                        )?));
                    }

                    found_long_entries.clear();
                    long_name_scratch.clear();
                }

                offset += FAT_DENTRY_SIZE;
            }
            Ok(None)
        })?;

        dentry.splice_alias(inode)
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
        _srcmap: &mut iomap::Map<'a>,
    ) -> Result {
        pr_info!("iomap::start called pos={pos}, len={length}, flags={flags}");

        let size = inode.size();
        if pos >= size {
            map.set_offset(pos)
                .set_length(length.try_into()?)
                .set_flags(iomap::map_flags::MERGED)
                .set_type(iomap::Type::Hole);
            return Ok(());
        }

        let fat_data = inode.super_block().data();
        let sector_size = fat_data.sector_size;

        let cluster_size = fat_data.cluster_size as Offset;
        let cluster_index = pos / cluster_size;

        let cluster_offset = match &inode.data().clusters {
            Some(clusters) => {
                let sectors_per_cluster = fat_data.sectors_per_cluster;
                let first_data_sector = fat_data.first_data_sector;

                let cluster = clusters[cluster_index as usize] as u64;
                let first_cluster_sector =
                    ((cluster - 2) * (sectors_per_cluster as u64)) + first_data_sector as u64;
                first_cluster_sector * sector_size as u64
            }
            None => {
                // FAT-12/16 root directory
                let first_rootdir_sector = (fat_data.first_fat_sector as u64)
                    + ((fat_data.num_fats as u64) * fat_data.fat_sectors as u64);
                (first_rootdir_sector * sector_size as u64) + (cluster_index * cluster_size) as u64
            }
        };

        map.set_offset(cluster_index * cluster_size)
            // TODO: this length might be wrong
            .set_length(cluster_size as u64)
            .set_flags(iomap::map_flags::MERGED)
            .set_type(iomap::Type::Mapped)
            .set_bdev(Some(inode.super_block().bdev()))
            .set_addr(cluster_offset);

        Ok(())
    }

    fn end<'a>(
        _inode: &'a INode<Self::FileSystem>,
        pos: Offset,
        length: Offset,
        written: isize,
        flags: u32,
        _map: &iomap::Map<'a>,
    ) -> Result {
        pr_info!("iomap::end called pos={pos}, len={length}, written={written}, flags={flags}");
        Ok(())
    }
}
