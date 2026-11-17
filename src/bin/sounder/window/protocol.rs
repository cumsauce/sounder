//! The `sounder://` custom protocol: the page and fonts from the embedded
//! `ui/`, and the render endpoints (see `design/window-api.md`). GET only.
//! The handlers are plain functions of a request path, so tests need no
//! window: `respond` takes the host, path and query and a [`Backend`] that
//! hands out rendered audio.

use sfx_analysis::view;
use sfx_graph::Audio;

include!(concat!(env!("OUT_DIR"), "/ui_assets.rs"));

/// The page shown when `ui/` is not in the build (the ui lane has not merged
/// or a checkout without it): the raw `state`, so the window is still alive.
const FALLBACK_HTML: &str = include_str!("fallback.html");

/// A response, before it becomes an HTTP one.
#[derive(Debug, Clone, PartialEq)]
pub struct Resp {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    /// The request carried `?rev=`: the bytes for that rev never change.
    pub immutable: bool,
}

impl Resp {
    fn ok(content_type: &'static str, body: Vec<u8>) -> Resp {
        Resp {
            status: 200,
            content_type,
            body,
            immutable: false,
        }
    }
    fn err(status: u16, why: impl Into<String>) -> Resp {
        Resp {
            status,
            content_type: "text/plain; charset=utf-8",
            body: why.into().into_bytes(),
            immutable: false,
        }
    }
}

/// Where the endpoints get audio: the session, through its thread.
pub trait Backend {
    /// A node's samples (the output's when `node` is None) of open sound `doc`.
    fn audio(&self, doc: &str, node: Option<&str>) -> Result<Audio, String>;
}

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "txt" | "md" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// `%41` -> `A`, `+` stays (names never contain spaces).
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn query_map(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (unescape(k), unescape(v)),
            None => (unescape(p), String::new()),
        })
        .collect()
}

struct Q(Vec<(String, String)>);

impl Q {
    fn get(&self, k: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty())
    }
    fn f32(&self, k: &str) -> Result<Option<f32>, String> {
        match self.get(k) {
            None => Ok(None),
            Some(v) => v
                .parse::<f32>()
                .ok()
                .filter(|x| x.is_finite())
                .map(Some)
                .ok_or_else(|| format!("`{k}={v}` is not a number")),
        }
    }
    fn usize(&self, k: &str, default: usize) -> Result<usize, String> {
        match self.get(k) {
            None => Ok(default),
            Some(v) => v
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=view::MAX_DIM).contains(n))
                .ok_or_else(|| format!("`{k}={v}` must be a whole number 1..{}", view::MAX_DIM)),
        }
    }
}

fn asset(path: &str) -> Option<&'static [u8]> {
    UI_ASSETS.iter().find(|(p, _)| *p == path).map(|(_, b)| *b)
}

/// The page and fonts: `sounder://app/<file>`, `sounder://fonts/<file>`.
fn serve_static(host: &str, path: &str) -> Resp {
    let rel = path.trim_start_matches('/');
    let rel = if host == "fonts" {
        format!("fonts/{rel}")
    } else if rel.is_empty() {
        "index.html".to_string()
    } else {
        rel.to_string()
    };
    if rel.contains("..") {
        return Resp::err(404, format!("no such file `{rel}`"));
    }
    if let Some(b) = asset(&rel) {
        return Resp::ok(mime(&rel), b.to_vec());
    }
    // No ui/ in this build: the fallback page stands in for the app page.
    if rel == "index.html" && asset("index.html").is_none() {
        return Resp::ok(mime("x.html"), FALLBACK_HTML.as_bytes().to_vec());
    }
    Resp::err(
        404,
        format!(
            "no such file `{rel}` in the embedded ui ({} file(s); the page is sounder://app/index.html)",
            UI_ASSETS.len()
        ),
    )
}

/// Answers one GET. `host` and `path` are the URL's (`sounder://<host><path>`).
pub fn respond(be: &dyn Backend, method: &str, host: &str, path: &str, query: &str) -> Resp {
    if method != "GET" {
        return Resp::err(
            405,
            format!("{method} is not allowed: the protocol is GET only"),
        );
    }
    match host {
        "app" | "fonts" => serve_static(host, path),
        "render" => {
            let q = Q(query_map(query));
            let mut r = render(be, path, &q);
            r.immutable = q.get("rev").is_some() && r.status == 200;
            r
        }
        other => Resp::err(
            404,
            format!(
                "unknown host `{other}`: use sounder://app/..., sounder://fonts/... or sounder://render/..."
            ),
        ),
    }
}

fn render(be: &dyn Backend, path: &str, q: &Q) -> Resp {
    let parts: Vec<String> = path
        .trim_matches('/')
        .split('/')
        .filter(|p| !p.is_empty())
        .map(unescape)
        .collect();
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let bin = "application/octet-stream";
    let result: Result<Vec<u8>, (u16, String)> = (|| match parts.as_slice() {
        ["colormap"] => Ok(view::colormap_rgba().to_vec()),
        [doc, "node", node, "spark"] => {
            let a = be.audio(doc, Some(node)).map_err(|e| (404, e))?;
            Ok(view::encode_f32(&view::spark(&a)))
        }
        [doc, what @ ("spectrogram" | "wave" | "loudness")] => {
            let bad = |e: String| (400, e);
            let w = q.usize("w", 800).map_err(bad)?;
            let (t0, t1) = (q.f32("t0").map_err(bad)?, q.f32("t1").map_err(bad)?);
            let a = be.audio(doc, q.get("node")).map_err(|e| (404, e))?;
            let (t0, t1) = view::time_range(&a, t0, t1);
            match *what {
                "spectrogram" => {
                    let h = q.usize("h", 256).map_err(bad)?;
                    let (hz0, hz1) =
                        view::hz_range(&a, q.f32("hz0").map_err(bad)?, q.f32("hz1").map_err(bad)?);
                    let g = view::spectrogram_grid(&a, w, h, t0, t1, hz0, hz1);
                    Ok(view::encode_spectrogram(w, h, t0, t1, &g))
                }
                "wave" => Ok(view::encode_wave(&view::wave_columns(&a, w, t0, t1))),
                _ => Ok(view::encode_f32(&view::loudness_columns(&a, w, t0, t1))),
            }
        }
        _ => Err((
            404,
            format!(
                "no such endpoint `/{}`: use colormap, <doc>/spectrogram, <doc>/wave, <doc>/loudness or <doc>/node/<name>/spark",
                parts.join("/")
            ),
        )),
    })();
    match result {
        Ok(body) => Resp::ok(bin, body),
        Err((status, why)) => Resp::err(status, why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl Backend for Fake {
        fn audio(&self, doc: &str, node: Option<&str>) -> Result<Audio, String> {
            if doc != "tone" {
                return Err(format!("`{doc}` is not open (open sounds: tone)"));
            }
            if node == Some("nope") {
                return Err("no node named `nope`".into());
            }
            let sr = 48_000u32;
            Ok(Audio::mono(
                sr,
                (0..sr as usize / 4)
                    .map(|i| 0.5 * (std::f32::consts::TAU * 1000.0 * i as f32 / sr as f32).sin())
                    .collect(),
            ))
        }
    }

    fn get(host: &str, path: &str, query: &str) -> Resp {
        respond(&Fake, "GET", host, path, query)
    }

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes([b[i * 4], b[i * 4 + 1], b[i * 4 + 2], b[i * 4 + 3]])
    }

    #[test]
    fn spectrogram_bytes_have_the_header_and_cells() {
        let r = get("render", "/tone/spectrogram", "w=40&h=30&rev=3");
        assert_eq!(r.status, 200);
        assert!(r.immutable);
        assert_eq!(r.body.len(), 16 + 40 * 30 * 4);
        assert_eq!((u32_at(&r.body, 0), u32_at(&r.body, 1)), (40, 30));
        let t1 = f32::from_bits(u32_at(&r.body, 3));
        assert!((t1 - 0.25).abs() < 1e-3, "{t1}");
        // Cells are dB in -120..0.
        for i in 4..4 + 40 * 30 {
            let v = f32::from_bits(u32_at(&r.body, i));
            assert!((-120.0..=0.0).contains(&v), "{v}");
        }
        assert!(!get("render", "/tone/spectrogram", "w=40&h=30").immutable);
    }

    #[test]
    fn wave_loudness_spark_colormap_sizes() {
        let w = get("render", "/tone/wave", "w=50");
        assert_eq!(w.body.len(), 3 * 50 * 4);
        let l = get("render", "/tone/loudness", "w=20&t0=0.05&t1=0.2");
        assert_eq!(l.body.len(), 20 * 4);
        let s = get("render", "/tone/node/out/spark", "");
        assert_eq!(s.body.len(), 92 * 4);
        let c = get("render", "/colormap", "");
        assert_eq!(c.body.len(), 1024);
        assert_eq!(c.content_type, "application/octet-stream");
    }

    #[test]
    fn bad_requests_are_404_or_400_with_a_reason() {
        let r = get("render", "/ghost/wave", "w=10");
        assert_eq!(r.status, 404);
        assert!(String::from_utf8_lossy(&r.body).contains("not open"));
        assert_eq!(get("render", "/tone/wave", "node=nope").status, 404);
        let r = get("render", "/tone/wave", "w=abc");
        assert_eq!(r.status, 400);
        assert!(String::from_utf8_lossy(&r.body).contains("`w=abc`"));
        assert_eq!(get("render", "/tone/wave", "w=999999").status, 400);
        assert_eq!(get("render", "/tone/chart", "").status, 404);
        assert_eq!(get("nowhere", "/", "").status, 404);
        let r = respond(&Fake, "POST", "render", "/colormap", "");
        assert_eq!(r.status, 405);
        assert_eq!(r.content_type, "text/plain; charset=utf-8");
    }

    #[test]
    fn static_files_and_traversal() {
        // The page always answers: the embedded ui or the fallback.
        let r = get("app", "/index.html", "");
        assert_eq!(r.status, 200);
        assert_eq!(r.content_type, "text/html; charset=utf-8");
        assert_eq!(get("app", "/", "").body, r.body);
        let r = get("app", "/missing.js", "");
        assert_eq!(r.status, 404);
        assert!(String::from_utf8_lossy(&r.body).contains("missing.js"));
        assert_eq!(get("app", "/../Cargo.toml", "").status, 404);
        assert_eq!(get("fonts", "/nothing.woff2", "").status, 404);
    }

    #[test]
    fn mime_and_escapes() {
        assert_eq!(mime("a.woff2"), "font/woff2");
        assert_eq!(mime("a.js"), "text/javascript; charset=utf-8");
        assert_eq!(unescape("a%20b%2Fc"), "a b/c");
        assert_eq!(unescape("100%"), "100%");
    }
}
