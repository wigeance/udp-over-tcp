#[cfg(target_os = "linux")]
use nix::sys::socket::{getsockopt, setsockopt, sockopt};

#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;

use std::fmt;
use std::io;
use std::time::Duration;

use tokio::net::{TcpSocket, TcpStream};

use crate::congestion_control::Algorithm;

/// Options to apply to the TCP socket involved in the tunneling.
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "clap", derive(clap::Parser))]
#[non_exhaustive]
pub struct TcpOptions {
    /// If given, sets the SO_RCVBUF option on the TCP socket to the given number of bytes.
    /// Changes the size of the operating system's receive buffer associated with the socket.
    #[cfg_attr(feature = "clap", arg(long = "recv-buffer"))]
    pub recv_buffer_size: Option<u32>,

    /// If given, sets the SO_SNDBUF option on the TCP socket to the given number of bytes.
    /// Changes the size of the operating system's send buffer associated with the socket.
    #[cfg_attr(feature = "clap", arg(long = "send-buffer"))]
    pub send_buffer_size: Option<u32>,

    /// An application timeout on receiving data from the TCP socket.
    #[cfg_attr(
        feature = "clap",
        arg(long = "tcp-recv-timeout", value_parser = duration_secs_from_str)
    )]
    pub recv_timeout: Option<Duration>,

    /// If given, sets the SO_MARK option on the TCP socket.
    /// This exists only on Linux.
    #[cfg(target_os = "linux")]
    #[cfg_attr(feature = "clap", arg(long = "fwmark"))]
    pub fwmark: Option<u32>,

    /// Enables TCP_NODELAY on the TCP socket.
    #[cfg_attr(feature = "clap", arg(long))]
    pub nodelay: bool,

    /// TCP congestion control algorithm to use (Linux only).
    /// Defaults to BBR if available, otherwise the system default.
    /// Supported values: bbr, cubic, reno
    #[cfg(target_os = "linux")]
    #[cfg_attr(
        feature = "clap",
        arg(long = "congestion-control", value_parser = parse_algorithm)
    )]
    pub congestion_control: Option<Algorithm>,
}

#[cfg(feature = "clap")]
fn parse_algorithm(s: &str) -> Result<crate::congestion_control::Algorithm, String> {
    s.parse()
}

/// Represents a failure to apply socket options to the TCP socket.
#[derive(Debug)]
pub struct ApplyTcpOptionsError(ApplyTcpOptionsErrorInternal);

#[derive(Debug)]
enum ApplyTcpOptionsErrorInternal {
    RecvBuffer(io::Error),
    SendBuffer(io::Error),

    #[cfg(target_os = "linux")]
    Mark(nix::Error),

    TcpNoDelay(io::Error),

    #[cfg(target_os = "linux")]
    CongestionControl(crate::congestion_control::CongestionControlError),
}

/// A list specifying what failed when applying the TCP options.
#[derive(Debug, Copy, Clone)]
#[non_exhaustive]
pub enum ApplyTcpOptionsErrorKind {
    /// Failed to get/set TCP_RCVBUF
    RecvBuffer,

    /// Failed to get/set TCP_SNDBUF
    SendBuffer,

    /// Failed to get/set SO_MARK
    #[cfg(target_os = "linux")]
    Mark,

    /// Failed to get/set TCP_NODELAY
    TcpNoDelay,

    /// Failed to set TCP congestion control algorithm
    #[cfg(target_os = "linux")]
    CongestionControl,
}

impl ApplyTcpOptionsError {
    /// Returns the kind of error that happened as an enum
    pub fn kind(&self) -> ApplyTcpOptionsErrorKind {
        use ApplyTcpOptionsErrorInternal::*;

        match self.0 {
            RecvBuffer(_) => ApplyTcpOptionsErrorKind::RecvBuffer,
            SendBuffer(_) => ApplyTcpOptionsErrorKind::SendBuffer,

            #[cfg(target_os = "linux")]
            Mark(_) => ApplyTcpOptionsErrorKind::Mark,

            TcpNoDelay(_) => ApplyTcpOptionsErrorKind::TcpNoDelay,

            #[cfg(target_os = "linux")]
            CongestionControl(_) => ApplyTcpOptionsErrorKind::CongestionControl,
        }
    }
}

impl From<ApplyTcpOptionsErrorInternal> for ApplyTcpOptionsError {
    fn from(value: ApplyTcpOptionsErrorInternal) -> Self {
        Self(value)
    }
}

impl fmt::Display for ApplyTcpOptionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use ApplyTcpOptionsErrorInternal::*;

        match &self.0 {
            RecvBuffer(_) => "Failed to get/set TCP_RCVBUF",
            SendBuffer(_) => "Failed to get/set TCP_SNDBUF",

            #[cfg(target_os = "linux")]
            Mark(_) => "Failed to get/set SO_MARK",

            TcpNoDelay(_) => "Failed to get/set TCP_NODELAY",

            #[cfg(target_os = "linux")]
            CongestionControl(e) => return write!(f, "{}", e),
        }
        .fmt(f)
    }
}

impl std::error::Error for ApplyTcpOptionsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        use ApplyTcpOptionsErrorInternal::*;

        match &self.0 {
            RecvBuffer(e) => Some(e),
            SendBuffer(e) => Some(e),

            #[cfg(target_os = "linux")]
            Mark(e) => Some(e),

            TcpNoDelay(e) => Some(e),

            #[cfg(target_os = "linux")]
            CongestionControl(e) => Some(e),
        }
    }
}

#[cfg(feature = "clap")]
fn duration_secs_from_str(
    str_duration: &str,
) -> Result<Duration, std::num::ParseIntError> {
    use std::str::FromStr;

    u64::from_str(str_duration).map(Duration::from_secs)
}

/// Applies the given options to the given TCP socket.
pub fn apply(
    socket: &TcpSocket,
    options: &TcpOptions,
) -> Result<(), ApplyTcpOptionsError> {
    if let Some(recv_buffer_size) = options.recv_buffer_size {
        socket
            .set_recv_buffer_size(recv_buffer_size)
            .map_err(ApplyTcpOptionsErrorInternal::RecvBuffer)?;
    }

    log::debug!(
        "SO_RCVBUF: {}",
        socket
            .recv_buffer_size()
            .map_err(ApplyTcpOptionsErrorInternal::RecvBuffer)?
    );

    if let Some(send_buffer_size) = options.send_buffer_size {
        socket
            .set_send_buffer_size(send_buffer_size)
            .map_err(ApplyTcpOptionsErrorInternal::SendBuffer)?;
    }

    log::debug!(
        "SO_SNDBUF: {}",
        socket
            .send_buffer_size()
            .map_err(ApplyTcpOptionsErrorInternal::SendBuffer)?
    );

    #[cfg(target_os = "linux")]
    {
        if let Some(fwmark) = options.fwmark {
            setsockopt(socket.as_raw_fd(), sockopt::Mark, &fwmark)
                .map_err(ApplyTcpOptionsErrorInternal::Mark)?;
        }

        log::debug!(
            "SO_MARK: {}",
            getsockopt(socket.as_raw_fd(), sockopt::Mark)
                .map_err(ApplyTcpOptionsErrorInternal::Mark)?
        );

        if let Some(algorithm) = options.congestion_control {
            crate::congestion_control::set_algorithm(socket, algorithm)
                .map_err(ApplyTcpOptionsErrorInternal::CongestionControl)?;
        } else {
            // Try to set BBR by default on Linux if not specified
            match crate::congestion_control::set_algorithm(
                socket,
                Algorithm::Bbr,
            ) {
                Ok(()) => {}

                Err(_) => {
                    log::debug!(
                        "BBR congestion control not available, using system default"
                    );
                }
            }
        }
    }

    Ok(())
}

/// We need to apply the nodelay option separately as it is not currently exposed on TcpSocket.
/// => https://github.com/tokio-rs/tokio/issues/5510
pub fn set_nodelay(
    tcp_stream: &TcpStream,
    nodelay: bool,
) -> Result<(), ApplyTcpOptionsError> {
    tcp_stream
        .set_nodelay(nodelay)
        .map_err(ApplyTcpOptionsErrorInternal::TcpNoDelay)?;

    log::debug!(
        "TCP_NODELAY: {}",
        tcp_stream
            .nodelay()
            .map_err(ApplyTcpOptionsErrorInternal::TcpNoDelay)?
    );

    Ok(())
}
