#![allow(dead_code)]
use core::ffi::c_long;
use core::mem::size_of;
use core::ptr::addr_of_mut;
use core::arch::global_asm;

use crate::kmalloc::{HEAP_CURR, HEAP_END};
use crate::{kmalloc, println};
use crate::start::{bss_start, bss_end, stack_init, data_start, data_end};

const RZ_SENTINAL: u8 = 0x11;
const RZ_SIZE: usize = 128;

#[derive(PartialEq, PartialOrd, Eq, Ord)]
enum CheckBlockState {
    ALLOCED = 11,
    FREED = 12,
}

type Align = c_long;
#[derive(Copy, Clone)]
#[repr(C)]
struct HeaderFields {
    ptr: *mut Header,
    size: usize
}

#[repr(C)]
union Header {
    s: HeaderFields,
    x: Align
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct SourceLocation {
    pub file: &'static str,
    pub func: &'static str,
    pub lineno: u32,
}

#[repr(C)]
struct CheckHeader {
    next: *mut CheckHeader,
    nbytes_alloc: usize,

    state: CheckBlockState,
    block_id: u32,
    alloc_loc: SourceLocation,
    
    refs_start: u32, // number of pointers to the start of the block
    refs_middle: u32, // number of pointers to the middle of the block
    mark: u16, // 0 initialize -> this basically is the visited variable

    redzone1: [u8; RZ_SIZE],
}

static mut base: Header = Header { x: 0 };
static mut freep: *mut Header = core::ptr::null_mut();
static mut block_id: u32 = 1;
static STACK_ADDR: usize = 0x0800_0000;


pub fn kr_malloc(nbytes: usize) -> *mut u32 {
    unsafe {
        let mut p: *mut Header;
        let mut prevp: *mut Header;
        let nunits: usize = ((nbytes + size_of::<Header>() - 1) / size_of::<Header>()) + 1;
        
        prevp = freep;
        if(prevp.is_null()) {
            let base_ptr: *mut Header = addr_of_mut!(base);
            prevp = base_ptr;
            freep = base_ptr;
            base.s.ptr = base_ptr;
            base.s.size = 0;
        }

        p = (*prevp).s.ptr;
        loop {
            if (*p).s.size >= nunits { // big enough
                if (*p).s.size == nunits { // exactly the right size
                    (*prevp).s.ptr = (*p).s.ptr;
                } else { // allocate the tail end of this block
                    (*p).s.size -= nunits;
                    p = p.add((*p).s.size as usize);
                    (*p).s.size = nunits;
                }

                freep = prevp;
                return (p.add(1).cast::<u32>());
            }

            if p == freep {
                p = kmalloc::kmalloc(nunits).cast::<Header>();
                if p.is_null() {
                    return core::ptr::null_mut();
                }
            }
        
            prevp = p;
            p = (*p).s.ptr;
        }
    }
}

pub fn kr_free(ap: *mut u8) {
    unsafe {
        let mut bp: *mut Header;
        let mut p: *mut Header;

        bp = (ap as *mut Header).sub(1);
        p = freep;
        loop {
            println!("kr_free loop: visiting/checking {:p}", p);

            if (
                (bp > p) && 
                (bp < (*p).s.ptr)
            ) {
                break;
            }

            if (
                (p >= (*p).s.ptr) &&
                (
                    (bp > p) ||
                    (bp < (*p).s.ptr)
                )
            ) {
                break;
            }

            p = (*p).s.ptr;
            println!("kr_free loop: moving to where this is pointed to: {:p}", (*p).s.ptr);
        }

        // join to upper nbr
        if(
            core::ptr::eq(bp.cast::<u8>().add((*bp).s.size), (*p).s.ptr.cast::<u8>()) 
        ) {
            (*bp).s.size += (*((*p).s.ptr)).s.size;
            (*bp).s.ptr = (*((*p).s.ptr)).s.ptr;
        } else {
            (*bp).s.ptr = (*p).s.ptr;
        }

        // join to lower nbr
        if(
            core::ptr::eq(p.add((*p).s.size), bp)
        ) {
            (*p).s.size += (*bp).s.size;
            (*p).s.ptr = (*bp).s.ptr;
        } else {
            (*p).s.ptr = bp;
        }

        freep = p;
    }
}

static mut ck_alloc_list: *mut CheckHeader = core::ptr::null_mut();

fn ck_ptr_is_alloced(addr: *const u32) -> *mut CheckHeader {
    unsafe {
        let mut h: *mut CheckHeader = ck_alloc_list;
        loop {
            if (h.is_null()) {
                break;
            }

            if((*h).state != CheckBlockState::ALLOCED) {
                panic!("Error! Should only have allocated blocks in the allocated list!");
            }

            let data_ptr: *const u32 = h.add(1).cast::<u32>();
            if(data_ptr <= addr && addr < data_ptr.byte_add((*h).nbytes_alloc)) { // TODO: make sure that the redzone allocator for this also being handled
                return h;
            }

            h = (*h).next;
        }
        return core::ptr::null_mut();
    }
} 

pub fn ckalloc(nbytes: usize, l: SourceLocation) -> *mut u8 {
    let ckheader_size = size_of::<CheckHeader>();
    unsafe {
        let mut buf = kr_malloc(nbytes + ckheader_size + RZ_SIZE);
        if (buf.is_null()) {
            panic!("kr_malloc returned null.");
        }

        core::ptr::write_bytes(buf as *mut u8, 0u8, nbytes + ckheader_size);
        let mut header_ptr: *mut CheckHeader = buf as *mut CheckHeader;
        let mut header: &mut CheckHeader = &mut *header_ptr;

        header.nbytes_alloc = nbytes;
        header.state = CheckBlockState::ALLOCED;
        header.alloc_loc = l;
        header.block_id = block_id;
        block_id = block_id + 1;

        header.next = ck_alloc_list;
        for i in 0..RZ_SIZE {
            header.redzone1[i] = RZ_SENTINAL;
        }

        ck_alloc_list = header_ptr;

        let data_start = header_ptr.add(1).cast::<u8>();
        assert!(data_start != core::ptr::null_mut());

        let data_end = data_start.byte_add(nbytes);
        let mut rz2_ptr = data_end;
        for i in 0..RZ_SIZE {
            // println!("updating position {:p} from {} to {}", rz2_ptr, (*rz2_ptr), RZ_SENTINAL);
            (*rz2_ptr) = RZ_SENTINAL;
            rz2_ptr = rz2_ptr.add(1);
        }

        ck_check_redzone(header, "ckalloc");

        return data_start
    }
}

pub fn ck_check_redzone(h: *mut CheckHeader, from: &str) {
    unsafe {
        // check the redzone start
        for i in 0..RZ_SIZE {
            if (*h).redzone1[i] != RZ_SENTINAL {
                panic!("Redzone (start) data has been modified at pointer={:p}, index={}, value={}, from={}!", core::ptr::addr_of_mut!((*h).redzone1[i]), i, ((*h).redzone1[i]), from);
            }
        }

        // check the redzone end
        let mut rz_curr_ptr = h.add(1).byte_add((*h).nbytes_alloc).cast::<u8>();
        for i in 0..RZ_SIZE {
            if (*rz_curr_ptr) != RZ_SENTINAL {
                panic!("Redzone (end) data has been modified at pointer={:p}, index={}, value={}, from={}!", rz_curr_ptr, i, (*rz_curr_ptr), from);
            }
            rz_curr_ptr = rz_curr_ptr.add(1);
        }
    }
}

fn ck_list_remove(header: *mut CheckHeader) {
    unsafe {
        assert!(!ck_alloc_list.is_null());
        let mut prev: *mut CheckHeader = ck_alloc_list;
        if(prev == header) {
            ck_alloc_list = (*ck_alloc_list).next;
            return;
        }

        let mut p: *mut CheckHeader = (*prev).next;
        while(!p.is_null()) {
            // println!("ck_list_remove: checking {:p}", p);
            if (p == header) {
                (*prev).next = (*p).next;
                return;
            }
            prev = p;
            p = (*p).next;
        }
        panic!("Did not find {:p} in list.", header);
    }   
}

pub fn ckfree(addr: *mut u32) {
    unsafe {
        let h: *mut CheckHeader = ck_ptr_is_alloced(addr);
        ck_check_redzone(h, "ckfree");

        if h.is_null() {
            panic!("Freeing bogus pointer: {:p}", addr);
        }

        let blk_start: *mut u32 = h.add(1).cast::<u32>();
        if(blk_start != addr) {
            panic!("not freeing using start pointer: have {:p}, need {:p}", addr, blk_start);
        }

        if((*h).state != CheckBlockState::ALLOCED) {
            panic!("Freeing unallocated memory");
        }
        (*h).state = CheckBlockState::FREED;

        ck_list_remove(h);
        println!("ckfree: finished removing {:p} from list", addr);
        kr_free(h as *mut u8);
    }
}

fn ck_mark(region: &str, p: *const u32, e: *const u32) {
    println!("Running ck_mark on region {}, with pointer ranges start={:p}, end={:p}", region, p, e);
    unsafe {
        assert!(p < e);
        assert_eq!((p as usize) % 4, 0);
        assert_eq!((e as usize) % 4, 0);

        let mut curr_p = p;
        while curr_p < e { 
            let possible_ptr = (*curr_p) as *const u32;
            let alloced_ptr: *mut CheckHeader = ck_ptr_is_alloced(possible_ptr);
            if (!alloced_ptr.is_null()) {
                let check_header = &mut *alloced_ptr;
                let start_ptr: *const u32 = alloced_ptr.add(1).cast::<u32>();
                let end_ptr: *const u32 = alloced_ptr.add(1).byte_add(check_header.nbytes_alloc).cast::<u32>();
                
                if (possible_ptr == start_ptr) {
                    check_header.refs_start += 1;
                } else if (start_ptr < possible_ptr && possible_ptr < end_ptr) {
                    check_header.refs_middle += 1;
                }

                println!("Found reference to alloced region region={}, current_ptr={:p}, range_start={:p}, range_end={:p}", region, curr_p, start_ptr, end_ptr);

                if(check_header.mark == 0) {
                    check_header.mark = 1;
                    ck_mark(region, start_ptr, end_ptr);                    
                }
            }

            curr_p = curr_p.add(1);
        }
    }
}

fn ck_mark_all(sp: *mut u32) {
    unsafe {
        let mut curr_alloc = ck_alloc_list;
        while !curr_alloc.is_null() {
            (*curr_alloc).mark = 0;
            (*curr_alloc).refs_start = 0;
            (*curr_alloc).refs_middle = 0;

            curr_alloc = (*curr_alloc).next;
        }

        let mut stack_top: usize = STACK_ADDR;
        ck_mark("stack", sp, stack_top as *mut u32);
        ck_mark( "bss", bss_start(), bss_end());

        assert!(HEAP_CURR != 0);
        assert!(HEAP_END != 0);
        ck_mark("global", data_start(), data_end());
        // ck_mark("heap", HEAP_CURR as *mut u32, HEAP_END as *mut u32);
    }
}

fn ck_sweep_leak() -> u32 {
    unsafe {
        let mut nblocks: u32 = 0;
        let mut errors: u32 = 0;
        let mut maybe_errors: u32 = 0;

        let mut curr_alloc = ck_alloc_list;
        while !curr_alloc.is_null() {
            if ((*curr_alloc).refs_start == 0 && (*curr_alloc).refs_middle == 0) {
                errors += 1;
            } else if ((*curr_alloc).refs_middle == 0) {
                maybe_errors += 1;
            }

            nblocks += 1;

            curr_alloc = (*curr_alloc).next;
        }
        
        if (errors == 0 && maybe_errors == 0) {
            println!("GC: No leaks found!");
        } else {
            println!("GC: Errors: {} errors, {} maybe errors", errors, maybe_errors);
        }
        
        return errors + maybe_errors;
    }
}

fn ck_sweep_free() -> usize {
    unsafe {
        let mut nblocks: u32 = 0;
        let mut nfreed: usize = 0;
        let mut nbytes_freed: usize = 0;

        let mut curr_alloc = ck_alloc_list;
        while !curr_alloc.is_null() {
            let next = (*curr_alloc).next;
            if((*curr_alloc).refs_start == 0 && (*curr_alloc).refs_middle == 0) {
                nfreed += 1;
                nbytes_freed += (*curr_alloc).nbytes_alloc;
                ckfree(curr_alloc.add(1).cast::<u32>());
            }
            nblocks += 1;
            curr_alloc = next;
        }

        return nbytes_freed;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ck_find_leaks_fn(sp: *mut u32) -> u32 {
    ck_mark_all(sp);
    return ck_sweep_leak();
}

#[unsafe(no_mangle)]
pub extern "C" fn ck_gc_fn(sp: *mut u32) -> usize {
    ck_mark_all(sp);
    return ck_sweep_free();
}

unsafe extern "C" {
    fn ck_find_leaks_tramp() -> u32;
    fn ck_gc_tramp() -> usize;
}

pub fn ck_find_leaks() -> u32 {
    unsafe { ck_find_leaks_tramp() }
}

pub fn ck_gc() -> usize {
    println!("Calling GC!");
    unsafe{ ck_gc_tramp() }
}

global_asm!(r#"
.globl ck_find_leaks_tramp
.type ck_find_leaks_tramp, %function
ck_find_leaks_tramp:
    push {{r4, r5, r6, r7, r8, r9, r10, r11, lr}}
    mov r0, sp
    bl ck_find_leaks_fn
    pop {{r4, r5, r6, r7, r8, r9, r10, r11, pc}}
"#);

global_asm!(r#"
.globl ck_gc_tramp
.type ck_gc_tramp, %function
ck_gc_tramp:
    push {{r4, r5, r6, r7, r8, r9, r10, r11, lr}}
    mov r0, sp
    bl ck_gc_fn
    pop {{r4, r5, r6, r7, r8, r9, r10, r11, pc}}
"#);