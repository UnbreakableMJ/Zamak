// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! KASLR random number generation (FR-MM-003).
//!
//! Provides a fallback chain for entropy:
//! 1. RDSEED (true hardware entropy, if available)
//! 2. RDRAND (conditioned DRBG output, if available)
//! 3. RDTSC (timestamp counter — weak but always available on x86)
//!
//! The UEFI path can use firmware RNG services instead.

// Rust guideline compliant 2026-03-30

/// Trait for KASLR random number generation.
///
/// Implementations should provide the best available entropy for the
/// platform. Weak entropy (e.g., TSC) is acceptable as a last resort
/// since KASLR only needs unpredictability, not cryptographic strength.
pub trait KaslrRng {
    /// Returns a 64-bit random value.
    fn get_u64(&mut self) -> u64;
}

/// x86/x86-64 KASLR RNG using the RDRAND/RDSEED/RDTSC fallback chain.
///
/// On construction, probes CPUID to determine which instructions are
/// available and uses the strongest source.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub struct X86KaslrRng {
    has_rdseed: bool,
    has_rdrand: bool,
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
impl X86KaslrRng {
    /// Creates a new RNG after probing CPU capabilities via CPUID.
    pub fn new() -> Self {
        let (has_rdrand, has_rdseed) = detect_rng_support();
        Self {
            has_rdseed,
            has_rdrand,
        }
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
impl Default for X86KaslrRng {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
impl KaslrRng for X86KaslrRng {
    fn get_u64(&mut self) -> u64 {
        // Try RDSEED first (true entropy).
        if self.has_rdseed {
            if let Some(val) = rdseed64() {
                return val;
            }
        }

        // Fall back to RDRAND (conditioned DRBG).
        if self.has_rdrand {
            if let Some(val) = rdrand64() {
                return val;
            }
        }

        // Last resort: RDTSC (weak but always available).
        crate::arch::x86::rdtsc()
    }
}

/// Probes CPUID for RDRAND (leaf 1, ECX bit 30) and RDSEED (leaf 7, EBX bit 18).
///
/// LLVM reserves `rbx` for internal use and rejects `out("ebx")` / `out("rbx")`
/// operands. We work around this by saving `rbx` via `push`/`pop` and moving
/// its contents through a temporary register chosen by the compiler (§3.9.4).
#[cfg(all(target_arch = "x86_64", not(miri)))]
fn detect_rng_support() -> (bool, bool) {
    let max_leaf: u32;
    // SAFETY: CPUID is always available on x86-64.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "pop rbx",
            inout("eax") 0u32 => max_leaf,
            out("ecx") _,
            out("edx") _,
            options(nostack, preserves_flags),
        );
    }

    let mut has_rdrand = false;
    let mut has_rdseed = false;

    // Leaf 1: ECX bit 30 = RDRAND.
    if max_leaf >= 1 {
        let ecx: u32;
        unsafe {
            core::arch::asm!(
                "push rbx",
                "cpuid",
                "pop rbx",
                inout("eax") 1u32 => _,
                out("ecx") ecx,
                out("edx") _,
                options(nostack, preserves_flags),
            );
        }
        has_rdrand = (ecx & (1 << 30)) != 0;
    }

    // Leaf 7, sub-leaf 0: EBX bit 18 = RDSEED. We need EBX, so copy it
    // via a scratch register before the compiler re-reads rbx.
    if max_leaf >= 7 {
        let ebx: u32;
        unsafe {
            core::arch::asm!(
                "push rbx",
                "cpuid",
                "mov {0:e}, ebx",
                "pop rbx",
                out(reg) ebx,
                inout("eax") 7u32 => _,
                inout("ecx") 0u32 => _,
                out("edx") _,
                options(nostack, preserves_flags),
            );
        }
        has_rdseed = (ebx & (1 << 18)) != 0;
    }

    (has_rdrand, has_rdseed)
}

/// 32-bit x86 variant: uses `push ebx` / `pop ebx` for the same workaround.
#[cfg(all(target_arch = "x86", not(miri)))]
fn detect_rng_support() -> (bool, bool) {
    let max_leaf: u32;
    unsafe {
        core::arch::asm!(
            "push ebx",
            "cpuid",
            "pop ebx",
            inout("eax") 0u32 => max_leaf,
            out("ecx") _,
            out("edx") _,
            options(nostack, preserves_flags),
        );
    }

    let mut has_rdrand = false;
    let mut has_rdseed = false;

    if max_leaf >= 1 {
        let ecx: u32;
        unsafe {
            core::arch::asm!(
                "push ebx",
                "cpuid",
                "pop ebx",
                inout("eax") 1u32 => _,
                out("ecx") ecx,
                out("edx") _,
                options(nostack, preserves_flags),
            );
        }
        has_rdrand = (ecx & (1 << 30)) != 0;
    }

    if max_leaf >= 7 {
        let ebx: u32;
        unsafe {
            core::arch::asm!(
                "push ebx",
                "cpuid",
                "mov {0}, ebx",
                "pop ebx",
                out(reg) ebx,
                inout("eax") 7u32 => _,
                inout("ecx") 0u32 => _,
                out("edx") _,
                options(nostack, preserves_flags),
            );
        }
        has_rdseed = (ebx & (1 << 18)) != 0;
    }

    (has_rdrand, has_rdseed)
}

/// Miri stub: reports no RDRAND/RDSEED support, forcing the RDTSC fallback.
#[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), miri))]
fn detect_rng_support() -> (bool, bool) {
    (false, false)
}

/// Attempts to read a 32-bit random value via RDRAND.
///
/// Returns `None` if RDRAND fails (carry flag not set). Retries up to
/// 10 times per Intel recommendation.
#[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), not(miri)))]
fn rdrand32() -> Option<u32> {
    for _ in 0..10 {
        let value: u32;
        let success: u8;
        // SAFETY: Caller verified RDRAND support via CPUID.
        unsafe {
            core::arch::asm!(
                "rdrand {val:e}",
                "setc {ok}",
                val = out(reg) value,
                ok = out(reg_byte) success,
                options(nomem, nostack),
            );
        }
        if success != 0 {
            return Some(value);
        }
    }
    None
}

/// Miri stub: RDRAND is not available under Miri.
#[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), miri))]
fn rdrand32() -> Option<u32> {
    None
}

/// Attempts to read a 64-bit random value via RDRAND (two 32-bit calls).
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn rdrand64() -> Option<u64> {
    let hi = rdrand32()? as u64;
    let lo = rdrand32()? as u64;
    Some((hi << 32) | lo)
}

/// Attempts to read a 32-bit true random value via RDSEED.
///
/// Returns `None` if RDSEED fails. Only retries a few times since
/// RDSEED can legitimately be exhausted.
#[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), not(miri)))]
fn rdseed32() -> Option<u32> {
    for _ in 0..10 {
        let value: u32;
        let success: u8;
        // SAFETY: Caller verified RDSEED support via CPUID.
        unsafe {
            core::arch::asm!(
                "rdseed {val:e}",
                "setc {ok}",
                val = out(reg) value,
                ok = out(reg_byte) success,
                options(nomem, nostack),
            );
        }
        if success != 0 {
            return Some(value);
        }
    }
    None
}

/// Miri stub: RDSEED is not available under Miri.
#[cfg(all(any(target_arch = "x86", target_arch = "x86_64"), miri))]
fn rdseed32() -> Option<u32> {
    None
}

/// Attempts to read a 64-bit true random value via RDSEED (two 32-bit calls).
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn rdseed64() -> Option<u64> {
    let hi = rdseed32()? as u64;
    let lo = rdseed32()? as u64;
    Some((hi << 32) | lo)
}

/// KASLR alignment: 1 GiB per FR-MM-003.
///
/// Aligning the kernel base to 1 GiB simplifies page-table construction
/// (a single PDPT entry covers the kernel's virtual range) and matches
/// Limine's behavior for the Limine Protocol.
pub const KASLR_ALIGNMENT: u64 = 1 << 30;

/// Computes a KASLR-randomized kernel base address within a range.
///
/// Given a candidate range `[min_addr, max_addr)` and a kernel size, returns
/// a randomized base that:
/// - is ≥ `min_addr`,
/// - is 1 GiB-aligned (§FR-MM-003),
/// - leaves at least `kernel_size` bytes before `max_addr`.
///
/// Returns `None` if no valid slot exists.
pub fn kaslr_base<R: KaslrRng>(
    rng: &mut R,
    min_addr: u64,
    max_addr: u64,
    kernel_size: u64,
) -> Option<u64> {
    let aligned_min = align_up(min_addr, KASLR_ALIGNMENT);
    let max_base = max_addr.checked_sub(kernel_size)?;
    let aligned_max = align_down(max_base, KASLR_ALIGNMENT);

    if aligned_min > aligned_max {
        return None;
    }

    let slot_count = (aligned_max - aligned_min) / KASLR_ALIGNMENT + 1;
    if slot_count == 0 {
        return None;
    }

    // Pick a slot index uniformly.
    let r = rng.get_u64();
    let slot = r % slot_count;
    Some(aligned_min + slot * KASLR_ALIGNMENT)
}

const fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

const fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

/// Timer-jitter fallback RNG.
///
/// Used when no hardware RNG is available (non-x86 platforms without a
/// firmware RNG). Repeatedly reads a time source and XORs adjacent reads
/// to extract jitter entropy. Weak but produces *some* unpredictability.
pub struct TimerJitterRng<F: FnMut() -> u64> {
    read_time: F,
}

impl<F: FnMut() -> u64> TimerJitterRng<F> {
    pub fn new(read_time: F) -> Self {
        Self { read_time }
    }
}

impl<F: FnMut() -> u64> KaslrRng for TimerJitterRng<F> {
    fn get_u64(&mut self) -> u64 {
        // Sample the timer 64 times; take the low bit of each as a jitter bit.
        let mut acc: u64 = 0;
        let mut prev = (self.read_time)();
        for i in 0..64 {
            // Busy-wait a few iterations to accumulate jitter.
            for _ in 0..8 {
                core::hint::spin_loop();
            }
            let now = (self.read_time)();
            let delta = now.wrapping_sub(prev);
            acc |= (delta & 1) << i;
            prev = now;
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CountingRng {
        state: u64,
    }
    impl KaslrRng for CountingRng {
        fn get_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(1);
            self.state
        }
    }

    #[test]
    fn kaslr_base_is_aligned() {
        let mut rng = CountingRng { state: 0 };
        for _ in 0..100 {
            let base = kaslr_base(&mut rng, 0x1_0000, 0x1_0000_0000_0000, 0x100_0000).unwrap();
            assert_eq!(base % KASLR_ALIGNMENT, 0);
        }
    }

    #[test]
    fn kaslr_base_respects_bounds() {
        let mut rng = CountingRng { state: 0 };
        let min_addr = KASLR_ALIGNMENT; // 1 GiB
        let max_addr = 4 * KASLR_ALIGNMENT; // 4 GiB
        let kernel_size = 0x100_0000; // 16 MiB
        let base = kaslr_base(&mut rng, min_addr, max_addr, kernel_size).unwrap();
        assert!(base >= min_addr);
        assert!(base + kernel_size <= max_addr);
    }

    #[test]
    fn kaslr_base_fails_when_no_room() {
        let mut rng = CountingRng { state: 0 };
        // min at 1 GiB + 1; max at 2 GiB - 1. No 1-GiB-aligned base ≥ min+kernel fits.
        let min_addr = KASLR_ALIGNMENT + 1;
        let max_addr = 2 * KASLR_ALIGNMENT - 1;
        let kernel_size = KASLR_ALIGNMENT; // 1 GiB kernel — doesn't fit in < 1 GiB gap.
        let result = kaslr_base(&mut rng, min_addr, max_addr, kernel_size);
        assert!(result.is_none());
    }

    #[test]
    fn timer_jitter_produces_output() {
        let mut counter = 0u64;
        let mut rng = TimerJitterRng::new(|| {
            counter = counter.wrapping_add(1);
            counter
        });
        // Just verify it returns without panicking.
        let _ = rng.get_u64();
    }

    #[test]
    fn timer_jitter_reads_timer_64_times() {
        // TimerJitterRng samples the callback 64 times per `get_u64`
        // (once for `prev`, then 64 deltas → 65 total reads). Verify
        // the callback is called the full 65 times, confirming no
        // short-circuit optimization silently dropped the loop.
        let mut reads: u32 = 0;
        let mut rng = TimerJitterRng::new(|| {
            reads += 1;
            reads as u64
        });
        let _ = rng.get_u64();
        assert_eq!(reads, 65, "TimerJitterRng did not sample the timer 65 times");
    }

    #[test]
    fn kaslr_base_exact_fit_returns_min() {
        // When max_addr - min_addr == kernel_size exactly, there is
        // only one valid base (min_addr) — the function must still
        // return it rather than None.
        let mut rng = CountingRng { state: 0 };
        let min_addr = KASLR_ALIGNMENT; // 1 GiB
        let kernel_size = KASLR_ALIGNMENT; // 1 GiB kernel.
        let max_addr = min_addr + kernel_size;
        let base = kaslr_base(&mut rng, min_addr, max_addr, kernel_size).unwrap();
        assert_eq!(base, min_addr);
    }

    #[test]
    fn x86_kaslr_rng_new_does_not_panic() {
        // Just exercises the X86KaslrRng::new() detection path — the
        // result (which flags are set) depends on host CPU but the
        // call itself must never trap on any x86-64 host.
        let _rng = X86KaslrRng::new();
    }

    #[test]
    fn x86_kaslr_rng_get_u64_returns_nonzero_sometimes() {
        // With fallback to rdtsc + mixing, we should see variety
        // across 100 calls. At minimum not all zero (which would
        // indicate every detection branch failed and the fallback
        // also collapsed).
        let mut rng = X86KaslrRng::default();
        let mut saw_nonzero = false;
        for _ in 0..100 {
            if rng.get_u64() != 0 {
                saw_nonzero = true;
                break;
            }
        }
        assert!(saw_nonzero, "X86KaslrRng yielded 100 zeros — fallback broken");
    }

    #[test]
    fn align_up_and_align_down_agree_on_aligned_input() {
        assert_eq!(align_up(0x1000, 0x1000), 0x1000);
        assert_eq!(align_down(0x1000, 0x1000), 0x1000);
    }

    #[test]
    fn align_up_rounds_toward_next_boundary() {
        assert_eq!(align_up(0x1001, 0x1000), 0x2000);
        assert_eq!(align_up(1, 0x1000), 0x1000);
    }

    #[test]
    fn align_down_rounds_toward_previous_boundary() {
        assert_eq!(align_down(0x1FFF, 0x1000), 0x1000);
        assert_eq!(align_down(0x2000, 0x1000), 0x2000);
    }
}
