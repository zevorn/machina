use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::code_buffer::CodeBuffer;
use crate::ir::tb::{TranslationBlock, TB_HASH_SIZE};
use crate::HostCodeGen;

const MAX_TBS: usize = 65536;
/// Max physical pages tracked (1M pages = 4 GB).
const CODE_REFCOUNT_PAGES: usize = 1 << 20;

/// Thread-safe storage and hash-table lookup for TBs.
///
/// Uses `UnsafeCell<Vec>` + `AtomicUsize` for lock-free reads
/// and a `Mutex` for hash table mutations.
///
/// Also maintains a per-page reference count: the count is
/// incremented when a TB is created on that page and
/// decremented when the TB is invalidated.  A count > 0 means
/// the page contains at least one valid TB.  The store helper
/// checks this lock-free to decide whether a write needs dirty
/// tracking -- replacing the old clear-and-rebuild bitmap
/// approach with O(1) incremental updates.
pub struct TbStore {
    tbs: UnsafeCell<Vec<TranslationBlock>>,
    len: AtomicUsize,
    hash: Mutex<Vec<Option<usize>>>,
    /// Per-page refcount (0 = no code, >0 = has code TBs).
    /// Index = phys_page = phys_addr >> 12.  Stored as
    /// AtomicU8 for lock-free read from store helpers.
    /// Saturates at 255 (never decrements past zero).
    code_pages: Vec<AtomicU8>,
    /// Per-page head of TB linked list.
    /// page_heads[phys_page] = Some(tb_idx) of first TB
    /// on that page, or None.
    page_heads: Mutex<Vec<Option<usize>>>,
    /// Global generation counter. Incremented by invalidate_all
    /// to instantly invalidate all TBs in O(1). Each TB records
    /// the generation at which it was created; a mismatch
    /// means the TB is stale.
    global_gen: AtomicUsize,
}

// SAFETY:
// - tbs Vec is pre-allocated (no realloc). New entries are
//   appended under translate_lock, then len is published
//   with Release. Readers use Acquire on len.
// - hash is protected by its own Mutex.
unsafe impl Sync for TbStore {}
unsafe impl Send for TbStore {}

impl TbStore {
    pub fn new() -> Self {
        let mut v = Vec::with_capacity(MAX_TBS);
        // Ensure capacity is reserved upfront.
        assert!(v.capacity() >= MAX_TBS);
        v.clear();
        let mut cp = Vec::with_capacity(CODE_REFCOUNT_PAGES);
        for _ in 0..CODE_REFCOUNT_PAGES {
            cp.push(AtomicU8::new(0));
        }
        Self {
            tbs: UnsafeCell::new(v),
            len: AtomicUsize::new(0),
            hash: Mutex::new(vec![None; TB_HASH_SIZE]),
            code_pages: cp,
            page_heads: Mutex::new(vec![None; CODE_REFCOUNT_PAGES]),
            global_gen: AtomicUsize::new(1),
        }
    }

    /// Allocate a new TB. Must be called under translate_lock.
    ///
    /// # Safety
    /// Caller must hold the translate_lock to ensure exclusive
    /// write access to the tbs Vec.
    pub unsafe fn alloc(
        &self,
        pc: u64,
        flags: u32,
        cflags: u32,
    ) -> Option<usize> {
        let tbs = &mut *self.tbs.get();
        let idx = tbs.len();
        if idx >= MAX_TBS {
            return None;
        }
        tbs.push(TranslationBlock::new(pc, flags, cflags));
        // Publish the new length so readers can see it.
        self.len.store(tbs.len(), Ordering::Release);
        Some(idx)
    }

    /// Get a shared reference to a TB by index.
    #[inline(always)]
    pub fn get(&self, idx: usize) -> &TranslationBlock {
        debug_assert!(
            idx < self.len.load(Ordering::Relaxed),
            "TB index out of bounds"
        );
        // SAFETY: idx < len, and the entry at idx is
        // fully initialized (written before len was
        // published with Release ordering in alloc).
        unsafe {
            let tbs = &*self.tbs.get();
            tbs.get_unchecked(idx)
        }
    }

    /// Get a mutable reference to a TB by index.
    ///
    /// # Safety
    /// Caller must ensure exclusive access (e.g. under
    /// translate_lock for immutable fields, or per-TB jmp lock
    /// for chaining fields).
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get_mut(&self, idx: usize) -> &mut TranslationBlock {
        let len = self.len.load(Ordering::Acquire);
        assert!(idx < len, "TB index out of bounds");
        &mut (&mut *self.tbs.get())[idx]
    }

    /// Lookup a valid TB by (pc, flags) in the hash table.
    /// Checks both tb.invalid and generation mismatch to
    /// filter stale entries left by invalidate_all.
    pub fn lookup(&self, pc: u64, flags: u32) -> Option<usize> {
        let gen = self.global_gen.load(Ordering::Acquire);
        let hash = self.hash.lock().unwrap();
        let bucket = TranslationBlock::hash(pc, flags);
        let mut cur = hash[bucket];
        while let Some(idx) = cur {
            let tb = self.get(idx);
            if !tb.invalid.load(Ordering::Acquire)
                && tb.gen.load(Ordering::Acquire) == gen
                && tb.pc == pc
                && tb.flags == flags
            {
                return Some(idx);
            }
            cur = tb.hash_next;
        }
        None
    }

    /// Insert a TB into the hash table (prepend to bucket).
    pub fn insert(&self, tb_idx: usize) {
        let tb = self.get(tb_idx);
        let pc = tb.pc;
        let flags = tb.flags;
        let bucket = TranslationBlock::hash(pc, flags);
        let mut hash = self.hash.lock().unwrap();
        // SAFETY: we need to set hash_next on the TB. This is
        // only called under translate_lock.
        unsafe {
            let tb_mut = self.get_mut(tb_idx);
            tb_mut.hash_next = hash[bucket];
        }
        hash[bucket] = Some(tb_idx);
    }

    /// Mark a TB as invalid, unlink all chained jumps, and
    /// remove it from the hash chain.
    pub fn invalidate<B: HostCodeGen>(
        &self,
        tb_idx: usize,
        code_buf: &CodeBuffer,
        backend: &B,
    ) {
        let tb = self.get(tb_idx);
        tb.invalid.store(true, Ordering::Release);

        // Decrement code-page refcount for the invalidated TB.
        self.dec_code_page(tb.phys_pc >> 12);
        // Unlink from per-page list.
        self.unlink_page(tb.phys_pc >> 12, tb_idx);

        // 1. Unlink incoming edges.
        let jmp_list = {
            let mut jmp = tb.jmp.lock().unwrap();
            std::mem::take(&mut jmp.jmp_list)
        };
        for (src, slot) in jmp_list {
            Self::reset_jump(self.get(src), code_buf, backend, slot);
            let src_tb = self.get(src);
            let mut src_jmp = src_tb.jmp.lock().unwrap();
            src_jmp.jmp_dest[slot] = None;
        }

        // 2. Unlink outgoing edges.
        let outgoing = {
            let mut jmp = tb.jmp.lock().unwrap();
            let mut out = [(0usize, 0usize); 2];
            let mut count = 0;
            for slot in 0..2 {
                if let Some(dst) = jmp.jmp_dest[slot].take() {
                    out[count] = (slot, dst);
                    count += 1;
                }
            }
            (out, count)
        };
        let (out, count) = outgoing;
        for &(_slot, dst) in out.iter().take(count) {
            let dst_tb = self.get(dst);
            let mut dst_jmp = dst_tb.jmp.lock().unwrap();
            dst_jmp
                .jmp_list
                .retain(|&(s, n)| !(s == tb_idx && n == _slot));
        }

        // 3. Remove from hash chain.
        let pc = tb.pc;
        let flags = tb.flags;
        let bucket = TranslationBlock::hash(pc, flags);
        let mut hash = self.hash.lock().unwrap();
        let mut prev: Option<usize> = None;
        let mut cur = hash[bucket];
        while let Some(idx) = cur {
            if idx == tb_idx {
                let next = self.get(idx).hash_next;
                if let Some(p) = prev {
                    unsafe {
                        self.get_mut(p).hash_next = next;
                    }
                } else {
                    hash[bucket] = next;
                }
                unsafe {
                    self.get_mut(idx).hash_next = None;
                }
                return;
            }
            prev = cur;
            cur = self.get(idx).hash_next;
        }
    }

    /// Reset a goto_tb jump back to its original target.
    fn reset_jump<B: HostCodeGen>(
        tb: &TranslationBlock,
        code_buf: &CodeBuffer,
        backend: &B,
        slot: usize,
    ) {
        if let (Some(jmp_off), Some(reset_off)) =
            (tb.jmp_insn_offset[slot], tb.jmp_reset_offset[slot])
        {
            backend.patch_jump(code_buf, jmp_off as usize, reset_off as usize);
        }
    }

    /// Invalidate all valid TBs in O(1) by bumping the
    /// global generation counter. TB lookup and execution
    /// paths check generation mismatch to detect stale TBs.
    /// The hash table and per-page lists retain stale
    /// entries that are filtered by generation checks
    /// during lookup and invalidation.
    pub fn invalidate_all<B: HostCodeGen>(
        &self,
        _code_buf: &CodeBuffer,
        _backend: &B,
    ) {
        // Bump generation: all existing TBs become stale.
        let old = self.global_gen.fetch_add(1, Ordering::Release);
        // Avoid wrap-around to 0 (reserved for new TBs).
        if old + 1 == 0 {
            self.global_gen.store(1, Ordering::Release);
        }
        // Bulk clear code-page refcounts. Not strictly
        // needed for correctness (stale TBs are filtered
        // by gen), but prevents false positives in
        // is_code_page() that would cause unnecessary
        // full-scan invalidation on dirty pages.
        for b in &self.code_pages {
            b.store(0, Ordering::Relaxed);
        }
    }

    /// Invalidate all TBs whose phys_pc falls within the
    /// given physical page. Uses per-page linked list for
    /// O(k) traversal where k = TBs on that page. Falls
    /// back to full scan if the page has refcount > 0 but
    /// the linked list is empty (TBs created before list
    /// was populated).
    pub fn invalidate_phys_page<B: HostCodeGen>(
        &self,
        phys_page: u64,
        code_buf: &CodeBuffer,
        backend: &B,
    ) {
        let page_idx = phys_page as usize;
        if page_idx >= CODE_REFCOUNT_PAGES {
            return;
        }
        let gen = self.global_gen.load(Ordering::Acquire);
        let len = self.len.load(Ordering::Acquire);
        // Fast path: per-page linked list.
        // Skip stale TBs (gen mismatch) and guard against
        // stale page_next indices (from flush interaction).
        let tb_list: Vec<usize> = {
            let heads = self.page_heads.lock().unwrap();
            let mut list = Vec::new();
            let mut cur = heads[page_idx];
            while let Some(idx) = cur {
                if idx >= len {
                    break;
                }
                let tb = self.get(idx);
                if !tb.invalid.load(Ordering::Acquire)
                    && tb.gen.load(Ordering::Acquire) == gen
                {
                    list.push(idx);
                }
                cur = tb.page_next;
            }
            list
        };
        if !tb_list.is_empty() {
            for idx in tb_list {
                self.invalidate(idx, code_buf, backend);
            }
            return;
        }
        // Fallback: full scan if refcount says there are
        // TBs on this page but the list was empty.
        if self.is_code_page(phys_page) {
            for i in 0..len {
                let tb = self.get(i);
                if !tb.invalid.load(Ordering::Acquire)
                    && tb.gen.load(Ordering::Acquire) == gen
                    && (tb.phys_pc >> 12) == phys_page
                {
                    self.invalidate(i, code_buf, backend);
                }
            }
        }
    }

    /// Flush all TBs and reset the hash table.
    ///
    /// # Safety
    /// Caller must ensure no other threads are accessing TBs.
    pub unsafe fn flush(&self) {
        let tbs = &mut *self.tbs.get();
        tbs.clear();
        self.len.store(0, Ordering::Release);
        self.hash.lock().unwrap().fill(None);
        self.page_heads.lock().unwrap().fill(None);
        for b in &self.code_pages {
            b.store(0, Ordering::Relaxed);
        }
    }

    // ── Code-page refcount ──────────────────────────

    /// Increment code-page refcount and prepend TB to
    /// per-page linked list. Called under translate_lock.
    pub fn mark_code_page(&self, phys_page: u64, tb_idx: usize) {
        let idx = phys_page as usize;
        if idx >= CODE_REFCOUNT_PAGES {
            return;
        }
        self.code_pages[idx]
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                Some(v.saturating_add(1))
            })
            .ok();
        // SAFETY: called under translate_lock.
        let mut heads = self.page_heads.lock().unwrap();
        let old_head = heads[idx];
        unsafe {
            self.get_mut(tb_idx).page_next = old_head;
        }
        heads[idx] = Some(tb_idx);
    }

    /// Decrement code-page refcount when a TB is invalidated.
    fn dec_code_page(&self, phys_page: u64) {
        let idx = phys_page as usize;
        if idx < self.code_pages.len() {
            self.code_pages[idx]
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    Some(v.saturating_sub(1))
                })
                .ok();
        }
    }

    /// Check whether a physical page contains code TBs.
    /// Lock-free; safe to call from store helpers.
    /// Returns true when the per-page refcount is > 0.
    pub fn is_code_page(&self, phys_page: u64) -> bool {
        let idx = phys_page as usize;
        if idx < self.code_pages.len() {
            self.code_pages[idx].load(Ordering::Relaxed) > 0
        } else {
            false
        }
    }

    /// Remove a TB from its per-page linked list.
    /// Guards against stale page_next indices from
    /// invalidate_all/flush interaction.
    fn unlink_page(&self, phys_page: u64, tb_idx: usize) {
        let idx = phys_page as usize;
        if idx >= CODE_REFCOUNT_PAGES {
            return;
        }
        let len = self.len.load(Ordering::Acquire);
        let mut heads = self.page_heads.lock().unwrap();
        let mut prev: Option<usize> = None;
        let mut cur = heads[idx];
        while let Some(i) = cur {
            if i >= len {
                break;
            }
            if i == tb_idx {
                let next = self.get(i).page_next;
                if let Some(p) = prev {
                    unsafe {
                        self.get_mut(p).page_next = next;
                    }
                } else {
                    heads[idx] = next;
                }
                unsafe {
                    self.get_mut(tb_idx).page_next = None;
                }
                return;
            }
            prev = cur;
            let next = self.get(i).page_next;
            cur = match next {
                Some(n) if n < len => Some(n),
                _ => None,
            };
        }
    }

    /// Return a raw pointer to the code_pages array for
    /// embedding in the CPU struct (store helper lookup).
    pub fn code_pages_ptr(&self) -> *const AtomicU8 {
        self.code_pages.as_ptr()
    }

    /// Number of bytes in the code-page bitmap.
    pub fn code_pages_len(&self) -> usize {
        self.code_pages.len()
    }

    /// Read the current global generation counter.
    pub fn global_gen(&self) -> usize {
        self.global_gen.load(Ordering::Acquire)
    }

    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for TbStore {
    fn default() -> Self {
        Self::new()
    }
}
