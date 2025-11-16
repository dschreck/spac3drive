use crate::{
	job_system::{
		job::{Job, JobReturn, JobTaskDispatcher, ReturnStatus},
		report::ReportOutputMetadata,
		DispatcherError, JobErrorOrDispatcherError, SerializableJob, SerializedTasks,
	},
	model3d_processor::Model3DProcessorError,
	Error, JobContext, JobName, OuterContext, ProgressUpdate,
};

use sd_ai::model3d::{GeometryAnalyzer, MeshParser, Thumbnailer3D, ThumbnailConfig, AITagger};
use sd_core_file_path_helper::IsolatedFilePathData;
use sd_file_ext::extensions::{Extension, MeshExtension};
use sd_prisma::{
	prisma::{file_path, location, PrismaClient},
	prisma_sync,
};
use sd_sync::{sync_db_not_null_entry, OperationFactory};
use sd_task_system::{TaskDispatcher, TaskOutput, TaskStatus};
use sd_utils::db::maybe_missing;

use std::{
	collections::HashMap,
	fmt,
	path::PathBuf,
	sync::Arc,
};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

#[derive(Debug, Serialize, Deserialize)]
pub struct Model3DProcessor {
	location: Arc<location::Data>,
	location_path: Arc<PathBuf>,
	sub_path: Option<PathBuf>,
	enable_ai_tagging: bool,
}

impl Model3DProcessor {
	pub fn new(
		location: location::Data,
		location_path: impl Into<PathBuf>,
		sub_path: Option<PathBuf>,
		enable_ai_tagging: bool,
	) -> Self {
		Self {
			location: Arc::new(location),
			location_path: Arc::new(location_path.into()),
			sub_path,
			enable_ai_tagging,
		}
	}
}

#[derive(Debug, Serialize, Deserialize)]
struct SaveState {
	processed_count: usize,
}

impl Job for Model3DProcessor {
	const NAME: JobName = JobName::Model3DProcessor;

	async fn resume_tasks<OuterCtx: OuterContext, JobCtx: JobContext<OuterCtx>>(
		&mut self,
		_dispatcher: &JobTaskDispatcher,
		_ctx: &JobCtx,
		_serialized_tasks: SerializedTasks,
	) -> Result<(), Error> {
		// TODO: Implement task resumption
		Ok(())
	}

	async fn run<OuterCtx: OuterContext, JobCtx: JobContext<OuterCtx>>(
		mut self,
		dispatcher: JobTaskDispatcher,
		ctx: &JobCtx,
	) -> Result<ReturnStatus, Error> {
		info!(
			"Starting 3D model processing for location: {}",
			self.location.id
		);

		let db = ctx.db();

		// Find all 3D model files in the location
		let file_paths = find_3d_model_files(
			db,
			self.location.id,
			self.sub_path.as_ref(),
		)
		.await?;

		let total_files = file_paths.len();
		info!("Found {} 3D model files to process", total_files);

		let mut processed_count = 0;

		for file_path_data in file_paths {
			// Parse isolated file path
			let iso_file_path = match IsolatedFilePathData::try_from(&file_path_data) {
				Ok(iso) => iso,
				Err(e) => {
					warn!("Failed to get isolated file path: {}", e);
					continue;
				}
			};

			let full_path = self.location_path.join(&iso_file_path);
			info!("Processing 3D model: {}", full_path.display());

			// Process the 3D model
			match process_3d_model(
				db,
				&file_path_data,
				&full_path,
				self.enable_ai_tagging,
				&ctx.sync(),
			)
			.await
			{
				Ok(_) => {
					processed_count += 1;
					debug!("Successfully processed: {}", full_path.display());
				}
				Err(e) => {
					error!("Failed to process {}: {}", full_path.display(), e);
				}
			}

			// Update progress
			ctx.progress(vec![ProgressUpdate::TaskCount(processed_count)]);
		}

		info!(
			"3D model processing complete: {}/{} files processed",
			processed_count, total_files
		);

		Ok(ReturnStatus::Completed(
			JobReturn::default()
				.with_metadata(ReportOutputMetadata::Model3DProcessor {
					processed: processed_count as u64,
					total: total_files as u64,
				}),
		))
	}
}

impl SerializableJob for Model3DProcessor {
	async fn serialize(self) -> Result<Option<Vec<u8>>, rmp_serde::encode::Error> {
		rmp_serde::to_vec_named(&self).map(Some)
	}

	async fn deserialize(
		serialized_job: &[u8],
		_: &impl OuterContext,
	) -> Result<Option<(Self, Option<SerializedTasks>)>, rmp_serde::decode::Error> {
		rmp_serde::from_slice(serialized_job).map(|job| Some((job, None)))
	}
}

/// Find all 3D model files in a location
async fn find_3d_model_files(
	db: &PrismaClient,
	location_id: location::id::Type,
	sub_path: Option<&PathBuf>,
) -> Result<Vec<file_path::Data>, Model3DProcessorError> {
	let mut query = db
		.file_path()
		.find_many(vec![
			file_path::location_id::equals(Some(location_id)),
			file_path::extension::in_vec(vec![
				"stl".to_string(),
				"obj".to_string(),
				"fbx".to_string(),
				"3mf".to_string(),
				"ply".to_string(),
				"gltf".to_string(),
				"glb".to_string(),
				"dae".to_string(),
				"blend".to_string(),
				"step".to_string(),
				"stp".to_string(),
			]),
		]);

	if let Some(sub_path) = sub_path {
		if let Some(sub_path_str) = sub_path.to_str() {
			query = query.and_where(vec![
				file_path::materialized_path::starts_with(sub_path_str.to_string())
			]);
		}
	}

	Ok(query.exec().await?)
}

/// Process a single 3D model file
async fn process_3d_model(
	db: &PrismaClient,
	file_path_data: &file_path::Data,
	full_path: &PathBuf,
	enable_ai_tagging: bool,
	sync: &sd_core_sync::Manager,
) -> Result<(), Model3DProcessorError> {
	// Parse the 3D model
	let parsed_mesh = MeshParser::parse(full_path).await?;

	// Analyze geometry
	let features = GeometryAnalyzer::analyze(&parsed_mesh.data)?;

	// Generate thumbnail
	let thumbnail_dir = full_path
		.parent()
		.ok_or_else(|| {
			Model3DProcessorError::AI(sd_ai::model3d::Model3DError::ParseFailed(
				"No parent directory".to_string(),
			))
		})?
		.join(".thumbnails");

	let thumbnail_path = thumbnail_dir.join(format!(
		"{}.png",
		full_path.file_stem().unwrap_or_default().to_string_lossy()
	));

	let config = ThumbnailConfig::default();
	Thumbnailer3D::generate(&parsed_mesh.data, &features, &thumbnail_path, &config).await?;

	// AI tagging (optional)
	let ai_tags = if enable_ai_tagging {
		Some(AITagger::tag_model(&[&thumbnail_path], true).await?)
	} else {
		None
	};

	// Get or create object
	let object_id = maybe_missing(file_path_data.object_id, "file_path.object_id")?;

	// Store metadata in database
	let _model_data = db
		.model_3_d_data()
		.upsert(
			sd_prisma::prisma::model_3_d_data::object_id::equals(object_id),
			(
				sd_prisma::prisma::object::id::equals(object_id),
				vec![],
			),
			vec![
				sd_prisma::prisma::model_3_d_data::vertices_count::set(Some(
					features.vertices_count as i64,
				)),
				sd_prisma::prisma::model_3_d_data::faces_count::set(Some(
					features.faces_count as i64,
				)),
				sd_prisma::prisma::model_3_d_data::edges_count::set(Some(
					features.edges_count as i64,
				)),
				sd_prisma::prisma::model_3_d_data::bbox_min_x::set(Some(
					features.bbox_min[0] as f64,
				)),
				sd_prisma::prisma::model_3_d_data::bbox_min_y::set(Some(
					features.bbox_min[1] as f64,
				)),
				sd_prisma::prisma::model_3_d_data::bbox_min_z::set(Some(
					features.bbox_min[2] as f64,
				)),
				sd_prisma::prisma::model_3_d_data::bbox_max_x::set(Some(
					features.bbox_max[0] as f64,
				)),
				sd_prisma::prisma::model_3_d_data::bbox_max_y::set(Some(
					features.bbox_max[1] as f64,
				)),
				sd_prisma::prisma::model_3_d_data::bbox_max_z::set(Some(
					features.bbox_max[2] as f64,
				)),
				sd_prisma::prisma::model_3_d_data::volume::set(Some(features.volume)),
				sd_prisma::prisma::model_3_d_data::surface_area::set(Some(
					features.surface_area,
				)),
				sd_prisma::prisma::model_3_d_data::is_manifold::set(Some(features.is_manifold)),
				sd_prisma::prisma::model_3_d_data::is_closed::set(Some(features.is_closed)),
				sd_prisma::prisma::model_3_d_data::has_normals::set(Some(features.has_normals)),
				sd_prisma::prisma::model_3_d_data::has_uvs::set(Some(features.has_uvs)),
				sd_prisma::prisma::model_3_d_data::has_colors::set(Some(features.has_colors)),
				sd_prisma::prisma::model_3_d_data::complexity_score::set(Some(
					features.complexity_score as f64,
				)),
				sd_prisma::prisma::model_3_d_data::materials_count::set(Some(
					parsed_mesh.materials_count as i32,
				)),
				sd_prisma::prisma::model_3_d_data::textures_count::set(Some(
					parsed_mesh.textures_count as i32,
				)),
			],
		)
		.exec()
		.await?;

	// Add AI tags if available
	if let Some(tags) = ai_tags {
		db.model_3_d_data()
			.update(
				sd_prisma::prisma::model_3_d_data::object_id::equals(object_id),
				vec![
					sd_prisma::prisma::model_3_d_data::ai_tags::set(Some(
						tags.tags.join(", "),
					)),
					sd_prisma::prisma::model_3_d_data::ai_description::set(Some(
						tags.description,
					)),
					sd_prisma::prisma::model_3_d_data::ai_category::set(Some(tags.category)),
				],
			)
			.exec()
			.await?;
	}

	info!("Successfully stored 3D model metadata for object {}", object_id);

	Ok(())
}
