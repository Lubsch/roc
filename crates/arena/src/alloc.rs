/// OS-level virtual memory allocation and deallocation functions, for use in the `arena` module.
/// The goal here is to avoid managing free lists and instead to directly ask the OS for memory.
/// In long-running compiler processes (e.g. watch mode, editor integrations, repl), this can
/// prevent memory usage from slowly growing over time because we're actually goving memory
/// back to the OS when we're done with it.
///
/// Since we should only use these to allocate memory for an entire module at a time, this should
/// result in 1 total syscall per module, which should be fine in terms of performance.
///
/// As of this writing, wasm uses the wee_alloc crate to emulate virtual memory by managing a free
/// list behind the scenes, since wasm only supports growing the heap and that's it. Although
/// wasm doesn't have a watch mode, it does have long-running processes in the form of the repl
/// and also potentially in the future a playground.
use core::{alloc::Layout, ptr::NonNull};

////////////////
// ALLOCATION //
////////////////

/// We use wee_alloc for allocations on wasm because wasm natively supports only growing the heap,
/// not releasing anything. Releasing has to be built in userspace, which wee_alloc provides.
#[cfg(wasm32)]
static WEE_ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

/// We'll exit after printing this message to stderr if allocation fails
const ALLOC_FAILED_MESSAGE: &str =
    "The Roc compiler had to exit unexpectedly because a virtual memory allocation operation failed.\n\nThis normally should never happen, but one way it could happen is if the compiler is running in an unusually memory-constrained environment, such as a virtualized operating system.\n\nIf you are seeing this message repeatedly, you might want to try contacting other Roc community members via <https://roc-lang.org/community> to see if they can help.";

/// We'll exit with this code if allocation fails
const ALLOC_FAILED_EXIT_CODE: u8 = 90;

/// Returns the pointer and also how many bytes were actually allocated,
/// since it will round up to the nearest page size depending on target OS.
pub(crate) fn alloc_virtual(layout: Layout) -> (NonNull<u8>, usize) {
    let size = layout.size();

    #[cfg(unix)]
    {
        use core::{ffi::c_void, ptr};

        extern "C" {
            fn mmap(
                addr: *mut c_void,
                length: usize,
                prot: i32,
                flags: i32,
                fd: i32,
                offset: i64,
            ) -> *mut c_void;
        }

        const MAP_FAILED: *mut c_void = !0 as *mut c_void;
        const PROT_READ: i32 = 1;
        const PROT_WRITE: i32 = 2;
        const MAP_PRIVATE: i32 = 0x0002;
        const MAP_ANONYMOUS: i32 = 0x0020;

        // Round up to nearest 4096B
        let size = {
            const PAGE_MULTIPLE: usize = 4096; // Pages must be a multiple of this.

            (size + (PAGE_MULTIPLE - 1)) & !(PAGE_MULTIPLE - 1)
        };

        // Safety: We rounded up `size` to the correct multiple already.
        let answer = unsafe {
            mmap(
                ptr::null_mut(),
                size,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        // We should never return a size smaller than what was requested!
        debug_assert!(size >= layout.size());

        match NonNull::new(answer) {
            Some(non_null) if answer != MAP_FAILED => (non_null.cast(), size),
            _ => crash::unrecoverable!(ALLOC_FAILED_MESSAGE, ALLOC_FAILED_EXIT_CODE),
        }
    }

    #[cfg(windows)]
    {
        use core::{ffi::c_void, ptr};

        extern "system" {
            fn VirtualAlloc(
                lpAddress: *mut c_void,
                dwSize: usize,
                flAllocationType: u32,
                flProtect: u32,
            ) -> *mut c_void;
        }

        // Round up to nearest 4096B
        let size = {
            const PAGE_MULTIPLE: usize = 4096; // Pages must be a multiple of this.

            (size + (PAGE_MULTIPLE - 1)) & !(PAGE_MULTIPLE - 1)
        };

        const MEM_COMMIT: u32 = 0x1000;
        const MEM_RESERVE: u32 = 0x2000;;
        const PAGE_READWRITE: u32 = 0x04;

        // Safety: We rounded up `size` to the correct multiple already.
        let ptr = unsafe {
            VirtualAlloc(
                ptr::null_mut(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };

        // We should never return a size smaller than what was requested!
        debug_assert!(size >= layout.size());

        match NonNull::new(ptr) {
            Some(non_null) => (non_null.cast(), size),
            None => crash::unrecoverable!(ALLOC_FAILED_MESSAGE, ALLOC_FAILED_EXIT_CODE),
        }
    }

    #[cfg(wasm32)]
    {
        let ptr = unsafe { WEE_ALLOC.alloc(layout) };

        // We should never return a size smaller than what was requested!
        debug_assert!(size >= layout.size());

        match NonNull::new(ptr) {
            Some(non_null) => (non_null.cast(), size),
            None => {
                extern "C" {
                    fn alloc_failed(ptr: *const u8, len: usize) -> !;
                }

                alloc_failed(ALLOC_FAILED_MESSAGE.as_ptr(), ALLOC_FAILED_MESSAGE.len());
            }
        }
    }
}

//////////////////
// DEALLOCATION //
//////////////////

#[cfg(debug_assertions)]
macro_rules! dealloc_failed {
    ($ptr:expr) => {
        let mut buf = [0; 256];

        let _ = fs::IoError::most_recent().write(&mut buf);

        panic!(
            "Tried to deallocate address {:?} but it failed with error: {:?}",
            $ptr,
            core::ffi::CStr::from_bytes_until_nul(&buf)
        );
    };
}

pub(crate) unsafe fn dealloc_virtual(ptr: *mut u8, layout: Layout) {
    let size = layout.size();

    #[cfg(unix)]
    {
        use core::ffi::c_void;

        extern "C" {
            fn munmap(addr: *mut c_void, length: usize) -> i32;
        }

        // If deallocation fails, panic in debug builds so we can try to diagnose it
        // (and so that it will fail tests), but silently continue in release builds
        // because a memory leak is generally a better user experience than a crash.
        let _answer = munmap(ptr as *mut c_void, size);

        #[cfg(debug_assertions)]
        {
            if _answer < 0 {
                dealloc_failed!(ptr);
            }
        }
    }

    #[cfg(windows)]
    {
        use core::ffi::c_void;

        extern "system" {
            fn VirtualFree(lpAddress: *mut c_void, dwSize: usize, dwFreeType: u32) -> i32;
        }

        const MEM_RELEASE: u32 = 0x8000;

        // When calling VirtualAlloc with MEM_RELEASE, the second argument must be 0.
        // https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtualfree#parameters
        let _answer = VirtualFree(ptr as *mut c_void, 0, MEM_RELEASE);

        #[cfg(debug_assertions)]
        {
            if _answer == 0 {
                dealloc_failed!(ptr);
            }
        }
    }

    #[cfg(wasm32)]
    {
        let _ptr = unsafe { WEE_ALLOC.dealloc(layout) };

        // If deallocation fails, panic in debug builds so we can try to diagnose it
        // (and so that it will fail tests), but silently continue in release builds
        // because a memory leak is generally a better user experience than a crash.
        #[cfg(debug_assertions)]
        {
            if _ptr.is_null() {
                panic!(
                    "Tried to deallocate address {:?} but it failed.",
                    $ptr,
                );
            }
        }
    }
}