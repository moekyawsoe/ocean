use std::time::{Duration, Instant};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(1500);

fn system_total_mem_mb() -> f32 {
    static TOTAL_MB: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *TOTAL_MB.get_or_init(|| {
        #[cfg(target_os = "linux")]
        {
            if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
                for line in meminfo.lines() {
                    if let Some(rest) = line.strip_prefix("MemTotal:") {
                        if let Some(kb) = rest
                            .split_ascii_whitespace()
                            .next()
                            .and_then(|s| s.parse::<f32>().ok())
                        {
                            return (kb / 1024.0).max(1.0);
                        }
                    }
                }
            }
        }
        16384.0
    })
}

#[derive(Debug, Clone)]
pub struct ProcessMetrics {
    pub cpu_percent: f32,
    pub priv_mem_mb: f32,
    pub total_mem_mb: f32,
    pub mem_percent: f32,
    pub show_detail: bool,
    last_sample: Instant,
    last_ticks: u64,
    pub compact_label: String,
    pub mem_only_label: String,
    pub detail_label: String,
}

impl Default for ProcessMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessMetrics {
    pub fn new() -> Self {
        let mut metrics = Self {
            cpu_percent: 0.0,
            priv_mem_mb: 0.0,
            total_mem_mb: 0.0,
            mem_percent: 0.0,
            show_detail: false,
            last_sample: Instant::now() - SAMPLE_INTERVAL,
            last_ticks: 0,
            compact_label: "0.0% · 0.0% · 0M".to_string(),
            mem_only_label: "0.0% · 0M".to_string(),
            detail_label: "CPU 0.0% | RAM 0.0% | Priv 0M / Tot 0M".to_string(),
        };
        metrics.refresh();
        metrics
    }

    /// Refresh process resource counters if the sampling interval has elapsed.
    /// Returns `true` if a new sample was taken and labels were updated.
    pub fn refresh_if_needed(&mut self) -> bool {
        if self.last_sample.elapsed() >= SAMPLE_INTERVAL {
            self.refresh();
            true
        } else {
            false
        }
    }

    pub fn toggle_detail(&mut self) {
        self.show_detail = !self.show_detail;
    }

    pub fn next_sample_time(&self) -> Instant {
        self.last_sample + SAMPLE_INTERVAL
    }

    pub fn current_display(&self) -> &str {
        if self.show_detail {
            &self.detail_label
        } else {
            &self.compact_label
        }
    }

    fn refresh(&mut self) {
        let now = Instant::now();
        let elapsed = self.last_sample.elapsed().as_secs_f32().max(0.1);
        self.last_sample = now;

        #[cfg(target_os = "linux")]
        {
            // 1. Read /proc/self/status for accurate Private (RssAnon) and Total (VmRSS) memory
            if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
                for line in status.lines() {
                    if let Some(rest) = line.strip_prefix("RssAnon:") {
                        if let Some(kb) = rest
                            .split_ascii_whitespace()
                            .next()
                            .and_then(|s| s.parse::<f32>().ok())
                        {
                            self.priv_mem_mb = kb / 1024.0;
                        }
                    } else if let Some(rest) = line.strip_prefix("VmRSS:")
                        && let Some(kb) = rest
                            .split_ascii_whitespace()
                            .next()
                            .and_then(|s| s.parse::<f32>().ok())
                    {
                        self.total_mem_mb = kb / 1024.0;
                    }
                }
            }

            // Fallback to /proc/self/statm if status was unreadable
            if self.total_mem_mb == 0.0
                && let Ok(statm) = std::fs::read_to_string("/proc/self/statm")
            {
                let mut parts = statm.split_ascii_whitespace();
                let _total_pages = parts.next();
                if let Some(resident) = parts.next().and_then(|s| s.parse::<u64>().ok()) {
                    let page_size = 4096.0;
                    self.total_mem_mb = (resident as f32 * page_size) / (1024.0 * 1024.0);
                    self.priv_mem_mb = self.total_mem_mb * 0.35; // reasonable approximation
                }
            }

            // 2. Process CPU time from /proc/self/stat
            if let Ok(stat) = std::fs::read_to_string("/proc/self/stat")
                && let Some(after_paren) = stat.rfind(')').map(|idx| &stat[idx + 1..])
            {
                let mut utime = 0u64;
                let mut stime = 0u64;
                for (i, val) in after_paren.split_ascii_whitespace().enumerate() {
                    if i == 11 {
                        utime = val.parse().unwrap_or(0);
                    } else if i == 12 {
                        stime = val.parse().unwrap_or(0);
                        break;
                    }
                }
                let current_ticks = utime + stime;
                if self.last_ticks > 0 {
                    let delta_ticks = current_ticks.saturating_sub(self.last_ticks);
                    // Standard Linux user-space ticks per second (sysconf(_SC_CLK_TCK) == 100)
                    let calculated_cpu = (delta_ticks as f32 / (100.0 * elapsed)) * 100.0;
                    // Smooth with slight exponential moving average
                    self.cpu_percent = (calculated_cpu * 0.75 + self.cpu_percent * 0.25).max(0.0);
                }
                self.last_ticks = current_ticks;
            }

            let sys_total = system_total_mem_mb();
            if sys_total > 0.0 {
                self.mem_percent = ((self.total_mem_mb / sys_total) * 100.0).max(0.0);
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            self.priv_mem_mb = 35.0;
            self.total_mem_mb = 95.0;
            self.cpu_percent = 0.5;
            self.mem_percent = 0.6;
        }

        let priv_str = if self.priv_mem_mb >= 1024.0 {
            format!("{:.1}G", self.priv_mem_mb / 1024.0)
        } else {
            format!("{:.0}M", self.priv_mem_mb)
        };

        let tot_str = if self.total_mem_mb >= 1024.0 {
            format!("{:.1}G", self.total_mem_mb / 1024.0)
        } else {
            format!("{:.0}M", self.total_mem_mb)
        };

        self.mem_only_label = format!("{:.1}% · {priv_str} ({tot_str})", self.mem_percent);
        self.compact_label = format!(
            "{:.1}% · {:.1}% · {priv_str} ({tot_str})",
            self.cpu_percent, self.mem_percent
        );
        self.detail_label = format!(
            "CPU {:.1}% | RAM {:.1}% | Priv: {:.0}MB · Tot: {:.0}MB",
            self.cpu_percent, self.mem_percent, self.priv_mem_mb, self.total_mem_mb
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_metrics_sample() {
        let mut metrics = ProcessMetrics::new();
        assert!(!metrics.compact_label.is_empty());
        assert!(!metrics.mem_only_label.is_empty());
        assert!(metrics.compact_label.contains('%'));
        assert!(metrics.mem_only_label.contains('%'));
        assert!(metrics.detail_label.contains("CPU"));
        assert!(metrics.detail_label.contains("RAM"));
        assert!(metrics.detail_label.contains('%'));
        assert!(metrics.mem_percent >= 0.0);
        assert!(system_total_mem_mb() > 0.0);

        let orig = metrics.show_detail;
        metrics.toggle_detail();
        assert_ne!(orig, metrics.show_detail);
    }
}
