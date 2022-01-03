use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::output::Output;

pub struct OutputTimer {
    output: Arc<Output>,
    time_changed: Instant,
    pub expired: bool,
}

impl OutputTimer {
    pub fn update_output(&mut self, output: Arc<Output>) {
        self.output = output;
    }

    pub fn check_timeout(&mut self) -> bool {
        // Config might have changed
        if let Some(duration) = self.output.time {
            let now = Instant::now();
            if now.checked_duration_since(self.time_changed).unwrap()
                > Duration::from_secs(duration.into())
            {
                self.expired = true;
                self.time_changed = now;
                return true;
            }
        }

        false
    }

    pub fn new(output: Arc<Output>) -> Self {
        Self {
            output,
            time_changed: Instant::now(),
            expired: false,
        }
    }
}
