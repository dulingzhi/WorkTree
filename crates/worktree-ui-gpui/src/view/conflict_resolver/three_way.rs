#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(in crate::view) enum ThreeWayColumn {
    Base,
    Ours,
    Theirs,
}

impl ThreeWayColumn {
    /// Index into `[base, ours, theirs]` arrays and the aligned map.
    pub(in crate::view) fn side_index(self) -> usize {
        match self {
            ThreeWayColumn::Base => 0,
            ThreeWayColumn::Ours => 1,
            ThreeWayColumn::Theirs => 2,
        }
    }

    pub(in crate::view) const ALL: [ThreeWayColumn; 3] = [
        ThreeWayColumn::Base,
        ThreeWayColumn::Ours,
        ThreeWayColumn::Theirs,
    ];
}

#[derive(Clone, Debug, Default)]
pub(in crate::view) struct ThreeWaySides<T> {
    pub(in crate::view) base: T,
    pub(in crate::view) ours: T,
    pub(in crate::view) theirs: T,
}

impl<T> std::ops::Index<ThreeWayColumn> for ThreeWaySides<T> {
    type Output = T;
    fn index(&self, side: ThreeWayColumn) -> &T {
        match side {
            ThreeWayColumn::Base => &self.base,
            ThreeWayColumn::Ours => &self.ours,
            ThreeWayColumn::Theirs => &self.theirs,
        }
    }
}

impl<T> std::ops::IndexMut<ThreeWayColumn> for ThreeWaySides<T> {
    fn index_mut(&mut self, side: ThreeWayColumn) -> &mut T {
        match side {
            ThreeWayColumn::Base => &mut self.base,
            ThreeWayColumn::Ours => &mut self.ours,
            ThreeWayColumn::Theirs => &mut self.theirs,
        }
    }
}
