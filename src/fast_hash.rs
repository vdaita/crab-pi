#![allow(dead_code)]

#[inline(always)]
unsafe fn get16bits(ptr: *const u8) -> u32 {
    let b0 = *ptr as u32;
    let b1 = *ptr.add(1) as u32;
    (b1 << 8) | b0
}

pub unsafe fn fast_hash_inc32(data: *const u8, len: u32, hash_in: u32) -> u32 {
    if len == 0 || data.is_null() {
        return 0;
    }

    let mut hash = hash_in;
    let mut rem = (len & 3) as i32;
    let mut len_words = (len >> 2) as i32;
    let mut p = data;

    while len_words > 0 {
        hash = hash.wrapping_add(get16bits(p));
        let tmp = (get16bits(p.add(2)) << 11) ^ hash;
        hash = (hash << 16) ^ tmp;
        p = p.add(2 * core::mem::size_of::<u16>());
        hash = hash.wrapping_add(hash >> 11);
        len_words -= 1;
    }

    match rem {
        3 => {
            hash = hash.wrapping_add(get16bits(p));
            hash ^= hash << 16;
            hash ^= ((*p.add(core::mem::size_of::<u16>()) as i8) as i32 as u32) << 18;
            hash = hash.wrapping_add(hash >> 11);
        }
        2 => {
            hash = hash.wrapping_add(get16bits(p));
            hash ^= hash << 11;
            hash = hash.wrapping_add(hash >> 17);
        }
        1 => {
            hash = hash.wrapping_add((*p as i8) as i32 as u32);
            hash ^= hash << 10;
            hash = hash.wrapping_add(hash >> 1);
        }
        _ => {}
    }

    hash ^= hash << 3;
    hash = hash.wrapping_add(hash >> 5);
    hash ^= hash << 4;
    hash = hash.wrapping_add(hash >> 17);
    hash ^= hash << 25;
    hash = hash.wrapping_add(hash >> 6);

    hash
}

pub unsafe fn fast_hash32(data: *const u8, len: u32) -> u32 {
    fast_hash_inc32(data, len, len)
}

pub unsafe fn fast_hash(data: *const u8, len: u32) -> u32 {
    fast_hash_inc32(data, len, len)
}
