// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::alloc::{alloc, Layout};
use x86_64::PhysAddr;

pub fn setup_paging(kernel_phys_base: u64, _kernel_vaddr_start: u64, _kernel_size: usize) -> PhysAddr {
    // Allocate PML4
    let pml4 = allocate_page();
    
    // 1. Identity map first 2MB (1 Huge Page or many 4K pages)
    // For simplicity, let's map the first 1GB as huge pages if supported, 
    // but on BIOS we'll just use 2MB pages for the first 1GB identity.
    let pdpt_ident = allocate_page();
    unsafe { *pml4.offset(0) = (pdpt_ident as u64) | 0x3; } // Present + Writable
    
    let pd_ident = allocate_page();
    unsafe { *pdpt_ident.offset(0) = (pd_ident as u64) | 0x3; }
    
    for i in 0..512 {
        let addr = i as u64 * 2 * 1024 * 1024; // 2MB pages
        unsafe { *pd_ident.offset(i) = addr | 0x83; } // Present + Writable + Huge
    }

    // 2. Map Kernel segments
    // Kernel is usually at 0xffffffff80000000.
    // That's PML4 entry 511, PDPT entry 510.
    let pdpt_kernel = allocate_page();
    unsafe { *pml4.offset(511) = (pdpt_kernel as u64) | 0x3; }
    
    let pd_kernel = allocate_page();
    unsafe { *pdpt_kernel.offset(510) = (pd_kernel as u64) | 0x3; }
    
    // Map kernel segments into this PD (2MB pages for simplicity if size permits, otherwise 4K)
    // Let's just map 128MB starting from 0xffffffff80000000 to the kernel_phys_base
    for i in 0..64 { // 128MB
        let phys = kernel_phys_base + (i as u64 * 2 * 1024 * 1024);
        unsafe { *pd_kernel.offset(i) = phys | 0x83; }
    }

    // 3. Map HHDM (0xffff800000000000)
    // 0xffff800000000000 corresponds to PML4 entry 256
    let pdpt_hhdm = allocate_page();
    unsafe { *pml4.offset(256) = (pdpt_hhdm as u64) | 0x3; }
    
    // Identity map first 1GB in HHDM
    let pd_hhdm = allocate_page();
    unsafe { *pdpt_hhdm.offset(0) = (pd_hhdm as u64) | 0x3; }
    for i in 0..512 {
        let addr = i as u64 * 2 * 1024 * 1024;
        unsafe { *pd_hhdm.offset(i) = addr | 0x83; }
    }

    PhysAddr::new(pml4 as u64)
}

fn allocate_page() -> *mut u64 {
    let layout = Layout::from_size_align(4096, 4096).unwrap();
    let ptr = unsafe { alloc(layout) as *mut u64 };
    if ptr.is_null() { panic!("Out of memory during paging setup"); }
    unsafe { core::ptr::write_bytes(ptr, 0, 512); }
    ptr
}
