use base64::Engine;
use mail_parser::{Address, Message, MessageParser, MimeHeaders};
use serde::Serialize;

const BLOCKED: &[&str] = &["exe", "bat", "cmd", "com", "js", "jse", "vbs", "vbe", "wsf", "ps1", "msi", "scr", "lnk", "hta", "cpl", "jar"];

#[derive(Debug, Serialize)]
pub struct AttachmentInfo {
    pub index: usize,
    pub name: String,
    pub mime: String,
    pub size: usize,
    pub blocked: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct MessageView {
    pub from: String,
    pub to: String,
    pub cc: String,
    pub date: Option<i64>,
    pub subject: String,
    pub html: String,
    pub has_remote_images: bool,
    pub attachments: Vec<AttachmentInfo>,
}

pub fn is_blocked(name: &str) -> bool {
    let name = name.trim_end_matches([' ', '.']);
    name.rsplit_once('.').is_some_and(|(_, ext)| BLOCKED.contains(&ext.to_ascii_lowercase().as_str()))
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn fmt_addr(a: Option<&Address>) -> String {
    a.map(|a| {
        a.iter()
            .map(|x| match (x.name(), x.address()) {
                (Some(n), Some(e)) => format!("{n} <{e}>"),
                (None, Some(e)) => e.to_string(),
                (Some(n), None) => n.to_string(),
                (None, None) => String::new(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    })
    .unwrap_or_default()
}

fn mime_of(part: &mail_parser::MessagePart) -> String {
    part.content_type()
        .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
        .unwrap_or_else(|| "application/octet-stream".into())
}

fn wrap(body: &str, allow_remote_images: bool) -> String {
    let img = if allow_remote_images { "data: https:" } else { "data:" };
    format!(
        r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src {img}; style-src 'unsafe-inline'"><base target="_blank"><style>body{{font-family:system-ui,sans-serif;margin:12px;overflow-wrap:anywhere}}pre{{white-space:pre-wrap}}</style></head><body>{body}</body></html>"#
    )
}

// ponytail: imagens cid são embutidas sem limite de tamanho; limitar se e-mails gigantes travarem a UI
fn inline_cids(html: &str, m: &Message) -> String {
    let mut out = html.to_string();
    for part in &m.parts {
        let Some(cid) = part.content_id() else { continue };
        let needle = format!("cid:{}", cid.trim_matches(['<', '>']));
        if out.contains(&needle) {
            let data = base64::engine::general_purpose::STANDARD.encode(part.contents());
            out = out.replace(&needle, &format!("data:{};base64,{data}", mime_of(part)));
        }
    }
    out
}

fn attachment_name(part: &mail_parser::MessagePart, i: usize) -> String {
    part.attachment_name().map(str::to_string).unwrap_or_else(|| format!("anexo-{}", i + 1))
}

pub fn render(raw: &[u8], allow_remote_images: bool) -> MessageView {
    let Some(m) = MessageParser::default().parse(raw) else {
        let body = format!("<pre>{}</pre>", escape(&String::from_utf8_lossy(raw)));
        return MessageView { html: wrap(&body, false), ..Default::default() };
    };
    // body_html converte texto sem escapar '>', então só confiamos nele quando há parte HTML real.
    let has_html = m.html_body.iter().any(|&i| m.part(i).is_some_and(|p| p.is_text_html()));
    let body = if has_html {
        m.body_html(0).map(|h| h.into_owned()).unwrap_or_default()
    } else {
        m.body_text(0).map(|t| format!("<pre>{}</pre>", escape(&t))).unwrap_or_default()
    };
    let body = inline_cids(&body, &m);
    let lower = body.to_ascii_lowercase();
    // ponytail: detecção por substring; um falso positivo só exibe o botão "Carregar imagens"
    let has_remote_images = ["src=\"http", "src='http", "src=http", "url(http", "url('http", "url(\"http"].iter().any(|p| lower.contains(p));
    let attachments = m
        .attachments()
        .enumerate()
        .map(|(i, p)| {
            let name = attachment_name(p, i);
            AttachmentInfo { index: i, blocked: is_blocked(&name), mime: mime_of(p), size: p.contents().len(), name }
        })
        .collect();
    MessageView {
        from: fmt_addr(m.from()),
        to: fmt_addr(m.to()),
        cc: fmt_addr(m.cc()),
        date: m.date().map(|d| d.to_timestamp()),
        subject: m.subject().unwrap_or("").to_string(),
        html: wrap(&body, allow_remote_images),
        has_remote_images,
        attachments,
    }
}

pub fn attachment(raw: &[u8], index: usize) -> Option<(String, Vec<u8>)> {
    let m = MessageParser::default().parse(raw)?;
    let part = m.attachments().nth(index)?;
    Some((attachment_name(part, index), part.contents().to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML_MSG: &str = "From: Ana <ana@x.com>\r\nTo: Joao <j@x.com>, b@x.com\r\nSubject: Oi\r\nDate: Mon, 6 Oct 2026 10:00:00 +0000\r\nContent-Type: multipart/mixed; boundary=B\r\n\r\n--B\r\nContent-Type: multipart/related; boundary=R\r\n\r\n--R\r\nContent-Type: text/html\r\n\r\n<p>Ol\u{e1}</p><img src=\"cid:img1\"><img src=\"https://track.example/p.gif\"><script>alert(1)</script>\r\n--R\r\nContent-Type: image/png\r\nContent-ID: <img1>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n--R--\r\n--B\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=\"nota.pdf\"\r\n\r\nPDF\r\n--B\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"virus.EXE\"\r\n\r\nMZ\r\n--B--\r\n";

    #[test]
    fn html_gets_csp_base_and_inline_cid() {
        let v = render(HTML_MSG.as_bytes(), false);
        assert_eq!(v.subject, "Oi");
        assert_eq!(v.from, "Ana <ana@x.com>");
        assert_eq!(v.to, "Joao <j@x.com>, b@x.com");
        assert!(v.html.contains("default-src 'none'; img-src data:;"));
        assert!(v.html.contains(r#"<base target="_blank">"#));
        assert!(v.html.contains("data:image/png;base64,"));
        assert!(!v.html.contains("cid:img1"));
        assert!(v.has_remote_images);
        assert!(render(HTML_MSG.as_bytes(), true).html.contains("img-src data: https:;"));
    }

    #[test]
    fn attachments_listed_and_executables_blocked() {
        let v = render(HTML_MSG.as_bytes(), false);
        let names: Vec<(&str, bool)> = v.attachments.iter().map(|a| (a.name.as_str(), a.blocked)).collect();
        assert!(names.contains(&("nota.pdf", false)));
        assert!(names.contains(&("virus.EXE", true)));
        let pdf = v.attachments.iter().find(|a| a.name == "nota.pdf").unwrap();
        let (name, bytes) = attachment(HTML_MSG.as_bytes(), pdf.index).unwrap();
        assert_eq!(name, "nota.pdf");
        assert_eq!(bytes, b"PDF");
    }

    #[test]
    fn plain_text_is_escaped() {
        let raw = b"Subject: t\r\n\r\n<script>alert(1)</script> & coisas\r\n";
        let v = render(raw, false);
        assert!(!v.html.contains("<script>"));
        assert!(v.html.contains("&lt;script&gt;"));
        assert!(!v.has_remote_images);
    }

    #[test]
    fn garbage_does_not_panic() {
        let v = render(b"\x00\x01\xff", false);
        assert!(v.html.contains("Content-Security-Policy"));
    }

    #[test]
    fn blocked_extensions() {
        for name in ["a.exe", "b.EXE", "c.exe.", "d.ps1", "e.lnk", "f.js"] {
            assert!(is_blocked(name), "{name}");
        }
        for name in ["a.pdf", "b.docx", "semextensao", "exe"] {
            assert!(!is_blocked(name), "{name}");
        }
    }
}
