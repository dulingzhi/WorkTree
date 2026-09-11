//! Levenshtein distance behind replacement-pair cost, including the
//! bit-parallel ASCII fast path.

const ASCII_BITPARALLEL_MAX_PATTERN_LEN: usize = 128;

#[derive(Default)]
pub(super) struct LevenshteinScratch {
    cache: Vec<usize>,
}

impl LevenshteinScratch {
    pub(super) fn distance<T: Eq>(&mut self, a: &[T], b: &[T]) -> usize {
        if a == b {
            return 0;
        }
        if a.is_empty() {
            return b.len();
        }
        if b.is_empty() {
            return a.len();
        }

        self.distance_non_empty_unequal(a, b)
    }

    pub(super) fn distance_non_empty_unequal<T: Eq>(&mut self, a: &[T], b: &[T]) -> usize {
        distance_non_empty_unequal_with_cache(&mut self.cache, a, b)
    }

    pub(super) fn distance_bytes(&mut self, a: &[u8], b: &[u8]) -> usize {
        if a == b {
            return 0;
        }
        if a.is_empty() {
            return b.len();
        }
        if b.is_empty() {
            return a.len();
        }

        if let Some(distance) = bitparallel_levenshtein_bytes(a, b) {
            return distance;
        }

        distance_non_empty_unequal_with_cache(&mut self.cache, a, b)
    }
}

fn distance_non_empty_unequal_with_cache<T: Eq>(cache: &mut Vec<usize>, a: &[T], b: &[T]) -> usize {
    debug_assert!(!a.is_empty());
    debug_assert!(!b.is_empty());

    let (a, b) = if b.len() > a.len() { (a, b) } else { (b, a) };
    debug_assert!(a != b);

    let b_len = b.len();
    cache.resize(b_len, 0);
    let cache = &mut cache[..b_len];
    for (ix, slot) in cache.iter_mut().enumerate() {
        *slot = ix + 1;
    }

    let mut result = b_len;
    for (i, a_ch) in a.iter().enumerate() {
        result = i + 1;
        let mut distance_b = i;
        for (j, b_ch) in b.iter().enumerate() {
            let cost = usize::from(a_ch != b_ch);
            let distance_a = distance_b + cost;
            distance_b = cache[j];
            result = (result + 1).min(distance_a).min(distance_b + 1);
            cache[j] = result;
        }
    }

    result
}

pub(super) fn bitparallel_levenshtein_bytes(a: &[u8], b: &[u8]) -> Option<usize> {
    let (pattern, text) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let pattern_len = pattern.len();
    if pattern_len == 0 {
        return Some(text.len());
    }
    if pattern_len > ASCII_BITPARALLEL_MAX_PATTERN_LEN {
        return None;
    }

    let mut eq_masks = [0u128; 256];
    for (ix, &byte) in pattern.iter().enumerate() {
        eq_masks[byte as usize] |= 1u128 << ix;
    }

    let mask = if pattern_len == ASCII_BITPARALLEL_MAX_PATTERN_LEN {
        u128::MAX
    } else {
        (1u128 << pattern_len) - 1
    };
    let high_bit = 1u128 << (pattern_len - 1);
    let mut positive = mask;
    let mut negative = 0u128;
    let mut distance = pattern_len;

    for &byte in text {
        let eq = eq_masks[byte as usize];
        let xv = eq | negative;
        let xh = (((eq & positive).wrapping_add(positive)) ^ positive) | eq;
        let mut positive_h = negative | !(xh | positive);
        let mut negative_h = positive & xh;

        if (positive_h & high_bit) != 0 {
            distance += 1;
        } else if (negative_h & high_bit) != 0 {
            distance -= 1;
        }

        positive_h = ((positive_h << 1) | 1) & mask;
        negative_h = (negative_h << 1) & mask;
        positive = (negative_h | !(xv | positive_h)) & mask;
        negative = positive_h & xv;
    }

    Some(distance)
}
