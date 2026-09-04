use alloc::vec::Vec;

use embassy_net::{
    Stack,
    dns::DnsSocket,
    tcp::client::{TcpClient, TcpClientState},
};
use reqwless::{
    client::{HttpClient, TlsConfig, TlsVerify},
    request::{Method, RequestBuilder},
};

pub(crate) const RESPONSE_BUFFER_SIZE: usize = 512 * 1024;
type RadarTcpClient = TcpClient<'static, 1, 16384, 16384>;
type RadarDnsSocket = DnsSocket<'static>;
type RequestBody<'a> = Option<(&'a [(&'a str, &'a str)], &'a [u8])>;

pub(super) struct ApiClient {
    tcp_client: RadarTcpClient,
    dns_client: RadarDnsSocket,
    tls_read: Vec<u8>,
    tls_write: Vec<u8>,
    response_buffer: Vec<u8>,
}

impl ApiClient {
    pub(super) fn new(
        stack: Stack<'static>,
        tcp_state: &'static TcpClientState<1, 16384, 16384>,
        tls_read: Vec<u8>,
        tls_write: Vec<u8>,
        response_buffer: Vec<u8>,
    ) -> Self {
        Self {
            tcp_client: TcpClient::new(stack, tcp_state),
            dns_client: DnsSocket::new(stack),
            tls_read,
            tls_write,
            response_buffer,
        }
    }

    pub(super) async fn get(&mut self, url: &str) -> Result<(u16, &[u8]), reqwless::Error> {
        self.request(Method::GET, url, None).await
    }

    pub(super) async fn post(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<(u16, &[u8]), reqwless::Error> {
        self.request(Method::POST, url, Some((headers, body))).await
    }

    async fn request(
        &mut self,
        method: Method,
        url: &str,
        body: RequestBody<'_>,
    ) -> Result<(u16, &[u8]), reqwless::Error> {
        let tls = TlsConfig::new(
            0x4f50_454e_534b_5901,
            self.tls_read.as_mut_slice(),
            self.tls_write.as_mut_slice(),
            TlsVerify::None,
        );
        let mut client = HttpClient::new_with_tls(&self.tcp_client, &self.dns_client, tls);
        self.response_buffer.clear();
        self.response_buffer.resize(RESPONSE_BUFFER_SIZE, 0);
        let request = client.request(method, url).await?;
        match body {
            Some((headers, body)) => {
                let mut request = request.headers(headers).body(body);
                let response = request.send(&mut self.response_buffer).await?;
                let status = response.status.0;
                Ok((status, response.body().read_to_end().await?))
            }
            None => {
                let mut request = request;
                let response = request.send(&mut self.response_buffer).await?;
                let status = response.status.0;
                Ok((status, response.body().read_to_end().await?))
            }
        }
    }
}
