//! Aplicación de planes de reconstrucción a una **imagen nueva** y resumen de cambios.
//!
//! La lógica de cada sistema de archivos (qué escribir) vive en su crate `fs-*`;
//! aquí solo se copia el origen y se aplican las escrituras en orden de fase.
//! El origen nunca se modifica.

pub mod image_writer;
pub mod summary;

pub use image_writer::{apply_in_memory, export_image, ExportError, ExportReport};
pub use summary::{summarize, DiffSummary};
