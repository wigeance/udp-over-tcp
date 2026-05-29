/// TCP Congestion Control configuration and utilities.
///
/// This module provides functionality to set and verify TCP congestion control
/// algorithms on Linux systems. BBR (Bottleneck Bandwidth and Round-trip time) is
/// recommended for most use cases as it provides better throughput and lower latency
/// compared to traditional algorithms like CUBIC or RENO.

use std::fmt;
use std::io;

#[cfg(target_os = "linux")]
use nix::sys::socket::{getsockopt, setsockopt, sockopt};

/// Supported TCP congestion control algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Algorithm {
    /// BBR (Bottleneck Bandwidth and Round-trip time)
    /// Modern algorithm optimized for lower latency and higher throughput
    Bbr,
    /// CUBIC - Default on many Linux distributions
    /// Good general-purpose algorithm
    Cubic,
    /// RENO - Conservative algorithm
    Reno,
}

impl Algorithm {
    /// Returns the string representation used by the kernel
    pub fn as_str(self) -> &'static str {
        match self {
            Algorithm::Bbr => "bbr",
            Algorithm::Cubic => "cubic",
            Algorithm::Reno => "reno",
        }
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Algorithm {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "bbr" => Ok(Algorithm::Bbr),
            "cubic" => Ok(Algorithm::Cubic),
            "reno" => Ok(Algorithm::Reno),
            other => Err(format!(
                "Unknown congestion control algorithm: '{}'. Supported: bbr, cubic, reno",
                other
            )),
        }
    }
}

/// Error type for congestion control operations
#[derive(Debug)]
pub struct CongestionControlError {
    kind: CongestionControlErrorKind,
}

#[derive(Debug)]
enum CongestionControlErrorKind {
    /// Failed to set the congestion control algorithm
    SetAlgorithm(String, io::Error),
    /// Failed to get/verify the congestion control algorithm
    GetAlgorithm(io::Error),
    /// System does not support congestion control (not on Linux)
    NotSupported,
}

impl fmt::Display for CongestionControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            CongestionControlErrorKind::SetAlgorithm(algo, e) => {
                write!(f, "Failed to set TCP congestion control to '{}': {}", algo, e)
            }
            CongestionControlErrorKind::GetAlgorithm(e) => {
                write!(f, "Failed to get TCP congestion control algorithm: {}", e)
            }
            CongestionControlErrorKind::NotSupported => {
                write!(
                    f,
                    "TCP congestion control configuration not supported on this platform"
                )
            }
        }
    }
}

impl std::error::Error for CongestionControlError {}

/// Sets the TCP congestion control algorithm on a socket
///
/// # Arguments
///
/// * `socket` - The TCP socket to configure
/// * `algorithm` - The congestion control algorithm to set
///
/// # Returns
///
/// * `Ok(())` on success
/// * `Err(CongestionControlError)` if the operation fails
///
/// # Platform Support
///
/// This function only works on Linux. On other platforms it returns `NotSupported`.
#[cfg(target_os = "linux")]
pub fn set_algorithm<S: AsRawSocketFd>(
    socket: &S,
    algorithm: Algorithm,
) -> Result<(), CongestionControlError> {
    let algo_str = algorithm.as_str();
    setsockopt(socket, sockopt::TcpCongestion, algo_str).map_err(|e| CongestionControlError {
        kind: CongestionControlErrorKind::SetAlgorithm(
            algo_str.to_string(),
            io::Error::from_raw_os_error(e.as_errno().map(|n| n as i32).unwrap_or(1)),
        ),
    })?;

    log::debug!("Set TCP congestion control to: {}", algorithm);
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn set_algorithm<S: AsRawSocketFd>(
    _socket: &S,
    _algorithm: Algorithm,
) -> Result<(), CongestionControlError> {
    Err(CongestionControlError {
        kind: CongestionControlErrorKind::NotSupported,
    })
}

/// Gets the current TCP congestion control algorithm from a socket
///
/// # Arguments
///
/// * `socket` - The TCP socket to query
///
/// # Returns
///
/// * `Ok(String)` containing the current algorithm name
/// * `Err(CongestionControlError)` if the operation fails
///
/// # Platform Support
///
/// This function only works on Linux.
#[cfg(target_os = "linux")]
pub fn get_algorithm<S: AsRawSocketFd>(socket: &S) -> Result<String, CongestionControlError> {
    use std::ffi::CStr;

    getsockopt(socket, sockopt::TcpCongestion)
        .map_err(|e| CongestionControlError {
            kind: CongestionControlErrorKind::GetAlgorithm(io::Error::from_raw_os_error(
                e.as_errno().map(|n| n as i32).unwrap_or(1),
            )),
        })
        .and_then(|cstr: CStr| {
            cstr.to_str()
                .map(|s| s.to_string())
                .map_err(|_| CongestionControlError {
                    kind: CongestionControlErrorKind::GetAlgorithm(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid UTF-8 in congestion control algorithm name",
                    )),
                })
        })
}

#[cfg(not(target_os = "linux"))]
pub fn get_algorithm<S: AsRawSocketFd>(_socket: &S) -> Result<String, CongestionControlError> {
    Err(CongestionControlError {
        kind: CongestionControlErrorKind::NotSupported,
    })
}

/// Trait to get raw socket file descriptor
#[cfg(target_os = "linux")]
pub trait AsRawSocketFd {
    fn as_raw_socket_fd(&self) -> std::os::unix::io::RawFd;
}

#[cfg(target_os = "linux")]
impl AsRawSocketFd for tokio::net::TcpSocket {
    fn as_raw_socket_fd(&self) -> std::os::unix::io::RawFd {
        use std::os::unix::io::AsRawFd;
        self.as_raw_fd()
    }
}

#[cfg(target_os = "linux")]
impl AsRawSocketFd for tokio::net::TcpStream {
    fn as_raw_socket_fd(&self) -> std::os::unix::io::RawFd {
        use std::os::unix::io::AsRawFd;
        self.as_raw_fd()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithm_display() {
        assert_eq!(Algorithm::Bbr.to_string(), "bbr");
        assert_eq!(Algorithm::Cubic.to_string(), "cubic");
        assert_eq!(Algorithm::Reno.to_string(), "reno");
    }

    #[test]
    fn test_algorithm_from_str() {
        assert_eq!("bbr".parse::<Algorithm>().unwrap(), Algorithm::Bbr);
        assert_eq!("CUBIC".parse::<Algorithm>().unwrap(), Algorithm::Cubic);
        assert_eq!("reno".parse::<Algorithm>().unwrap(), Algorithm::Reno);
        assert!("invalid".parse::<Algorithm>().is_err());
    }
}
