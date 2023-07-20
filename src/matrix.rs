use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};
use std::fmt::{Debug, Formatter, Result};
use std::marker::PhantomData;
use std::mem::{size_of, take};
use std::ops::Index;
use std::ptr::{slice_from_raw_parts_mut, NonNull};

use crate::chars::Char;

const MAX_MATRIX_SIZE: usize = 100 * 1024; // 4*60*1024 = 240KB

// these two aren't hard maxima, instead we simply allow whatever will fit into memory
const MAX_HAYSTACK_LEN: usize = 2048; // 64KB
const MAX_NEEDLE_LEN: usize = 2048; // 64KB

struct MatrixLayout<C: Char> {
    haystack_len: usize,
    needle_len: usize,
    cell_count: usize,
    layout: Layout,
    haystack_off: usize,
    bonus_off: usize,
    rows_off: usize,
    cells_off: usize,
    _phantom: PhantomData<C>,
}
impl<C: Char> MatrixLayout<C> {
    fn new(haystack_len: usize, needle_len: usize, cell_count: usize) -> MatrixLayout<C> {
        let mut layout = Layout::from_size_align(0, 1).unwrap();
        let haystack_layout = Layout::array::<C>(haystack_len).unwrap();
        let bonus_layout = Layout::array::<u16>(haystack_len).unwrap();
        let rows_layout = Layout::array::<u16>(needle_len).unwrap();
        let cells_layout = Layout::array::<MatrixCell>(cell_count).unwrap();

        let haystack_off;
        (layout, haystack_off) = layout.extend(haystack_layout).unwrap();
        let bonus_off;
        (layout, bonus_off) = layout.extend(bonus_layout).unwrap();
        let rows_off;
        (layout, rows_off) = layout.extend(rows_layout).unwrap();
        let cells_off;
        (layout, cells_off) = layout.extend(cells_layout).unwrap();
        MatrixLayout {
            haystack_len,
            needle_len,
            cell_count,
            layout,
            haystack_off,
            bonus_off,
            rows_off,
            cells_off,
            _phantom: PhantomData,
        }
    }
    /// # Safety
    ///
    /// `ptr` must point at an allocated with MARTIX_ALLOC_LAYOUT
    unsafe fn fieds_from_ptr(
        &self,
        ptr: NonNull<u8>,
    ) -> (*mut [C], *mut [u16], *mut [u16], *mut [MatrixCell]) {
        // sanity checks, should not be necessary

        let base = ptr.as_ptr();
        let haystack = base.add(self.haystack_off) as *mut C;
        let haystack = slice_from_raw_parts_mut(haystack, self.haystack_len);
        let bonus = base.add(self.bonus_off) as *mut u16;
        let bonus = slice_from_raw_parts_mut(bonus, self.haystack_len);
        let rows = base.add(self.rows_off) as *mut u16;
        let rows = slice_from_raw_parts_mut(rows, self.needle_len);
        let cells = base.add(self.cells_off) as *mut MatrixCell;
        let cells = slice_from_raw_parts_mut(cells, self.cell_count);
        (haystack, bonus, rows, cells)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct MatrixCell {
    pub score: u16,
    pub consecutive_chars: u16,
}

impl Debug for MatrixCell {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        (self.score, self.consecutive_chars).fmt(f)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct HaystackChar<C: Char> {
    pub char: C,
    pub bonus: u16,
}

impl<C: Char> Debug for HaystackChar<C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        (self.char, self.bonus).fmt(f)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MatrixRow<'a> {
    pub off: u16,
    pub cells: &'a [MatrixCell],
}
impl Index<u16> for MatrixRow<'_> {
    type Output = MatrixCell;

    fn index(&self, index: u16) -> &Self::Output {
        &self.cells[index as usize]
    }
}

impl Debug for MatrixRow<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let mut f = f.debug_list();
        f.entries((0..self.off).map(|_| &(0, 0)));
        f.entries(self.cells.iter());
        f.finish()
    }
}

pub(crate) struct MatrixRowMut<'a> {
    pub off: u16,
    pub cells: &'a mut [MatrixCell],
}

impl Debug for MatrixRowMut<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let mut f = f.debug_list();
        f.entries((0..self.off).map(|_| &(0, 0)));
        f.entries(self.cells.iter());
        f.finish()
    }
}

pub struct DebugList<I>(I);
impl<I> Debug for DebugList<I>
where
    I: Iterator + Clone,
    I::Item: Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_list().entries(self.0.clone()).finish()
    }
}

pub(crate) struct Matrix<'a, C: Char> {
    pub haystack: &'a mut [C],
    // stored as a seperate array instead of struct
    // to avoid padding sine char is too large and u8 too small :/
    pub bonus: &'a mut [u16],
    pub row_offs: &'a mut [u16],
    pub cells: &'a mut [MatrixCell],
}

impl<'a, C: Char> Matrix<'a, C> {
    pub fn rows(&self) -> impl Iterator<Item = MatrixRow> + ExactSizeIterator + Clone + Sized {
        let mut cells = &*self.cells;
        self.row_offs.iter().map(move |&off| {
            let len = self.haystack.len() - off as usize;
            let (row, tmp) = cells.split_at(len);
            cells = tmp;
            MatrixRow { off, cells: row }
        })
    }

    pub fn rows_rev(&self) -> impl Iterator<Item = MatrixRow> + ExactSizeIterator {
        let mut cells = &*self.cells;
        self.row_offs.iter().rev().map(move |&off| {
            let len = self.haystack.len() - off as usize;
            let (tmp, row) = cells.split_at(cells.len() - len);
            cells = tmp;
            MatrixRow { off, cells: row }
        })
    }
    pub fn haystack(
        &self,
    ) -> impl Iterator<Item = HaystackChar<C>> + ExactSizeIterator + '_ + Clone {
        haystack(self.haystack, self.bonus, 0)
    }
}

impl<'a, C: Char> Debug for Matrix<'a, C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("Matrix")
            .field("haystack", &DebugList(self.haystack()))
            .field("matrix", &DebugList(self.rows()))
            .finish()
    }
}
pub(crate) fn haystack<'a, C: Char>(
    haystack: &'a [C],
    bonus: &'a [u16],
    skip: u16,
) -> impl Iterator<Item = HaystackChar<C>> + ExactSizeIterator + Clone + 'a {
    haystack[skip as usize..]
        .iter()
        .zip(bonus[skip as usize..].iter())
        .map(|(&char, &bonus)| HaystackChar { char, bonus })
}

pub(crate) fn rows_mut<'a>(
    row_offs: &'a [u16],
    mut cells: &'a mut [MatrixCell],
    haystack_len: usize,
) -> impl Iterator<Item = MatrixRowMut<'a>> + ExactSizeIterator + 'a {
    row_offs.iter().map(move |&off| {
        let len = haystack_len - off as usize;
        let (row, tmp) = take(&mut cells).split_at_mut(len);
        cells = tmp;
        MatrixRowMut { off, cells: row }
    })
}

// we only use this to construct the layout for the slab allocation
#[allow(unused)]
struct MatrixData {
    haystack: [char; MAX_HAYSTACK_LEN],
    bonus: [u16; MAX_HAYSTACK_LEN],
    row_offs: [u16; MAX_NEEDLE_LEN],
    cells: [MatrixCell; MAX_MATRIX_SIZE],
}

// const MATRIX_ALLOC_LAYOUT: Layout =
//     MatrixLayout::<char>::new(MAX_HAYSTACK_LEN, MAX_NEEDLE_LEN, MAX_MATRIX_SIZE).layout;

pub(crate) struct MatrixSlab(NonNull<u8>);

impl MatrixSlab {
    pub fn new() -> Self {
        let layout = Layout::new::<MatrixData>();
        // safety: the matrix is never zero sized (hardcoded constants)
        let ptr = unsafe { alloc_zeroed(layout) };
        let Some(ptr) = NonNull::new(ptr) else{
            handle_alloc_error(layout)
        };
        MatrixSlab(ptr.cast())
    }

    pub(crate) fn alloc<C: Char>(
        &mut self,
        haystack_: &[C],
        needle_len: usize,
    ) -> Option<Matrix<'_, C>> {
        let cells = haystack_.len() * needle_len;
        if cells > MAX_MATRIX_SIZE || haystack_.len() > u16::MAX as usize {
            return None;
        }
        let matrix_layout = MatrixLayout::<C>::new(
            haystack_.len(),
            needle_len,
            (haystack_.len() - needle_len / 2) * needle_len,
        );
        if matrix_layout.layout.size() > size_of::<MatrixData>() {
            return None;
        }
        unsafe {
            // safetly: this allocation is valid for MATRIX_ALLOC_LAYOUT
            let (haystack, bonus, rows, cells) = matrix_layout.fieds_from_ptr(self.0);
            // copy haystack before creating refernces to ensure we donu't crate
            // refrences to invalid chars (which may or may not be UB)
            haystack_
                .as_ptr()
                .copy_to_nonoverlapping(haystack as *mut _, haystack_.len());
            Some(Matrix {
                haystack: &mut *haystack,
                row_offs: &mut *rows,
                bonus: &mut *bonus,
                cells: &mut *cells,
            })
        }
    }
}

impl Drop for MatrixSlab {
    fn drop(&mut self) {
        unsafe { dealloc(self.0.as_ptr(), Layout::new::<MatrixData>()) };
    }
}
