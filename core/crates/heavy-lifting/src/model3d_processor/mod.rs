use crate::Error;
use sd_prisma::prisma::file_path;
use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error as ThisError;

pub mod job;

#[derive(ThisError, Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "snake_case")]
pub enum NonCriticalModel3DProcessorError {
	#[error("Failed to parse 3D model: {0}")]
	ParseFailed(String),
	#[error("Failed to analyze geometry: {0}")]
	GeometryAnalysisFailed(String),
	#[error("Failed to generate thumbnail: {0}")]
	ThumbnailGenerationFailed(String),
	#[error("AI tagging failed: {0}")]
	AITaggingFailed(String),
}

#[derive(ThisError, Debug)]
pub enum Model3DProcessorError {
	#[error("database error: {0}")]
	Database(#[from] prisma_client_rust::QueryError),
	#[error("3D model processing error: {0}")]
	AI(#[from] sd_ai::model3d::Model3DError),
	#[error(transparent)]
	NonCritical(#[from] NonCriticalModel3DProcessorError),
}

impl From<Model3DProcessorError> for rspc::Error {
	fn from(e: Model3DProcessorError) -> Self {
		match e {
			Model3DProcessorError::Database(e) => {
				Self::with_cause(rspc::ErrorCode::InternalServerError, e.to_string(), e)
			}
			Model3DProcessorError::AI(e) => {
				Self::with_cause(rspc::ErrorCode::InternalServerError, e.to_string(), e)
			}
			Model3DProcessorError::NonCritical(e) => {
				Self::with_cause(rspc::ErrorCode::InternalServerError, e.to_string(), e)
			}
		}
	}
}

impl From<Model3DProcessorError> for Error {
	fn from(e: Model3DProcessorError) -> Self {
		Self::Model3DProcessor(e)
	}
}
