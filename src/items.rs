use std::mem::swap;
use std::ptr::NonNull;

use crate::Utf32String;

pub(crate) struct ItemCache {
    live: Vec<Item>,
    evicted: Vec<Item>,
}
impl ItemCache {
    pub(crate) fn new() -> Self {
        Self {
            live: Vec::with_capacity(1024),
            evicted: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        if self.evicted.is_empty() {
            self.evicted.reserve(1024);
            swap(&mut self.evicted, &mut self.live)
        } else {
            self.evicted.append(&mut self.live)
        }
    }

    pub(crate) fn cleared(&self) -> bool {
        !self.evicted.is_empty()
    }

    pub(crate) fn push(&mut self, item: Box<[Utf32String]>) {
        self.live.push(Item {
            cols: Box::leak(item).into(),
        })
    }

    pub(crate) fn get(&mut self) -> &mut [Item] {
        &mut self.live
    }
}

#[derive(PartialEq, Eq, Clone)]
pub struct Item {
    // TODO: small vec optimization??
    cols: NonNull<[Utf32String]>,
}

impl std::fmt::Debug for Item {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ItemText")
            .field("cols", &self.cols())
            .finish()
    }
}

unsafe impl Send for Item {}
unsafe impl Sync for Item {}

impl Item {
    pub fn cols(&self) -> &[Utf32String] {
        // safety: cols is basically a box and treated the same as a box,
        // however there can be other references  so using a box (unique ptr)
        // would be an alias violation
        unsafe { self.cols.as_ref() }
    }
}
impl Drop for Item {
    fn drop(&mut self) {
        // safety: cols is basically a box and treated the same as a box,
        // however there can be other references (that won't be accessed
        // anymore at this point) so using a box (unique ptr) would be an alias
        // violation
        unsafe { drop(Box::from_raw(self.cols.as_ptr())) }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ItemSnapshot {
    cols: NonNull<[Utf32String]>,
    pub(crate) len: u32,
}

unsafe impl Send for ItemSnapshot {}
unsafe impl Sync for ItemSnapshot {}

#[derive(Debug, Clone)]
pub(crate) struct ItemsSnapshot {
    items: Vec<ItemSnapshot>,
}

impl ItemsSnapshot {
    pub(crate) fn new(items: &ItemCache) -> Self {
        Self {
            items: items
                .live
                .iter()
                .map(|item| ItemSnapshot {
                    cols: item.cols,
                    len: item.cols().iter().map(|s| s.len() as u32).sum(),
                })
                .collect(),
        }
    }

    pub(crate) fn outdated(&self, items: &ItemCache) -> bool {
        items.live.len() != self.items.len()
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn update(&mut self, items: &ItemCache) -> bool {
        let cleared = !items.evicted.is_empty();
        // drop in another thread to ensure we don't wait for a long drop here
        if cleared {
            self.items.clear();
        };
        let start = self.items.len();
        self.items
            .extend(items.live[start..].iter().map(|item| ItemSnapshot {
                cols: item.cols,
                len: item.cols().iter().map(|s| s.len() as u32).sum(),
            }));
        cleared
    }

    pub(crate) unsafe fn get(&self) -> &[ItemSnapshot] {
        &self.items
    }
}

impl ItemSnapshot {
    pub(crate) fn cols(&self) -> &[Utf32String] {
        // safety: we only hand out ItemSnapshot ranges
        // if the caller asserted via the unsafe ItemsSnapshot::get
        // function that the pointers are valid
        unsafe { self.cols.as_ref() }
    }
}
