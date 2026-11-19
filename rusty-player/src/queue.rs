//! The play queue with YouTube-Music-style shuffle.

use crate::webapi::TrackSummary;

#[derive(Debug, Clone, PartialEq)]
pub struct QueueEntry {
    pub id: u64,
    pub track: TrackSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueueSource {
    Playlist(String),
    Liked,
    Search,
    Stats,
    Link,
}

pub struct Queue {
    entries: Vec<QueueEntry>,
    pos: Option<usize>,
    original: Option<Vec<QueueEntry>>,
    source: QueueSource,
    next_id: u64,
    rng: u64,
}

impl Queue {
    pub fn new(tracks: Vec<TrackSummary>, start: usize, source: QueueSource, seed: u64) -> Self {
        let entries: Vec<_> = tracks
            .into_iter()
            .enumerate()
            .map(|(index, track)| QueueEntry {
                id: index as u64,
                track,
            })
            .collect();
        let pos = entries.len().checked_sub(1).map(|last| start.min(last));
        Self {
            next_id: entries.len() as u64,
            entries,
            pos,
            original: None,
            source,
            rng: if seed == 0 { 0x9e3779b97f4a7c15 } else { seed },
        }
    }

    pub fn entries(&self) -> &[QueueEntry] {
        &self.entries
    }

    pub fn pos(&self) -> Option<usize> {
        self.pos
    }

    pub fn current(&self) -> Option<&TrackSummary> {
        self.pos.map(|pos| &self.entries[pos].track)
    }

    pub fn source(&self) -> &QueueSource {
        &self.source
    }

    pub fn is_shuffled(&self) -> bool {
        self.original.is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[allow(dead_code)] // pairs with `len` (clippy::len_without_is_empty)
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn peek_next(&self) -> Option<&TrackSummary> {
        self.entries.get(self.pos? + 1).map(|entry| &entry.track)
    }

    pub fn advance(&mut self) -> Option<&TrackSummary> {
        self.jump_to(self.pos? + 1)
    }

    pub fn back(&mut self) -> Option<&TrackSummary> {
        self.jump_to(self.pos?.checked_sub(1)?)
    }

    pub fn jump_to(&mut self, index: usize) -> Option<&TrackSummary> {
        if index >= self.entries.len() {
            return None;
        }
        self.pos = Some(index);
        self.current()
    }

    pub fn move_entry(&mut self, from: usize, to: usize) {
        if from >= self.entries.len() {
            return;
        }
        let current_id = self.pos.map(|pos| self.entries[pos].id);
        let entry = self.entries.remove(from);
        self.entries.insert(to.min(self.entries.len()), entry);
        self.pos = current_id.and_then(|id| self.entries.iter().position(|entry| entry.id == id));
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if on {
            if self.is_shuffled() {
                return;
            }
            self.original = Some(self.entries.clone());
            let first = self.pos.map_or(0, |pos| pos + 1);
            for index in (first + 1..self.entries.len()).rev() {
                let other = first + self.rand_below(index - first + 1);
                self.entries.swap(index, other);
            }
        } else if let Some(original) = self.original.take() {
            let current_id = self.pos.map(|pos| self.entries[pos].id);
            self.entries = original;
            self.pos =
                current_id.and_then(|id| self.entries.iter().position(|entry| entry.id == id));
        }
    }

    pub fn append(&mut self, tracks: Vec<TrackSummary>) {
        for track in tracks {
            let entry = QueueEntry {
                id: self.next_id,
                track,
            };
            self.next_id += 1;
            if self.is_shuffled() {
                let first = self.pos.map_or(0, |pos| pos + 1);
                let index = first + self.rand_below(self.entries.len() - first + 1);
                if let Some(original) = &mut self.original {
                    original.push(entry.clone());
                }
                self.entries.insert(index, entry);
            } else {
                self.entries.push(entry);
            }
        }
    }

    fn rand_below(&mut self, n: usize) -> usize {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng % n as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: usize) -> TrackSummary {
        TrackSummary {
            uri: format!("spotify:track:{n}"),
            name: format!("Track {n}"),
            artists: String::new(),
            album: String::new(),
            duration_ms: 0,
            image_url: None,
        }
    }

    fn queue(start: usize) -> Queue {
        Queue::new((0..8).map(t).collect(), start, QueueSource::Search, 42)
    }

    fn ids(queue: &Queue) -> Vec<u64> {
        queue.entries().iter().map(|entry| entry.id).collect()
    }

    #[test]
    fn navigation_and_clamped_start() {
        let source = QueueSource::Playlist("test-playlist".into());
        let mut queue = Queue::new(vec![t(0), t(1), t(2)], 0, source.clone(), 1);
        assert_eq!(queue.source(), &source);
        assert_eq!(queue.len(), 3);
        assert!(!queue.is_empty());
        assert!(!queue.is_shuffled());
        assert_eq!(queue.pos(), Some(0));
        assert_eq!(queue.current(), Some(&t(0)));
        assert_eq!(queue.peek_next(), Some(&t(1)));
        assert_eq!(queue.back(), None);
        assert_eq!(queue.pos(), Some(0));
        assert_eq!(queue.advance(), Some(&t(1)));
        assert_eq!(queue.back(), Some(&t(0)));
        assert_eq!(queue.advance(), Some(&t(1)));
        assert_eq!(queue.advance(), Some(&t(2)));
        assert_eq!(queue.peek_next(), None);
        assert_eq!(queue.advance(), None);
        assert_eq!(queue.pos(), Some(2));
        assert_eq!(queue.current(), Some(&t(2)));
        let clamped = Queue::new(vec![t(0), t(1)], usize::MAX, QueueSource::Liked, 1);
        assert_eq!(clamped.pos(), Some(1));
        assert_eq!(clamped.current(), Some(&t(1)));
    }

    #[test]
    fn empty_queue() {
        let mut queue = Queue::new(Vec::new(), usize::MAX, QueueSource::Link, 0);
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
        assert!(queue.entries().is_empty());
        assert_eq!(queue.pos(), None);
        assert_eq!(queue.current(), None);
        assert_eq!(queue.peek_next(), None);
        assert_eq!(queue.advance(), None);
        assert_eq!(queue.back(), None);
        assert_eq!(queue.jump_to(0), None);
        queue.move_entry(0, 0);
        queue.set_shuffle(true);
        assert!(queue.is_shuffled());
        queue.set_shuffle(false);
        assert!(!queue.is_shuffled());
        assert_eq!(queue.pos(), None);
    }

    #[test]
    fn move_current_down_three_and_clamp_destination() {
        let mut queue = queue(1);
        queue.move_entry(1, 4);
        assert_eq!(ids(&queue), vec![0, 2, 3, 4, 1, 5, 6, 7]);
        assert_eq!(queue.pos(), Some(4));
        assert_eq!(queue.current(), Some(&t(1)));
        assert_eq!(queue.peek_next(), Some(&t(5)));
        queue.move_entry(4, usize::MAX);
        assert_eq!(queue.pos(), Some(7));
        assert_eq!(queue.current(), Some(&t(1)));
        assert_eq!(queue.peek_next(), None);
    }

    #[test]
    fn move_across_current_and_within_up_next() {
        let mut queue = queue(3);
        queue.move_entry(6, 1);
        assert_eq!(ids(&queue), vec![0, 6, 1, 2, 3, 4, 5, 7]);
        assert_eq!(queue.pos(), Some(4));
        assert_eq!(queue.current(), Some(&t(3)));
        queue.move_entry(1, 6);
        assert_eq!(queue.pos(), Some(3));
        assert_eq!(queue.current(), Some(&t(3)));
        assert_eq!(ids(&queue), (0..8).collect::<Vec<_>>());
        queue.move_entry(4, 6);
        assert_eq!(ids(&queue), vec![0, 1, 2, 3, 5, 6, 4, 7]);
        assert_eq!(queue.pos(), Some(3));
        assert_eq!(queue.current(), Some(&t(3)));
        let before = queue.entries().to_vec();
        queue.move_entry(usize::MAX, 0);
        queue.move_entry(3, 3);
        assert_eq!(queue.entries(), before);
        assert_eq!(queue.pos(), Some(3));
    }

    #[test]
    fn shuffle_preserves_prefix_and_is_a_deterministic_permutation() {
        let mut first = queue(2);
        let original = first.entries().to_vec();
        let mut second = queue(2);
        first.set_shuffle(true);
        second.set_shuffle(true);
        assert!(first.is_shuffled());
        assert_eq!(first.entries(), second.entries());
        assert_eq!(&first.entries()[..3], &original[..3]);
        assert_eq!(first.pos(), Some(2));
        assert_eq!(first.current(), Some(&t(2)));
        let mut upcoming = ids(&first)[3..].to_vec();
        upcoming.sort_unstable();
        assert_eq!(upcoming, vec![3, 4, 5, 6, 7]);
        let shuffled = first.entries().to_vec();
        first.set_shuffle(true);
        assert_eq!(first.entries(), shuffled);
        let current_id = first.entries()[5].id;
        first.jump_to(5);
        first.set_shuffle(false);
        assert!(!first.is_shuffled());
        assert_eq!(first.entries(), original);
        assert_eq!(first.pos(), Some(current_id as usize));
        assert_eq!(first.current(), Some(&t(current_id as usize)));
        first.set_shuffle(false);
        assert_eq!(first.entries(), original);
    }

    #[test]
    fn append_shuffled_entries_after_current_then_restore_append_order() {
        let mut queue = queue(2);
        queue.set_shuffle(true);
        queue.append(vec![t(8), t(9)]);
        queue.append(vec![t(10)]);
        assert_eq!(queue.len(), 11);
        assert_eq!(queue.pos(), Some(2));
        assert_eq!(queue.current(), Some(&t(2)));
        for id in 8..11 {
            assert!(
                queue
                    .entries()
                    .iter()
                    .position(|entry| entry.id == id)
                    .unwrap()
                    > 2
            );
        }
        let mut sorted = ids(&queue);
        sorted.sort_unstable();
        assert_eq!(sorted, (0..11).collect::<Vec<_>>());
        queue.set_shuffle(false);
        assert_eq!(ids(&queue), (0..11).collect::<Vec<_>>());
        assert_eq!(
            queue
                .entries()
                .iter()
                .map(|entry| entry.track.clone())
                .collect::<Vec<_>>(),
            (0..11).map(t).collect::<Vec<_>>()
        );
        assert_eq!(queue.current(), Some(&t(2)));
    }

    #[test]
    fn shuffle_and_append_at_the_end() {
        let mut queue = Queue::new(vec![t(0)], 0, QueueSource::Liked, 1);
        queue.set_shuffle(true);
        assert_eq!(queue.current(), Some(&t(0)));
        assert_eq!(queue.peek_next(), None);
        queue.append(vec![t(1)]);
        assert_eq!(queue.pos(), Some(0));
        assert_eq!(queue.peek_next(), Some(&t(1)));
        assert_eq!(queue.advance(), Some(&t(1)));
        queue.set_shuffle(false);
        assert_eq!(queue.pos(), Some(1));
        assert_eq!(queue.current(), Some(&t(1)));
        assert_eq!(ids(&queue), vec![0, 1]);
    }

    #[test]
    fn append_without_current_and_shuffle_all_with_zero_seed() {
        let mut first = Queue::new(Vec::new(), 0, QueueSource::Stats, 0);
        first.append((0..8).map(t).collect());
        assert_eq!(ids(&first), (0..8).collect::<Vec<_>>());
        assert_eq!(first.pos(), None);
        first.set_shuffle(true);
        let mut second = Queue::new(Vec::new(), 0, QueueSource::Stats, 0);
        second.append((0..8).map(t).collect());
        second.set_shuffle(true);
        assert_eq!(first.entries(), second.entries());
        assert_ne!(ids(&first), (0..8).collect::<Vec<_>>());
        first.append(vec![t(8), t(9)]);
        assert_eq!(first.pos(), None);
        first.set_shuffle(false);
        assert_eq!(ids(&first), (0..10).collect::<Vec<_>>());
        assert_eq!(first.current(), None);
    }

    #[test]
    fn jump_to_changes_position_only_for_valid_indices() {
        let mut queue = queue(0);
        assert_eq!(queue.jump_to(5), Some(&t(5)));
        assert_eq!(queue.pos(), Some(5));
        assert_eq!(queue.peek_next(), Some(&t(6)));
        assert_eq!(queue.jump_to(8), None);
        assert_eq!(queue.jump_to(usize::MAX), None);
        assert_eq!(queue.pos(), Some(5));
        assert_eq!(queue.current(), Some(&t(5)));
        assert_eq!(queue.jump_to(0), Some(&t(0)));
    }
}
