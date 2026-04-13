use crate::ckalloc;
use crate::ckalloc::SourceLocation;
use crate::println;
use core::mem::size_of;
use core::ptr;
use core::sync::atomic::{compiler_fence, Ordering};

#[repr(C)]
struct Root {
    child: *mut u8,
    tag: u32,
}

static mut GLOBAL_P: *mut u8 = ptr::null_mut();
static mut ROOT_P: *mut Root = ptr::null_mut();
static mut MIDDLE_P: *mut u8 = ptr::null_mut();

fn scrub_stack() {
    let mut x: [u32; 32] = [0u32; 32];
    for i in 0..x.len() {
        x[i] = i as u32;
    }
    compiler_fence(Ordering::SeqCst);
    core::hint::black_box(x);
}

fn alloc_one(nbytes: usize) -> *mut u8 {
    let p = ckalloc::ckalloc(
        nbytes,
        SourceLocation {
            file: "src/programs/ckmalloc_test.rs",
            func: "alloc_one",
            lineno: line!(),
        },
    );
    assert!(!p.is_null());
    p
}

fn free_one(p: *mut u8) {
    ckalloc::ckfree(
        p.cast::<u32>(),
        SourceLocation {
            file: "src/programs/ckmalloc_test.rs",
            func: "free_one",
            lineno: line!(),
        },
    );
}

fn test_simple_alloc_free() {
    let p = alloc_one(4);
    unsafe { ptr::write_bytes(p, 0xAA, 4) };
    free_one(p);
}

fn test_alloc_free_reverse_order() {
    const N: usize = 10;
    let mut allocs = [ptr::null_mut::<u8>(); N];

    for i in 0..N {
        allocs[i] = alloc_one(i + 1);
    }

    for i in (0..N).rev() {
        free_one(allocs[i]);
    }
}

fn test_no_leak_after_free() {
    let p = alloc_one(4);
    unsafe { ptr::write_bytes(p, 0, 4) };
    free_one(p);

    let leaks = ckalloc::ck_find_leaks();
    assert_eq!(leaks, 0);
}

fn alloc_global_root() {
    unsafe {
        GLOBAL_P = alloc_one(4);
        ptr::write_bytes(GLOBAL_P, 0x11, 4);
    }
}

fn test_global_root_behavior() {
    alloc_global_root();
    assert_eq!(ckalloc::ck_gc(), 0);

    unsafe {
        GLOBAL_P = ptr::null_mut();
    }
    scrub_stack();

    let freed = ckalloc::ck_gc();
    assert_eq!(freed, 4);
    assert_eq!(ckalloc::ck_gc(), 0);
}

fn alloc_graph() {
    unsafe {
        ROOT_P = alloc_one(size_of::<Root>()) as *mut Root;
        ptr::write_bytes(ROOT_P.cast::<u8>(), 0, size_of::<Root>());
        (*ROOT_P).tag = 0xDEAD_BEEF;
        (*ROOT_P).child = alloc_one(4);
        ptr::write_bytes((*ROOT_P).child, 0x33, 4);
    }
}

fn test_reachable_graph_behavior() {
    alloc_graph();
    assert_eq!(ckalloc::ck_gc(), 0);

    unsafe {
        ROOT_P = ptr::null_mut();
    }
    scrub_stack();

    let freed = ckalloc::ck_gc();
    assert_eq!(freed, size_of::<Root>() + 4);
    assert_eq!(ckalloc::ck_gc(), 0);
}

fn alloc_middle_root() -> *mut u8 {
    let p = alloc_one(8);
    unsafe { ptr::write_bytes(p, 0x55, 8) };
    unsafe { p.add(1) }
}

fn test_middle_pointer_behavior() {
    unsafe {
        MIDDLE_P = alloc_middle_root();
    }
    scrub_stack();
    assert_eq!(ckalloc::ck_gc(), 0);

    unsafe {
        MIDDLE_P = ptr::null_mut();
    }
    scrub_stack();

    let freed = ckalloc::ck_gc();
    assert_eq!(freed, 8);
    assert_eq!(ckalloc::ck_gc(), 0);
}

pub fn test_ckmalloc() {
    println!("Testing CKMalloc, Leak Checker, Garbage Collector");

    println!("Test 1: simple alloc/free");
    test_simple_alloc_free();

    println!("Test 2: reverse-order frees");
    test_alloc_free_reverse_order();

    println!("Test 3: no leak after explicit free");
    test_no_leak_after_free();

    println!("Test 4: global root behavior");
    test_global_root_behavior();

    println!("Test 5: reachable graph behavior");
    test_reachable_graph_behavior();

    println!("Test 6: middle pointer behavior");
    test_middle_pointer_behavior();

    println!("DONE: ckmalloc tests");
}
