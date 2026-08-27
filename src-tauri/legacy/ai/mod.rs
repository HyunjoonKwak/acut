use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiClassification {
    pub category: String,
    pub tags: Vec<String>,
    pub description: String,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlbumSuggestion {
    pub name: String,
    pub description: String,
    pub media_ids: Vec<String>,
}

pub async fn classify_image(
    api_key: &str,
    image_base64: &str,
) -> Result<AiClassification, String> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "model": "claude-sonnet-4-20250514",
        "max_tokens": 1024,
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/jpeg",
                        "data": image_base64
                    }
                },
                {
                    "type": "text",
                    "text": "Analyze this image and return a JSON object with: category (landscape/portrait/food/animal/document/screenshot/architecture/nature/event/other), tags (array of descriptive keywords), description (one sentence), confidence (0-1). Return ONLY valid JSON."
                }
            ]
        }]
    });

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("API request failed: {}", e))?;

    let result: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let text = result["content"][0]["text"]
        .as_str()
        .ok_or("No text in response")?;

    // Extract JSON from response (handle markdown code blocks)
    let json_str = if text.contains("```") {
        text.split("```")
            .nth(1)
            .unwrap_or(text)
            .trim_start_matches("json")
            .trim()
    } else {
        text.trim()
    };

    serde_json::from_str::<AiClassification>(json_str)
        .map_err(|e| format!("Failed to parse AI response: {}", e))
}
