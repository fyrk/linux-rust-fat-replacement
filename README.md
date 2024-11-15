*this branch is subject to force-pushing*

this branch contains Wedson's work on VFS in Rust [0,1], rebased onto a current [`rust-next`](https://github.com/Rust-for-Linux/linux/tree/rust-next/)

[0] https://lore.kernel.org/rust-for-linux/20240514131711.379322-1-wedsonaf@gmail.com/

[1] https://github.com/wedsonaf/linux/tree/vfs-v2

**some notes on what was changed during rebase:**

- a672c7393a49 rust: introduce `InPlaceModule`
  - add `use kernel::init::PinInit;`
- 9add409b5346 samples: rust: add in-place initialisation sample
  - old `Vec` -> `KVec`
- 96ef0376887f rust: init: introduce `Opaque::try_ffi_init`
- ~~6fe57e020b4d rust: alloc: introduce `resize` and `resize_with`~~
  - skipped
  - `VecExt` has been removed in 405966efc789888c3e1a53cd09d2c2b338064438
  - `VecExt::resize` is already implemented as `Vec::extend_with` (though `extend_with` takes number of elements to add rather than new len)
  - `VecExt::resize_with` just takes a fn instead of a value to fill with
- 9eed0fe3ec36 rust: alloc: introduce `new_slice` and `new_uninit_slice`
  - `BoxExt` has been removed in e8c6ccdbcaaf31f26c0fffd4073edd0b0147cdc6
  - `Box<MaybeUninit<T>>::assume_init()` had to be implemented for `Box<[MaybeUninit<T>]>` as well
- ecc54b2bbd08 rust: kernel: update `offset_of` to not require unsafe blocks
  - remove `unsafe` around `container_of` in rbtree.rs
- 300ab577d33e rust: alloc: add GFP_NOFS
- 98f6c0bb7175 rust: time: introduce `time` module
  - also adds `Timespec::seconds`, `Timespec::nanoseconds`, which are now required for "rust: fs: introduce `FileSystem::init_root`"
- 475d59e88d66 rust: types: add little-endian type
- 695c58302286 rust: types: introduce `FromBytes` trait
- d74848eb4d8c rust: mem_cache: introduce `MemCache`
  - add bindings helper for `kmem_cache_create`
- f60e1c65ab72 rust: str: modify `CString` to use `Box<[u8]>` instead of `Vec`
  - `Box` -> `KBox`
- 802339f9b993 rust: str: implement `ForeignOwnable` for `CString`
- c5f5fc72c583 rust: str: implement `TryFrom<Box<[u8]>>` for CString
- 0d384e0ea01a rust: str: add conversion from `&[u8]` to `CString`
- ~~8dca84ec304a rust: user: add helper for `copy_to_user`~~
  - skipped; already implemented (`uaccess::UserSliceWriter::write` instead of `user::Writer::write_all`)
- 64c812a93704 rust: types: introduce `Locked` type
- 4a2a0b0247d9 rust: block: introduce `block::Device`
  - empty `block` module already exists
- 5b9c88f6229c rust: file: add Rust abstraction for `struct file`
- 183ea65d1fcd rust: error: add errors used in file systems
- 79c843449bdc rust: fs: add registration/unregistration of file systems
- be077708fb7e rust: fs: introduce the `module_fs` macro
- 28204e1530ef samples: rust: add initial ro file system sample
- bee3cb6a63b0 rust: fs: introduce `FileSystem::fill_super`
- f631cc27f1cb rust: fs: introduce `INode<T>`
  - `Device::inode`: `block_device.bd_inode` has been removed (203c1ce0bb063d1620698e39637b64f2d09c1368); use  `bdev_inode.vfs_inode`  instead (like df65f1660b6141f0b051d2439cc99222a6ae865b does)
  - adds `struct bdev_inode` to `bindings_helper.h`
- eb0c65e362a3 rust: fs: introduce `DEntry<T>`
- aae9bc801c8b rust: fs: introduce `FileSystem::init_root`
  - inodes now store time sec/nsec separately; added `Timespec::seconds`, `Timespec::nanoseconds` (3aa63a569c64e708df547a8913c84e64a06e7853)
- a0e1f1c0832b rust: file: move `kernel::file` to `kernel::fs::file`
- 30e88c831ef9 rust: fs: generalise `File` for different file systems
- e0bc2ed31f8e rust: fs: add empty file operations
  - `file_operations.mmap_supported_flags` has been removed; `file_operations.fop_flags` has been added (210a03c9d51aa0e6e6f06980116e3256da8d4c48)
- 40089b3eb6c0 rust: fs: introduce `file::Operations::read_dir`
- 5dc2f72eef50 rust: fs: introduce `file::Operations::seek`
- adfb28ec5cee rust: fs: introduce `file::Operations::read`
- beee27f2516c rust: fs: add empty inode operations
  - compilation fails already in original commit (?) since inode Ops field 0 is never read
- fe47514c8ae0 rust: fs: introduce `inode::Operations::lookup`
- 6e278ed7b428 rust: folio: introduce basic support for folios
- 6e5f4e38240e rust: fs: add empty address space operations
- b971ade00965 rust: fs: introduce `address_space::Operations::read_folio`
- 31af11a7e054 rust: fs: introduce `FileSystem::read_xattr`
- 35b2b956c1a8 rust: fs: introduce `FileSystem::statfs`
- bc914f6b2431 rust: fs: introduce more inode types
  - add `helpers/{delayed_call,kdev_t}.c`
- 28e8d78ba103 rust: fs: add per-superblock data
- bc4f26414cf7 rust: fs: allow file systems backed by a block device
- e296beaeceac rust: fs: allow per-inode data
- c2e8edfe650d rust: fs: export file type from mode constants
- 4a8a26b867be rust: fs: allow populating i_lnk
- b682a749d3f2 rust: fs: add `iomap` module
- 1a4faa0efe06 rust: fs: add memalloc_nofs support
- 54b312e1340d tarfs: introduce tar fs
  - `Vec` -> `KVec`, `Box` -> `KBox`
  - `kernel::user::Writer` -> `kernel::uaccess::UserSliceWriter`
  - `name.resize(name_len, ...)` -> `name.extend_with(name_len - name.len(), ...)`
- 5bb3588bb138 WIP: fs: ext2: add rust ro ext2 implementation
  - `Vec` -> `KVec`, `Box` -> `KBox`
  - `kernel::user::Writer` -> `kernel::uaccess::UserSliceWriter`
