//! Minimal NVMC driver for persisting RTC epoch to flash.
//!
//! Uses the last 4KB flash page (0xFF000) to store a magic word + epoch.
//! The nRF52840 NVMC is simple: erase sets all bits to 1, writes can only
//! clear bits to 0. We erase + write on each save.

// NVMC register addresses (nRF52840 Product Specification §4.3.9)
const NVMC_BASE: u32 = 0x4001_E000;
const NVMC_READY: *const u32 = (NVMC_BASE + 0x400) as *const u32;
const NVMC_CONFIG: *mut u32 = (NVMC_BASE + 0x504) as *mut u32;
const NVMC_ERASEPAGE: *mut u32 = (NVMC_BASE + 0x508) as *mut u32;

/// Flash page reserved for NVM storage (must match memory.x reservation).
const NVM_PAGE_ADDR: u32 = 0x000F_F000;

/// Magic word to validate stored data ("MSHR" = meshcore RTC).
const MAGIC: u32 = 0x4D53_4852;

#[allow(unsafe_code)]
fn nvmc_wait() {
    // SAFETY: NVMC_READY is a read-only MMIO register; polling it is safe.
    while unsafe { core::ptr::read_volatile(NVMC_READY) } == 0 {}
}

/// Read the persisted epoch from flash. Returns `None` if the magic word
/// doesn't match (erased flash, first boot, or corruption).
#[allow(unsafe_code)]
pub fn read_epoch() -> Option<u32> {
    // SAFETY: NVM_PAGE_ADDR is within flash; read_volatile is correct for MMIO/flash reads.
    let magic = unsafe { core::ptr::read_volatile(NVM_PAGE_ADDR as *const u32) };
    if magic != MAGIC {
        return None;
    }
    let epoch = unsafe { core::ptr::read_volatile((NVM_PAGE_ADDR + 4) as *const u32) };
    Some(epoch)
}

/// Erase the NVM page and write the current epoch.
///
/// This takes ~85ms (page erase) + ~41µs per word write on nRF52840.
/// Call from a low-priority background task, not a time-critical path.
#[allow(unsafe_code)]
pub fn write_epoch(epoch: u32) {
    // SAFETY: All writes are to NVMC MMIO registers or the reserved flash page.
    // The NVM_PAGE_ADDR is excluded from the linker's FLASH region (memory.x),
    // so no code or data lives there.
    unsafe {
        // Step 1: Enable erase mode
        core::ptr::write_volatile(NVMC_CONFIG, 2);
        nvmc_wait();

        // Step 2: Erase the page
        core::ptr::write_volatile(NVMC_ERASEPAGE, NVM_PAGE_ADDR);
        nvmc_wait();

        // Step 3: Enable write mode
        core::ptr::write_volatile(NVMC_CONFIG, 1);
        nvmc_wait();

        // Step 4: Write magic + epoch (two 32-bit words)
        core::ptr::write_volatile(NVM_PAGE_ADDR as *mut u32, MAGIC);
        nvmc_wait();
        core::ptr::write_volatile((NVM_PAGE_ADDR + 4) as *mut u32, epoch);
        nvmc_wait();

        // Step 5: Return to read-only mode
        core::ptr::write_volatile(NVMC_CONFIG, 0);
        nvmc_wait();
    }
}
