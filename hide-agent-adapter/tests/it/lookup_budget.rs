use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct Counted;

fn record() {
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    }
}

// SAFETY: every operation delegates its unchanged pointer/layout to System.
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        // SAFETY: the caller provides GlobalAlloc's valid layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the pointer and layout came from this System allocator.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record();
        // SAFETY: the caller provides GlobalAlloc's valid pointer/layout/size.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counted = Counted;

#[test]
fn repeated_alias_and_missing_lookups_allocate_nothing() {
    ALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|enabled| enabled.set(true));
    let mut found = 0;
    for _ in 0..10_000 {
        for name in [
            " Claude_Code ",
            "CLAUDE-CODE",
            "Codex",
            "OPENCODE",
            "future-agent",
            "",
        ] {
            found += usize::from(std::hint::black_box(hide_agent_adapter::adapter(name)).is_some());
        }
    }
    COUNTING.with(|enabled| enabled.set(false));
    assert_eq!(found, 40_000);
    assert_eq!(
        ALLOCATIONS.with(Cell::get),
        0,
        "agent lookup must borrow static declarations without allocating"
    );
}
