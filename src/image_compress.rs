//! 图片压缩模块
//!
//! 在 JSONL 入库前，将 image content block 中的 base64 图片压缩为 WebP 格式。
//! 压缩后更小才替换，任何失败都保留原始数据。

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::io::Cursor;

/// lossy WebP 编码质量（0-100）
const WEBP_QUALITY: f32 = 85.0;

/// base64 长度阈值：低于此值不压缩（约 200KB 原始数据 ≈ 270KB base64）
const MIN_BASE64_LEN: usize = 270_000;

/// 压缩 JSONL 行中的 image content block。
///
/// 扫描 `message.content[]` 中的 `type: "image"` block，
/// 将 base64 编码的图片压缩为 lossy WebP 并替换。
///
/// 返回原始行不变的情况：
/// - 没有 image block
/// - 所有压缩都失败
/// - 所有压缩结果都比原始更大
pub fn compress_images_in_line(line: &str) -> String {
    // 快速路径：99%+ 的行没有 image block
    if !line.contains("\"type\":\"image\"") && !line.contains("\"type\": \"image\"") {
        return line.to_string();
    }

    let mut json: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return line.to_string(),
    };

    // 导航到 message.content（必须是数组）
    let content_array = match json
        .get_mut("message")
        .and_then(|m| m.get_mut("content"))
        .and_then(|c| c.as_array_mut())
    {
        Some(arr) => arr,
        None => return line.to_string(),
    };

    let mut modified = false;
    let mut total_saved: i64 = 0;

    for block in content_array.iter_mut() {
        if block.get("type").and_then(|t| t.as_str()) != Some("image") {
            continue;
        }

        let source = match block.get_mut("source") {
            Some(s) => s,
            None => continue,
        };

        if source.get("type").and_then(|t| t.as_str()) != Some("base64") {
            continue;
        }

        let media_type = source
            .get("media_type")
            .and_then(|t| t.as_str())
            .unwrap_or("");

        // 已经是 webp 的跳过
        if media_type == "image/webp" {
            continue;
        }

        let data_str = match source.get("data").and_then(|d| d.as_str()) {
            Some(d) => d,
            None => continue,
        };

        match compress_base64_image(data_str) {
            Some(result) => {
                let saved = result.original_b64_len as i64 - result.compressed_b64_len as i64;
                total_saved += saved;
                source["data"] = serde_json::Value::String(result.data);
                source["media_type"] = serde_json::Value::String("image/webp".to_string());
                modified = true;
                tracing::debug!(
                    "Image compressed: {} -> {} bytes (saved {})",
                    result.original_b64_len,
                    result.compressed_b64_len,
                    saved
                );
            }
            None => continue,
        }
    }

    if !modified {
        return line.to_string();
    }

    tracing::info!(
        "Compressed images in JSONL line, total saved: {} bytes",
        total_saved
    );

    // 重新序列化（compact JSON）
    match serde_json::to_string(&json) {
        Ok(s) => s,
        Err(_) => line.to_string(),
    }
}

struct CompressResult {
    data: String,
    original_b64_len: usize,
    compressed_b64_len: usize,
}

fn compress_base64_image(base64_data: &str) -> Option<CompressResult> {
    let original_b64_len = base64_data.len();

    // 小图不压缩（< 200KB）
    if original_b64_len < MIN_BASE64_LEN {
        return None;
    }

    // 1. 解码 base64
    let raw_bytes = BASE64.decode(base64_data).ok()?;

    // 2. 解码图片
    let img = image::ImageReader::new(Cursor::new(&raw_bytes))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;

    // 3. 编码为 lossy WebP
    let encoder = webp::Encoder::from_image(&img).ok()?;
    let webp_data = encoder.encode(WEBP_QUALITY);

    // 4. 重新编码为 base64
    let new_b64 = BASE64.encode(&*webp_data);

    // 5. 比较大小：更小才替换
    if new_b64.len() >= original_b64_len {
        return None;
    }

    Some(CompressResult {
        data: new_b64.clone(),
        original_b64_len,
        compressed_b64_len: new_b64.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成一个大 PNG（超过 MIN_BASE64_LEN 阈值）用于压缩测试
    fn make_large_test_png() -> String {
        use image::codecs::png::PngEncoder;
        use image::{ImageEncoder, RgbaImage};

        // 1000x1000 随机色块，确保 base64 > 270KB
        let mut img = RgbaImage::new(1000, 1000);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]);
        }
        let mut buf = Vec::new();
        let encoder = PngEncoder::new(&mut buf);
        encoder
            .write_image(img.as_raw(), 1000, 1000, image::ExtendedColorType::Rgba8)
            .unwrap();
        let b64 = BASE64.encode(&buf);
        assert!(b64.len() > MIN_BASE64_LEN, "test PNG must exceed threshold");
        b64
    }

    /// 生成一个小 PNG（低于阈值）
    fn make_small_test_png() -> String {
        use image::codecs::png::PngEncoder;
        use image::{ImageEncoder, RgbaImage};

        let img = RgbaImage::from_pixel(10, 10, image::Rgba([255, 0, 0, 255]));
        let mut buf = Vec::new();
        let encoder = PngEncoder::new(&mut buf);
        encoder
            .write_image(img.as_raw(), 10, 10, image::ExtendedColorType::Rgba8)
            .unwrap();
        BASE64.encode(&buf)
    }

    fn make_jsonl_with_image(b64: &str, media_type: &str) -> String {
        format!(
            r#"{{"type":"user","uuid":"test-123","message":{{"role":"user","content":[{{"type":"text","text":"看这张图"}},{{"type":"image","source":{{"type":"base64","media_type":"{}","data":"{}"}}}}]}}}}"#,
            media_type, b64
        )
    }

    #[test]
    fn no_image_passthrough() {
        let line = r#"{"type":"user","message":{"content":"hello"}}"#;
        let result = compress_images_in_line(line);
        assert_eq!(result, line);
    }

    #[test]
    fn no_image_keyword_fast_path() {
        // 不含 "type":"image" 的行直接返回
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ok"}]}}"#;
        let result = compress_images_in_line(line);
        assert_eq!(result, line);
    }

    #[test]
    fn compress_png_to_webp() {
        let png_b64 = make_large_test_png();
        let line = make_jsonl_with_image(&png_b64, "image/png");

        let result = compress_images_in_line(&line);
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();

        let block = &json["message"]["content"][1];
        let media = block["source"]["media_type"].as_str().unwrap();
        assert_eq!(media, "image/webp");

        let new_data = block["source"]["data"].as_str().unwrap();
        assert!(
            new_data.len() < png_b64.len(),
            "WebP ({}) should be smaller than PNG ({})",
            new_data.len(),
            png_b64.len()
        );

        // 验证 WebP base64 可以解码
        let decoded = BASE64.decode(new_data).unwrap();
        assert!(decoded.starts_with(b"RIFF"));
    }

    #[test]
    fn skip_already_webp() {
        let line = r#"{"type":"user","message":{"content":[{"type":"image","source":{"type":"base64","media_type":"image/webp","data":"dGVzdA=="}}]}}"#;
        let result = compress_images_in_line(line);
        // 不应修改已经是 webp 的
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let media = json["message"]["content"][0]["source"]["media_type"]
            .as_str()
            .unwrap();
        assert_eq!(media, "image/webp");
    }

    #[test]
    fn small_image_skip() {
        let small_b64 = make_small_test_png();
        let line = make_jsonl_with_image(&small_b64, "image/png");
        let result = compress_images_in_line(&line);
        // 小图不压缩，保持 PNG
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let media = json["message"]["content"][1]["source"]["media_type"]
            .as_str()
            .unwrap();
        assert_eq!(media, "image/png");
    }

    #[test]
    fn invalid_base64_passthrough() {
        let line = make_jsonl_with_image("not-valid-base64!!!", "image/png");
        let result = compress_images_in_line(&line);
        // 无法解码，保留原始数据
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let data = json["message"]["content"][1]["source"]["data"]
            .as_str()
            .unwrap();
        assert_eq!(data, "not-valid-base64!!!");
        let media = json["message"]["content"][1]["source"]["media_type"]
            .as_str()
            .unwrap();
        assert_eq!(media, "image/png"); // 未改变
    }

    #[test]
    fn malformed_json_passthrough() {
        // 包含 image 关键字但不是合法 JSON
        let line = r#"{"type":"image" this is broken"#;
        let result = compress_images_in_line(line);
        assert_eq!(result, line);
    }

    #[test]
    fn no_message_content_passthrough() {
        // 有 image 关键字但不在 message.content 路径下
        let line = r#"{"type":"image","data":"something"}"#;
        let result = compress_images_in_line(line);
        assert_eq!(result, line);
    }

    #[test]
    fn text_content_not_array() {
        // content 是字符串不是数组
        let line =
            r#"{"type":"user","message":{"content":"just text with type:\"image\" keyword"}}"#;
        let result = compress_images_in_line(line);
        assert_eq!(result, line);
    }

    #[test]
    fn preserves_other_fields() {
        let png_b64 = make_large_test_png();
        let line = format!(
            r#"{{"type":"user","uuid":"abc-123","timestamp":"2026-01-01T00:00:00Z","message":{{"role":"user","content":[{{"type":"text","text":"hello"}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{}"}}}}]}}}}"#,
            png_b64
        );

        let result = compress_images_in_line(&line);
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();

        // 其他字段完整保留
        assert_eq!(json["uuid"].as_str().unwrap(), "abc-123");
        assert_eq!(json["timestamp"].as_str().unwrap(), "2026-01-01T00:00:00Z");
        assert_eq!(
            json["message"]["content"][0]["text"].as_str().unwrap(),
            "hello"
        );
    }

    #[test]
    fn multiple_images() {
        let png_b64 = make_large_test_png();
        let line = format!(
            r#"{{"type":"user","message":{{"content":[{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{}"}}}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"{}"}}}}]}}}}"#,
            png_b64, png_b64
        );

        let result = compress_images_in_line(&line);
        let json: serde_json::Value = serde_json::from_str(&result).unwrap();
        let blocks = json["message"]["content"].as_array().unwrap();

        // 两个都应该被压缩
        for block in blocks {
            assert_eq!(
                block["source"]["media_type"].as_str().unwrap(),
                "image/webp"
            );
        }
    }
}
