use super::Model3DError;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info, warn};

/// Result of AI tagging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AITagResult {
	pub tags: Vec<String>,
	pub description: String,
	pub category: String,
	pub confidence: f32,
}

impl AITagResult {
	pub fn new() -> Self {
		Self {
			tags: Vec::new(),
			description: String::new(),
			category: String::from("unknown"),
			confidence: 0.0,
		}
	}
}

impl Default for AITagResult {
	fn default() -> Self {
		Self::new()
	}
}

/// AI-powered semantic tagger for 3D models
pub struct AITagger;

impl AITagger {
	/// Generate semantic tags using local AI model (Ollama)
	pub async fn tag_from_renders(thumbnail_paths: &[impl AsRef<Path>]) -> Result<AITagResult, Model3DError> {
		info!("Generating AI tags from {} rendered views", thumbnail_paths.len());

		// TODO: Implement actual Ollama/CLIP integration
		// For now, return a placeholder result
		Self::generate_heuristic_tags(thumbnail_paths).await
	}

	/// Generate tags using local vision model via Ollama
	async fn call_ollama_vision(image_path: &Path, prompt: &str) -> Result<String, Model3DError> {
		use reqwest::Client;
		use serde_json::json;

		debug!("Calling Ollama vision model for {}", image_path.display());

		// Ollama API endpoint (default local installation)
		let ollama_url = std::env::var("OLLAMA_URL")
			.unwrap_or_else(|_| "http://localhost:11434".to_string());

		// Use Llama 3.2 Vision model (or fallback to llava)
		let model = std::env::var("OLLAMA_MODEL")
			.unwrap_or_else(|_| "llama3.2-vision".to_string());

		// Read image and encode as base64
		let image_data = tokio::fs::read(image_path).await?;
		let image_base64 = base64::encode(&image_data);

		let client = Client::new();
		let response = client
			.post(format!("{}/api/generate", ollama_url))
			.json(&json!({
				"model": model,
				"prompt": prompt,
				"images": [image_base64],
				"stream": false
			}))
			.send()
			.await
			.map_err(|e| {
				Model3DError::AITaggingFailed(format!("Failed to call Ollama API: {}", e))
			})?;

		if !response.status().is_success() {
			return Err(Model3DError::AITaggingFailed(format!(
				"Ollama API returned error: {}",
				response.status()
			)));
		}

		let result: serde_json::Value = response.json().await.map_err(|e| {
			Model3DError::AITaggingFailed(format!("Failed to parse Ollama response: {}", e))
		})?;

		let text = result["response"]
			.as_str()
			.ok_or_else(|| Model3DError::AITaggingFailed("No response from Ollama".to_string()))?
			.to_string();

		Ok(text)
	}

	/// Generate heuristic tags (fallback when AI is not available)
	async fn generate_heuristic_tags(_thumbnail_paths: &[impl AsRef<Path>]) -> Result<AITagResult, Model3DError> {
		warn!("Using heuristic tagging (AI integration not yet complete)");

		// Simple heuristic tags based on file existence
		Ok(AITagResult {
			tags: vec![
				"3d-model".to_string(),
				"mesh".to_string(),
				"geometry".to_string(),
			],
			description: "3D model file".to_string(),
			category: "model".to_string(),
			confidence: 0.5,
		})
	}

	/// Main tagging function with fallback
	pub async fn tag_model(
		thumbnail_paths: &[impl AsRef<Path>],
		enable_ai: bool,
	) -> Result<AITagResult, Model3DError> {
		if !enable_ai {
			return Self::generate_heuristic_tags(thumbnail_paths).await;
		}

		// Try AI tagging first
		match Self::tag_with_ollama(thumbnail_paths).await {
			Ok(result) => Ok(result),
			Err(e) => {
				warn!("AI tagging failed: {}, falling back to heuristics", e);
				Self::generate_heuristic_tags(thumbnail_paths).await
			}
		}
	}

	/// Tag using Ollama vision model
	async fn tag_with_ollama(thumbnail_paths: &[impl AsRef<Path>]) -> Result<AITagResult, Model3DError> {
		if thumbnail_paths.is_empty() {
			return Err(Model3DError::AITaggingFailed(
				"No thumbnails provided".to_string(),
			));
		}

		// Use the first thumbnail (or perspective view if available)
		let thumbnail_path = thumbnail_paths[0].as_ref();

		// Generate description
		let description_prompt = "Describe this 3D model in one sentence. \
			Focus on what object it represents and its key characteristics.";
		let description = Self::call_ollama_vision(thumbnail_path, description_prompt).await?;

		// Generate tags
		let tags_prompt = "List 5-7 relevant tags for this 3D model, separated by commas. \
			Include object type, style, complexity, and intended use.";
		let tags_text = Self::call_ollama_vision(thumbnail_path, tags_prompt).await?;
		let tags: Vec<String> = tags_text
			.split(',')
			.map(|s| s.trim().to_lowercase())
			.filter(|s| !s.is_empty())
			.collect();

		// Generate category
		let category_prompt = "Classify this 3D model into one category: \
			mechanical, organic, architectural, character, vehicle, furniture, or abstract.";
		let category = Self::call_ollama_vision(thumbnail_path, category_prompt)
			.await?
			.trim()
			.to_lowercase();

		info!(
			"AI tagging complete: {} tags, category: {}",
			tags.len(),
			category
		);

		Ok(AITagResult {
			tags,
			description,
			category,
			confidence: 0.8, // Assume good confidence when AI succeeds
		})
	}
}

// Example of how to use CLIP for image-based similarity search
/*
use rust_bert::pipelines::zero_shot_classification::ZeroShotClassificationModel;

pub async fn tag_with_clip(image_path: &Path) -> Result<Vec<String>, Model3DError> {
    // CLIP-based zero-shot classification
    let model = ZeroShotClassificationModel::new(Default::default())?;

    let candidate_labels = vec![
        "mechanical part",
        "organic shape",
        "architectural model",
        "character",
        "vehicle",
        "furniture",
        "tool",
        "decoration",
    ];

    let results = model.predict(
        &[image_path.to_str().unwrap()],
        candidate_labels,
        None,
        128
    );

    Ok(results.into_iter()
        .filter(|r| r.score > 0.3)
        .map(|r| r.label)
        .collect())
}
*/
