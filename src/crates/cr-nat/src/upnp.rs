//! UPnP IGD 客户端（std-only，最小实现）。
//!
//! 流程：SSDP 发现 IGD → 拉取设备描述 → 定位 WANIPConnection/WANPPPConnection 控制 URL
//! → SOAP 调用 GetExternalIPAddress / AddPortMapping / DeletePortMapping。
//! 内置极简 HTTP/1.1 客户端（仅支持 Content-Length + Connection: close），不引入任何依赖。

use std::io::Write;
use std::net::{TcpStream, UdpSocket};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct UpnpResult {
    pub available: bool,
    pub locations: Vec<String>,
    pub error: String,
}

#[derive(Debug, Clone)]
pub struct UpnpMapping {
    pub external_ip: String,
    pub external_port: u16,
    pub internal_port: u16,
}

pub struct UpnpClient {
    pub location: String, // e.g. http://192.168.1.1:1900/desc.xml
}

/// SSDP 多播发现（仅局域网有效）。返回发现到的 IGD LOCATION 列表。
pub fn discover(timeout: Duration) -> UpnpResult {
    let mut out = UpnpResult {
        available: false,
        locations: Vec::new(),
        error: String::new(),
    };
    let msg = b"M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n\
                MAN: \"ssdp:discover\"\r\nMX: 2\r\n\
                ST: urn:schemas-upnp-org:device:InternetGatewayDevice:1\r\n\r\n";
    let s = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(e) => {
            out.error = format!("bind: {}", e);
            return out;
        }
    };
    let _ = s.set_read_timeout(Some(timeout));
    if s.send_to(msg, "239.255.255.250:1900").is_err() {
        out.error = "SSDP 发送失败".into();
        return out;
    }
    let deadline = std::time::Instant::now() + timeout;
    let mut buf = [0u8; 4096];
    loop {
        let remain = deadline.saturating_duration_since(std::time::Instant::now());
        if remain.is_zero() {
            break;
        }
        let _ = s.set_read_timeout(Some(remain));
        match s.recv_from(&mut buf) {
            Ok((n, _)) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                if let Some(loc) = extract_header(&text, "LOCATION") {
                    if !out.locations.contains(&loc) {
                        out.locations.push(loc);
                    }
                }
            }
            Err(_) => break,
        }
    }
    out.available = !out.locations.is_empty();
    out
}

fn extract_header(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.to_ascii_uppercase().strip_prefix(&format!("{}:", key)) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

impl UpnpClient {
    pub fn from_location(location: String) -> Self {
        Self { location }
    }

    /// 拉取设备描述，定位 WANIPConnection/WANPPPConnection 控制 URL。
    fn control_url(&self, timeout: Duration) -> Result<String, String> {
        let body = http_get(&self.location, timeout)?;
        let svc = find_service_control_url(&body)
            .ok_or_else(|| "设备描述中未找到 WANIPConnection/WANPPPConnection 服务".to_string())?;
        // 设备描述里的 controlURL 可能是相对路径，需相对 IGD 基址解析。
        resolve_url(&self.location, &svc)
    }

    /// 取网关公网 IP。
    pub fn get_external_ip(&self, timeout: Duration) -> Result<String, String> {
        let ctl = self.control_url(timeout)?;
        let soap = soap_envelope(
            "urn:schemas-upnp-org:service:WANIPConnection:1",
            "GetExternalIPAddress",
            "",
        );
        let resp = http_post(&ctl, &soap, timeout)?;
        extract_tag(&resp, "NewExternalIPAddress")
            .ok_or_else(|| "SOAP 响应缺少 NewExternalIPAddress".into())
    }

    /// 添加端口映射：把内网 (internal_port) 暴露到外网 (external_port)。
    pub fn add_port_mapping(
        &self,
        proto: &str, // "UDP" | "TCP"
        internal_port: u16,
        external_port: u16,
        lifetime_sec: u32,
        desc: &str,
        timeout: Duration,
    ) -> Result<(), String> {
        let ctl = self.control_url(timeout)?;
        let args = format!(
            "<NewRemoteHost></NewRemoteHost>\
             <NewExternalPort>{ext}</NewExternalPort>\
             <NewProtocol>{proto}</NewProtocol>\
             <NewInternalPort>{int}</NewInternalPort>\
             <NewInternalClient>{client}</NewInternalClient>\
             <NewEnabled>1</NewEnabled>\
             <NewPortMappingDescription>{desc}</NewPortMappingDescription>\
             <NewLeaseDuration>{life}</NewLeaseDuration>",
            ext = external_port,
            proto = proto,
            int = internal_port,
            client = local_ip_hint(),
            life = lifetime_sec,
            desc = desc,
        );
        let soap = soap_envelope(
            "urn:schemas-upnp-org:service:WANIPConnection:1",
            "AddPortMapping",
            &args,
        );
        let _ = http_post(&ctl, &soap, timeout)?;
        Ok(())
    }

    pub fn delete_port_mapping(
        &self,
        proto: &str,
        external_port: u16,
        timeout: Duration,
    ) -> Result<(), String> {
        let ctl = self.control_url(timeout)?;
        let args = format!(
            "<NewRemoteHost></NewRemoteHost>\
             <NewExternalPort>{ext}</NewExternalPort>\
             <NewProtocol>{proto}</NewProtocol>",
            ext = external_port,
            proto = proto,
        );
        let soap = soap_envelope(
            "urn:schemas-upnp-org:service:WANIPConnection:1",
            "DeletePortMapping",
            &args,
        );
        let _ = http_post(&ctl, &soap, timeout)?;
        Ok(())
    }
}

fn local_ip_hint() -> String {
    let s = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return "0.0.0.0".into(),
    };
    if s.connect("8.8.8.8:9").is_err() {
        return "0.0.0.0".into();
    }
    s.local_addr().map(|a| a.ip().to_string()).unwrap_or_else(|_| "0.0.0.0".into())
}

/// 极简 HTTP GET（仅 Content-Length + Connection: close）。
fn http_get(url: &str, timeout: Duration) -> Result<String, String> {
    let (host, port, path) = parse_url(url)?;
    let mut stream = TcpStream::connect((host.as_str(), port)).map_err(|e| format!("connect: {}", e))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let req = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept: */*\r\n\r\n",
        path, host
    );
    stream
        .write_all(req.as_bytes())
        .map_err(|e| e.to_string())?;
    read_http_body(stream)
}

fn http_post(url: &str, body: &str, timeout: Duration) -> Result<String, String> {
    let (host, port, path) = parse_url(url)?;
    let mut stream = TcpStream::connect((host.as_str(), port)).map_err(|e| format!("connect: {}", e))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        host,
        body.as_bytes().len(),
        body
    );
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    read_http_body(stream)
}

fn parse_url(url: &str) -> Result<(String, u16, String), String> {
    let u = url.trim();
    let rest = u
        .strip_prefix("http://")
        .ok_or_else(|| "仅支持 http://".to_string())?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (rest[..i].to_string(), rest[i..].to_string()),
        None => (rest.to_string(), "/".to_string()),
    };
    let (host, port) = match authority.rfind(':') {
        Some(i) => (
            authority[..i].to_string(),
            authority[i + 1..]
                .parse::<u16>()
                .map_err(|_| "非法端口".to_string())?,
        ),
        None => (authority, 80),
    };
    Ok((host, port, path))
}

/// 将相对 controlURL 相对 IGD 基址解析为绝对 URL。
/// 若 `rel` 已是 http(s):// 绝对地址则原样返回。
fn resolve_url(base: &str, rel: &str) -> Result<String, String> {
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return Ok(rel.to_string());
    }
    let (host, port, _path) = parse_url(base)?;
    let path = if rel.starts_with('/') {
        rel.to_string()
    } else {
        // 相对当前路径的兄弟资源
        let base_path = match base.rfind('/') {
            Some(i) => &base[..=i],
            None => "/",
        };
        format!("{}{}", base_path, rel)
    };
    Ok(format!("http://{}:{}{}", host, port, path))
}

fn read_http_body(mut stream: TcpStream) -> Result<String, String> {
    use std::io::{Read, Write};
    let _ = Write::flush(&mut stream);
    let mut all = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => all.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    let text = String::from_utf8_lossy(&all);
    // 以第一个 \r\n\r\n 切分头部与正文
    match text.find("\r\n\r\n") {
        Some(i) => Ok(text[i + 4..].to_string()),
        None => Ok(text.to_string()),
    }
}

fn soap_envelope(service: &str, action: &str, args: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body>\
         <u:{action} xmlns:u=\"{service}\">{args}</u:{action}>\
         </s:Body></s:Envelope>",
        action = action,
        service = service,
        args = args
    )
}

/// 从设备描述 XML 中找第一个 WANIPConnection/WANPPPConnection 服务的 controlURL。
///
/// 按 `<service>...</service>` 分块扫描：块内同时含匹配的 serviceType 与 controlURL 才采纳，
/// 避免把不同服务的标签错误配对。
fn find_service_control_url(desc: &str) -> Option<String> {
    let lower = desc.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(start) = lower[from..].find("<service") {
        let abs_start = from + start;
        // 找对应的 </service>
        let end = match lower[abs_start..].find("</service>") {
            Some(i) => abs_start + i + "</service>".len(),
            None => break,
        };
        let block = &desc[abs_start..end];
        let block_lower = block.to_ascii_lowercase();
        if block_lower.contains("wanipconnection") || block_lower.contains("wanpppconnection") {
            if let Some(url) = extract_tag(block, "controlURL") {
                if !url.is_empty() {
                    return Some(url);
                }
            }
        }
        from = end;
    }
    None
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let l = xml.to_ascii_lowercase();
    let lo = match l.find(&open.to_ascii_lowercase()) {
        Some(i) => i + open.len(),
        None => return None,
    };
    let lc = match l[lo..].find(&close.to_ascii_lowercase()) {
        Some(i) => lo + i,
        None => return None,
    };
    Some(xml[lo..lc].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn spawn_mock_igd() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            for conn in listener.incoming().take(3) {
                if let Ok(mut stream) = conn {
                    let mut buf = [0u8; 4096];
                    let _ = stream.read(&mut buf);
                    let req = String::from_utf8_lossy(&buf);
                    let body = if req.starts_with("GET") {
                        // 设备描述
                        r#"<?xml version="1.0"?>
                        <root><device>
                          <serviceList>
                            <service>
                              <serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
                              <controlURL>/ctl</controlURL>
                            </service>
                          </serviceList>
                        </device></root>"#
                            .to_string()
                    } else {
                        // SOAP：根据 action 返回
                        if req.contains("GetExternalIPAddress") {
                            r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:GetExternalIPAddressResponse xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:1"><NewExternalIPAddress>203.0.113.5</NewExternalIPAddress></u:GetExternalIPAddressResponse></s:Body></s:Envelope>"#.to_string()
                        } else {
                            r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"><s:Body><u:AddPortMappingResponse xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:1"></u:AddPortMappingResponse></s:Body></s:Envelope>"#.to_string()
                        }
                    };
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.as_bytes().len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
            }
        });
        format!("http://127.0.0.1:{}/desc.xml", port)
    }

    #[test]
    fn control_url_found() {
        let loc = spawn_mock_igd();
        let c = UpnpClient::from_location(loc);
        let ctl = c.control_url(Duration::from_secs(2)).unwrap();
        assert!(ctl.ends_with("/ctl"), "controlURL 应为 /ctl，实际 {}", ctl);
    }

    #[test]
    fn get_external_ip() {
        let loc = spawn_mock_igd();
        let c = UpnpClient::from_location(loc);
        let ip = c.get_external_ip(Duration::from_secs(2)).unwrap();
        assert_eq!(ip, "203.0.113.5");
    }

    #[test]
    fn add_port_mapping_ok() {
        let loc = spawn_mock_igd();
        let c = UpnpClient::from_location(loc);
        c.add_port_mapping("UDP", 17801, 45000, 3600, "coldrevive", Duration::from_secs(2))
            .unwrap();
    }

    #[test]
    fn extract_tag_works() {
        let xml = r#"<a><NewExternalIPAddress>1.2.3.4</NewExternalIPAddress></a>"#;
        assert_eq!(extract_tag(xml, "NewExternalIPAddress").as_deref(), Some("1.2.3.4"));
    }
}
