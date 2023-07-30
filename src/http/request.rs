use std::io::{self, ErrorKind, Read, Write};
use std::io::{Error as IoError, Result as IoResult};

use std::fmt;
use std::net::{SocketAddr, TcpStream};

use std::str::FromStr;
use std::sync::mpsc::Sender;

// use crate::util::{EqualReader, FusedReader};
use super::{HTTPVersion, Header, Method};

/// Represents an HTTP request made by a client.
///
/// A `Request` object is what is produced by the server, and is your what
/// your code must analyse and answer.
///
/// This object implements the `Send` trait, therefore you can dispatch your requests to
/// worker threads.
///
/// # Pipelining
///
/// If a client sends multiple requests in a row (without waiting for the response), then you will
/// get multiple `Request` objects simultaneously. This is called *requests pipelining*.
/// Tiny-http automatically reorders the responses so that you don't need to worry about the order
/// in which you call `respond` or `into_writer`.
///
/// This mechanic is disabled if:
///
///  - The body of a request is large enough (handling requires pipelining requires storing the
///    body of the request in a buffer ; if the body is too big, tiny-http will avoid doing that)
///  - A request sends a `Expect: 100-continue` header (which means that the client waits to
///    know whether its body will be processed before sending it)
///  - A request sends a `Connection: close` header or `Connection: upgrade` header (used for
///    websockets), which indicates that this is the last request that will be received on this
///    connection
///
/// # Automatic cleanup
///
/// If a `Request` object is destroyed without `into_writer` or `respond` being called,
/// an empty response with a 500 status code (internal server error) will automatically be
/// sent back to the client.
/// This means that if your code fails during the handling of a request, this "internal server
/// error" response will automatically be sent during the stack unwinding.
///
/// # Testing
///
/// If you want to build fake requests to test your server, use [`TestRequest`](crate::test::TestRequest).

pub struct Request {
    remote_addr: Option<SocketAddr>,
    method: Method,
    path: String,
    http_version: HTTPVersion,
    headers: Vec<Header>,
    body_length: usize,
    body: Option<String>,
}

struct NotifyOnDrop<R> {
    sender: Sender<()>,
    inner: R,
}

impl<R: Read> Read for NotifyOnDrop<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}
impl<R: Write> Write for NotifyOnDrop<R> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
impl<R> Drop for NotifyOnDrop<R> {
    fn drop(&mut self) {
        self.sender.send(()).unwrap();
    }
}

impl Request {
    pub fn new(
        remote_addr: Option<SocketAddr>,
        method: Method,
        path: String,
        http_version: HTTPVersion,
        headers: Vec<Header>,
        body_length: usize,
        body: Option<String>,
    ) -> Self {
        Self {
            remote_addr,
            method,
            path,
            headers,
            http_version,
            body_length,
            body,
        }
    }

    /// Returns the method requested by the client (eg. `GET`, `POST`, etc.).
    #[inline]
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Returns the resource requested by the client.
    #[inline]
    pub fn url(&self) -> &str {
        &self.path
    }

    /// Returns a list of all headers sent by the client.
    #[inline]
    pub fn headers(&self) -> &[Header] {
        &self.headers
    }

    /// Returns the HTTP version of the request.
    #[inline]
    pub fn http_version(&self) -> &HTTPVersion {
        &self.http_version
    }

    /// Returns the body
    ///
    /// Returns `None` if the body is empty.
    #[inline]
    pub fn body(&self) -> Option<&String> {
        self.body.as_ref()
    }

    /// Returns the length of the body in bytes.
    ///
    /// Returns `None` if the length is unknown.
    #[inline]
    pub fn body_length(&self) -> usize {
        self.body_length
    }

    /// Returns the address of the client that sent this request.
    ///
    /// The address is always `Some` for TCP listeners, but always `None` for UNIX listeners
    /// (as the remote address of a UNIX client is almost always unnamed).
    ///
    /// Note that this is gathered from the socket. If you receive the request from a proxy,
    /// this function will return the address of the proxy and not the address of the actual
    /// user.
    #[inline]
    pub fn remote_addr(&self) -> Option<&SocketAddr> {
        self.remote_addr.as_ref()
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            formatter,
            "Request({} {} {} from {:?}\n\
            Headers: {:?}\n\
            Length: {}\n\
            Body: {:?}\n",
            self.method,
            self.path,
            self.http_version,
            self.remote_addr.as_ref().unwrap(),
            self.headers,
            self.body_length,
            self.body.as_deref().unwrap_or("")
        )
    }
}

#[inline]
fn read_next_line<R: Read>(reader: &mut R) -> std::io::Result<String> {
    let mut buf = Vec::new();
    let mut prev_byte_was_cr = false;

    loop {
        let byte = reader.bytes().next();

        let byte = match byte {
            Some(b) => b?,
            None => {
                return Err(std::io::Error::new(
                    ErrorKind::ConnectionAborted,
                    "Unexpected EOF",
                ))
            }
        };

        if byte == b'\n' && prev_byte_was_cr {
            buf.pop(); // removing the '\r'
            return String::from_utf8(buf).map_err(|_| {
                std::io::Error::new(ErrorKind::InvalidInput, "Header is not in ASCII")
            });
        }

        prev_byte_was_cr = byte == b'\r';

        buf.push(byte);
    }
}

/// Parses a "HTTP/1.1" string.
fn parse_http_version(version: &str) -> IoResult<HTTPVersion> {
    let (major, minor) = match version {
        "HTTP/0.9" => (0, 9),
        "HTTP/1.0" => (1, 0),
        "HTTP/1.1" => (1, 1),
        "HTTP/2.0" => (2, 0),
        "HTTP/3.0" => (3, 0),
        _ => {
            return Err(IoError::new(
                ErrorKind::InvalidInput,
                "Invalid HTTP version",
            ))
        }
    };

    Ok(HTTPVersion(major, minor))
}

pub fn create_request(stream: &mut TcpStream, remote_addr: &SocketAddr) -> IoResult<Request> {
    let mut headers = Vec::new();
    let mut body = None;
    let mut body_length = None;
    let remote_addr = Some(*remote_addr);

    let line = read_next_line(stream)?;
    let mut parts = line.split_whitespace();

    let method = match parts.next() {
        Some(method) => Some(
            Method::from_str(method)
                .map_err(|_| IoError::new(ErrorKind::InvalidData, "Invalid Method"))?,
        ),
        None => Err(IoError::new(ErrorKind::InvalidData, "Invalid Header"))?,
    };

    let path = parts.next().map(|s| s.to_string());

    let http_version = match parts.next() {
        Some(http_version) => Some(parse_http_version(http_version)?),
        None => Err(IoError::new(ErrorKind::InvalidData, "Invalid Http version"))?,
    };

    loop {
        let line = read_next_line(stream)?;
        if line.is_empty() {
            break;
        }
        headers.push(
            Header::from_str(&line)
                .map_err(|_| IoError::new(ErrorKind::InvalidData, "Invalid Header"))?,
        );
    }

    if let Some(header) = headers
        .iter()
        .find(|header| header.field.equiv("Content-Length"))
    {
        body_length = Some(header.value.as_str().parse::<usize>().unwrap());
    }

    let body_length = body_length.unwrap_or(0);
    if body_length > 0 {
        let mut buf = vec![0; body_length];
        stream.read_exact(&mut buf)?;
        body = Some(
            String::from_utf8(buf)
                .map_err(|_| IoError::new(ErrorKind::InvalidData, "body is not in UTF-8"))?,
        );
    }

    Ok(Request::new(
        remote_addr,
        method.unwrap(),
        path.unwrap(),
        http_version.unwrap(),
        headers,
        body_length,
        body,
    ))
}

/// Dummy trait that regroups the `Read` and `Write` traits.
///
/// Automatically implemented on all types that implement both `Read` and `Write`.
pub trait ReadWrite: Read + Write {}
impl<T> ReadWrite for T where T: Read + Write {}
