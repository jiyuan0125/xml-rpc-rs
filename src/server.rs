#![allow(deprecated)]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::{Arc, Mutex};

use crate::http::create_request;

use super::error::{ErrorKind, Result};
use super::xmlfmt::{error, from_params, into_params, parse, Call, Fault, Response, Value};

use super::http::{Request as HttpRequest, Response as HttpResponse};

type Handler = Box<dyn Fn(Vec<Value>) -> Response + Send + Sync>;
type HandlerMap = HashMap<String, Handler>;

pub fn on_decode_fail(err: &error::Error) -> Response {
    Err(Fault::new(
        400,
        format!("Failed to decode request: {}", err),
    ))
}

pub fn on_encode_fail(err: &error::Error) -> Response {
    Err(Fault::new(
        500,
        format!("Failed to encode response: {}", err),
    ))
}

fn on_missing_method(_: Vec<Value>) -> Response {
    Err(Fault::new(404, "Requested method does not exist"))
}

pub struct Server {
    handlers: HandlerMap,
    on_missing_method: Handler,
}

impl Default for Server {
    fn default() -> Self {
        Server {
            handlers: HashMap::new(),
            on_missing_method: Box::new(on_missing_method),
        }
    }
}

impl Server {
    pub fn new() -> Server {
        Server::default()
    }

    pub fn register_value<K, T>(&mut self, name: K, handler: T)
    where
        K: Into<String>,
        T: Fn(Vec<Value>) -> Response + Send + Sync + 'static,
    {
        self.handlers.insert(name.into(), Box::new(handler));
    }

    pub fn register<'a, K, Treq, Tres, Thandler, Tef, Tdf>(
        &mut self,
        name: K,
        handler: Thandler,
        encode_fail: Tef,
        decode_fail: Tdf,
    ) where
        K: Into<String>,
        Treq: Deserialize<'a>,
        Tres: Serialize,
        Thandler: Fn(Treq) -> std::result::Result<Tres, Fault> + Send + Sync + 'static,
        Tef: Fn(&error::Error) -> Response + Send + Sync + 'static,
        Tdf: Fn(&error::Error) -> Response + Send + Sync + 'static,
    {
        self.register_value(name, move |req| {
            let params = match from_params(req) {
                Ok(v) => v,
                Err(err) => return decode_fail(&err),
            };
            let response = handler(params)?;
            into_params(&response).or_else(|v| encode_fail(&v))
        });
    }

    pub fn register_simple<'a, K, Treq, Tres, Thandler>(&mut self, name: K, handler: Thandler)
    where
        K: Into<String>,
        Treq: Deserialize<'a>,
        Tres: Serialize,
        Thandler: Fn(Treq) -> std::result::Result<Tres, Fault> + Send + Sync + 'static,
    {
        self.register(name, handler, on_encode_fail, on_decode_fail);
    }

    pub fn set_on_missing<T>(&mut self, handler: T)
    where
        T: Fn(Vec<Value>) -> Response + Send + Sync + 'static,
    {
        self.on_missing_method = Box::new(handler);
    }

    pub fn bind(
        self,
        uri: &std::net::SocketAddr,
    ) -> Result<BoundServer<impl Fn(&HttpRequest) -> HttpResponse + Send + Sync + 'static>> {
        let tcp_listener =
            TcpListener::bind(uri).map_err(|err| ErrorKind::BindFail(err.to_string().into()))?;
        let udp_socket =
            UdpSocket::bind(uri).map_err(|err| ErrorKind::BindFail(err.to_string().into()))?;

        Ok(BoundServer::new(tcp_listener, udp_socket, move |request| {
            self.handle_outer(request)
        }))
    }

    fn handle_outer(&self, request: &HttpRequest) -> HttpResponse {
        use super::xmlfmt::value::ToXml;

        let body = match request.body() {
            Some(data) => data,
            None => return HttpResponse::empty_400(),
        };

        // TODO: use the right error type
        let call: Call = match parse::call(body.as_bytes()) {
            Ok(data) => data,
            Err(_err) => return HttpResponse::empty_400(),
        };
        let res = self.handle(call);
        let body = res.to_xml();
        HttpResponse::from_data("text/xml", Some(body))
    }

    fn handle(&self, req: Call) -> Response {
        self.handlers
            .get(&req.name)
            .unwrap_or(&self.on_missing_method)(req.params)
    }
}

pub struct BoundServer<F>
where
    F: Send + Sync + 'static + Fn(&HttpRequest) -> HttpResponse,
{
    tcp_listener: Arc<Mutex<Option<TcpListener>>>,
    udp_socket: Arc<Mutex<Option<UdpSocket>>>,
    handler: Arc<F>,
}

impl<F> BoundServer<F>
where
    F: Send + Sync + 'static + Fn(&HttpRequest) -> HttpResponse,
{
    fn new(tcp_listener: TcpListener, udp_socket: UdpSocket, handler: F) -> Self {
        Self {
            tcp_listener: Arc::new(Mutex::new(Some(tcp_listener))),
            udp_socket: Arc::new(Mutex::new(Some(udp_socket))),
            handler: Arc::new(handler),
        }
    }

    pub fn local_addr(&self) -> Option<std::net::SocketAddr> {
        self.tcp_listener
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|v| v.local_addr().ok())
    }

    pub fn run(&self) {
        let tcp_listener = self.tcp_listener.lock().unwrap().take().unwrap();
        accept_loop_tcp(tcp_listener, self.handler.clone());

        let udp_socket = self.udp_socket.lock().unwrap().take().unwrap();
        accept_loop_udp(udp_socket, self.handler.clone());
    }
}

fn accept_loop_tcp<F>(tcp_listener: TcpListener, handler: Arc<F>)
where
    F: Send + Sync + 'static + Fn(&HttpRequest) -> HttpResponse,
{
    loop {
        let handler = handler.clone();
        let accept = tcp_listener.accept();
        match accept {
            Ok((stream, remote_addr)) => {
                println!("a connection accepted: {}", remote_addr);
                std::thread::spawn(move || {
                    handle_connection(stream, &remote_addr, handler.clone());
                });
            }
            Err(e) => eprintln!("failed to accept connection: {}", e),
        }
    }
}

fn accept_loop_udp<F>(udp_socket: UdpSocket, handler: Arc<F>)
where
    F: Send + Sync + 'static + Fn(&HttpRequest) -> HttpResponse,
{
    loop {
        let handler = handler.clone();
        let mut buf = vec![0; 4096];
        let received = udp_socket.recv_from(&mut buf);
        match received {
            Ok((_amt, remote_addr)) => {
                println!("received from: {}", remote_addr);
                handle_udp_message(&udp_socket, buf, &remote_addr, handler.clone());
            }
            Err(e) => eprintln!("failed to accept connection: {}", e),
        }
    }
}

fn handle_connection<F>(mut stream: TcpStream, remote_addr: &SocketAddr, handler: Arc<F>)
where
    F: Send + Sync + 'static + Fn(&HttpRequest) -> HttpResponse,
{
    loop {
        let request = create_request(&mut stream, &remote_addr);
        match request {
            Ok(request) => {
                println!("request: {:?}", request);
                let response = handler(&request);
                if let Err(e) = response.raw_print(&mut stream, false) {
                    println!("failed to send response: {}", e);
                }
            }
            Err(_) => {
                // eprintln!("failed parse request: {}", e);
                // let _ = stream.shutdown(std::net::Shutdown::Both);
                break;
            }
        }
    }
}

fn handle_udp_message<F>(udp_socket: &UdpSocket, buf: Vec<u8>, remote_addr: &SocketAddr, handler: Arc<F>)
where
    F: Send + Sync + 'static + Fn(&HttpRequest) -> HttpResponse,
{
    let mut reader = Cursor::new(buf);
    let request = create_request(&mut reader, &remote_addr);
    match request {
        Ok(request) => {
            println!("request: {:?}", request);
            let response = handler(&request);
            let mut writer = Cursor::new(vec![0; 4096]);
            if let Err(e) = response.raw_print(&mut writer, false) {
                println!("failed to write response: {}", e);
            }
            if let Err(e) = udp_socket.send(&writer.into_inner()) {
                eprintln!("failed to send response: {}", e);
            }
        }
        Err(_) => {
            // eprintln!("failed parse request: {}", e);
            // let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
}
