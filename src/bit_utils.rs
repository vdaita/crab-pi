#![allow(dead_code)]

// Simple bit manipulation helpers: keep code clearer at call sites.

// set x[bit]=0 (leave the rest unaltered) and return the value
#[inline]
pub fn bit_clr(x: u32, bit: u32) -> u32 {
    assert!(bit < 32);
    x & !(1u32 << bit)
}

// set x[bit]=1 (leave the rest unaltered) and return the value
#[inline]
pub fn bit_set(x: u32, bit: u32) -> u32 {
    assert!(bit < 32);
    x | (1u32 << bit)
}

#[inline]
pub fn bit_not(x: u32, bit: u32) -> u32 {
    assert!(bit < 32);
    x ^ (1u32 << bit)
}

// is x[bit] == 1?
#[inline]
pub fn bit_is_on(x: u32, bit: u32) -> u32 {
    assert!(bit < 32);
    (x >> bit) & 1
}

pub use bit_is_on as bit_get;
pub use bit_is_on as bit_isset;

#[inline]
pub fn bit_is_off(x: u32, bit: u32) -> bool {
    bit_is_on(x, bit) == 0
}

// return a mask with the <n> low bits set to 1.
// error if nbits > 32. ok if nbits = 0.
//
// we use this routine because unsigned shifts by a variable greater than
// the bit width give unexpected results.
// eg. gcc on x86:
//   n = 32;
//   ~0 >> n == ~0
#[inline]
pub fn bits_mask(nbits: u32) -> u32 {
    if nbits == 32 {
        !0
    } else {
        assert!(nbits < 32);
        (1u32 << nbits) - 1
    }
}

// extract bits [lb:ub] inclusive
#[inline]
pub fn bits_get(x: u32, lb: u32, ub: u32) -> u32 {
    assert!(lb <= ub);
    assert!(ub < 32);
    (x >> lb) & bits_mask(ub - lb + 1)
}

// set bits[off:n]=0, leave the rest unchanged.
#[inline]
pub fn bits_clr(x: u32, lb: u32, ub: u32) -> u32 {
    assert!(lb <= ub);
    assert!(ub < 32);

    let mask = bits_mask(ub - lb + 1);

    // XXX: check that gcc handles shift by more bit-width as expected.
    x & !(mask << lb)
}

// set bits [lb:ub] to v (inclusive). v must fit within the specified width.
#[inline]
pub fn bits_set(x: u32, lb: u32, ub: u32, v: u32) -> u32 {
    assert!(lb <= ub);
    assert!(ub < 32);

    let n = ub - lb + 1;
    assert!(n <= 32);
    assert!((bits_mask(n) & v) == v);

    bits_clr(x, lb, ub) | (v << lb)
}

// bits [lb:ub] == <val>?
#[inline]
pub fn bits_eq(x: u32, lb: u32, ub: u32, val: u32) -> bool {
    assert!(lb <= ub);
    assert!(ub < 32);
    bits_get(x, lb, ub) == val
}

#[inline]
pub fn bit_count(x: u32) -> u32 {
    let mut cnt = 0;
    for bit in 0..32 {
        if bit_is_on(x, bit) != 0 {
            cnt += 1;
        }
    }
    cnt
}

#[inline]
pub fn bits_union(x: u32, y: u32) -> u32 {
    x | y
}

#[inline]
pub fn bits_intersect(x: u32, y: u32) -> u32 {
    x & y
}

#[inline]
pub fn bits_not(x: u32) -> u32 {
    !x
}

// forall x in A and not in B
#[inline]
pub fn bits_diff(a: u32, b: u32) -> u32 {
    bits_intersect(a, bits_not(b))
}