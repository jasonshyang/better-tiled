//! Work that blocks (loading a map, asking for a file) runs on its own thread; its result comes
//! back as intents through one channel, and the thread wakes the window when it is done.

use std::sync::mpsc::{Receiver, Sender, channel};

use bt_editor::Intent;

/// The threads doing work for the window, and the results they sent back.
pub(crate) struct Workers {
    results: Receiver<Vec<Intent>>,
    sender: Sender<Vec<Intent>>,
    waker: egui::Context,
    running: usize,
}

impl Workers {
    /// Workers that wake `waker` when a result arrives.
    pub(crate) fn new(waker: egui::Context) -> Workers {
        let (sender, results) = channel();
        Workers {
            results,
            sender,
            waker,
            running: 0,
        }
    }

    /// Runs `job` on a thread named `name`; the intents it returns are delivered later.
    pub(crate) fn spawn(
        &mut self,
        name: &str,
        job: impl FnOnce() -> Vec<Intent> + Send + 'static,
    ) -> Result<(), String> {
        let sender = self.sender.clone();
        let waker = self.waker.clone();
        std::thread::Builder::new()
            .name(format!("bt-{name}"))
            .spawn(move || {
                let intents = job();
                // The window may be gone (closing); then nobody waits for the result.
                let _ = sender.send(intents);
                waker.request_repaint();
            })
            .map_err(|error| format!("no thread for {name}: {error}"))?;
        self.running += 1;
        Ok(())
    }

    /// The intents of the jobs that finished since the last call.
    pub(crate) fn take_results(&mut self) -> Vec<Intent> {
        let mut intents = Vec::new();
        while let Ok(batch) = self.results.try_recv() {
            self.running = self.running.saturating_sub(1);
            intents.extend(batch);
        }
        intents
    }

    /// Whether a job is still running.
    pub(crate) fn is_busy(&self) -> bool {
        self.running > 0
    }
}

#[cfg(test)]
mod tests {
    use bt_editor::ActionId;

    use super::*;

    #[test]
    fn results_come_back_in_one_channel() {
        let mut workers = Workers::new(egui::Context::default());
        workers
            .spawn("test", || vec![Intent::Run(ActionId::Save)])
            .unwrap();
        assert!(workers.is_busy());
        let mut intents = Vec::new();
        while intents.is_empty() {
            intents = workers.take_results();
            std::thread::yield_now();
        }
        assert!(matches!(intents.as_slice(), [Intent::Run(ActionId::Save)]));
        assert!(!workers.is_busy());
    }
}
