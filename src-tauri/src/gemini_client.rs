use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::image_processor::WatermarkRegion;

const GEMINI_API_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-image:generateContent";

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
struct ResponsePart {
    #[serde(rename = "inlineData")]
    inline_data: Option<ResponseInlineData>,
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
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

    pub async fn remove_watermark(
        &self,
        image_path: &str,
        region: &WatermarkRegion,
    ) -> Result<Vec<u8>, String> {
        // Read and encode image
        let image_bytes = fs::read(image_path)
            .map_err(|e| format!("Failed to read image: {}", e))?;
        let image_base64 = BASE64.encode(&image_bytes);

        // Determine mime type
        let extension = Path::new(image_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("png")
            .to_lowercase();

        let mime_type = match extension.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            _ => "image/png",
        };

        // Build prompt with region information
        let prompt = format!(
            "Remove the watermark or unwanted element from this image. \
            The watermark is located at position x={}, y={} with width={} and height={}. \
            Seamlessly fill the area with appropriate background content that matches the surrounding pixels. \
            Return only the edited image without any text response.",
            region.x, region.y, region.width, region.height
        );

        // Build request
        let request = GeminiRequest {
            contents: vec![Content {
                parts: vec![
                    Part::InlineData {
                        inline_data: InlineData {
                            mime_type: mime_type.to_string(),
                            data: image_base64,
                        },
                    },
                    Part::Text { text: prompt },
                ],
            }],
            generation_config: GenerationConfig {
                response_modalities: vec!["TEXT".to_string(), "IMAGE".to_string()],
            },
        };

        // Make API request
        let url = format!("{}?key={}", GEMINI_API_URL, self.api_key);

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("API request failed: {}", e))?;

        let status = response.status();
        let response_text = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        if !status.is_success() {
            return Err(format!("API returned error status {}: {}", status, response_text));
        }

        let gemini_response: GeminiResponse = serde_json::from_str(&response_text)
            .map_err(|e| format!("Failed to parse response: {}. Response: {}", e, response_text))?;

        // Check for API error
        if let Some(error) = gemini_response.error {
            return Err(format!(
                "Gemini API error: {} (status: {})",
                error.message,
                error.status.unwrap_or_default()
            ));
        }

        // Extract image from response
        let candidates = gemini_response
            .candidates
            .ok_or("No candidates in response")?;

        let candidate = candidates.first().ok_or("Empty candidates array")?;

        let content = candidate
            .content
            .as_ref()
            .ok_or("No content in candidate")?;

        let parts = content
            .parts
            .as_ref()
            .ok_or("No parts in content")?;

        // Find the image part
        for part in parts {
            if let Some(inline_data) = &part.inline_data {
                let image_bytes = BASE64
                    .decode(&inline_data.data)
                    .map_err(|e| format!("Failed to decode response image: {}", e))?;
                return Ok(image_bytes);
            }
        }

        Err("No image found in API response".to_string())
    }

    pub async fn test_connection(&self) -> Result<bool, String> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models?key={}",
            self.api_key
        );

        let response = self
            .client
            .get(&url)
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
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models?key={}",
            self.api_key
        );

        let response = self
            .client
            .get(&url)
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
