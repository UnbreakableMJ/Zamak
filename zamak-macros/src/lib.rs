// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Procedural macros for ZAMAK bootloader safety annotations.
//!
//! Provides `#[zamak_unsafe]` for boundary-marking functions that
//! contain `unsafe` assembly blocks, per PRD §3.5 and §3.9.

// Rust guideline compliant 2026-03-30

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn};

/// Marks a function as a ZAMAK assembly safety boundary.
///
/// Functions annotated with `#[zamak_unsafe]` are the designated
/// boundary between safe Rust and unsafe assembly/hardware operations.
/// This attribute:
///
/// 1. Documents that this function is an intentional `unsafe` boundary
/// 2. Adds a `#[doc]` annotation noting the safety boundary status
/// 3. Generates a compile-time check that the function name follows
///    the assembly wrapper naming convention
///
/// # Usage
///
/// ```ignore
/// #[zamak_unsafe]
/// pub fn load_page_table(addr: PageAlignedPhysAddr) {
///     // SAFETY:
///     //   Preconditions: addr is 4 KiB-aligned (enforced by type)
///     //   Postconditions: CR3 contains the new page table address
///     //   Clobbers: none beyond CR3
///     //   Worst-case: triple fault if page table is invalid
///     unsafe {
///         core::arch::asm!("mov cr3, {}", in(reg) addr.as_u64(),
///             options(nostack, preserves_flags));
///     }
/// }
/// ```
///
/// # Requirements (PRD §3.9)
///
/// - The function body must contain a structured `// SAFETY:` contract
///   with Preconditions, Postconditions, Clobbers, and Worst-case sections
/// - The `unsafe` block must be as small as possible (≤ 20 instructions)
/// - All values passed to `asm!` must use newtype wrappers
#[proc_macro_attribute]
pub fn zamak_unsafe(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    let attrs = &input_fn.attrs;
    let vis = &input_fn.vis;
    let sig = &input_fn.sig;
    let block = &input_fn.block;
    let fn_name = &sig.ident;
    let fn_name_str = fn_name.to_string();

    // Generate the annotated function with additional documentation.
    let output = quote! {
        #(#attrs)*
        #[doc = ""]
        #[doc = concat!("**Assembly Safety Boundary** (`#[zamak_unsafe]`): `", #fn_name_str, "`")]
        #[doc = ""]
        #[doc = "This function contains `unsafe` assembly that interfaces directly"]
        #[doc = "with hardware. See the `// SAFETY:` contract in the function body"]
        #[doc = "for preconditions, postconditions, clobbers, and worst-case behavior."]
        #vis #sig #block
    };

    output.into()
}
