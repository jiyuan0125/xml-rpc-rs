extern crate httpdate;

use super::common::{HTTPVersion, Header, StatusCode};
use self::httpdate::HttpDate;

use std::io::Result as IoResult;
use std::io::{self, Write};

use std::str::FromStr;
use std::time::SystemTime;

/// Object representing an HTTP response whose purpose is to be given to a `Request`.
///
/// Some headers cannot be changed. Trying to define the value
/// of one of these will have no effect:
///
///  - `Connection`
///  - `Trailer`
///  - `Transfer-Encoding`
///  - `Upgrade`
///
/// Some headers have special behaviors:
///
///  - `Content-Encoding`: If you define this header, the library
///     will assume that the data from the `Read` object has the specified encoding
///     and will just pass-through.
///
///  - `Content-Length`: The length of the data should be set manually
///     using the `Reponse` object's API. Attempting to set the value of this
///     header will be equivalent to modifying the size of the data but the header
///     itself may not be present in the final result.
///
///  - `Content-Type`: You may only set this header to one value at a time. If you
///     try to set it more than once, the existing value will be overwritten. This
///     behavior differs from the default for most headers, which is to allow them to
///     be set multiple times in the same response.
///
pub struct Response {
    status_code: StatusCode,
    headers: Vec<Header>,
    data: Option<String>,
    data_length: usize,
}

/// Builds a Date: header with the current date.
fn build_date_header() -> Header {
    let d = HttpDate::from(SystemTime::now());
    Header::from_bytes(&b"Date"[..], &d.to_string().into_bytes()[..]).unwrap()
}

fn write_message_header<W>(
    mut writer: W,
    http_version: &HTTPVersion,
    status_code: &StatusCode,
    headers: &[Header],
) -> IoResult<()>
where
    W: Write,
{
    // writing status line
    write!(
        &mut writer,
        "HTTP/{}.{} {} {}\r\n",
        http_version.0,
        http_version.1,
        status_code.0,
        status_code.default_reason_phrase()
    )?;

    // writing headers
    for header in headers.iter() {
        writer.write_all(header.field.as_str().as_ref())?;
        write!(&mut writer, ": ")?;
        writer.write_all(header.value.as_str().as_ref())?;
        write!(&mut writer, "\r\n")?;
    }

    // separator between header and data
    write!(&mut writer, "\r\n")?;

    Ok(())
}

impl Response
{
    /// Creates a new Response object.
    ///
    /// The `additional_headers` argument is a receiver that
    ///  may provide headers even after the response has been sent.
    ///
    /// All the other arguments are straight-forward.
    pub fn new(
        status_code: StatusCode,
        headers: Vec<Header>,
        data: Option<String>,
        data_length: usize,
    ) -> Response {
        let mut response = Response {
            data,
            status_code,
            headers: Vec::with_capacity(16),
            data_length,
        };

        for h in headers {
            response.add_header(h)
        }

        response
    }

    /// Adds a header to the list.
    /// Does all the checks.
    pub fn add_header<H>(&mut self, header: H)
    where
        H: Into<Header>,
    {
        let header = header.into();

        // ignoring forbidden headers
        if header.field.equiv("Connection")
            || header.field.equiv("Trailer")
            || header.field.equiv("Transfer-Encoding")
            || header.field.equiv("Upgrade")
        {
            return;
        }

        // if the header is Content-Length, setting the data length
        if header.field.equiv("Content-Length") {
            if let Ok(val) = usize::from_str(header.value.as_str()) {
                self.data_length = val;
            }

            return;
        // if the header is Content-Type and it's already set, overwrite it
        } else if header.field.equiv("Content-Type") {
            if let Some(content_type_header) = self
                .headers
                .iter_mut()
                .find(|h| h.field.equiv("Content-Type"))
            {
                content_type_header.value = header.value;
                return;
            }
        }

        self.headers.push(header);
    }

    /// Returns the same request, but with an additional header.
    ///
    /// Some headers cannot be modified and some other have a
    ///  special behavior. See the documentation above.
    #[inline]
    pub fn with_header<H>(mut self, header: H) -> Response
    where
        H: Into<Header>,
    {
        self.add_header(header.into());
        self
    }

    /// Returns the same request, but with a different status code.
    #[inline]
    pub fn with_status_code<S>(mut self, code: S) -> Response
    where
        S: Into<StatusCode>,
    {
        self.status_code = code.into();
        self
    }

    /// Returns the same request, but with different data.
    pub fn with_data(self, data: Option<String>, data_length: usize) -> Response
    {
        Response {
            data,
            headers: self.headers,
            status_code: self.status_code,
            data_length,
        }
    }

    /// Prints the HTTP response to a writer.
    ///
    /// This function is the one used to send the response to the client's socket.
    /// Therefore you shouldn't expect anything pretty-printed or even readable.
    ///
    /// The HTTP version and headers passed as arguments are used to
    ///  decide which features (most notably, encoding) to use.
    ///
    /// Note: does not flush the writer.
    pub fn raw_print<W: Write>(
        mut self,
        writer: &mut W,
        do_not_send_body: bool
    ) -> IoResult<()> {
        // add `Date` if not in the headers
        if !self.headers.iter().any(|h| h.field.equiv("Date")) {
            self.headers.insert(0, build_date_header());
        }

        // add `Server` if not in the headers
        if !self.headers.iter().any(|h| h.field.equiv("Server")) {
            self.headers.insert(
                0,
                Header::from_bytes(&b"Server"[..], &b"Xml Rpc ArceOS (Rust)"[..]).unwrap(),
            );
        }

        // checking whether to ignore the body of the response
        let do_not_send_body = do_not_send_body
            || match self.status_code.0 {
                // status code 1xx, 204 and 304 MUST not include a body
                100..=199 | 204 | 304 => true,
                _ => false,
            };

        self.headers.push(
            Header::from_bytes(
                &b"Content-Length"[..],
                format!("{}", self.data_length).as_bytes(),
            )
            .unwrap(),
        );

        // sending headers
        write_message_header(
            writer.by_ref(),
            &HTTPVersion(1, 0),
            &self.status_code,
            &self.headers,
        )?;

        // sending the body
        if !do_not_send_body && self.data.is_some() {
            if self.data_length >= 1 {
                io::copy(&mut self.data.unwrap().as_bytes(), writer)?;
            }
        }

        Ok(())
    }

    /// Retrieves the current value of the `Response` status code
    pub fn status_code(&self) -> StatusCode {
        self.status_code
    }

    /// Retrieves the current value of the `Response` data length
    pub fn data_length(&self) -> usize {
        self.data_length
    }

    /// Retrieves the current list of `Response` headers
    pub fn headers(&self) -> &[Header] {
        &self.headers
    }
}

impl Response {
    pub fn empty_400() -> Response {
        Response::new(
            StatusCode(400),
            vec![],
            None,
            0,
        )
    }

    pub fn from_data(content_type: &str, data: Option<String>) -> Response {
        let mut headers = vec![];
        headers.push(Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes()).unwrap());

        let data_length = match &data {
            Some(data) => data.len(),
            None => 0
        };

        Response::new(
            StatusCode(200),
            headers,
            data,
            data_length,
        )
    }
}