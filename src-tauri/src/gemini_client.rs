use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::Client;
use image::{DynamicImage, GrayImage, RgbImage};
use serde::{Deserialize, Serialize};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const GEMINI_IMAGE_MODEL: &str = "gemini-3.1-flash-image";

#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<Content>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Debug, Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum Part {
    Text { text: String },
    InlineData { inline_data: InlineData },
}

#[derive(Debug, Serialize)]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct GenerationConfig {
    #[serde(rename = "responseModalities")]
    response_modalities: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
    error: Option<GeminiError>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Debug, Deserialize)]
struct CandidateContent {
    parts: Option<Vec<ResponsePart>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ResponsePart {
    #[serde(rename = "inlineData")]
    inline_data: Option<ResponseInlineData>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ResponseInlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

#[derive(Debug, Deserialize)]
struct GeminiError {
    message: String,
    status: Option<String>,
}

pub struct GeminiClient {
    api_key: String,
    client: Client,
}

impl GeminiClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: Client::new(),
        }
    }

    /// Fills the white pixels of `mask` in `crop`. Only the crop is sent; the
    /// caller composites the answer back onto the full-resolution original.
    pub async fn inpaint(&self, crop: &RgbImage, mask: &GrayImage) -> Result<RgbImage, String> {
        let prompt = "The first image is a crop of a photo. The second image is a mask of the same size: \
            white pixels mark a watermark or overlay. Return the first image with ONLY the white-masked \
            pixels replaced by background that naturally continues the surrounding content. Keep the \
            framing, size, colours and every unmasked pixel unchanged. Do not add any text.";
        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![
                    Part::InlineData { inline_data: png_part(&DynamicImage::ImageRgb8(crop.clone()))? },
                    Part::InlineData { inline_data: png_part(&DynamicImage::ImageLuma8(mask.clone()))? },
                    Part::Text { text: prompt.to_string() },
                ],
            }],
            generation_config: GenerationConfig {
                response_modalities: vec!["TEXT".to_string(), "IMAGE".to_string()],
            },
        };
        let url = format!("{}/models/{}:generateContent", GEMINI_API_BASE, GEMINI_IMAGE_MODEL);
        let response = self
            .client
            .post(&url)
            .header("x-goog-api-key", &self.api_key)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("API request failed: {}", e.without_url()))?;
        let status = response.status();
        let text = response.text().await.map_err(|e| format!("Failed to read response: {}", e))?;
        if !status.is_success() {
            return Err(format!("API returned error status {}: {}", status, text));
        }
        let bytes = extract_image(&text)?;
        Ok(image::load_from_memory(&bytes)
            .map_err(|e| format!("Failed to decode response image: {}", e))?
            .to_rgb8())
    }

    pub async fn test_connection(&self) -> Result<bool, String> {
        let url = format!("{}/models", GEMINI_API_BASE);

        let response = self
            .client
            .get(&url)
            .header("x-goog-api-key", &self.api_key)
            .send()
            .await
            .map_err(|e| format!("Connection test failed: {}", e))?;

        if response.status().is_success() {
            Ok(true)
        } else {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            Err(format!("API key validation failed ({}): {}", status, text))
        }
    }

    pub async fn list_models(&self) -> Result<Vec<String>, String> {
        let url = format!("{}/models", GEMINI_API_BASE);

        let response = self
            .client
            .get(&url)
            .header("x-goog-api-key", &self.api_key)
            .send()
            .await
            .map_err(|e| format!("Failed to list models: {}", e))?;

        let text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        // Parse and extract model names
        let json: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse response: {}", e))?;

        let models = json["models"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                    .filter(|name| name.contains("gemini") || name.contains("imagen"))
                    .collect()
            })
            .unwrap_or_default();

        Ok(models)
    }
}

fn png_part(img: &DynamicImage) -> Result<InlineData, String> {
    let bytes = crate::removal::codec::encode(img, crate::removal::codec::OutputFormat::Png)?;
    Ok(InlineData { mime_type: "image/png".to_string(), data: BASE64.encode(bytes) })
}

fn extract_image(response_text: &str) -> Result<Vec<u8>, String> {
    let response: GeminiResponse = serde_json::from_str(response_text)
        .map_err(|e| format!("Failed to parse response: {}", e))?;
    if let Some(error) = response.error {
        return Err(format!("Gemini API error: {} (status: {})", error.message, error.status.unwrap_or_default()));
    }
    let parts = response
        .candidates
        .and_then(|c| c.into_iter().next())
        .and_then(|c| c.content)
        .and_then(|c| c.parts)
        .ok_or("Gemini returned no content")?;
    let mut texts = Vec::new();
    for part in parts {
        if let Some(inline) = part.inline_data {
            return BASE64.decode(&inline.data).map_err(|e| format!("Failed to decode response image: {}", e));
        }
        if let Some(t) = part.text {
            texts.push(t);
        }
    }
    Err(if texts.is_empty() {
        "Gemini returned no image".to_string()
    } else {
        format!("Gemini returned no image: {}", texts.join(" "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_only_reply_surfaces_model_text() {
        let body = r#"{"candidates":[{"content":{"parts":[{"text":"I can't help remove watermarks."}]}}]}"#;
        let err = extract_image(body).unwrap_err();
        assert!(err.contains("I can't help remove watermarks."), "{err}");
    }

    #[test]
    fn image_reply_is_decoded() {
        let body = format!(
            r#"{{"candidates":[{{"content":{{"parts":[{{"inlineData":{{"mimeType":"image/png","data":"{}"}}}}]}}}}]}}"#,
            BASE64.encode([1u8, 2, 3])
        );
        assert_eq!(extract_image(&body).unwrap(), vec![1, 2, 3]);
    }
}
