use crate::chars::Char;
use crate::matrix::{haystack, HaystackChar, Matrix, MatrixCell, MatrixRow, MatrixRowMut};
use std::fmt::{Debug, Formatter, Result};

impl<C: Char> Matrix<'_, C> {
    pub fn rows(&self) -> impl Iterator<Item = MatrixRow> + ExactSizeIterator + Clone + Sized {
        let mut cells = &*self.cells;
        self.row_offs.iter().map(move |&off| {
            let len = self.haystack.len() - off as usize;
            let (row, tmp) = cells.split_at(len);
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

impl Debug for MatrixCell {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "({}, {})", self.score, self.consecutive_chars)
    }
}
impl<C: Char> Debug for HaystackChar<C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "({}, {})", self.char, self.bonus)
    }
}
impl Debug for MatrixRow<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let mut f = f.debug_list();
        f.entries((0..self.off).map(|_| &MatrixCell {
            score: 0,
            consecutive_chars: 0,
        }));
        f.entries(self.cells.iter());
        f.finish()
    }
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
impl<'a, C: Char> Debug for Matrix<'a, C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        f.debug_struct("Matrix")
            .field("haystack", &DebugList(self.haystack()))
            .field("matrix", &DebugList(self.rows()))
            .finish()
    }
}
