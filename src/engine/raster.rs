//! Page image export and image-only PDFs, on the existing PDFium worker.
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use image::{ImageEncoder, RgbImage};
use pdfium_render::prelude::*;

use super::{PageSize, page_pixels};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpg,
    Png,
    Pdf,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub format: Format,
    pub dpi: f32,
    pub quality: u8,
}

impl Settings {
    pub fn dimensions(self, size: PageSize) -> Result<(u32, u32), String> {
        if !self.dpi.is_finite()
            || !(36.0..=600.0).contains(&self.dpi)
            || !(1..=100).contains(&self.quality)
            || !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return Err("图片尺寸或质量无效".into());
        }
        let dimensions = page_pixels(size, self.dpi / 72.0);
        if dimensions.0 > 16384
            || dimensions.1 > 16384
            || u64::from(dimensions.0) * u64::from(dimensions.1) > 48_000_000
        {
            return Err("单页图片超过 4800 万像素或 16384 像素边长，请降低倍率".into());
        }
        Ok(dimensions)
    }
}

pub fn load_image(path: &Path) -> Result<RgbImage, String> {
    use image::ImageDecoder;
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("无法读取图片：{e}"))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let (width, height) = decoder.dimensions();
    if u64::from(width) * u64::from(height) > 48_000_000 {
        return Err("图片超过 4800 万像素，请先缩小图片".into());
    }
    let mut image = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    image.apply_orientation(orientation);
    let rgba = image.into_rgba8();
    Ok(RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y).0;
        let alpha = u32::from(p[3]);
        image::Rgb(
            [p[0], p[1], p[2]]
                .map(|value| ((u32::from(value) * alpha + 255 * (255 - alpha) + 127) / 255) as u8),
        )
    }))
}

pub fn append_image(document: &mut PdfDocument, image: &RgbImage) -> Result<(), String> {
    let (width, height) = image.dimensions();
    let mut bgra: Vec<u8> = image
        .pixels()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let bitmap = PdfBitmap::from_bytes(
        width as Pixels,
        height as Pixels,
        PdfBitmapFormat::BGRA,
        &mut bgra,
    )
    .map_err(|e| e.to_string())?;
    let mut object = PdfPageImageObject::new(document).map_err(|e| e.to_string())?;
    object.set_bitmap(&bitmap).map_err(|e| e.to_string())?;
    object
        .scale(width as f32, height as f32)
        .map_err(|e| e.to_string())?;
    let mut page = document
        .pages_mut()
        .create_page_at_end(PdfPagePaperSize::Custom(
            PdfPoints::new(width as f32),
            PdfPoints::new(height as f32),
        ))
        .map_err(|e| e.to_string())?;
    page.objects_mut()
        .add_image_object(object)
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn render(
    document: &PdfDocument,
    page: u32,
    size: PageSize,
    settings: Settings,
) -> Result<RgbImage, String> {
    let (width, height) = settings.dimensions(size)?;
    let page = document
        .pages()
        .get(page as PdfPageIndex)
        .map_err(|e| e.to_string())?;
    let bitmap = page
        .render_with_config(
            &PdfRenderConfig::new()
                .set_fixed_size(width as Pixels, height as Pixels)
                .set_clear_color(PdfColor::WHITE)
                .render_annotations(true),
        )
        .map_err(|e| e.to_string())?;
    // Render over white before converting to RGB; transparent pages stay white.
    let rgba = bitmap.as_rgba_bytes();
    let rgb = rgba
        .chunks_exact(4)
        .flat_map(|p| [p[0], p[1], p[2]])
        .collect();
    RgbImage::from_raw(width, height, rgb).ok_or_else(|| "无法读取页面图像".into())
}

pub fn jpeg(image: &RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality)
        .encode(
            image.as_raw(),
            image.width(),
            image.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

pub enum Output<'p> {
    Images(Images),
    Pdf(PdfDocument<'p>),
}

pub struct Images {
    stage: PathBuf,
    files: Vec<PathBuf>,
}

impl Drop for Images {
    fn drop(&mut self) {
        for file in &self.files {
            let _ = fs::remove_file(file);
        }
        let _ = fs::remove_dir(&self.stage);
    }
}

impl<'p> Output<'p> {
    pub fn new(pdfium: &'p Pdfium, dest: &Path, format: Format) -> Result<Self, String> {
        if format == Format::Pdf {
            return pdfium
                .create_new_pdf()
                .map(Self::Pdf)
                .map_err(|e| e.to_string());
        }
        if dest.exists() {
            return Err("导出文件夹已存在，请选择新名称".into());
        }
        static SEQUENCE: AtomicU64 = AtomicU64::new(1);
        let parent = dest.parent().ok_or("无效的导出目录")?;
        for _ in 0..32 {
            let stage = parent.join(format!(
                ".folio-images-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&stage) {
                Ok(()) => {
                    return Ok(Self::Images(Images {
                        stage,
                        files: Vec::new(),
                    }));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("无法创建导出临时目录".into())
    }

    pub fn add(
        &mut self,
        index: u32,
        size: PageSize,
        image: &RgbImage,
        settings: Settings,
    ) -> Result<(), String> {
        match self {
            Self::Images(output) => {
                let extension = if settings.format == Format::Png {
                    "png"
                } else {
                    "jpg"
                };
                let path = output
                    .stage
                    .join(format!("第{:04}页.{extension}", index + 1));
                output.files.push(path.clone());
                let mut file = fs::File::create(&path).map_err(|e| e.to_string())?;
                if settings.format == Format::Png {
                    image::codecs::png::PngEncoder::new(&mut file)
                        .write_image(
                            image.as_raw(),
                            image.width(),
                            image.height(),
                            image::ExtendedColorType::Rgb8,
                        )
                        .map_err(|e| e.to_string())?;
                } else {
                    use std::io::Write;
                    file.write_all(&jpeg(image, settings.quality)?)
                        .map_err(|e| e.to_string())?;
                }
            }
            Self::Pdf(document) => {
                let mut object = PdfPageImageObject::new_from_jpeg_reader(
                    document,
                    Cursor::new(jpeg(image, settings.quality)?),
                )
                .map_err(|e| e.to_string())?;
                object
                    .scale(size.width, size.height)
                    .map_err(|e| e.to_string())?;
                let mut page = document
                    .pages_mut()
                    .create_page_at_end(PdfPagePaperSize::Custom(
                        PdfPoints::new(size.width),
                        PdfPoints::new(size.height),
                    ))
                    .map_err(|e| e.to_string())?;
                page.objects_mut()
                    .add_image_object(object)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub fn publish_images(&self, dest: &Path) -> Result<(), String> {
        let Self::Images(output) = self else {
            return Err("导出类型无效".into());
        };
        if dest.exists() {
            return Err("导出文件夹已存在，未覆盖已有文件".into());
        }
        fs::rename(&output.stage, dest).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimensions_use_72_dpi_and_reject_oversized_pages() {
        let size = PageSize {
            width: 600.0,
            height: 800.0,
        };
        for factor in [1.0, 1.5, 2.0, 3.0] {
            let settings = Settings {
                format: Format::Png,
                dpi: 72.0 * factor,
                quality: 85,
            };
            assert_eq!(
                settings.dimensions(size).unwrap(),
                ((600.0 * factor) as u32, (800.0 * factor) as u32)
            );
        }
        let settings = Settings {
            format: Format::Pdf,
            dpi: 216.0,
            quality: 75,
        };
        assert!(
            settings
                .dimensions(PageSize {
                    width: 10000.0,
                    height: 10000.0
                })
                .is_err()
        );
        assert!(
            Settings {
                dpi: f32::NAN,
                ..settings
            }
            .dimensions(size)
            .is_err()
        );
    }
}
