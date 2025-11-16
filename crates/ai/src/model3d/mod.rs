use sd_prisma::prisma::file_path;
use sd_utils::{db::MissingFieldError, error::FileIOError};

use std::path::Path;

use thiserror::Error;
use tracing::{debug, error, info};

pub mod geometry;
pub mod parser;
pub mod thumbnailer;
pub mod ai_tagger;

pub use geometry::{GeometryAnalyzer, GeometryFeatures};
pub use parser::{MeshParser, MeshData, ParsedMesh};
pub use thumbnailer::{Thumbnailer3D, ThumbnailConfig};
pub use ai_tagger::{AITagger, AITagResult};

#[derive(Debug)]
pub struct Model3DOutput {
	pub file_path_id: file_path::id::Type,
	pub features: Option<GeometryFeatures>,
	pub thumbnail_path: Option<String>,
	pub ai_tags: Option<AITagResult>,
	pub result: Result<(), Model3DError>,
}

#[derive(Debug, Error)]
pub enum Model3DError {
	#[error("failed to parse 3D model: {0}")]
	ParseFailed(String),
	#[error("unsupported 3D format: {0}")]
	UnsupportedFormat(String),
	#[error("geometry analysis failed: {0}")]
	GeometryAnalysisFailed(String),
	#[error("thumbnail generation failed: {0}")]
	ThumbnailGenerationFailed(String),
	#[error("AI tagging failed: {0}")]
	AITaggingFailed(String),
	#[error("file_path with unsupported extension: <id='{0}', extension='{1}'>")]
	UnsupportedExtension(file_path::id::Type, String),
	#[error("file_path too big: <id='{0}', size='{1}'>")]
	FileTooBig(file_path::id::Type, usize),
	#[error("failed to get isolated file path data: {0}")]
	IsolateFilePathData(#[from] MissingFieldError),
	#[error("database error: {0}")]
	Database(#[from] prisma_client_rust::QueryError),
	#[error(transparent)]
	FileIO(#[from] FileIOError),
	#[error("IO error: {0}")]
	Io(#[from] std::io::Error),
}
