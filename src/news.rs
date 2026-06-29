//! `jak news` — crawl tin tức từ nguồn RSS uy tín rồi dùng AI (Claude) để phân
//! loại theo chủ đề và tóm tắt ý chính.
//!
//! Thiết kế (đã chốt với người dùng):
//!   - Crawl: RSS qua `curl` (đồng bộ, không async — như `jak weather`). Nguồn
//!     để trong `[news] sources` của ~/.jakshrc.toml.
//!   - AI: Claude Haiku 4.5 mặc định, gọi qua `curl POST /v1/messages` với
//!     structured output (JSON schema) để nhận kết quả đáng tin cậy. Gộp NHIỀU
//!     bài vào 1 request → ít request, rẻ.
//!   - API key: người dùng tự cung cấp qua env `ANTHROPIC_API_KEY` (hoặc
//!     `[news] api_key`). Thiếu key → vẫn crawl & hiện tin thô, chỉ bỏ phần AI.
//!   - Cache: `~/.config/jaksh/news-cache.json`. `jak news` đọc cache; nếu cũ
//!     quá `ttl_minutes` (hoặc rỗng) thì tự làm mới ở foreground (người dùng
//!     vừa gọi nên mong tin mới). `jak news refresh` ép làm mới.
//!
//! Không có daemon nền: AI tốn tiền, nên chỉ gọi khi người dùng chủ động
//! `jak news` (và cache đã hết hạn) hoặc `jak news refresh`.

use anyhow::Result;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::shell::{NewsConfig, Shell};

/// Bộ chủ đề chuẩn — vừa là `enum` ép cho AI, vừa là thứ tự hiển thị.
const CATEGORIES: &[&str] = &[
    "Thời sự",
    "Thế giới",
    "Kinh tế",
    "Công nghệ",
    "Khoa học",
    "Thể thao",
    "Giải trí",
    "Sức khỏe",
    "Giáo dục",
    "Khác",
];

/// Nhãn cho bài chưa được AI phân loại (thiếu key / AI lỗi).
const UNCLASSIFIED: &str = "Chưa phân loại";

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Hướng dẫn cho model (ổn định → đặt cache_control để tái dùng giữa các lần).
const SYSTEM_PROMPT: &str = "Bạn là biên tập viên tin tức tiếng Việt. \
Với mỗi bài báo (gồm id, tiêu đề, mô tả) trong tin nhắn của người dùng, hãy:\n\
1) Phân loại vào ĐÚNG MỘT chủ đề trong danh sách cho phép.\n\
2) Tóm tắt 2-3 ý chính bằng tiếng Việt, mỗi ý một câu ngắn gọn, trung lập, \
KHÔNG suy diễn hay thêm thông tin ngoài tiêu đề và mô tả được cung cấp.\n\
Giữ nguyên id của từng bài. Nếu không chắc chủ đề, chọn \"Khác\". \
Trả về đúng định dạng JSON theo schema yêu cầu.";

// ─────────────────────────── mô hình dữ liệu + cache ───────────────────────────

#[derive(Clone, Serialize, Deserialize)]
struct Article {
    /// Hash ổn định của link — khoá để map kết quả AI về đúng bài.
    id: String,
    title: String,
    link: String,
    /// Nhãn nguồn (domain), vd "vnexpress.net".
    source: String,
    /// Thời gian xuất bản đã format (hoặc raw nếu parse fail).
    published: String,
    /// Mô tả thô từ RSS (đã strip HTML) — đầu vào cho AI + fallback hiển thị.
    #[serde(default)]
    summary_raw: String,
    /// Chủ đề do AI gán (rỗng = chưa phân loại).
    #[serde(default)]
    category: String,
    /// Các ý chính do AI tóm tắt.
    #[serde(default)]
    points: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    /// Unix secs lần crawl gần nhất.
    #[serde(default)]
    last_fetch: u64,
    #[serde(default)]
    articles: Vec<Article>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".config").join("jaksh").join("news-cache.json"))
        .unwrap_or_else(|| PathBuf::from("news-cache.json"))
}

fn read_cache() -> Cache {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_cache(c: &Cache) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(c) {
        let tmp = path.with_file_name(format!("news-cache.json.tmp.{}", std::process::id()));
        if std::fs::write(&tmp, &s).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

// ─────────────────────────────── entry point ──────────────────────────────────

pub fn run(shell: &Rc<RefCell<Shell>>, args: &[&str]) -> Result<i32> {
    let cfg = shell.borrow().news.clone();
    let sub = args.first().copied().unwrap_or("");
    match sub {
        "help" | "?" | "--help" | "-h" => {
            help();
            Ok(0)
        }
        "sources" => {
            list_sources(&cfg);
            Ok(0)
        }
        "refresh" | "update" | "reload" => {
            let arts = do_refresh(&cfg);
            Ok(show(&arts, None))
        }
        "" | "show" | "list" => {
            let arts = ensure_fresh(&cfg);
            Ok(show(&arts, None))
        }
        s => {
            // Số → xem chi tiết 1 bài (từ cache, không gọi mạng).
            if let Ok(n) = s.parse::<usize>() {
                let arts = read_cache().articles;
                return Ok(detail(&arts, n));
            }
            // Còn lại: coi như lọc theo chủ đề.
            let arts = ensure_fresh(&cfg);
            Ok(show(&arts, Some(s)))
        }
    }
}

fn help() {
    println!("\x1b[1mjak news — tin tức + AI tóm tắt\x1b[0m\n");
    let items: &[(&str, &str)] = &[
        ("jak news", "tin mới (tự crawl + tóm tắt nếu cache đã cũ)"),
        ("jak news refresh", "ép crawl + tóm tắt lại ngay"),
        ("jak news <chủ-đề>", "lọc theo chủ đề, vd: jak news cong-nghe"),
        ("jak news <số>", "xem chi tiết 1 bài (tiêu đề, ý chính, link)"),
        ("jak news sources", "liệt kê nguồn RSS đang dùng + cấu hình"),
    ];
    for (cmd, desc) in items {
        println!("  \x1b[36m{:24}\x1b[0m {}", cmd, desc);
    }
    println!("\n\x1b[1mCấu hình\x1b[0m \x1b[2m(~/.jakshrc.toml, mục [news])\x1b[0m");
    println!("  \x1b[2msources\x1b[0m      danh sách link RSS");
    println!("  \x1b[2mmax_items\x1b[0m    số bài tối đa mỗi lần (mặc định 20)");
    println!("  \x1b[2mttl_minutes\x1b[0m  cache tươi trong bao lâu (mặc định 30)");
    println!("  \x1b[2mmodel\x1b[0m        model Claude (mặc định claude-haiku-4-5)");
    println!("  \x1b[2mai\x1b[0m           bật/tắt phân loại + tóm tắt bằng AI");
    println!(
        "\n\x1b[2mAI cần \x1b[0m\x1b[36mANTHROPIC_API_KEY\x1b[0m\x1b[2m — \
         export trong shell hoặc đặt api_key trong [news]. \
         Thiếu key vẫn xem được tin thô.\x1b[0m"
    );
}

fn list_sources(cfg: &NewsConfig) {
    println!("\x1b[1mNguồn RSS ({}):\x1b[0m", cfg.sources.len());
    if cfg.sources.is_empty() {
        println!("  \x1b[2m(chưa có — thêm vào [news] sources trong ~/.jakshrc.toml)\x1b[0m");
    }
    for s in &cfg.sources {
        println!("  \x1b[36m{}\x1b[0m \x1b[2m({})\x1b[0m", domain_of(s), s);
    }
    let key_state = if api_key(cfg).is_some() {
        "\x1b[32m✓ có\x1b[0m"
    } else {
        "\x1b[33m✗ thiếu\x1b[0m"
    };
    println!(
        "\n  \x1b[2mmodel:\x1b[0m {}   \x1b[2mai:\x1b[0m {}   \x1b[2mmax_items:\x1b[0m {}   \x1b[2mttl:\x1b[0m {}m   \x1b[2mapi_key:\x1b[0m {}",
        cfg.model, cfg.ai, cfg.max_items, cfg.ttl_minutes, key_state
    );
}

// ─────────────────────────────── làm mới ──────────────────────────────────────

/// Trả về danh sách bài để hiển thị: dùng cache nếu còn tươi, ngược lại crawl mới.
fn ensure_fresh(cfg: &NewsConfig) -> Vec<Article> {
    let cache = read_cache();
    let ttl = cfg.ttl_minutes.saturating_mul(60);
    let fresh = !cache.articles.is_empty() && now_secs().saturating_sub(cache.last_fetch) < ttl;
    if fresh {
        return cache.articles;
    }
    do_refresh(cfg)
}

/// Crawl + (nếu có key & bật ai) phân loại/tóm tắt → ghi cache → trả về bài.
fn do_refresh(cfg: &NewsConfig) -> Vec<Article> {
    if cfg.sources.is_empty() {
        eprintln!(
            "\x1b[33m⚠ chưa cấu hình nguồn tin.\x1b[0m \x1b[2mThêm link RSS vào [news] sources trong ~/.jakshrc.toml.\x1b[0m"
        );
        return read_cache().articles;
    }
    if !has_cmd("curl") {
        eprintln!("\x1b[33m⚠ cần `curl` để lấy tin.\x1b[0m");
        return read_cache().articles;
    }

    eprintln!("\x1b[2m⟳ đang tải tin từ {} nguồn…\x1b[0m", cfg.sources.len());
    let mut arts = crawl(cfg);
    if arts.is_empty() {
        eprintln!(
            "\x1b[33m⚠ không lấy được bài nào.\x1b[0m \x1b[2m(mạng lỗi, hoặc nguồn không phải RSS hợp lệ?)\x1b[0m"
        );
        return read_cache().articles;
    }
    arts.truncate(cfg.max_items);

    if cfg.ai {
        match api_key(cfg) {
            Some(key) => {
                eprintln!(
                    "\x1b[2m⟳ đang phân loại + tóm tắt {} bài bằng {}…\x1b[0m",
                    arts.len(),
                    cfg.model
                );
                if !ai_enrich(cfg, &key, &mut arts) {
                    eprintln!(
                        "\x1b[33m⚠ AI không phản hồi — hiển thị tin thô (mô tả gốc).\x1b[0m"
                    );
                }
            }
            None => {
                eprintln!(
                    "\x1b[33m⚠ thiếu ANTHROPIC_API_KEY — bỏ qua AI.\x1b[0m \x1b[2mexport ANTHROPIC_API_KEY=… (hoặc [news] api_key) để bật tóm tắt.\x1b[0m"
                );
            }
        }
    }

    // Fallback cho bài chưa được AI gán.
    for a in arts.iter_mut() {
        if a.category.is_empty() {
            a.category = UNCLASSIFIED.to_string();
        }
        if a.points.is_empty() && !a.summary_raw.is_empty() {
            a.points = vec![truncate(&a.summary_raw, 220)];
        }
    }

    write_cache(&Cache {
        last_fetch: now_secs(),
        articles: arts.clone(),
    });
    arts
}

// ─────────────────────────────── crawl RSS ────────────────────────────────────

fn crawl(cfg: &NewsConfig) -> Vec<Article> {
    // Chia đều hạn ngạch giữa các nguồn để có sự đa dạng (nguồn lỗi không chặn
    // các nguồn khác).
    let per_source = (cfg.max_items / cfg.sources.len().max(1)).max(3);
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Article> = Vec::new();

    for src in &cfg.sources {
        let Some(xml) = curl_get(src) else {
            eprintln!("  \x1b[2m↳ bỏ qua {} (không tải được)\x1b[0m", domain_of(src));
            continue;
        };
        let source = domain_of(src);
        let mut taken = 0usize;
        for block in item_blocks(&xml) {
            if taken >= per_source {
                break;
            }
            let title = inner_text(&block, "title")
                .map(|t| strip_html(&t))
                .unwrap_or_default();
            let link = link_from_block(&block).unwrap_or_default();
            if title.is_empty() || link.is_empty() {
                continue;
            }
            if !seen.insert(link.clone()) {
                continue; // trùng link giữa các nguồn / trong cùng feed
            }
            let summary_raw = inner_text(&block, "description")
                .or_else(|| inner_text(&block, "summary"))
                .or_else(|| inner_text(&block, "content"))
                .map(|d| strip_html(&d))
                .unwrap_or_default();
            let published = inner_text(&block, "pubDate")
                .or_else(|| inner_text(&block, "published"))
                .or_else(|| inner_text(&block, "updated"))
                .map(|s| fmt_time(&strip_html(&s)))
                .unwrap_or_default();
            out.push(Article {
                id: hash_id(&link),
                title,
                link,
                source: source.clone(),
                published,
                summary_raw,
                category: String::new(),
                points: Vec::new(),
            });
            taken += 1;
        }
    }
    out
}

/// `curl -sL` lấy nội dung URL; None nếu lỗi / non-zero.
fn curl_get(url: &str) -> Option<String> {
    let out = Command::new("curl")
        .args([
            "-sL",
            "--max-time",
            "20",
            "-A",
            "jaksh-news/1.0",
            url,
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ── parser RSS/Atom tối giản, không phụ thuộc thư viện ngoài ──

/// Cắt các khối `<item>…</item>` (RSS) hoặc `<entry>…</entry>` (Atom).
fn item_blocks(xml: &str) -> Vec<String> {
    for (open, close) in [("<item", "</item>"), ("<entry", "</entry>")] {
        let mut blocks = Vec::new();
        let mut start = 0;
        while let Some(p) = xml[start..].find(open) {
            let abs = start + p;
            // Đảm bảo `<item` là tên thẻ thật, không phải tiền tố (vd <itemX>).
            let after = xml[abs + open.len()..].chars().next();
            if !matches!(
                after,
                Some('>') | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('/')
            ) {
                start = abs + open.len();
                continue;
            }
            match xml[abs..].find(close) {
                Some(e) => {
                    let end = abs + e + close.len();
                    blocks.push(xml[abs..end].to_string());
                    start = end;
                }
                None => break,
            }
        }
        if !blocks.is_empty() {
            return blocks; // RSS hoặc Atom, không trộn cả hai
        }
    }
    Vec::new()
}

/// Lấy nội dung của thẻ `<tag …>…</tag>` đầu tiên trong `block`.
fn inner_text(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let mut from = 0;
    loop {
        let pos = from + block[from..].find(&open)?;
        let rest = &block[pos + open.len()..];
        let next = rest.chars().next()?;
        // Ranh giới tên thẻ hợp lệ.
        if matches!(next, '>' | ' ' | '\t' | '\n' | '\r' | '/') {
            let gt = pos + open.len() + rest.find('>')?;
            let content_start = gt + 1;
            let close = format!("</{}>", tag);
            let end = content_start + block[content_start..].find(&close)?;
            return Some(block[content_start..end].to_string());
        }
        from = pos + open.len();
    }
}

/// Link của 1 bài: RSS `<link>URL</link>` hoặc Atom `<link href="URL" …/>`.
fn link_from_block(block: &str) -> Option<String> {
    if let Some(t) = inner_text(block, "link") {
        let t = decode_entities(t.trim());
        if !t.is_empty() {
            return Some(t);
        }
    }
    // Atom: ưu tiên rel="alternate"; nếu không, lấy href đầu tiên.
    let mut first: Option<String> = None;
    let mut from = 0;
    while let Some(p) = block[from..].find("<link") {
        let abs = from + p;
        let end = block[abs..].find('>').map(|e| abs + e).unwrap_or(block.len());
        let tag = &block[abs..end];
        if let Some(href) = attr(tag, "href") {
            let href = decode_entities(&href);
            if attr(tag, "rel").as_deref() == Some("alternate") {
                return Some(href);
            }
            if first.is_none() {
                first = Some(href);
            }
        }
        from = end;
    }
    first
}

/// Lấy giá trị attribute `name="…"` / `name='…'` từ một chuỗi thẻ mở.
fn attr(tag: &str, name: &str) -> Option<String> {
    for q in ['"', '\''] {
        let pat = format!("{}={}", name, q);
        if let Some(p) = tag.find(&pat) {
            let rest = &tag[p + pat.len()..];
            if let Some(e) = rest.find(q) {
                return Some(rest[..e].to_string());
            }
        }
    }
    None
}

/// Giải mã thực thể XML/HTML thông dụng.
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
        .replace("&nbsp;", " ")
        .replace("&hellip;", "…")
}

/// Gỡ CDATA + thẻ HTML, giải mã entity, gộp khoảng trắng.
fn strip_html(s: &str) -> String {
    let s = s.replace("<![CDATA[", "").replace("]]>", "");
    let mut out = String::new();
    let mut depth = 0u32;
    for c in s.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ─────────────────────────────── gọi AI ───────────────────────────────────────

fn api_key(cfg: &NewsConfig) -> Option<String> {
    if let Ok(k) = std::env::var("ANTHROPIC_API_KEY") {
        let k = k.trim().to_string();
        if !k.is_empty() {
            return Some(k);
        }
    }
    let k = cfg.api_key.trim();
    if k.is_empty() {
        None
    } else {
        Some(k.to_string())
    }
}

/// Gửi tất cả bài trong 1 request, nhận structured output, gán category + points.
/// Trả false nếu thất bại (caller giữ tin thô).
fn ai_enrich(cfg: &NewsConfig, key: &str, articles: &mut [Article]) -> bool {
    // Nội dung user: id + tiêu đề + mô tả (cắt bớt cho gọn token).
    let mut user = String::from(
        "Dưới đây là các bài báo cần phân loại và tóm tắt:\n\n",
    );
    for a in articles.iter() {
        user.push_str(&format!(
            "id: {}\ntiêu đề: {}\nmô tả: {}\n\n",
            a.id,
            a.title,
            truncate(&a.summary_raw, 500)
        ));
    }

    let max_tokens = (articles.len() as u64 * 260).clamp(1024, 8000);
    let body = json!({
        "model": cfg.model,
        "max_tokens": max_tokens,
        "system": [{
            "type": "text",
            "text": SYSTEM_PROMPT,
            "cache_control": {"type": "ephemeral"}
        }],
        "output_config": {"format": {"type": "json_schema", "schema": result_schema()}},
        "messages": [{"role": "user", "content": user}]
    });

    let resp = match curl_post_json(key, &body.to_string()) {
        Some(r) => r,
        None => return false,
    };
    let map = match parse_ai_response(&resp) {
        Some(m) if !m.is_empty() => m,
        _ => return false,
    };
    for a in articles.iter_mut() {
        if let Some((cat, pts)) = map.get(&a.id) {
            if !cat.is_empty() {
                a.category = cat.clone();
            }
            if !pts.is_empty() {
                a.points = pts.clone();
            }
        }
    }
    true
}

/// Parse phản hồi Claude (structured output) → map id → (chủ đề, các ý).
/// None nếu API trả lỗi, body không phải JSON, hay thiếu trường mong đợi.
fn parse_ai_response(resp: &str) -> Option<HashMap<String, (String, Vec<String>)>> {
    let v: Value = serde_json::from_str(resp).ok()?;
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("lỗi không rõ");
        eprintln!("\x1b[33m⚠ Claude API: {}\x1b[0m", msg);
        return None;
    }

    // structured output ⇒ content[0] là text chứa JSON đúng schema.
    let text = v
        .get("content")?
        .as_array()?
        .iter()
        .find(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        .and_then(|b| b.get("text"))
        .and_then(|t| t.as_str())?;
    let parsed: Value = serde_json::from_str(text).ok()?;
    let results = parsed.get("results")?.as_array()?;

    let mut map: HashMap<String, (String, Vec<String>)> = HashMap::new();
    for r in results {
        let id = r.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if id.is_empty() {
            continue;
        }
        let cat = r
            .get("category")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let pts = r
            .get("summary")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        map.insert(id, (cat, pts));
    }
    Some(map)
}

/// Bắt buộc JSON đúng cấu trúc {results:[{id,category,summary[]}]}.
fn result_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["results"],
        "properties": {
            "results": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["id", "category", "summary"],
                    "properties": {
                        "id": {"type": "string"},
                        "category": {"type": "string", "enum": CATEGORIES},
                        "summary": {"type": "array", "items": {"type": "string"}}
                    }
                }
            }
        }
    })
}

/// POST JSON tới Anthropic qua `curl`; body đẩy vào stdin (`-d @-`) tránh giới
/// hạn độ dài tham số và rắc rối escaping. None nếu curl lỗi / non-zero.
fn curl_post_json(key: &str, body: &str) -> Option<String> {
    let mut child = Command::new("curl")
        .args(["-s", "--max-time", "90", ANTHROPIC_URL])
        .args(["-H", "content-type: application/json"])
        .args(["-H", &format!("x-api-key: {}", key)])
        .args(["-H", &format!("anthropic-version: {}", ANTHROPIC_VERSION)])
        .args(["-d", "@-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    {
        // Đóng stdin (drop) để gửi EOF, nếu không curl chờ mãi.
        let mut si = child.stdin.take()?;
        si.write_all(body.as_bytes()).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ─────────────────────────────── hiển thị ─────────────────────────────────────

fn show(articles: &[Article], filter: Option<&str>) -> i32 {
    if articles.is_empty() {
        println!(
            "\x1b[2mChưa có tin. Thử \x1b[0m\x1b[36mjak news refresh\x1b[0m\x1b[2m, hoặc kiểm tra \x1b[0m\x1b[36mjak news sources\x1b[0m\x1b[2m.\x1b[0m"
        );
        return 0;
    }

    // Thứ tự chủ đề: CATEGORIES → "Chưa phân loại" → bất kỳ nhãn lạ nào còn lại.
    let mut order: Vec<String> = CATEGORIES.iter().map(|s| s.to_string()).collect();
    order.push(UNCLASSIFIED.to_string());
    for a in articles {
        if !order.iter().any(|c| c == &a.category) {
            order.push(a.category.clone());
        }
    }

    let want = filter.map(fold);
    let mut shown = 0usize;
    for cat in &order {
        if let Some(f) = &want {
            let fc = fold(cat);
            if fc != *f && !fc.starts_with(f.as_str()) {
                continue;
            }
        }
        let items: Vec<(usize, &Article)> = articles
            .iter()
            .enumerate()
            .filter(|(_, a)| &a.category == cat)
            .collect();
        if items.is_empty() {
            continue;
        }
        shown += 1;
        println!(
            "\n\x1b[1m\x1b[36m▸ {}\x1b[0m \x1b[2m({})\x1b[0m",
            cat,
            items.len()
        );
        for (i, a) in items {
            let n = i + 1;
            println!("  \x1b[2m{:>2}.\x1b[0m \x1b[1m{}\x1b[0m", n, a.title);
            let meta = meta_line(a);
            if !meta.is_empty() {
                println!("      \x1b[2m{}\x1b[0m", meta);
            }
            for p in &a.points {
                println!("      \x1b[36m•\x1b[0m {}", p);
            }
        }
    }

    if shown == 0 {
        if let Some(f) = filter {
            println!(
                "\x1b[2mKhông có tin thuộc chủ đề '{}'. Gõ \x1b[0m\x1b[36mjak news\x1b[0m\x1b[2m để xem tất cả.\x1b[0m",
                f
            );
            return 0;
        }
    }

    println!(
        "\n\x1b[2mChi tiết: \x1b[0m\x1b[36mjak news <số>\x1b[0m\x1b[2m  ·  Làm mới: \x1b[0m\x1b[36mjak news refresh\x1b[0m"
    );
    0
}

fn meta_line(a: &Article) -> String {
    match (a.source.is_empty(), a.published.is_empty()) {
        (false, false) => format!("{} · {}", a.source, a.published),
        (false, true) => a.source.clone(),
        (true, false) => a.published.clone(),
        (true, true) => String::new(),
    }
}

fn detail(articles: &[Article], n: usize) -> i32 {
    if n == 0 || n > articles.len() {
        eprintln!(
            "\x1b[33m⚠ không có bài số {}.\x1b[0m \x1b[2m(hiện có {} bài — gõ `jak news` để xem)\x1b[0m",
            n,
            articles.len()
        );
        return 1;
    }
    let a = &articles[n - 1];
    println!("\x1b[1m{}\x1b[0m", a.title);
    let meta = meta_line(a);
    if !meta.is_empty() {
        println!("\x1b[2m{}\x1b[0m", meta);
    }
    let cat = if a.category.is_empty() {
        UNCLASSIFIED
    } else {
        &a.category
    };
    println!("\x1b[2mChủ đề:\x1b[0m \x1b[36m{}\x1b[0m", cat);
    if !a.points.is_empty() {
        println!();
        for p in &a.points {
            println!("  \x1b[36m•\x1b[0m {}", p);
        }
    } else if !a.summary_raw.is_empty() {
        println!("\n  {}", truncate(&a.summary_raw, 400));
    }
    println!("\n\x1b[2mĐọc đầy đủ:\x1b[0m \x1b[4m{}\x1b[0m", a.link);
    println!(
        "\x1b[2m(mở trong trình duyệt: \x1b[0m\x1b[36mjak open {}\x1b[0m\x1b[2m)\x1b[0m",
        a.link
    );
    0
}

// ─────────────────────────────── tiện ích ─────────────────────────────────────

/// Hash ngắn, ổn định của 1 chuỗi (8 hex).
fn hash_id(s: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:08x}", (h.finish() & 0xffff_ffff) as u32)
}

/// Domain (bỏ scheme + "www.") để làm nhãn nguồn.
fn domain_of(url: &str) -> String {
    let no_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let host = no_scheme.split('/').next().unwrap_or(no_scheme);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// Cắt chuỗi theo số ký tự (không cắt giữa char), thêm "…" nếu dài hơn.
fn truncate(s: &str, max: usize) -> String {
    let mut it = s.chars();
    let head: String = it.by_ref().take(max).collect();
    if it.next().is_some() {
        format!("{}…", head.trim_end())
    } else {
        head
    }
}

/// Chuẩn hoá để so khớp chủ đề: lowercase + bỏ dấu tiếng Việt + slug bằng '-'.
/// "Công nghệ" → "cong-nghe".
fn fold(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars().flat_map(|c| c.to_lowercase()) {
        let mapped = match ch {
            'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ'
            | 'ấ' | 'ẩ' | 'ẫ' | 'ậ' => 'a',
            'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => 'e',
            'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => 'i',
            'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ'
            | 'ớ' | 'ở' | 'ỡ' | 'ợ' => 'o',
            'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => 'u',
            'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
            'đ' => 'd',
            'a'..='z' | '0'..='9' => ch,
            _ => '-',
        };
        out.push(mapped);
    }
    out.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Thời gian xuất bản → "dd/mm HH:MM" (local). Fallback: cắt raw.
fn fmt_time(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return String::new();
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(raw) {
        return dt
            .with_timezone(&chrono::Local)
            .format("%d/%m %H:%M")
            .to_string();
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        return dt
            .with_timezone(&chrono::Local)
            .format("%d/%m %H:%M")
            .to_string();
    }
    // Một số feed (vd tuoitre.vn) dùng định dạng US "M/D/YYYY h:mm:ss AM/PM".
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(raw, "%m/%d/%Y %I:%M:%S %p") {
        return ndt.format("%d/%m %H:%M").to_string();
    }
    truncate(raw, 25)
}

fn has_cmd(name: &str) -> bool {
    which::which(name).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_strips_scheme_and_www() {
        assert_eq!(domain_of("https://www.vnexpress.net/rss/x.rss"), "vnexpress.net");
        assert_eq!(domain_of("http://tuoitre.vn/a"), "tuoitre.vn");
        assert_eq!(domain_of("thanhnien.vn"), "thanhnien.vn");
    }

    #[test]
    fn fold_vietnamese_to_slug() {
        assert_eq!(fold("Công nghệ"), "cong-nghe");
        assert_eq!(fold("Thời sự"), "thoi-su");
        assert_eq!(fold("Sức khỏe"), "suc-khoe");
        assert_eq!(fold("Thế giới"), "the-gioi");
    }

    #[test]
    fn strip_html_removes_tags_cdata_entities() {
        assert_eq!(
            strip_html("<![CDATA[<p>Xin &amp; chào <b>bạn</b></p>]]>"),
            "Xin & chào bạn"
        );
        assert_eq!(strip_html("a   b\n c"), "a b c");
    }

    #[test]
    fn inner_text_first_tag_only() {
        let b = "<item><title>Tin A</title><description>mô tả</description></item>";
        assert_eq!(inner_text(b, "title").as_deref(), Some("Tin A"));
        assert_eq!(inner_text(b, "description").as_deref(), Some("mô tả"));
        assert_eq!(inner_text(b, "pubDate"), None);
    }

    #[test]
    fn link_rss_and_atom() {
        let rss = "<item><link>https://x.vn/a</link></item>";
        assert_eq!(link_from_block(rss).as_deref(), Some("https://x.vn/a"));
        let atom = r#"<entry><link rel="alternate" href="https://x.vn/b"/></entry>"#;
        assert_eq!(link_from_block(atom).as_deref(), Some("https://x.vn/b"));
    }

    #[test]
    fn item_blocks_rss() {
        let xml = "<rss><channel><title>Feed</title>\
            <item><title>A</title></item>\
            <item><title>B</title></item></channel></rss>";
        let blocks = item_blocks(xml);
        assert_eq!(blocks.len(), 2);
        assert_eq!(inner_text(&blocks[0], "title").as_deref(), Some("A"));
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("abc", 3), "abc");
        // Không panic với ký tự nhiều byte.
        let s = "ăâđêôơư";
        let _ = truncate(s, 2);
    }
}
