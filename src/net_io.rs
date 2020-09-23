use crate::buckets::*;
use embedded_websocket as ws;
use embedded_websocket::{HttpHeader, WebSocketReceiveMessageType, WebSocketSendMessageType};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::str::Utf8Error;
use std::thread;

#[derive(Debug)]
pub enum WebServerError {
    Io(std::io::Error),
    WebSocket(ws::Error),
    RequestTooLarge(usize),
    PayloadTooLarge(usize),
    ResponseTooLarge(usize),
    Utf8Error,
}

impl From<std::io::Error> for WebServerError {
    fn from(err: std::io::Error) -> WebServerError {
        WebServerError::Io(err)
    }
}

impl From<ws::Error> for WebServerError {
    fn from(err: ws::Error) -> WebServerError {
        WebServerError::WebSocket(err)
    }
}

impl From<Utf8Error> for WebServerError {
    fn from(_: Utf8Error) -> WebServerError {
        WebServerError::Utf8Error
    }
}

pub enum WSResponse<'a> {
    Response {
        message_type: WebSocketSendMessageType,
        message: &'a [u8],
    },
    None,
    Close,
}

pub struct HttpResponse<'a> {
    pub status: u16,
    pub body: &'a [u8],
}

pub type HttpHandler =
    for<'a> fn(HttpHeader, &'a mut [u8]) -> Result<HttpResponse<'a>, WebServerError>;
pub type WSHandler = for<'a> fn(
    WebSocketReceiveMessageType,
    &[u8],
    &'a mut [u8],
) -> Result<WSResponse<'a>, WebServerError>;

#[derive(Clone, Copy)]
pub struct WebServer {
    pub http_handler: HttpHandler,
    pub ws_handler: WSHandler,
}

impl WebServer {
    pub fn serve(&self) -> Result<(), WebServerError> {
        let addr = "127.0.0.1:3000";
        let listener = TcpListener::bind(addr)?;
        println!("listening on: {}", addr);

        // accept connections and process them serially
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let s = *self;
                    thread::spawn(move || match s.handle(stream) {
                        Ok(()) => {}
                        Err(e) => println!("error: {:?}", e),
                    });
                }
                Err(e) => println!("failed to establish a connection: {}", e),
            }
        }

        return Ok(());
    }

    pub fn handle(&self, mut stream: TcpStream) -> Result<(), WebServerError> {
        println!("received connection");

        let http_handler = self.http_handler;
        let ws_handler = self.ws_handler;

        let buckets = BucketList::with_capacity(12288);

        // In lieu of defer
        struct BucketDealloc<'a>(BucketListRef<'a>);
        impl<'a> Drop for BucketDealloc<'a> {
            fn drop(&mut self) {
                unsafe { self.0.dealloc() };
            }
        }
        let dealloc_buckets = BucketDealloc(buckets);
        let tcp_recv = buckets.uninit(4096).unwrap();
        let ws_buf = buckets.uninit(4096).unwrap();
        let scratch_buf = buckets.uninit(4096).unwrap();

        let mut web_socket = ws::WebSocketServer::new_server();
        let mut num_bytes = 0;

        loop {
            let mut headers = [ws::httparse::EMPTY_HEADER; 32];

            if num_bytes >= tcp_recv.len() {
                return Err(WebServerError::RequestTooLarge(num_bytes));
            }

            num_bytes += stream.read(&mut tcp_recv[num_bytes..])?;

            // read until the stream is closed (zero bytes read from the stream)
            if num_bytes == 0 {
                return Ok(());
            }

            // assume that the client has sent us an http request. Since we may not read the
            // header all in one go we need to check for HttpHeaderIncomplete and continue reading
            let http_header = match ws::read_http_header(&mut headers, &tcp_recv[..num_bytes]) {
                Ok(header) => {
                    num_bytes = 0;
                    header
                }
                Err(ws::Error::HttpHeaderIncomplete) => continue,
                Err(e) => return Err(WebServerError::WebSocket(e)),
            };

            if let Some(ws_context) = &http_header.websocket_context {
                let to_send =
                    web_socket.server_accept(&ws_context.sec_websocket_key, None, ws_buf)?;
                write_to_stream(&mut stream, &ws_buf[..to_send])?;
                break;
            } else {
                let resp = http_handler(http_header, ws_buf)?;
                let bytes = serialize_http_response(resp, scratch_buf)?;
                write_to_stream(&mut stream, bytes)?;
                return Ok(());
            }
        }

        let mut ws_num_bytes = 0;
        loop {
            if num_bytes >= tcp_recv.len() {
                return Err(WebServerError::PayloadTooLarge(num_bytes));
            }

            num_bytes += stream.read(&mut tcp_recv[num_bytes..])?;
            if num_bytes == 0 {
                return Ok(());
            }

            let ws_result = web_socket.read(&tcp_recv[..num_bytes], &mut ws_buf[ws_num_bytes..])?;
            ws_num_bytes += ws_result.len_to;

            if ws_num_bytes == ws_buf.len() {
                return Err(WebServerError::RequestTooLarge(ws_num_bytes));
            }

            num_bytes -= ws_result.len_from;
            for i in 0..num_bytes {
                tcp_recv[i] = tcp_recv[i + ws_result.len_from];
            }

            if !ws_result.end_of_message {
                continue;
            }

            let ws_buffer = &ws_buf[..ws_num_bytes];
            ws_num_bytes = 0;
            let response = ws_handler(ws_result.message_type, ws_buffer, scratch_buf)?;
            match response {
                WSResponse::Response {
                    message_type,
                    message,
                } => {
                    let to_send = web_socket.write(message_type, true, message, ws_buf)?;
                    write_to_stream(&mut stream, &ws_buf[..to_send])?;
                }
                WSResponse::None => {}
                WSResponse::Close => return Ok(()),
            }
        }
    }
}

pub fn serialize_http_response<'a>(
    resp: HttpResponse,
    bytes: &'a mut [u8],
) -> Result<&'a [u8], WebServerError> {
    let mut num_bytes = 0;
    macro_rules! write_to_bytes {
        ($str:expr) => {
            let string = $str.as_bytes();

            for byte in string {
                let map_err = || WebServerError::ResponseTooLarge(num_bytes);
                let slot = bytes.get_mut(num_bytes).ok_or_else(map_err)?;
                *slot = *byte;
                num_bytes += 1;
            }
        };
    };

    write_to_bytes!("HTTP/1.1 ");
    num_bytes += itoa::write(&mut bytes[num_bytes..], resp.status)?;
    write_to_bytes!(" Ok\r\nContent-Length: ");
    num_bytes += itoa::write(&mut bytes[num_bytes..], resp.body.len())?;
    write_to_bytes!("\r\nContent-Type: text;html; charset=UTF-8\r\nConnection: close\r\n\r\n");

    if resp.body.len() + num_bytes > bytes.len() {
        return Err(WebServerError::ResponseTooLarge(num_bytes));
    }

    bytes[num_bytes..(resp.body.len() + num_bytes)].copy_from_slice(resp.body);
    num_bytes += resp.body.len();

    return Ok(&bytes[..num_bytes]);
}

// Below is licensed via the MIT License (MIT)
// Below is Copyright (c) 2019 David Haig

pub fn write_to_stream(stream: &mut TcpStream, buffer: &[u8]) -> Result<(), WebServerError> {
    let mut start = 0;
    loop {
        let bytes_sent = stream.write(&buffer[start..])?;
        start += bytes_sent;

        if start == buffer.len() {
            return Ok(());
        }
    }
}
