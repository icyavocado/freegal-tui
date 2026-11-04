use crate::api::models::Song;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QueueStatus {
    #[default]
    Queued,
    Playing,
    Played,
    Failed,
}

impl QueueStatus {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Playing => "playing",
            Self::Played => "played",
            Self::Failed => "failed (expired)",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueueItem {
    pub song: Song,
    pub status: QueueStatus,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Queue {
    pub items: Vec<QueueItem>,
}

impl Queue {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[allow(dead_code)]
    pub fn append(&mut self, song: Song) {
        self.items.push(QueueItem {
            song,
            status: QueueStatus::Queued,
        });
    }

    #[allow(dead_code)]
    pub fn play_next(&mut self, song: Song) {
        let pos = self
            .items
            .iter()
            .position(|i| i.status == QueueStatus::Playing)
            .map(|p| p + 1)
            .unwrap_or(0);
        self.items.insert(
            pos,
            QueueItem {
                song,
                status: QueueStatus::Queued,
            },
        );
    }

    #[allow(dead_code)]
    pub fn play_now(&mut self, song: Song) {
        let insert_at = self
            .items
            .iter()
            .position(|i| i.status == QueueStatus::Playing)
            .map(|p| {
                self.items[p].status = QueueStatus::Played;
                p + 1
            })
            .unwrap_or(0);
        self.items.insert(
            insert_at,
            QueueItem {
                song,
                status: QueueStatus::Playing,
            },
        );
    }

    #[allow(dead_code)]
    pub fn play_at(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        if let Some((pos, _)) = self.current() {
            self.items[pos].status = QueueStatus::Played;
        }
        self.items[index].status = QueueStatus::Playing;
    }

    pub fn current(&self) -> Option<(usize, &QueueItem)> {
        self.items
            .iter()
            .enumerate()
            .find(|(_, i)| i.status == QueueStatus::Playing)
    }

    pub fn advance(&mut self) {
        let Some(pos) = self
            .items
            .iter()
            .position(|i| i.status == QueueStatus::Playing)
        else {
            return;
        };
        self.items[pos].status = QueueStatus::Played;
        if let Some(next) = self
            .items
            .iter_mut()
            .skip(pos + 1)
            .find(|i| i.status == QueueStatus::Queued)
        {
            next.status = QueueStatus::Playing;
        }
    }

    pub fn fail_current(&mut self) {
        if let Some((pos, _)) = self.current() {
            self.items[pos].status = QueueStatus::Failed;
        }
    }

    pub fn remove(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    pub fn move_up(&mut self, index: usize) {
        if index > 0 && index < self.items.len() {
            self.items.swap(index - 1, index);
        }
    }

    pub fn move_down(&mut self, index: usize) {
        if index + 1 < self.items.len() {
            self.items.swap(index, index + 1);
        }
    }

    pub fn enqueue_rest(&mut self, rest: Vec<Song>) {
        for song in rest {
            let duplicate = self.items.iter().any(|i| {
                i.song.id == song.id
                    && i.song.provider == song.provider
                    && matches!(i.status, QueueStatus::Queued | QueueStatus::Playing)
            });
            if !duplicate {
                self.append(song);
            }
        }
    }

    #[allow(dead_code)]
    pub fn next_queued(&self) -> Option<(usize, &QueueItem)> {
        let start = self.current().map(|(p, _)| p + 1).unwrap_or(0);
        self.items
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, i)| i.status == QueueStatus::Queued)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(id: u64) -> Song {
        Song {
            id,
            title: format!("t{id}"),
            ..Song::default()
        }
    }

    #[test]
    fn append_playnext_insertion() {
        let mut q = Queue::default();
        q.append(song(1));
        q.append(song(2));
        q.append(song(4));
        q.play_next(song(3));
        assert_eq!(q.items[0].song.id, 3);
        q.items[0].status = QueueStatus::Playing;
        q.play_next(song(9));
        assert_eq!(q.items[1].song.id, 9);
    }

    #[test]
    fn advance_marks_played_then_next() {
        let mut q = Queue::default();
        q.append(song(1));
        q.append(song(2));
        q.items[0].status = QueueStatus::Playing;
        q.advance();
        assert_eq!(q.items[0].status, QueueStatus::Played);
        assert_eq!(q.items[1].status, QueueStatus::Playing);
        q.advance();
        assert_eq!(q.items[1].status, QueueStatus::Played);
    }

    #[test]
    fn advance_promotes_next_queued_past_failed() {
        let mut q = Queue::default();
        q.append(song(1));
        q.append(song(2));
        q.append(song(3));
        q.items[0].status = QueueStatus::Playing;
        q.items[1].status = QueueStatus::Failed;
        q.advance();
        assert_eq!(q.items[0].status, QueueStatus::Played);
        assert_eq!(q.items[2].status, QueueStatus::Playing);
    }

    #[test]
    fn fail_current_and_reorder() {
        let mut q = Queue::default();
        q.append(song(1));
        q.append(song(2));
        q.append(song(3));
        q.items[1].status = QueueStatus::Playing;
        q.move_up(2);
        assert_eq!(q.items[1].song.id, 3);
        q.remove(0);
        assert_eq!(q.items[0].song.id, 3);
        q.fail_current();
        assert_eq!(q.items[1].status, QueueStatus::Failed);
    }

    #[test]
    fn next_queued_skips_played() {
        let mut q = Queue::default();
        q.append(song(1));
        q.append(song(2));
        q.append(song(3));
        q.items[0].status = QueueStatus::Playing;
        assert_eq!(q.next_queued().unwrap().1.song.id, 2);
        q.items[1].status = QueueStatus::Failed;
        assert_eq!(q.next_queued().unwrap().1.song.id, 3);
    }
}
