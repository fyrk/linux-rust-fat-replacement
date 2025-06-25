// SPDX-License-Identifier: GPL-2.0

//! File system io maps.
//!
//! This module allows Rust code to use iomaps to implement filesystems.
//!
//! C headers: [`include/linux/iomap.h`](srctree/include/linux/iomap.h)

use super::{address_space, FileSystem, INode, Offset};
use crate::error::{from_result, Result};
use crate::{bindings, block};
use core::marker::PhantomData;

/// The type of mapping.
///
/// This is used in [`Map`].
#[repr(u16)]
pub enum Type {
    /// No blocks allocated, need allocation.
    Hole = bindings::IOMAP_HOLE as u16,

    /// Delayed allocation blocks.
    DelAlloc = bindings::IOMAP_DELALLOC as u16,

    /// Blocks allocated at the given address.
    Mapped = bindings::IOMAP_MAPPED as u16,

    /// Blocks allocated at the given address in unwritten state.
    Unwritten = bindings::IOMAP_UNWRITTEN as u16,

    /// Data inline in the inode.
    Inline = bindings::IOMAP_INLINE as u16,
}

/// Flags usable in [`Map`], in [`Map::set_flags`] in particular.
pub mod map_flags {
    /// Indicates that the blocks have been newly allocated and need zeroing for areas that no data
    /// is copied to.
    pub const NEW: u16 = bindings::IOMAP_F_NEW as u16;

    /// Indicates that the inode has uncommitted metadata needed to access written data and
    /// requires fdatasync to commit them to persistent storage. This needs to take into account
    /// metadata changes that *may* be made at IO completion, such as file size updates from direct
    /// IO.
    pub const DIRTY: u16 = bindings::IOMAP_F_DIRTY as u16;

    /// Indicates that the blocks are shared, and will need to be unshared as part a write.
    pub const SHARED: u16 = bindings::IOMAP_F_SHARED as u16;

    /// Indicates that the iomap contains the merge of multiple block mappings.
    pub const MERGED: u16 = bindings::IOMAP_F_MERGED as u16;

    /// Indicates that the file system requires the use of buffer heads for this mapping.
    pub const BUFFER_HEAD: u16 = bindings::IOMAP_F_BUFFER_HEAD as u16;

    /// Indicates that the iomap is for an extended attribute extent rather than a file data
    /// extent.
    pub const XATTR: u16 = bindings::IOMAP_F_XATTR as u16;

    /// Indicates to the iomap_end method that the file size has changed as the result of this
    /// write operation.
    pub const SIZE_CHANGED: u16 = bindings::IOMAP_F_SIZE_CHANGED as u16;

    /// Indicates that the iomap is not valid any longer and the file range it covers needs to be
    /// remapped by the high level before the operation can proceed.
    pub const STALE: u16 = bindings::IOMAP_F_STALE as u16;

    /// Flags from 0x1000 up are for file system specific usage.
    pub const PRIVATE: u16 = bindings::IOMAP_F_PRIVATE as u16;
}

/// A map from address space to block device.
#[repr(transparent)]
pub struct Map<'a>(pub bindings::iomap, PhantomData<&'a ()>);

impl<'a> Map<'a> {
    /// Sets the map type.
    pub fn set_type(&mut self, t: Type) -> &mut Self {
        self.0.type_ = t as u16;
        self
    }

    /// Sets the file offset, in bytes.
    pub fn set_offset(&mut self, v: Offset) -> &mut Self {
        self.0.offset = v;
        self
    }

    /// Sets the length of the mapping, in bytes.
    pub fn set_length(&mut self, len: u64) -> &mut Self {
        self.0.length = len;
        self
    }

    /// Sets the mapping flags.
    ///
    /// Values come from the [`map_flags`] module.
    pub fn set_flags(&mut self, flags: u16) -> &mut Self {
        self.0.flags = flags;
        self
    }

    /// Sets the disk offset of the mapping, in bytes.
    pub fn set_addr(&mut self, addr: u64) -> &mut Self {
        self.0.addr = addr;
        self
    }

    /// Sets the block device of the mapping.
    pub fn set_bdev(&mut self, bdev: Option<&'a block::Device>) -> &mut Self {
        self.0.bdev = if let Some(b) = bdev {
            b.0.get()
        } else {
            core::ptr::null_mut()
        };
        self
    }
}

/// Flags passed to [`Operations::begin`] and [`Operations::end`].
pub mod flags {
    /// Writing, must allocate block.
    pub const WRITE: u32 = bindings::IOMAP_WRITE;

    /// Zeroing operation, may skip holes.
    pub const ZERO: u32 = bindings::IOMAP_ZERO;

    /// Report extent status, e.g. FIEMAP.
    pub const REPORT: u32 = bindings::IOMAP_REPORT;

    /// Mapping for page fault.
    pub const FAULT: u32 = bindings::IOMAP_FAULT;

    /// Direct I/O.
    pub const DIRECT: u32 = bindings::IOMAP_DIRECT;

    /// Do not block.
    pub const NOWAIT: u32 = bindings::IOMAP_NOWAIT;

    /// Only pure overwrites allowed.
    pub const OVERWRITE_ONLY: u32 = bindings::IOMAP_OVERWRITE_ONLY;

    /// `unshare_file_range`.
    pub const UNSHARE: u32 = bindings::IOMAP_UNSHARE;

    /// DAX mapping.
    pub const DAX: u32 = bindings::IOMAP_DAX;
}

/// Operations implemented by iomap users.
pub trait Operations {
    /// File system that these operations are compatible with.
    type FileSystem: FileSystem + ?Sized;

    /// Returns the existing mapping at `pos`, or reserves space starting at `pos` for up to
    /// `length`, as long as it can be done as a single mapping. The actual length is returned in
    /// `iomap`.
    ///
    /// The values of `flags` come from the [`flags`] module.
    fn begin<'a>(
        inode: &'a INode<Self::FileSystem>,
        pos: Offset,
        length: Offset,
        flags: u32,
        map: &mut Map<'a>,
        srcmap: &mut Map<'a>,
    ) -> Result;

    /// Commits and/or unreserves space previously allocated using [`Operations::begin`]. `written`
    /// indicates the length of the successful write operation which needs to be commited, while
    /// the rest needs to be unreserved. `written` might be zero if no data was written.
    ///
    /// The values of `flags` come from the [`flags`] module.
    fn end<'a>(
        _inode: &'a INode<Self::FileSystem>,
        _pos: Offset,
        _length: Offset,
        _written: isize,
        _flags: u32,
        _map: &Map<'a>,
    ) -> Result {
        Ok(())
    }
}

/// Returns address space oprerations backed by iomaps.
pub const fn ro_aops<T: Operations + ?Sized>() -> address_space::Ops<T::FileSystem> {
    struct Table<T: Operations + ?Sized>(PhantomData<T>);
    impl<T: Operations + ?Sized> Table<T> {
        const MAP_TABLE: bindings::iomap_ops = bindings::iomap_ops {
            iomap_begin: Some(Self::iomap_begin_callback),
            iomap_end: Some(Self::iomap_end_callback),
        };

        extern "C" fn iomap_begin_callback(
            inode_ptr: *mut bindings::inode,
            pos: Offset,
            length: Offset,
            flags: u32,
            map: *mut bindings::iomap,
            srcmap: *mut bindings::iomap,
        ) -> i32 {
            from_result(|| {
                // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
                let inode = unsafe { INode::from_raw(inode_ptr) };
                T::begin(
                    inode,
                    pos,
                    length,
                    flags,
                    // SAFETY: The C API guarantees that `map` is valid for write.
                    unsafe { &mut *map.cast::<Map<'_>>() },
                    // SAFETY: The C API guarantees that `srcmap` is valid for write.
                    unsafe { &mut *srcmap.cast::<Map<'_>>() },
                )?;
                Ok(0)
            })
        }

        extern "C" fn iomap_end_callback(
            inode_ptr: *mut bindings::inode,
            pos: Offset,
            length: Offset,
            written: isize,
            flags: u32,
            map: *mut bindings::iomap,
        ) -> i32 {
            from_result(|| {
                // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
                let inode = unsafe { INode::from_raw(inode_ptr) };
                // SAFETY: The C API guarantees that `map` is valid for read.
                T::end(inode, pos, length, written, flags, unsafe {
                    &*map.cast::<Map<'_>>()
                })?;
                Ok(0)
            })
        }

        const TABLE: bindings::address_space_operations = bindings::address_space_operations {
            writepage: None,
            read_folio: Some(Self::read_folio_callback),
            writepages: None,
            dirty_folio: None,
            readahead: Some(Self::readahead_callback),
            write_begin: None,
            write_end: None,
            bmap: Some(Self::bmap_callback),
            invalidate_folio: Some(bindings::iomap_invalidate_folio),
            release_folio: Some(bindings::iomap_release_folio),
            free_folio: None,
            direct_IO: Some(bindings::noop_direct_IO),
            migrate_folio: None,
            launder_folio: None,
            is_partially_uptodate: None,
            is_dirty_writeback: None,
            error_remove_folio: None,
            swap_activate: None,
            swap_deactivate: None,
            swap_rw: None,
        };

        extern "C" fn read_folio_callback(
            _file: *mut bindings::file,
            folio: *mut bindings::folio,
        ) -> i32 {
            // SAFETY: `folio` is just forwarded from C and `Self::MAP_TABLE` is always valid.
            unsafe { bindings::iomap_read_folio(folio, &Self::MAP_TABLE) }
        }

        extern "C" fn readahead_callback(rac: *mut bindings::readahead_control) {
            // SAFETY: `rac` is just forwarded from C and `Self::MAP_TABLE` is always valid.
            unsafe { bindings::iomap_readahead(rac, &Self::MAP_TABLE) }
        }

        extern "C" fn bmap_callback(mapping: *mut bindings::address_space, block: u64) -> u64 {
            // SAFETY: `mapping` is just forwarded from C and `Self::MAP_TABLE` is always valid.
            unsafe { bindings::iomap_bmap(mapping, block, &Self::MAP_TABLE) }
        }
    }
    address_space::Ops(&Table::<T>::TABLE, PhantomData)
}

/// Returns address space oprerations backed by iomaps for writing.
pub const fn rw_aops<T: Operations + ?Sized>() -> address_space::Ops<T::FileSystem> {
    struct Table<T: Operations + ?Sized>(PhantomData<T>);
    impl<T: Operations + ?Sized> Table<T> {
        const MAP_TABLE: bindings::iomap_ops = bindings::iomap_ops {
            iomap_begin: Some(Self::iomap_begin_callback),
            iomap_end: Some(Self::iomap_end_callback),
        };

        extern "C" fn iomap_begin_callback(
            inode_ptr: *mut bindings::inode,
            pos: Offset,
            length: Offset,
            flags: u32,
            map: *mut bindings::iomap,
            srcmap: *mut bindings::iomap,
        ) -> i32 {
            from_result(|| {
                // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
                let inode = unsafe { INode::from_raw(inode_ptr) };
                T::begin(
                    inode,
                    pos,
                    length,
                    flags,
                    // SAFETY: The C API guarantees that `map` is valid for write.
                    unsafe { &mut *map.cast::<Map<'_>>() },
                    // SAFETY: The C API guarantees that `srcmap` is valid for write.
                    unsafe { &mut *srcmap.cast::<Map<'_>>() },
                )?;
                Ok(0)
            })
        }

        extern "C" fn iomap_end_callback(
            inode_ptr: *mut bindings::inode,
            pos: Offset,
            length: Offset,
            written: isize,
            flags: u32,
            map: *mut bindings::iomap,
        ) -> i32 {
            from_result(|| {
                // SAFETY: The C API guarantees that `inode_ptr` is a valid inode.
                let inode = unsafe { INode::from_raw(inode_ptr) };
                // SAFETY: The C API guarantees that `map` is valid for read.
                T::end(inode, pos, length, written, flags, unsafe {
                    &*map.cast::<Map<'_>>()
                })?;
                Ok(0)
            })
        }

        const TABLE: bindings::address_space_operations = bindings::address_space_operations {
            writepage: None,
            read_folio: Some(Self::read_folio_callback),
            writepages: None,
            dirty_folio: Some(bindings::iomap_dirty_folio),
            readahead: Some(Self::readahead_callback),
            write_begin: Some(Self::write_begin_callback),
            write_end: Some(Self::write_end_callback),
            bmap: Some(Self::bmap_callback),
            invalidate_folio: Some(bindings::iomap_invalidate_folio),
            release_folio: Some(bindings::iomap_release_folio),
            free_folio: None,
            direct_IO: Some(bindings::noop_direct_IO),
            migrate_folio: None,
            launder_folio: None,
            is_partially_uptodate: Some(bindings::iomap_is_partially_uptodate),
            is_dirty_writeback: None,
            error_remove_folio: None,
            swap_activate: None,
            swap_deactivate: None,
            swap_rw: None,
        };

        extern "C" fn read_folio_callback(
            _file: *mut bindings::file,
            folio: *mut bindings::folio,
        ) -> i32 {
            // SAFETY: `folio` is just forwarded from C and `Self::MAP_TABLE` is always valid.
            unsafe { bindings::iomap_read_folio(folio, &Self::MAP_TABLE) }
        }

        extern "C" fn readahead_callback(rac: *mut bindings::readahead_control) {
            // SAFETY: `rac` is just forwarded from C and `Self::MAP_TABLE` is always valid.
            unsafe { bindings::iomap_readahead(rac, &Self::MAP_TABLE) }
        }

        extern "C" fn bmap_callback(mapping: *mut bindings::address_space, block: u64) -> u64 {
            // SAFETY: `mapping` is just forwarded from C and `Self::MAP_TABLE` is always valid.
            unsafe { bindings::iomap_bmap(mapping, block, &Self::MAP_TABLE) }
        }

        // ``write_begin``
        // 	Called by the generic buffered write code to ask the filesystem
        // 	to prepare to write len bytes at the given offset in the file.
        // 	The address_space should check that the write will be able to
        // 	complete, by allocating space if necessary and doing any other
        // 	internal housekeeping.  If the write will update parts of any
        // 	basic-blocks on storage, then those blocks should be pre-read
        // 	(if they haven't been read already) so that the updated blocks
        // 	can be written out properly.

        // 	The filesystem must return the locked pagecache folio for the
        // 	specified offset, in ``*foliop``, for the caller to write into.

        // 	It must be able to cope with short writes (where the length
        // 	passed to write_begin is greater than the number of bytes copied
        // 	into the folio).

        // 	A void * may be returned in fsdata, which then gets passed into
        // 	write_end.

        // 	Returns 0 on success; < 0 on failure (which is the error code),
        // 	in which case write_end is not called.
        #[allow(unused)]
        extern "C" fn write_begin_callback(
            file: *mut bindings::file,
            mapping: *mut bindings::address_space,
            pos: bindings::loff_t,
            len: ffi::c_uint,
            foliop: *mut *mut bindings::folio,
            fsdata: *mut *mut ffi::c_void,
        ) -> ffi::c_int {
            from_result(|| {
                panic!("write_begin pos={pos}, len={len}");
                Ok(0)
            })
        }

        // ``write_end``
        // 	After a successful write_begin, and data copy, write_end must be
        // 	called.  len is the original len passed to write_begin, and
        // 	copied is the amount that was able to be copied.

        // 	The filesystem must take care of unlocking the folio,
        // 	decrementing its refcount, and updating i_size.

        // 	Returns < 0 on failure, otherwise the number of bytes (<=
        // 	'copied') that were able to be copied into pagecache.
        #[allow(unused)]
        extern "C" fn write_end_callback(
            file: *mut bindings::file,
            mapping: *mut bindings::address_space,
            pos: bindings::loff_t,
            len: ffi::c_uint,
            copied: ffi::c_uint,
            folio: *mut bindings::folio,
            fsdata: *mut ffi::c_void,
        ) -> ffi::c_int {
            from_result(|| {
                panic!("write_end pos={pos}, len={len}, copied={copied}");
                Ok(0)
            })
        }
    }
    address_space::Ops(&Table::<T>::TABLE, PhantomData)
}
