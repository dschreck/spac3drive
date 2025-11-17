use super::{parser::MeshData, Model3DError};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

/// Geometry features extracted from a 3D model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeometryFeatures {
	// Counts
	pub vertices_count: u64,
	pub faces_count: u64,
	pub edges_count: u64,

	// Bounding box
	pub bbox_min: [f32; 3],
	pub bbox_max: [f32; 3],

	// Physical properties
	pub volume: f64,
	pub surface_area: f64,

	// Quality metrics
	pub is_manifold: bool,
	pub is_closed: bool,
	pub has_normals: bool,
	pub has_uvs: bool,
	pub has_colors: bool,

	// Complexity
	pub complexity_score: f32,
}

impl GeometryFeatures {
	pub fn bbox_center(&self) -> [f32; 3] {
		[
			(self.bbox_min[0] + self.bbox_max[0]) / 2.0,
			(self.bbox_min[1] + self.bbox_max[1]) / 2.0,
			(self.bbox_min[2] + self.bbox_max[2]) / 2.0,
		]
	}

	pub fn bbox_size(&self) -> [f32; 3] {
		[
			self.bbox_max[0] - self.bbox_min[0],
			self.bbox_max[1] - self.bbox_min[1],
			self.bbox_max[2] - self.bbox_min[2],
		]
	}
}

/// Analyzes geometry properties of 3D meshes
pub struct GeometryAnalyzer;

impl GeometryAnalyzer {
	/// Analyze a mesh and extract geometric features
	pub fn analyze(mesh: &MeshData) -> Result<GeometryFeatures, Model3DError> {
		info!("Analyzing geometry for mesh with {} vertices", mesh.vertices.len());

		if mesh.vertices.is_empty() {
			return Err(Model3DError::GeometryAnalysisFailed(
				"Mesh has no vertices".to_string(),
			));
		}

		let vertices_count = mesh.vertices.len() as u64;
		let faces_count = mesh.face_count() as u64;

		// Calculate bounding box
		let (bbox_min, bbox_max) = Self::calculate_bounding_box(&mesh.vertices);

		// Calculate edges count (rough estimate for triangular meshes)
		let edges_count = (faces_count * 3 / 2) as u64;

		// Calculate volume (using signed volume for watertight meshes)
		let volume = Self::calculate_volume(mesh);

		// Calculate surface area
		let surface_area = Self::calculate_surface_area(mesh);

		// Check manifold and closed properties
		let (is_manifold, is_closed) = Self::check_topology(mesh);

		// Calculate complexity score (0-1)
		let complexity_score = Self::calculate_complexity(vertices_count, faces_count);

		debug!(
			"Geometry analysis complete: {} vertices, {} faces, volume: {:.2}, area: {:.2}",
			vertices_count, faces_count, volume, surface_area
		);

		Ok(GeometryFeatures {
			vertices_count,
			faces_count,
			edges_count,
			bbox_min,
			bbox_max,
			volume,
			surface_area,
			is_manifold,
			is_closed,
			has_normals: mesh.normals.is_some(),
			has_uvs: mesh.uvs.is_some(),
			has_colors: mesh.colors.is_some(),
			complexity_score,
		})
	}

	fn calculate_bounding_box(vertices: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
		let mut min = vertices[0];
		let mut max = vertices[0];

		for vertex in vertices.iter().skip(1) {
			for i in 0..3 {
				if vertex[i] < min[i] {
					min[i] = vertex[i];
				}
				if vertex[i] > max[i] {
					max[i] = vertex[i];
				}
			}
		}

		(min, max)
	}

	fn calculate_volume(mesh: &MeshData) -> f64 {
		// Calculate signed volume using divergence theorem
		// For triangulated meshes: V = (1/6) * sum of (v1 · (v2 × v3))
		let mut volume = 0.0;

		let vertices = &mesh.vertices;
		let face_count = mesh.face_count();

		for i in 0..face_count {
			let idx = i * 3;
			if idx + 2 >= vertices.len() {
				break;
			}

			let v1 = vertices[idx];
			let v2 = vertices[idx + 1];
			let v3 = vertices[idx + 2];

			// Calculate cross product v2 × v3
			let cross = [
				v2[1] * v3[2] - v2[2] * v3[1],
				v2[2] * v3[0] - v2[0] * v3[2],
				v2[0] * v3[1] - v2[1] * v3[0],
			];

			// Dot product v1 · cross
			let dot = (v1[0] * cross[0] + v1[1] * cross[1] + v1[2] * cross[2]) as f64;
			volume += dot;
		}

		(volume / 6.0).abs()
	}

	fn calculate_surface_area(mesh: &MeshData) -> f64 {
		let mut area = 0.0;
		let vertices = &mesh.vertices;
		let face_count = mesh.face_count();

		for i in 0..face_count {
			let idx = i * 3;
			if idx + 2 >= vertices.len() {
				break;
			}

			let v1 = vertices[idx];
			let v2 = vertices[idx + 1];
			let v3 = vertices[idx + 2];

			// Calculate triangle area using cross product
			let edge1 = [v2[0] - v1[0], v2[1] - v1[1], v2[2] - v1[2]];
			let edge2 = [v3[0] - v1[0], v3[1] - v1[1], v3[2] - v1[2]];

			let cross = [
				edge1[1] * edge2[2] - edge1[2] * edge2[1],
				edge1[2] * edge2[0] - edge1[0] * edge2[2],
				edge1[0] * edge2[1] - edge1[1] * edge2[0],
			];

			let magnitude = ((cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]) as f64).sqrt();
			area += magnitude / 2.0;
		}

		area
	}

	fn check_topology(mesh: &MeshData) -> (bool, bool) {
		// Simplified topology check
		// A proper implementation would check:
		// - Each edge is shared by exactly 2 faces (manifold)
		// - No boundary edges (closed)

		// For now, use heuristics
		let vertex_count = mesh.vertices.len();
		let face_count = mesh.face_count();

		// Euler characteristic for closed manifold: V - E + F = 2
		// For triangular mesh: E ≈ 3F/2
		let expected_edges = (face_count * 3) / 2;
		let euler_char = vertex_count as i64 - expected_edges as i64 + face_count as i64;

		let is_closed = euler_char.abs() <= 2; // Allow small deviation
		let is_manifold = vertex_count > 0 && face_count > 0;

		(is_manifold, is_closed)
	}

	fn calculate_complexity(vertices: u64, faces: u64) -> f32 {
		// Simple complexity score based on polygon count
		// Scale logarithmically: 1K faces = 0.1, 10K = 0.3, 100K = 0.5, 1M = 0.7, 10M = 0.9
		let face_score = (faces as f32).log10() / 10.0;
		face_score.clamp(0.0, 1.0)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_bounding_box() {
		let vertices = vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [-1.0, -1.0, -1.0]];
		let (min, max) = GeometryAnalyzer::calculate_bounding_box(&vertices);

		assert_eq!(min, [-1.0, -1.0, -1.0]);
		assert_eq!(max, [1.0, 1.0, 1.0]);
	}

	#[test]
	fn test_complexity_score() {
		assert!(GeometryAnalyzer::calculate_complexity(100, 100) < 0.3);
		assert!(GeometryAnalyzer::calculate_complexity(100_000, 100_000) > 0.4);
	}
}
