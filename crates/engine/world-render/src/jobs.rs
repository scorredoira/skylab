//! Work done on background threads, a few at a time, and collected by the frame that finds it finished.

use std::collections::HashSet;
use std::hash::Hash;
use std::sync::mpsc::{channel, Receiver, Sender};

/// Jobs identified by a key. Each runs on its own thread and its result is picked up with `collect`.
pub struct Jobs<K, T> {
    in_flight: HashSet<K>,
    sender: Sender<(K, T)>,
    receiver: Receiver<(K, T)>,
}

impl<K: Copy + Eq + Hash + Send + 'static, T: Send + 'static> Jobs<K, T> {
    pub fn new() -> Jobs<K, T> {
        let (sender, receiver) = channel();
        Jobs { in_flight: HashSet::new(), sender, receiver }
    }

    /// Number of jobs started and not collected yet.
    pub fn busy(&self) -> usize {
        self.in_flight.len()
    }

    pub fn contains(&self, key: &K) -> bool {
        self.in_flight.contains(key)
    }

    pub fn start(&mut self, key: K, work: impl FnOnce() -> T + Send + 'static) {
        self.in_flight.insert(key);
        let sender = self.sender.clone();
        engine::threads::spawn(move || {
            // The receiver is gone when the renderer was dropped meanwhile: nobody wants the result.
            let _ = sender.send((key, work()));
        });
    }

    /// The results that have arrived since the last call.
    pub fn collect(&mut self) -> Vec<(K, T)> {
        let done: Vec<(K, T)> = self.receiver.try_iter().collect();
        for (key, _) in &done {
            self.in_flight.remove(key);
        }
        done
    }
}
