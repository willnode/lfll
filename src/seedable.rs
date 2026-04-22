/// A trait for keys that can generate pseudo randomness
pub trait Seedable {
    fn generate_seed(&self) -> usize;
}

/// https://prng.di.unimi.it/splitmix64.c
#[inline(always)]
fn mix_int(mut z: u64) -> usize {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    (z ^ (z >> 31)) as usize
}

macro_rules! impl_seedable_for_int {
    ($($t:ty),*) => {
        $(
            impl Seedable for $t {
                fn generate_seed(&self) -> usize {
                    // Cast to u64 and mix it
                    mix_int(*self as u64)
                }
            }
        )*
    };
}

// Implement for all the integer types you plan to use as keys
impl_seedable_for_int!(i8, u8, i16, u16, i32, u32, i64, u64, isize, usize);
