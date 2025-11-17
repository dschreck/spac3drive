use super::Model3DError;
use std::path::Path;
use tracing::{debug, info, warn};

/// Represents parsed mesh data from various 3D formats
#[derive(Debug, Clone)]
pub struct MeshData {
	pub vertices: Vec<[f32; 3]>,
	pub normals: Option<Vec<[f32; 3]>>,
	pub uvs: Option<Vec<[f32; 2]>>,
	pub indices: Option<Vec<u32>>,
	pub colors: Option<Vec<[f32; 3]>>,
}

impl MeshData {
	pub fn new() -> Self {
		Self {
			vertices: Vec::new(),
			normals: None,
			uvs: None,
			indices: None,
			colors: None,
		}
	}

	pub fn vertex_count(&self) -> usize {
		self.vertices.len()
	}

	pub fn face_count(&self) -> usize {
		if let Some(ref indices) = self.indices {
			indices.len() / 3
		} else {
			self.vertices.len() / 3
		}
	}
}

impl Default for MeshData {
	fn default() -> Self {
		Self::new()
	}
}

/// Parsed 3D model with metadata
#[derive(Debug, Clone)]
pub struct ParsedMesh {
	pub data: MeshData,
	pub format: String,
	pub materials_count: usize,
	pub textures_count: usize,
}

/// Parser for various 3D mesh formats
pub struct MeshParser;

impl MeshParser {
	/// Parse a 3D model file based on its extension
	pub async fn parse(path: &Path) -> Result<ParsedMesh, Model3DError> {
		let extension = path
			.extension()
			.and_then(|e| e.to_str())
			.ok_or_else(|| Model3DError::ParseFailed("No file extension".to_string()))?
			.to_lowercase();

		info!("Parsing 3D model: {} (format: {})", path.display(), extension);

		match extension.as_str() {
			"stl" => Self::parse_stl(path).await,
			"obj" => Self::parse_obj(path).await,
			"ply" => Self::parse_ply(path).await,
			"3mf" => Self::parse_3mf(path).await,
			"gltf" | "glb" => Self::parse_gltf(path).await,
			_ => Err(Model3DError::UnsupportedFormat(extension)),
		}
	}

	/// Parse STL files (both ASCII and binary)
	async fn parse_stl(path: &Path) -> Result<ParsedMesh, Model3DError> {
		use tokio::fs;

		let data = fs::read(path).await.map_err(|e| {
			Model3DError::ParseFailed(format!("Failed to read STL file: {}", e))
		})?;

		// Check if binary or ASCII
		let is_binary = data.len() > 5 && &data[0..5] != b"solid";

		let mesh_data = if is_binary {
			Self::parse_binary_stl(&data)?
		} else {
			Self::parse_ascii_stl(&data)?
		};

		Ok(ParsedMesh {
			data: mesh_data,
			format: "stl".to_string(),
			materials_count: 0,
			textures_count: 0,
		})
	}

	fn parse_binary_stl(data: &[u8]) -> Result<MeshData, Model3DError> {
		if data.len() < 84 {
			return Err(Model3DError::ParseFailed(
				"Binary STL file too small".to_string(),
			));
		}

		// Skip 80-byte header
		let triangle_count = u32::from_le_bytes([data[80], data[81], data[82], data[83]]) as usize;

		debug!("Parsing binary STL with {} triangles", triangle_count);

		let mut vertices = Vec::with_capacity(triangle_count * 3);
		let mut normals = Vec::with_capacity(triangle_count * 3);

		let mut offset = 84;
		for _ in 0..triangle_count {
			if offset + 50 > data.len() {
				return Err(Model3DError::ParseFailed(
					"Unexpected end of STL file".to_string(),
				));
			}

			// Read normal
			let normal = [
				f32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]),
				f32::from_le_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]),
				f32::from_le_bytes([data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11]]),
			];
			offset += 12;

			// Read 3 vertices
			for _ in 0..3 {
				let vertex = [
					f32::from_le_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]),
					f32::from_le_bytes([data[offset + 4], data[offset + 5], data[offset + 6], data[offset + 7]]),
					f32::from_le_bytes([data[offset + 8], data[offset + 9], data[offset + 10], data[offset + 11]]),
				];
				vertices.push(vertex);
				normals.push(normal);
				offset += 12;
			}

			// Skip attribute byte count
			offset += 2;
		}

		Ok(MeshData {
			vertices,
			normals: Some(normals),
			uvs: None,
			indices: None,
			colors: None,
		})
	}

	fn parse_ascii_stl(data: &[u8]) -> Result<MeshData, Model3DError> {
		let content = std::str::from_utf8(data)
			.map_err(|e| Model3DError::ParseFailed(format!("Invalid UTF-8 in STL: {}", e)))?;

		let mut vertices = Vec::new();
		let mut normals = Vec::new();
		let mut current_normal = [0.0f32; 3];

		for line in content.lines() {
			let line = line.trim();

			if line.starts_with("facet normal") {
				let parts: Vec<&str> = line.split_whitespace().collect();
				if parts.len() >= 5 {
					current_normal = [
						parts[2].parse().unwrap_or(0.0),
						parts[3].parse().unwrap_or(0.0),
						parts[4].parse().unwrap_or(0.0),
					];
				}
			} else if line.starts_with("vertex") {
				let parts: Vec<&str> = line.split_whitespace().collect();
				if parts.len() >= 4 {
					vertices.push([
						parts[1].parse().unwrap_or(0.0),
						parts[2].parse().unwrap_or(0.0),
						parts[3].parse().unwrap_or(0.0),
					]);
					normals.push(current_normal);
				}
			}
		}

		debug!("Parsed ASCII STL with {} vertices", vertices.len());

		Ok(MeshData {
			vertices,
			normals: Some(normals),
			uvs: None,
			indices: None,
			colors: None,
		})
	}

	/// Parse OBJ files
	async fn parse_obj(path: &Path) -> Result<ParsedMesh, Model3DError> {
		use tokio::fs;

		let content = fs::read_to_string(path).await.map_err(|e| {
			Model3DError::ParseFailed(format!("Failed to read OBJ file: {}", e))
		})?;

		let mut vertices = Vec::new();
		let mut normals_raw = Vec::new();
		let mut uvs_raw = Vec::new();
		let mut indices = Vec::new();

		for line in content.lines() {
			let line = line.trim();
			if line.is_empty() || line.starts_with('#') {
				continue;
			}

			let parts: Vec<&str> = line.split_whitespace().collect();
			if parts.is_empty() {
				continue;
			}

			match parts[0] {
				"v" if parts.len() >= 4 => {
					vertices.push([
						parts[1].parse().unwrap_or(0.0),
						parts[2].parse().unwrap_or(0.0),
						parts[3].parse().unwrap_or(0.0),
					]);
				}
				"vn" if parts.len() >= 4 => {
					normals_raw.push([
						parts[1].parse().unwrap_or(0.0),
						parts[2].parse().unwrap_or(0.0),
						parts[3].parse().unwrap_or(0.0),
					]);
				}
				"vt" if parts.len() >= 3 => {
					uvs_raw.push([
						parts[1].parse().unwrap_or(0.0),
						parts[2].parse().unwrap_or(0.0),
					]);
				}
				"f" if parts.len() >= 4 => {
					// Parse face indices (simplified - assumes triangulated faces)
					for i in 1..parts.len() {
						let idx_str = parts[i].split('/').next().unwrap_or("0");
						if let Ok(idx) = idx_str.parse::<u32>() {
							if idx > 0 {
								indices.push(idx - 1); // OBJ indices are 1-based
							}
						}
					}
				}
				_ => {}
			}
		}

		debug!("Parsed OBJ with {} vertices, {} indices", vertices.len(), indices.len());

		Ok(ParsedMesh {
			data: MeshData {
				vertices,
				normals: if normals_raw.is_empty() { None } else { Some(normals_raw) },
				uvs: if uvs_raw.is_empty() { None } else { Some(uvs_raw) },
				indices: if indices.is_empty() { None } else { Some(indices) },
				colors: None,
			},
			format: "obj".to_string(),
			materials_count: 0,
			textures_count: 0,
		})
	}

	/// Parse PLY files (simplified - would need proper PLY parsing library)
	async fn parse_ply(path: &Path) -> Result<ParsedMesh, Model3DError> {
		warn!("PLY parsing not yet fully implemented - using placeholder");
		Ok(ParsedMesh {
			data: MeshData::new(),
			format: "ply".to_string(),
			materials_count: 0,
			textures_count: 0,
		})
	}

	/// Parse 3MF files (ZIP-based format)
	async fn parse_3mf(path: &Path) -> Result<ParsedMesh, Model3DError> {
		warn!("3MF parsing not yet fully implemented - using placeholder");
		Ok(ParsedMesh {
			data: MeshData::new(),
			format: "3mf".to_string(),
			materials_count: 0,
			textures_count: 0,
		})
	}

	/// Parse glTF/GLB files
	async fn parse_gltf(path: &Path) -> Result<ParsedMesh, Model3DError> {
		warn!("glTF parsing not yet fully implemented - using placeholder");
		Ok(ParsedMesh {
			data: MeshData::new(),
			format: "gltf".to_string(),
			materials_count: 0,
			textures_count: 0,
		})
	}
}
