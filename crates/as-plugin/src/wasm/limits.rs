//! The memory limit of a run: every linear memory and table of its store
//! together.
//!
//! wasmtime's own `StoreLimits` holds each memory to the limit alone, and a
//! Python component has several. [`Limiter`] counts what each memory took
//! (also when it is made: wasmtime asks for its first size then) and
//! refuses a growth past the limit: `memory.grow` fails inside the plugin.
//! A Rust plugin's `try_reserve` then returns an error; a plain allocation
//! traps, which the run reads as out of memory, since the limiter said no
//! last.
//!
//! A table's elements are the app's memory, not the plugin's: they count
//! against the same limit, [`TABLE_ELEMENT_BYTES`] each, also when a table
//! is made. Else 64 tables of a million elements would take half a
//! gigabyte of the app's under any limit.
//!
//! What AutoScene holds for a plugin (its resources, a request body on its
//! way) isn't counted: it is bounded by the resource table
//! ([`super::MAX_TABLE`]) and wasmtime's own caps per resource.

/// The most instances a run may make (componentize-py's output has 9 core
/// instances), tables and linear memories.
const MAX_INSTANCES: usize = 64;
const MAX_TABLES: usize = 64;
const MAX_MEMORIES: usize = 16;
/// The most elements a table may grow to.
const MAX_TABLE_ELEMENTS: usize = 1_000_000;
/// The most an element of a table takes in wasmtime: a function's is a
/// pointer, a continuation's two.
const TABLE_ELEMENT_BYTES: usize = 16;

pub(crate) struct Limiter {
    /// The bytes all memories and tables may take together.
    cap: usize,
    used: usize,
    /// The bytes of the growth allowed last, given back when it failed.
    last: usize,
    /// The growth asked for last was refused for the limit, so a trap now
    /// is for want of memory. A growth allowed after it (the plugin went on
    /// after the refusal: Python caught its `MemoryError`) clears it.
    pub hit: bool,
}

impl Limiter {
    /// A limiter for `memory` bytes in all.
    pub(crate) fn new(memory: u64) -> Self {
        Self { cap: usize::try_from(memory).unwrap_or(usize::MAX), used: 0, last: 0, hit: false }
    }

    /// Whether `more` bytes still fit, counted when they do.
    fn grow(&mut self, more: usize) -> bool {
        let fits = self.used.saturating_add(more) <= self.cap;
        if fits {
            self.used += more;
        }
        self.last = if fits { more } else { 0 };
        self.hit = !fits;
        fits
    }

    fn grow_failed(&mut self) {
        self.used -= self.last;
        self.last = 0;
    }
}

impl wasmtime::ResourceLimiter for Limiter {
    fn memory_growing(&mut self, current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        Ok(self.grow(desired.saturating_sub(current)))
    }

    fn memory_grow_failed(&mut self, _error: wasmtime::Error) -> wasmtime::Result<()> {
        self.grow_failed();
        Ok(())
    }

    fn table_growing(&mut self, current: usize, desired: usize, _maximum: Option<usize>) -> wasmtime::Result<bool> {
        if desired > MAX_TABLE_ELEMENTS {
            return Ok(false);
        }
        Ok(self.grow(desired.saturating_sub(current).saturating_mul(TABLE_ELEMENT_BYTES)))
    }

    fn table_grow_failed(&mut self, _error: wasmtime::Error) -> wasmtime::Result<()> {
        self.grow_failed();
        Ok(())
    }

    fn instances(&self) -> usize {
        MAX_INSTANCES
    }

    fn tables(&self) -> usize {
        MAX_TABLES
    }

    fn memories(&self) -> usize {
        MAX_MEMORIES
    }
}

#[cfg(test)]
mod tests {
    use wasmtime::{Instance, Module, ResourceLimiter, Store};

    use super::super::engine::engine;
    use super::*;

    const MIB: usize = 1 << 20;

    #[test]
    fn the_memory_limit_counts_every_memory() {
        let mut l = Limiter::new(64 << 20);
        // Two memories made with 16 MiB each, then each grows by 16 MiB.
        assert!(l.memory_growing(0, 16 * MIB, None).unwrap());
        assert!(l.memory_growing(0, 16 * MIB, None).unwrap());
        assert!(l.memory_growing(16 * MIB, 32 * MIB, None).unwrap());
        assert!(!l.hit);
        // Alone, the second would fit; together they pass the limit.
        assert!(!l.memory_growing(16 * MIB, 48 * MIB, None).unwrap());
        assert!(l.hit);
        assert!(l.memory_growing(16 * MIB, 32 * MIB, None).unwrap());
        assert!(!l.memory_growing(32 * MIB, 32 * MIB + (64 << 10), None).unwrap());
    }

    #[test]
    fn a_growth_that_failed_is_given_back() {
        let mut l = Limiter::new(32 << 20);
        assert!(l.memory_growing(0, 16 * MIB, None).unwrap());
        // Allowed here, then refused by the memory's own maximum.
        assert!(l.memory_growing(16 * MIB, 32 * MIB, Some(16 * MIB)).unwrap());
        l.memory_grow_failed(wasmtime::Error::msg("past its maximum")).unwrap();
        assert!(l.memory_growing(16 * MIB, 32 * MIB, None).unwrap());
        assert!(!l.hit);
        // So is a table's.
        let mut l = Limiter::new(16 << 20);
        assert!(l.table_growing(0, 1_000_000, Some(10)).unwrap());
        l.table_grow_failed(wasmtime::Error::msg("past its maximum")).unwrap();
        assert!(l.table_growing(0, 1_000_000, None).unwrap());
    }

    #[test]
    fn instances_tables_and_memories_are_bounded() {
        let mut l = Limiter::new(1 << 30);
        assert_eq!((l.instances(), l.tables(), l.memories()), (64, 64, 16));
        assert!(l.table_growing(0, 1_000_000, None).unwrap());
        assert!(!l.table_growing(0, 1_000_001, None).unwrap());
    }

    #[test]
    fn tables_count_against_the_memory_limit() {
        let mut l = Limiter::new(16 << 20);
        // A million elements: 16 MB of the 16 MiB.
        assert!(l.table_growing(0, 1_000_000, None).unwrap());
        assert!(!l.table_growing(0, 100_000, None).unwrap());
        assert!(l.hit);
        assert!(!l.memory_growing(0, MIB, None).unwrap());
        assert!(l.memory_growing(0, 512 << 10, None).unwrap());

        // In a store: 64 tables of a million functions (half a gigabyte of
        // the app's memory) are refused under a limit of 16 MiB, at once.
        let engine = engine().unwrap();
        let tables = "(table 1000000 funcref)".repeat(64);
        let module = Module::new(engine, wat::parse_str(format!("(module {tables})")).unwrap()).unwrap();
        let mut store = Store::new(engine, Limiter::new(16 << 20));
        store.limiter(|l| l);
        let e = Instance::new(&mut store, &module, &[]).unwrap_err();
        assert!(format!("{e:#}").contains("table minimum size"), "{e:#}");
        assert!(store.data().hit && store.data().used <= 16 << 20);
    }

    #[test]
    fn a_growth_allowed_after_a_refusal_clears_it() {
        // A plugin that minds a refused allocation and goes on: a trap
        // after is no longer for want of memory.
        let mut l = Limiter::new(16 << 20);
        assert!(!l.memory_growing(0, 32 * MIB, None).unwrap());
        assert!(l.hit);
        assert!(l.memory_growing(0, 8 * MIB, None).unwrap());
        assert!(!l.hit);
    }
}
