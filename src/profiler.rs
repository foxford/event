use std::collections::HashMap;
use std::future::Future;
use std::hash::Hash;
use std::thread;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use log::warn;

#[derive(Default)]
pub(crate) struct EntryReport {
    pub(crate) count: usize,
    pub(crate) p95: usize,
    pub(crate) p99: usize,
    pub(crate) max: usize,
}

struct Entry {
    values: Vec<(usize, Instant)>,
}

impl Entry {
    fn new() -> Self {
        Self { values: vec![] }
    }

    fn register(&mut self, value: usize) {
        self.values.push((value, Instant::now()));
    }

    fn flush(&mut self, duration: u64) -> EntryReport {
        if self.values.is_empty() {
            EntryReport::default()
        } else {
            let now = Instant::now();
            self.values
                .retain(|(_val, added_at)| now.duration_since(*added_at).as_secs() < duration);
            self.values.sort();

            let count = self.values.len();
            let p95_idx = (count as f32 * 0.95) as usize;
            let p99_idx = (count as f32 * 0.99) as usize;
            let max_idx = count - 1;
            let max = self.values[max_idx].0;

            let p95 = if p95_idx < max_idx {
                (self.values[p95_idx].0 + max) / 2
            } else {
                max
            };

            let p99 = if p99_idx < max_idx {
                (self.values[p99_idx].0 + max) / 2
            } else {
                max
            };

            let report = EntryReport {
                count,
                p95,
                p99,
                max,
            };
            report
        }
    }
}

enum Message<K> {
    Register { key: K, value: usize },
    Flush(u64),
    Stop,
}

pub(crate) struct Profiler<K> {
    tx: crossbeam_channel::Sender<Message<K>>,
    back_rx: crossbeam_channel::Receiver<Vec<(K, EntryReport)>>,
}

impl<K: 'static + Eq + Hash + Send + Copy> Profiler<K> {
    pub(crate) fn start() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        let (back_tx, back_rx) = crossbeam_channel::unbounded();

        thread::spawn(move || {
            let mut data: HashMap<K, Entry> = HashMap::new();

            for message in rx {
                match message {
                    Message::Register { key, value } => match data.get_mut(&key) {
                        Some(entry) => entry.register(value),
                        None => {
                            let mut entry = Entry::new();
                            entry.register(value);
                            data.insert(key, entry);
                        }
                    },
                    Message::Flush(duration) => {
                        let report = data
                            .iter_mut()
                            .map(|(k, v)| (*k, v.flush(duration)))
                            .collect();

                        if let Err(err) = back_tx.send(report) {
                            warn!("Failed to send profiler report: {}", err);
                        }
                    }
                    Message::Stop => break,
                }
            }
        });

        Self { tx, back_rx }
    }

    pub(crate) async fn measure<F, R>(&self, key: K, func: F) -> R
    where
        F: Future<Output = R>,
    {
        let start_time = Instant::now();
        let result = func.await;
        let duration = start_time.elapsed();

        let message = Message::Register {
            key,
            value: duration.as_micros() as usize,
        };

        if let Err(err) = self.tx.send(message) {
            warn!("Failed to register profiler value: {}", err);
        }

        result
    }

    pub(crate) fn flush(&self, duration: u64) -> Result<Vec<(K, EntryReport)>> {
        self.tx
            .send(Message::Flush(duration))
            .map_err(|err| anyhow!(err.to_string()))
            .context("Failed to send flush message to the profiler")?;

        self.back_rx
            .recv()
            .context("Failed to receive the profiler report")
    }
}

impl<K> Drop for Profiler<K> {
    fn drop(&mut self) {
        if let Err(err) = self.tx.send(Message::Stop) {
            warn!("Failed to stop profiler: {}", err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum Key {
        One,
        Two,
    }

    #[test]
    fn entry_flush() {
        let mut entry = Entry::new();

        for i in (1..1000).rev() {
            entry.register(i);
        }

        let report = entry.flush(5);
        assert_eq!(report.count, 999);
        assert_eq!(report.p95, 974);
        assert_eq!(report.p99, 994);
        assert_eq!(report.max, 999);
    }

    #[test]
    fn profiler() {
        futures::executor::block_on(async {
            let profiler = Profiler::<Key>::start();
            profiler
                .measure(
                    Key::One,
                    async_std::task::sleep(Duration::from_micros(10000)),
                )
                .await;
            profiler
                .measure(
                    Key::Two,
                    async_std::task::sleep(Duration::from_micros(1000)),
                )
                .await;

            let reports = profiler.flush(5).expect("Failed to flush profiler");
            assert_eq!(reports.len(), 2);

            for (key, report) in reports {
                match key {
                    Key::One => assert!(report.max >= 10000),
                    Key::Two => assert!(report.max >= 1000),
                }
            }
        });
    }
}
