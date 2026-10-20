//! `TColStd_PackedMapOfInteger` (`NCollection_PackedMap<int>`) with its iteration order, and
//! `Poly_MakeLoops::HeapOfInteger`.
use crate::mathlin::MathError;

/// `NCollection_Primes::NextPrimeForMap`: the first prime of the list that is at least `n + 1` (the last one when
/// none is).
fn next_prime_for_map(n: usize) -> usize {
    const PRIMES: [usize; 13] =
        [101, 1009, 2003, 5003, 10007, 20011, 37003, 57037, 65003, 100019, 209953, 472393, 995329];
    PRIMES.iter().copied().find(|&p| p > n).unwrap_or(995329)
}

/// A node of the map: a block of 32 keys.
#[derive(Debug, Clone, Copy)]
struct Node {
    block: u32,
    mask: u32,
}

/// `TColStd_PackedMapOfInteger`: the keys are stored in blocks of 32, in hash chains whose order (head first) and
/// whose bucket count are those of OCCT, so that iteration gives the same sequence.
#[derive(Debug, Clone)]
pub struct PackedMapOfInteger {
    nb_buckets: usize,
    /// `buckets[b]` is the chain of bucket `b` (`1..=nb_buckets`), the head being the **last** element.
    buckets: Vec<Vec<Node>>,
    nodes: usize,
    extent: i32,
}

/// Position of an iterator: the bucket, the block and the key.
#[derive(Debug, Clone, Copy)]
struct IterPos {
    bucket: usize,
    block: u32,
    key: i32,
}

impl PackedMapOfInteger {
    /// `TColStd_PackedMapOfInteger(NbBuckets)` (less than 1 is 1).
    pub fn new(nb_buckets: i32) -> Self {
        Self { nb_buckets: nb_buckets.max(1) as usize, buckets: Vec::new(), nodes: 0, extent: 0 }
    }
    fn block_of(key: i32) -> u32 {
        (key as u32) >> 5
    }
    fn bucket_of(block: u32, nb: usize) -> usize {
        block as usize % nb + 1
    }
    fn resize(&mut self, n: usize) {
        let mut nb = next_prime_for_map(n);
        if nb <= self.nb_buckets {
            if !self.is_empty() {
                return;
            }
            nb = self.nb_buckets;
        }
        let mut new_buckets: Vec<Vec<Node>> = vec![Vec::new(); nb + 1];
        for chain in &self.buckets {
            for node in chain.iter().rev() {
                let b = Self::bucket_of(node.block, nb);
                if let Some(c) = new_buckets.get_mut(b) {
                    c.push(*node);
                }
            }
        }
        self.buckets = new_buckets;
        self.nb_buckets = nb;
    }
    fn find(&self, block: u32) -> Option<(usize, usize)> {
        let b = Self::bucket_of(block, self.nb_buckets);
        let chain = self.buckets.get(b)?;
        let p = chain.iter().rposition(|n| n.block == block)?;
        Some((b, p))
    }
    /// `Add(key)`: true when the key was not there.
    pub fn add(&mut self, key: i32) -> bool {
        if self.is_empty() || self.nodes > self.nb_buckets {
            self.resize(self.nodes);
        }
        if self.buckets.len() <= self.nb_buckets {
            self.buckets.resize(self.nb_buckets + 1, Vec::new());
        }
        let block = Self::block_of(key);
        let bit = 1u32 << (key as u32 & 31);
        if let Some((b, p)) = self.find(block) {
            if let Some(node) = self.buckets.get_mut(b).and_then(|c| c.get_mut(p)) {
                if node.mask & bit != 0 {
                    return false;
                }
                node.mask |= bit;
            }
            self.extent += 1;
            return true;
        }
        let b = Self::bucket_of(block, self.nb_buckets);
        if let Some(c) = self.buckets.get_mut(b) {
            c.push(Node { block, mask: bit });
        }
        self.nodes += 1;
        self.extent += 1;
        true
    }
    /// `Remove(key)`: true when the key was there.
    pub fn remove(&mut self, key: i32) -> bool {
        let block = Self::block_of(key);
        let bit = 1u32 << (key as u32 & 31);
        let Some((b, p)) = self.find(block) else { return false };
        let Some(chain) = self.buckets.get_mut(b) else { return false };
        let Some(node) = chain.get_mut(p) else { return false };
        if node.mask & bit == 0 {
            return false;
        }
        node.mask &= !bit;
        self.extent -= 1;
        if node.mask == 0 {
            chain.remove(p);
            self.nodes -= 1;
        }
        true
    }
    /// `Contains(key)`.
    pub fn contains(&self, key: i32) -> bool {
        let block = Self::block_of(key);
        let bit = 1u32 << (key as u32 & 31);
        self.find(block)
            .and_then(|(b, p)| self.buckets.get(b).and_then(|c| c.get(p)))
            .is_some_and(|n| n.mask & bit != 0)
    }
    /// `Extent()`: the number of keys.
    pub fn extent(&self) -> i32 {
        self.extent
    }
    /// `IsEmpty()`.
    pub fn is_empty(&self) -> bool {
        self.extent == 0
    }
    /// `Clear()`: the bucket count stays.
    pub fn clear(&mut self) {
        self.buckets.clear();
        self.nodes = 0;
        self.extent = 0;
    }
    fn key_of(block: u32, bit: u32) -> i32 {
        ((block << 5) | bit) as i32
    }
    /// The first key of a chain starting at (`bucket`, node position `from` counting down from the head).
    fn first_from(&self, bucket: usize, mut from: Option<usize>) -> Option<IterPos> {
        let mut b = bucket;
        while b < self.buckets.len() {
            if let Some(chain) = self.buckets.get(b) {
                let start = from.take().or(chain.len().checked_sub(1));
                let mut p = start;
                while let Some(i) = p {
                    if let Some(node) = chain.get(i)
                        && node.mask != 0
                    {
                        let bit = node.mask.trailing_zeros();
                        return Some(IterPos { bucket: b, block: node.block, key: Self::key_of(node.block, bit) });
                    }
                    p = i.checked_sub(1);
                }
            }
            b += 1;
        }
        None
    }
    fn iter_first(&self) -> Option<IterPos> {
        self.first_from(0, None)
    }
    fn iter_next(&self, pos: IterPos) -> Option<IterPos> {
        let chain = self.buckets.get(pos.bucket)?;
        let p = chain.iter().rposition(|n| n.block == pos.block)?;
        let node = chain.get(p)?;
        let bit = pos.key as u32 & 31;
        let above = if bit >= 31 { 0 } else { node.mask & !((1u32 << (bit + 1)) - 1) };
        if above != 0 {
            return Some(IterPos {
                bucket: pos.bucket,
                block: pos.block,
                key: Self::key_of(pos.block, above.trailing_zeros()),
            });
        }
        match p.checked_sub(1) {
            Some(q) => self.first_from(pos.bucket, Some(q)),
            None => self.first_from(pos.bucket + 1, None),
        }
    }
    /// The keys in iteration order (buckets ascending, each chain from its head, each block from its lowest key).
    pub fn keys(&self) -> Vec<i32> {
        let mut v = Vec::new();
        let mut it = self.iter_first();
        while let Some(pos) = it {
            v.push(pos.key);
            it = self.iter_next(pos);
        }
        v
    }
}

/// `Poly_MakeLoops::HeapOfInteger`: a packed map with a lazily initialised iterator whose current key is the top.
#[derive(Debug, Clone)]
pub struct HeapOfInteger {
    map: PackedMapOfInteger,
    iter: Option<Option<IterPos>>,
}

impl HeapOfInteger {
    /// `HeapOfInteger(theNbPreAllocated)`.
    pub fn new(nb: i32) -> Self {
        Self { map: PackedMapOfInteger::new(nb), iter: None }
    }
    /// `Clear()`.
    pub fn clear(&mut self) {
        self.map.clear();
        self.iter = None;
    }
    /// `Add(value)`.
    pub fn add(&mut self, value: i32) {
        self.map.add(value);
        self.iter = None;
    }
    fn ready(&mut self) -> Option<IterPos> {
        match self.iter {
            Some(p) => p,
            None => {
                let p = self.map.iter_first();
                self.iter = Some(p);
                p
            }
        }
    }
    /// `Top()` (an empty heap is `Err`).
    pub fn top(&mut self) -> Result<i32, MathError> {
        self.ready().map(|p| p.key).ok_or(MathError::Domain)
    }
    /// `Contains(value)`.
    pub fn contains(&self, value: i32) -> bool {
        self.map.contains(value)
    }
    /// `Remove(value)`: the iterator first moves on when it points at that key.
    pub fn remove(&mut self, value: i32) {
        if let Some(Some(pos)) = self.iter
            && pos.key == value
        {
            self.iter = Some(self.map.iter_next(pos));
        }
        self.map.remove(value);
    }
    /// `IsEmpty()` (initialises the iterator).
    pub fn is_empty(&mut self) -> bool {
        self.ready().is_none()
    }
}
