use alloc::vec::Vec;

use heapless::String;

pub(crate) fn try_heap_buffer(capacity: usize) -> Result<Vec<u8>, ()> {
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(capacity).map_err(|_| ())?;
    buffer.resize(capacity, 0);
    Ok(buffer)
}

pub(crate) fn truncate_string<const N: usize>(source: &str) -> String<N> {
    let mut value = String::new();
    let cut = source.floor_char_boundary(N);
    let _ = value.push_str(&source[..cut]);
    value
}

#[macro_export]
macro_rules! make_static {
    ($ty:ty, $value:expr) => {{
        static CELL: ::static_cell::StaticCell<$ty> = ::static_cell::StaticCell::new();
        CELL.init_with(|| $value)
    }};
}

#[macro_export]
macro_rules! heap_buffer {
    ($capacity:expr, $error_message:literal) => {{
        match $crate::helpers::try_heap_buffer($capacity) {
            Ok(buffer) => buffer,
            Err(()) => {
                log::error!($error_message);
                return;
            }
        }
    }};
}
