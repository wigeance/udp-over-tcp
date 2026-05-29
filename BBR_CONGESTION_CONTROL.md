# BBR Congestion Control Implementation Guide

This document explains how to use and verify BBR (Bottleneck Bandwidth and Round-trip time) congestion control with `udp-over-tcp`.

## Overview

BBR is a modern TCP congestion control algorithm developed by Google that optimizes for both **throughput** and **latency**, making it ideal for high-performance UDP-over-TCP tunneling. On Linux systems, `udp-over-tcp` now automatically attempts to use BBR by default.

## Features

- **Automatic BBR Selection**: On Linux, BBR is set by default on all TCP sockets
- **Fallback Support**: If BBR is unavailable, the system default algorithm is used
- **Explicit Algorithm Selection**: Users can specify `bbr`, `cubic`, or `reno` via CLI flags
- **Zero Configuration**: Works out of the box on any Linux system with BBR support

## Requirements

### Linux Kernel Support

BBR requires **Linux kernel 4.9 or later**. To check your kernel version:

```bash
uname -r
```

### Verify BBR is Available

Check if your system supports BBR:

```bash
# List available congestion control algorithms
cat /proc/sys/net/ipv4/tcp_available_congestion_control

# Output should include 'bbr':
# bbr cubic reno
```

If BBR is not listed, you may need to:
1. Update your kernel to 4.9+
2. Enable BBR in kernel build config
3. Load the BBR module: `modprobe tcp_bbr`

## Usage

### Default BBR (No Configuration Needed)

Simply run the binaries normally. BBR will be used automatically:

```bash
# Server side - automatically uses BBR
tcp2udp --tcp-listen 0.0.0.0:5001 --udp-forward 127.0.0.1:51820

# Client side - automatically uses BBR
udp2tcp --udp-listen 127.0.0.1:5353 --tcp-forward 1.2.3.4:5001
```

### Explicit Algorithm Selection

#### Using BBR explicitly:
```bash
tcp2udp --tcp-listen 0.0.0.0:5001 --udp-forward 127.0.0.1:51820 --congestion-control bbr
```

#### Using CUBIC:
```bash
tcp2udp --tcp-listen 0.0.0.0:5001 --udp-forward 127.0.0.1:51820 --congestion-control cubic
```

#### Using RENO:
```bash
tcp2udp --tcp-listen 0.0.0.0:5001 --udp-forward 127.0.0.1:51820 --congestion-control reno
```

## Verification

### Enable Debug Logging

Set the `RUST_LOG` environment variable to see which algorithm is being used:

```bash
RUST_LOG=debug tcp2udp --tcp-listen 0.0.0.0:5001 --udp-forward 127.0.0.1:51820
```

Look for log messages like:
```
DEBUG - Set TCP congestion control to: bbr
```

### Check Socket Configuration

At runtime, you can check which algorithm is active on a specific socket using `ss` or `netstat`:

```bash
# List TCP connections with congestion control info
ss -tin | grep -A1 ESTAB

# Or use netstat (older systems)
netstat -tin
```

For detailed congestion control info using socket debugging:

```bash
# Get the PID of the running process
ps aux | grep tcp2udp

# Monitor TCP stats
watch -n1 'ss -tin | head -20'
```

### Test with iperf3

Benchmark your tunnel with different algorithms:

```bash
# Terminal 1: Start server
RUST_LOG=debug tcp2udp --tcp-listen 0.0.0.0:5001 --udp-forward 127.0.0.1:5001 --congestion-control bbr

# Terminal 2: Forward local UDP -> TCP tunnel
udp2tcp --udp-listen 127.0.0.1:5353 --tcp-forward localhost:5001

# Terminal 3: Run iperf3 UDP test
iperf3 -c 127.0.0.1 -p 5353 -u -b 100M
```

Compare results across different algorithms (`bbr`, `cubic`, `reno`).

## Implementation Details

### Code Structure

The BBR implementation is located in:
- **`src/congestion_control.rs`** - Core congestion control module
  - `Algorithm` enum - Supported algorithms (BBR, CUBIC, RENO)
  - `set_algorithm()` - Sets algorithm on a socket
  - `get_algorithm()` - Queries current algorithm
  
- **`src/tcp_options.rs`** - Integration with socket options
  - New `congestion_control: Option<Algorithm>` field in `TcpOptions`
  - Automatic BBR selection with fallback

### Algorithm Selection Flow

1. **Socket Creation**: When a TCP socket is created
2. **Try BBR First**: Attempts to set BBR algorithm (Linux only)
3. **Fallback**: If BBR fails, logs debug message and uses system default
4. **Explicit Override**: If `--congestion-control` flag is provided, that algorithm is used

### Platform Support

| Platform | Support | Default |
|----------|---------|---------|
| Linux 4.9+ | ✅ Full | BBR |
| Linux <4.9 | ⚠️ Partial | System default |
| macOS | ❌ No | System default |
| Windows | ❌ No | System default |

## Performance Tuning

### Recommended Buffer Sizes with BBR

For optimal BBR performance, consider adjusting TCP buffer sizes:

```bash
# Larger buffers for BBR (recommended for high-bandwidth links)
tcp2udp --tcp-listen 0.0.0.0:5001 \
        --udp-forward 127.0.0.1:51820 \
        --congestion-control bbr \
        --send-buffer 4194304 \
        --recv-buffer 4194304
```

### Network Conditions

BBR adapts well to different network conditions:
- **Low latency, high bandwidth**: Achieves maximum throughput
- **High latency, high bandwidth**: Maintains lower RTT than CUBIC
- **Lossy networks**: Better than RENO, comparable to CUBIC

## Troubleshooting

### BBR Not Found Error

**Symptom**: `Failed to set TCP congestion control to 'bbr'`

**Solution**:
1. Check kernel version: `uname -r` (need 4.9+)
2. Verify BBR available: `cat /proc/sys/net/ipv4/tcp_available_congestion_control`
3. Load module if needed: `sudo modprobe tcp_bbr`
4. Set as default: `echo bbr | sudo tee /proc/sys/net/ipv4/tcp_congestion_control`

### Platform Not Supported

**Symptom**: `TCP congestion control configuration not supported on this platform`

**Solution**: This is expected on non-Linux systems. The application will continue using the system's default TCP settings.

### Performance Worse Than Expected

**Check**:
1. Enable debug logging: `RUST_LOG=debug`
2. Verify algorithm is actually BBR in logs
3. Check system load: `top`, `htop`
4. Monitor network: `iftop`, `nethogs`
5. Test with increased buffer sizes (see Performance Tuning above)

## References

- [BBR Paper](https://queue.acm.org/detail.cfm?id=3022184)
- [Linux TCP Congestion Control](https://www.kernel.org/doc/html/latest/networking/tcp.html)
- [Kernel net.ipv4 Documentation](https://www.kernel.org/doc/Documentation/networking/ip-sysctl.txt)
