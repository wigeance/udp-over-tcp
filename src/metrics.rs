/// Metrics collection for userspace congestion control.
///
/// This module provides application-level measurement of network performance
/// metrics needed for BBR implementation:
/// - Round-trip time (RTT) samples
/// - Packet loss detection
/// - Delivery rate measurement
///
/// These metrics are collected at the UDP-over-TCP layer without kernel involvement,
/// enabling userspace congestion control on systems without BBR kernel module.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Maximum number of RTT samples to keep in the rolling window
const RTT_SAMPLE_WINDOW: usize = 100;

/// Tracks the state of a sent datagram for RTT and loss measurement
#[derive(Clone, Copy, Debug)]
struct SentDatagram {
    /// When this datagram was sent
    sent_at: Instant,
    /// Size of the datagram in bytes (including UDP header)
    size: usize,
    /// Sequence number for ordering
    seq: u64,
}

/// Provides application-level metrics for congestion control.
///
/// This tracks:
/// - RTT (round-trip time) from UDP send to TCP acknowledgment
/// - Loss events (datagrams sent but not acknowledged)
/// - Delivery rate (bytes delivered successfully)
/// - Inflight data (bytes sent but not yet acknowledged)
#[derive(Debug)]
pub struct MetricsCollector {
    /// Sent datagrams waiting for acknowledgment
    in_flight: VecDeque<SentDatagram>,
    
    /// Rolling window of RTT samples (in microseconds) for statistical analysis
    rtt_samples: VecDeque<u64>,
    
    /// Total bytes successfully delivered (acknowledged)
    bytes_delivered: u64,
    
    /// Total bytes lost (sent but not acknowledged within timeout)
    bytes_lost: u64,
    
    /// Total bytes sent at TCP layer
    bytes_sent: u64,
    
    /// Current sequence number for tracking datagrams
    current_seq: u64,
    
    /// Timestamp of the last loss event
    last_loss_event: Option<Instant>,
    
    /// Time window for loss detection
    loss_detection_timeout: Duration,
}

impl MetricsCollector {
    /// Creates a new metrics collector with default settings
    pub fn new() -> Self {
        Self {
            in_flight: VecDeque::with_capacity(RTT_SAMPLE_WINDOW),
            rtt_samples: VecDeque::with_capacity(RTT_SAMPLE_WINDOW),
            bytes_delivered: 0,
            bytes_lost: 0,
            bytes_sent: 0,
            current_seq: 0,
            last_loss_event: None,
            loss_detection_timeout: Duration::from_millis(1000), // 1 second RTT timeout
        }
    }

    /// Records a UDP datagram that was sent over TCP.
    ///
    /// # Arguments
    /// * `size` - Size of the UDP datagram in bytes
    /// * `now` - Current time (allows for testing with synthetic time)
    pub fn record_send(&mut self, size: usize, now: Instant) {
        self.bytes_sent += size as u64;
        self.current_seq += 1;
        
        self.in_flight.push_back(SentDatagram {
            sent_at: now,
            size,
            seq: self.current_seq,
        });
        
        log::trace!("Sent datagram seq={}, size={}, inflight={}", 
                    self.current_seq, size, self.in_flight.len());
    }

    /// Records that a datagram was successfully delivered (received ACK).
    ///
    /// This is inferred when we receive TCP data (ACKs from server),
    /// as it confirms the corresponding UDP datagram was processed.
    ///
    /// # Arguments
    /// * `acked_bytes` - Number of bytes that were acknowledged
    /// * `now` - Current time
    pub fn record_ack(&mut self, acked_bytes: usize, now: Instant) {
        let mut remaining = acked_bytes;
        let mut rtt_samples = Vec::new();

        // Process ACKs: remove delivered datagrams and collect RTT samples
        while remaining > 0 && !self.in_flight.is_empty() {
            if let Some(datagram) = self.in_flight.pop_front() {
                let rtt = now.saturating_duration_since(datagram.sent_at);
                rtt_samples.push(rtt);
                
                self.bytes_delivered += datagram.size as u64;
                remaining = remaining.saturating_sub(datagram.size);
                
                log::trace!("ACK datagram seq={}, rtt={:?}", datagram.seq, rtt);
            }
        }

        // Add RTT samples to the rolling window
        for rtt in rtt_samples {
            let rtt_us = rtt.as_micros().min(u64::MAX as u128) as u64;
            self.rtt_samples.push_back(rtt_us);
            
            if self.rtt_samples.len() > RTT_SAMPLE_WINDOW {
                self.rtt_samples.pop_front();
            }
        }
    }

    /// Detects and records lost datagrams based on timeout.
    ///
    /// Should be called periodically (e.g., in the event loop) to detect
    /// datagrams that haven't been acknowledged within the loss detection timeout.
    ///
    /// # Arguments
    /// * `now` - Current time
    pub fn detect_loss(&mut self, now: Instant) {
        let mut lost_bytes = 0;
        
        // Find datagrams that timed out
        while let Some(&datagram) = self.in_flight.front() {
            if now.saturating_duration_since(datagram.sent_at) > self.loss_detection_timeout {
                if let Some(lost) = self.in_flight.pop_front() {
                    lost_bytes += lost.size;
                    log::warn!("Loss detected: datagram seq={}, timeout={:?}", 
                               lost.seq, self.loss_detection_timeout);
                }
            } else {
                break;
            }
        }

        if lost_bytes > 0 {
            self.bytes_lost += lost_bytes as u64;
            self.last_loss_event = Some(now);
        }
    }

    /// Returns the current number of bytes in flight (sent but not acknowledged).
    pub fn bytes_in_flight(&self) -> usize {
        self.in_flight.iter().map(|d| d.size).sum()
    }

    /// Returns the number of unacknowledged datagrams
    pub fn datagrams_in_flight(&self) -> usize {
        self.in_flight.len()
    }

    /// Returns the minimum RTT sample from the rolling window (in microseconds).
    pub fn min_rtt_us(&self) -> Option<u64> {
        self.rtt_samples.iter().copied().min()
    }

    /// Returns the maximum RTT sample from the rolling window (in microseconds).
    pub fn max_rtt_us(&self) -> Option<u64> {
        self.rtt_samples.iter().copied().max()
    }

    /// Returns the smoothed/average RTT from the rolling window (in microseconds).
    pub fn smoothed_rtt_us(&self) -> Option<u64> {
        if self.rtt_samples.is_empty() {
            return None;
        }
        let sum: u64 = self.rtt_samples.iter().sum();
        Some(sum / self.rtt_samples.len() as u64)
    }

    /// Returns the latest RTT sample (in microseconds), if available.
    pub fn latest_rtt_us(&self) -> Option<u64> {
        self.rtt_samples.back().copied()
    }

    /// Returns total bytes successfully delivered
    pub fn total_delivered(&self) -> u64 {
        self.bytes_delivered
    }

    /// Returns total bytes lost
    pub fn total_lost(&self) -> u64 {
        self.bytes_lost
    }

    /// Returns total bytes sent
    pub fn total_sent(&self) -> u64 {
        self.bytes_sent
    }

    /// Returns the loss rate as a percentage (0.0 to 1.0)
    pub fn loss_rate(&self) -> f64 {
        if self.bytes_sent == 0 {
            0.0
        } else {
            self.bytes_lost as f64 / self.bytes_sent as f64
        }
    }

    /// Returns number of loss events detected
    pub fn has_recent_loss(&self, window: Duration) -> bool {
        if let Some(last_loss) = self.last_loss_event {
            Instant::now().saturating_duration_since(last_loss) < window
        } else {
            false
        }
    }

    /// Gets a snapshot of current metrics
    pub fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            bytes_in_flight: self.bytes_in_flight() as u64,
            bytes_delivered: self.bytes_delivered,
            bytes_lost: self.bytes_lost,
            bytes_sent: self.bytes_sent,
            min_rtt_us: self.min_rtt_us(),
            smoothed_rtt_us: self.smoothed_rtt_us(),
            latest_rtt_us: self.latest_rtt_us(),
            loss_rate: self.loss_rate(),
            datagrams_in_flight: self.datagrams_in_flight(),
        }
    }
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// A snapshot of metrics at a point in time
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub bytes_in_flight: u64,
    pub bytes_delivered: u64,
    pub bytes_lost: u64,
    pub bytes_sent: u64,
    pub min_rtt_us: Option<u64>,
    pub smoothed_rtt_us: Option<u64>,
    pub latest_rtt_us: Option<u64>,
    pub loss_rate: f64,
    pub datagrams_in_flight: usize,
}

impl std::fmt::Display for MetricsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Delivered: {}, Lost: {}, Loss%: {:.2}%, Inflight: {} bytes ({} datagrams), ",
            self.bytes_delivered,
            self.bytes_lost,
            self.loss_rate * 100.0,
            self.bytes_in_flight,
            self.datagrams_in_flight,
        )?;

        match self.smoothed_rtt_us {
            Some(rtt) => write!(f, "RTT: {:.1}ms", rtt as f64 / 1000.0)?,
            None => write!(f, "RTT: N/A")?,
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_record_send_and_ack() {
        let mut collector = MetricsCollector::new();
        let now = Instant::now();

        collector.record_send(1000, now);
        assert_eq!(collector.bytes_in_flight(), 1000);
        assert_eq!(collector.datagrams_in_flight(), 1);

        // Simulate delay and ACK
        let later = now + Duration::from_millis(10);
        collector.record_ack(1000, later);

        assert_eq!(collector.bytes_in_flight(), 0);
        assert_eq!(collector.datagrams_in_flight(), 0);
        assert_eq!(collector.total_delivered(), 1000);
    }

    #[test]
    fn test_rtt_calculation() {
        let mut collector = MetricsCollector::new();
        let now = Instant::now();

        collector.record_send(500, now);
        let later = now + Duration::from_millis(20);
        collector.record_ack(500, later);

        let rtt_us = collector.smoothed_rtt_us().unwrap();
        assert!(rtt_us >= 19000 && rtt_us <= 21000, "RTT should be ~20ms");
    }

    #[test]
    fn test_loss_detection() {
        let mut collector = MetricsCollector::new();
        let now = Instant::now();

        collector.record_send(1000, now);
        collector.record_send(1000, now);

        // Simulate timeout - move time forward beyond loss_detection_timeout
        let future = now + Duration::from_secs(2);
        collector.detect_loss(future);

        assert_eq!(collector.total_lost(), 2000);
        assert_eq!(collector.bytes_in_flight(), 0);
    }

    #[test]
    fn test_loss_rate() {
        let mut collector = MetricsCollector::new();
        let now = Instant::now();

        collector.record_send(1000, now);
        collector.record_send(1000, now);
        let later = now + Duration::from_millis(10);
        collector.record_ack(1000, later);

        // One packet delivered, one still in flight
        assert_eq!(collector.total_delivered(), 1000);
        assert_eq!(collector.bytes_in_flight(), 1000);

        // Detect loss on the remaining packet
        let future = now + Duration::from_secs(2);
        collector.detect_loss(future);

        assert_eq!(collector.total_lost(), 1000);
        assert_eq!(collector.loss_rate(), 0.5); // 50% loss
    }
}
