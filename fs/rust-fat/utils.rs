#[macro_export]
/// Unwrap [`LE`] value from a packed struct.
///
/// # Examples
///
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

pub(crate) fn clear_kvec<T>(vec: &mut kernel::alloc::KVec<T>) {
    let elems: *mut [T] = vec.as_mut_slice();

    // SAFETY:
    // see alloc::vec::Vec::clear
    unsafe {
        vec.set_len(0);
        core::ptr::drop_in_place(elems);
    }
}
