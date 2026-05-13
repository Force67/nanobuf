/// Fixed-width little-endian wire scalar. Implemented for the primitive
/// types that can appear in `list<...>` payloads and fixed-section slots.
pub trait Scalar: Copy {
    /// Encoded size in bytes.
    const SIZE: usize;
    /// Reads from `bytes`, which holds at least `SIZE` bytes.
    fn read(bytes: &[u8]) -> Self;
    /// Stores into `bytes`, which holds at least `SIZE` bytes.
    fn store(self, bytes: &mut [u8]);
    fn write(self, out: &mut Vec<u8>);
}

macro_rules! int_scalar {
    ($($t:ty),*) => {$(
        impl Scalar for $t {
            const SIZE: usize = size_of::<$t>();
            #[inline]
            fn read(bytes: &[u8]) -> Self {
                <$t>::from_le_bytes(bytes[..Self::SIZE].try_into().unwrap())
            }
            #[inline]
            fn store(self, bytes: &mut [u8]) {
                bytes[..Self::SIZE].copy_from_slice(&self.to_le_bytes());
            }
            #[inline]
            fn write(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
        }
    )*};
}

int_scalar!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);

impl Scalar for bool {
    const SIZE: usize = 1;

    #[inline]
    fn read(bytes: &[u8]) -> Self {
        bytes[0] != 0
    }

    #[inline]
    fn store(self, bytes: &mut [u8]) {
        bytes[0] = self as u8;
    }

    #[inline]
    fn write(self, out: &mut Vec<u8>) {
        out.push(self as u8);
    }
}
