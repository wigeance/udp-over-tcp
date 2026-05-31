# Userspace BBR Implementation Roadmap

## Goal
Enable BBR congestion control in pure userspace on aarch64 routers without kernel BBR module.

## Architecture

### Phase 1: Application-Layer Metrics Collection
- Track RTT (round-trip time) at application level
- Measure delivered bytes and delivery rate
- Collect loss events
- No kernel dependency required

### Phase 2: Userspace BBR Algorithm
- Implement BBR state machine:
  - **Startup**: Quickly find available bandwidth
  - **Drain**: Reduce inflight to match BDP (Bandwidth-Delay Product)
  - **Probe-BW**: Track changing bandwidth in different cycles
  - **Probe-RTT**: Periodically measure minimum RTT
- Rate limiter for send-side control
- Loss detection and recovery

### Phase 3: Integration Points
1. **In `tcp2udp`**: Control send rate based on BBR state
2. **In `udp2tcp`**: Provide RTT and loss feedback
3. **New module**: `src/userspace_bbr.rs`

### Phase 4: Fallback Strategy
- Attempt kernel BBR first (existing code)
- Fall back to userspace BBR if kernel BBR unavailable
- Transparent to users

## Implementation Details

### 1. New Module Structure
```
src/
├── userspace_bbr.rs          # BBR implementation
├── congestion_control.rs     # Existing (kernel-based)
└── tcp_options.rs            # Existing
```

### 2. Measurement Infrastructure
- Timestamp every datagram
- Track acknowledgments/responses
- Calculate RTT samples
- Detect packet loss at application level

### 3. Rate Control
- Token bucket or leaky bucket algorithm
- Controlled send scheduling
- CWND (congestion window) management

### 4. API Design
```rust
pub trait CongestionController {
    fn on_send(&mut self, bytes: usize) -> Option<Duration>; // Rate limit duration
    fn on_ack(&mut self, bytes: usize, rtt: Duration);
    fn on_loss(&mut self, bytes: usize);
    fn inflight(&self) -> usize;
}

pub struct UserspaceBbr { /* ... */ }
impl CongestionController for UserspaceBbr { /* ... */ }
```

## Priority
- 🟢 Phase 1: Metrics collection (foundation)
- 🟡 Phase 2: BBR algorithm (core feature)
- 🟡 Phase 3: Integration (deployment)
- 🟢 Phase 4: Fallback (reliability)

## Alternative: QUIC Consideration
If userspace TCP BBR becomes too complex, consider:
- Switch to QUIC transport (HTTP/3 compatible)
- QUIC has native BBR and other congestion control algorithms
- Better security and multiplexing guarantees
- Many Rust QUIC implementations available (s2n-quic, quinn, neqo)

## Deployment Target
- Platform: Linux aarch64
- Use case: Router without BBR kernel module
- Constraint: Pure userspace (no kernel module installation)
